//! Dockable-panel geometry for the map editor.
//!
//! The editor UI is *immediate mode* (rebuilt twice a frame, retaining nothing —
//! see [`crate::ui`]). So the one thing that must survive between frames — where
//! each panel lives — is kept here as plain value state on [`DockManager`], owned
//! by the [`MapViewer`](super::MapViewer). Each frame `step` calls
//! [`DockManager::recompute`] *once* to tile the panels into absolute [`Rect`]s
//! ([`Solved`]); both the hit pass and the later draw pass read that one result,
//! so they can never disagree about where a panel is.
//!
//! A panel is either **docked** to a screen edge (tiling a strip off that edge,
//! the remaining centre becoming the world view) or **floating** at an absolute
//! rect (drawn over the world, z-ordered). Panels lay out at the origin and are
//! *placed* by translating their resolved rects ([`Ui::draw_at`](crate::ui::layout::Ui::draw_at)),
//! which sidesteps the resolver's `Position::Absolute` double-offset.
#![allow(dead_code)] // fields/variants fill in across the editor's build phases.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::geometry::Vec2;
use crate::ui::layout::Rect;

/// Default width of a left/right dock (and height of a top/bottom dock), px. The
/// classic editor column was 84px wide, so that is the docked default.
pub const DEFAULT_DOCK: i16 = 84;
/// Smallest a dock can be dragged to before it stops shrinking.
pub const MIN_DOCK: i16 = 24;
/// Smallest the leftover world view is allowed to get, px — docks can't eat it
/// entirely.
pub const MIN_WORLD: i16 = 32;
/// Minimum floating-panel size, px.
pub const MIN_FLOAT_W: i16 = 40;
pub const MIN_FLOAT_H: i16 = 32;
/// How far (Manhattan px) a docked panel's title must be dragged before it tears
/// off into a float — small enough to feel responsive, large enough that a click
/// just focuses.
pub const TEAR_THRESHOLD: i16 = 4;
/// How close to a screen edge a dragged panel's cursor must be to snap-dock there.
pub const EDGE_SNAP: i16 = 10;
/// Size of a floating panel's south-east resize handle, px.
pub const FLOAT_HANDLE: i16 = 6;

/// The independent editor panels. (The old Interacts/Warps tool tabs live
/// together under [`Objects`](Self::Objects) as sub-tabs.)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub enum PanelKind {
    Layers,
    Paint,
    Objects,
    Maps,
    /// Map-level settings: camera, background colour, resize.
    Map,
    /// Preview + author the dialogue an object's interaction triggers.
    Dialogue,
}

impl PanelKind {
    pub const ALL: [PanelKind; 6] = [
        Self::Layers,
        Self::Paint,
        Self::Objects,
        Self::Maps,
        Self::Map,
        Self::Dialogue,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Layers => "Layers",
            Self::Paint => "Paint",
            Self::Objects => "Objects",
            Self::Maps => "Maps",
            // "Setup" (not "Map") so its global-bar letter doesn't collide with
            // the "Maps" browser's "M".
            Self::Map => "Setup",
            Self::Dialogue => "Dialog",
        }
    }
}

/// Which screen edge a docked panel tiles off of.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Where a panel sits: tiled to an edge (sized in px along its axis) or floating
/// at an absolute framebuffer rect.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub enum Placement {
    /// `size` is width for Left/Right, height for Top/Bottom.
    Dock {
        side: Side,
        size: i16,
    },
    Float {
        x: i16,
        y: i16,
        w: i16,
        h: i16,
    },
}

/// One editor panel: its content kind, where it sits, its stacking order (higher
/// draws later / on top), and whether it is shown at all.
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Panel {
    pub kind: PanelKind,
    pub place: Placement,
    pub z: u16,
    pub open: bool,
}

/// The serialized dock arrangement (no live drag transients). Persisted to
/// `config/layout.json`; [`Default`] reproduces the classic single left column.
#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct DockLayout {
    pub panels: Vec<Panel>,
}

/// The only interaction state that persists across frames (the UI tree retains
/// nothing). Advanced once per `step`, then read by draw — never serialized.
#[derive(Clone, Copy, Default, Debug)]
pub enum DragState {
    #[default]
    Idle,
    /// Moving a floating panel by its title bar. `arming` marks a docked panel
    /// that has been pressed but not yet dragged past the tear-off threshold —
    /// a still click just focuses it.
    MovePanel {
        idx: usize,
        grab_dx: i16,
        grab_dy: i16,
        arming: bool,
    },
    /// Resizing a floating panel by its south-east handle.
    ResizeFloat { idx: usize, anchor: Rect },
    /// Dragging a docked side's inner-edge splitter — resizes every panel docked
    /// to that side together (they share the side's thickness).
    ResizeDock { side: Side },
}

/// The geometry computed once per frame by [`DockManager::recompute`] and read by
/// both the hit pass and the draw pass — the single source of truth that keeps
/// the two immediate-mode rebuilds in agreement.
#[derive(Clone, Default, Debug)]
pub struct Solved {
    /// The screen size this was solved against.
    pub screen: (i16, i16),
    /// Panel index → absolute rect, in **draw order**: docked panels first (in
    /// panel order), then floating panels ascending by z (so the highest z draws
    /// last / on top). Hit-test walks this in reverse for front-to-back picking.
    pub rects: Vec<(usize, Rect)>,
    /// The leftover centre region after docked panels are subtracted — the world
    /// view, where canvas (paint/object) interaction is allowed.
    pub world: Rect,
    /// The draggable resize band between each occupied dock side and the world
    /// (a thin strip straddling the boundary). Hit to start a [`DragState::ResizeDock`].
    pub splitters: Vec<(Side, Rect)>,
    /// While dragging a panel, the edge its drop would dock to (for highlight).
    pub hot_edge: Option<Side>,
}

impl Solved {
    /// The absolute rect of panel `idx`, if it is in the layout.
    pub fn rect_of(&self, idx: usize) -> Option<Rect> {
        self.rects.iter().find(|(i, _)| *i == idx).map(|(_, r)| *r)
    }
}

/// Owns the panels and the live drag FSM, and caches the per-frame [`Solved`]
/// geometry. Lives on the [`MapViewer`](super::MapViewer).
#[derive(Clone, Debug)]
pub struct DockManager {
    pub panels: Vec<Panel>,
    pub drag: DragState,
    /// Next z to assign when a panel is raised/focused (monotonic).
    pub z_top: u16,
    pub solved: Solved,
    /// Whether a persisted layout has been loaded yet (primary view only).
    pub loaded: bool,
    /// Set when the layout changed and should be re-saved (debounce flag).
    pub dirty: bool,
    /// Per-panel vertical scroll offset (px), keyed by panel index. A transient —
    /// not part of the saved layout. Clamped against content height each frame at
    /// its use site, so a stale value can't strand content off-view.
    pub scrolls: HashMap<usize, i16>,
}

impl DockManager {
    /// This panel's stored scroll offset (0 if never scrolled).
    pub fn scroll(&self, idx: usize) -> i16 {
        self.scrolls.get(&idx).copied().unwrap_or(0)
    }
    /// Set this panel's scroll offset.
    pub fn set_scroll(&mut self, idx: usize, value: i16) {
        self.scrolls.insert(idx, value);
    }
}

impl Default for DockManager {
    /// The classic look approximated with independent panels: the three tool
    /// panels stacked in one left column, Maps hidden until opened. (`Maps` is
    /// listed so a layout file / a future panel menu can show it.)
    fn default() -> Self {
        let dock_left = |kind, z| Panel {
            kind,
            place: Placement::Dock {
                side: Side::Left,
                size: DEFAULT_DOCK,
            },
            z,
            open: true,
        };
        Self {
            panels: vec![
                dock_left(PanelKind::Layers, 0),
                dock_left(PanelKind::Paint, 1),
                dock_left(PanelKind::Objects, 2),
                Panel {
                    kind: PanelKind::Maps,
                    place: Placement::Float {
                        x: 60,
                        y: 20,
                        w: 110,
                        h: 96,
                    },
                    z: 3,
                    open: false,
                },
                Panel {
                    kind: PanelKind::Map,
                    place: Placement::Float {
                        x: 70,
                        y: 24,
                        w: 86,
                        h: 96,
                    },
                    z: 4,
                    open: false,
                },
                Panel {
                    kind: PanelKind::Dialogue,
                    place: Placement::Float {
                        x: 74,
                        y: 20,
                        w: 104,
                        h: 108,
                    },
                    z: 5,
                    open: false,
                },
            ],
            drag: DragState::Idle,
            z_top: 6,
            solved: Solved::default(),
            loaded: false,
            dirty: false,
            scrolls: HashMap::new(),
        }
    }
}

impl DockManager {
    /// Tile the panels against `screen` and store the result in [`solved`](Self::solved).
    /// Pure geometry; the cursor-dependent `hot_edge` highlight is filled in by
    /// the editor's step (which has the mouse) when a drag is active.
    pub fn recompute(&mut self, screen: (f32, f32)) {
        self.solved = self.solve(screen);
    }

    /// The panels (in panel order) docked to `side` and currently open.
    fn members(&self, side: Side) -> Vec<usize> {
        self.panels
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                matches!(p.place, Placement::Dock { side: s, .. } if s == side) && p.open
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The thickness (px along its perpendicular) a side's dock wants — the max
    /// of its panels' stored sizes, so resizing the side moves them together.
    /// Takes the side's already-resolved `members` so `solve` doesn't re-scan.
    fn side_thickness(&self, members: &[usize]) -> i16 {
        members
            .iter()
            .filter_map(|&i| match self.panels[i].place {
                Placement::Dock { size, .. } => Some(size),
                _ => None,
            })
            .max()
            .unwrap_or(DEFAULT_DOCK)
            .max(MIN_DOCK)
    }

    /// Pure layout. Docked sides claim full-edge strips off the shrinking world
    /// rect — Left/Right first (full height), then Top/Bottom (between them);
    /// panels sharing a side stack and split it equally. The leftover centre is
    /// the world view. Floating panels are then placed (clamped on screen)
    /// ascending by z. Pure (takes only `&self`); [`recompute`](Self::recompute)
    /// stores the result into `self.solved` for the frame.
    fn solve(&self, screen: (f32, f32)) -> Solved {
        let sw = screen.0 as i16;
        let sh = screen.1 as i16;
        let mut world = Rect {
            x: 0,
            y: 0,
            w: sw,
            h: sh,
        };
        let mut rects: Vec<(usize, Rect)> = Vec::new();
        let mut splitters: Vec<(Side, Rect)> = Vec::new();

        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let members = self.members(side);
            if members.is_empty() {
                continue;
            }
            let n = members.len() as i16;
            let horizontal = matches!(side, Side::Left | Side::Right);
            let near = matches!(side, Side::Left | Side::Top);
            // `main` is the strip's thickness axis (claimed off the world rect);
            // `cross` is the shared axis the side's panels stack along and split
            // equally. Reading/writing both axes through these keeps Left/Right
            // and Top/Bottom one algorithm, so a seam/clamp fix can't land on one
            // axis and miss the other.
            let (main_pos, main_len) = if horizontal {
                (world.x, world.w)
            } else {
                (world.y, world.h)
            };
            let (cross_pos, cross_len) = if horizontal {
                (world.y, world.h)
            } else {
                (world.x, world.w)
            };
            let thick = self
                .side_thickness(&members)
                .min((main_len - MIN_WORLD).max(0));
            for (k, &i) in members.iter().enumerate() {
                let c0 = cross_pos + (cross_len * k as i16) / n;
                let c1 = cross_pos + (cross_len * (k as i16 + 1)) / n;
                let main_start = if near {
                    main_pos
                } else {
                    main_pos + main_len - thick
                };
                rects.push((
                    i,
                    if horizontal {
                        Rect {
                            x: main_start,
                            y: c0,
                            w: thick,
                            h: c1 - c0,
                        }
                    } else {
                        Rect {
                            x: c0,
                            y: main_start,
                            w: c1 - c0,
                            h: thick,
                        }
                    },
                ));
            }
            let seam = if near {
                main_pos + thick - 1
            } else {
                main_pos + main_len - thick - 1
            };
            splitters.push((
                side,
                if horizontal {
                    Rect {
                        x: seam,
                        y: world.y,
                        w: 2,
                        h: world.h,
                    }
                } else {
                    Rect {
                        x: world.x,
                        y: seam,
                        w: world.w,
                        h: 2,
                    }
                },
            ));
            if near && horizontal {
                world.x += thick;
            } else if near {
                world.y += thick;
            }
            if horizontal {
                world.w -= thick;
            } else {
                world.h -= thick;
            }
        }

        // Floating panels, painted after docked ones, ascending by z.
        let mut floats: Vec<usize> = self
            .panels
            .iter()
            .enumerate()
            .filter(|(_, p)| p.open && matches!(p.place, Placement::Float { .. }))
            .map(|(i, _)| i)
            .collect();
        floats.sort_by_key(|&i| self.panels[i].z);
        for i in floats {
            let Placement::Float { x, y, w, h } = self.panels[i].place else {
                continue;
            };
            let w = w.max(MIN_FLOAT_W);
            let h = h.max(MIN_FLOAT_H);
            let x = x.clamp(0, (sw - w).max(0));
            let y = y.clamp(0, (sh - h).max(0));
            rects.push((i, Rect { x, y, w, h }));
        }

        Solved {
            screen: (sw, sh),
            rects,
            world,
            splitters,
            hot_edge: None,
        }
    }

    /// The dock side whose splitter band contains `point`, if any — the target of
    /// a [`DragState::ResizeDock`] press.
    pub fn splitter_at(&self, point: Vec2) -> Option<Side> {
        self.solved
            .splitters
            .iter()
            .find(|(_, band)| band.contains(point))
            .map(|(side, _)| *side)
    }

    /// Resize a dock side to `thick` px (clamped), setting every panel on that
    /// side so they keep sharing one thickness.
    pub fn set_side_thickness(&mut self, side: Side, thick: i16) {
        let thick = thick.max(MIN_DOCK);
        for i in self.members(side) {
            if let Placement::Dock { size, .. } = &mut self.panels[i].place {
                *size = thick;
            }
        }
    }

    /// Bring panel `idx` to the front (give it the next z). Only affects draw/hit
    /// order among floating panels, but is cheap to always do on focus.
    pub fn raise(&mut self, idx: usize) {
        if let Some(p) = self.panels.get_mut(idx) {
            p.z = self.z_top;
            self.z_top = self.z_top.wrapping_add(1);
        }
    }

    /// The first open panel of `kind` and its solved rect, if shown.
    pub fn open_panel(&self, kind: PanelKind) -> Option<(usize, Rect)> {
        let idx = self.panels.iter().position(|p| p.kind == kind && p.open)?;
        self.solved.rect_of(idx).map(|r| (idx, r))
    }

    /// Show/hide the (first) panel of `kind`.
    pub fn toggle_panel(&mut self, kind: PanelKind) {
        if let Some(p) = self.panels.iter_mut().find(|p| p.kind == kind) {
            p.open = !p.open;
        }
        self.dirty = true;
    }

    /// Add a (closed, floating) panel for every [`PanelKind`] not already present.
    /// A layout file written before a panel kind existed won't list it, so this
    /// keeps its global-bar toggle working after an upgrade.
    pub fn ensure_all_kinds(&mut self) {
        for kind in PanelKind::ALL {
            if !self.panels.iter().any(|p| p.kind == kind) {
                let z = self.z_top;
                self.z_top = z.wrapping_add(1);
                self.panels.push(Panel {
                    kind,
                    place: Placement::Float {
                        x: 70,
                        y: 24,
                        w: 86,
                        h: 96,
                    },
                    z,
                    open: false,
                });
            }
        }
    }

    /// Float panel `idx` at top-left `pos` with size `(w, h)`.
    pub fn set_float(&mut self, idx: usize, pos: Vec2, w: i16, h: i16) {
        if let Some(p) = self.panels.get_mut(idx) {
            p.place = Placement::Float {
                x: pos.x,
                y: pos.y,
                w: w.max(MIN_FLOAT_W),
                h: h.max(MIN_FLOAT_H),
            };
        }
    }

    /// Move a floating panel's top-left to `pos`, keeping its size.
    pub fn move_float(&mut self, idx: usize, pos: Vec2) {
        if let Some(p) = self.panels.get_mut(idx)
            && let Placement::Float { w, h, .. } = p.place
        {
            p.place = Placement::Float {
                x: pos.x,
                y: pos.y,
                w,
                h,
            };
        }
    }

    /// Resize a floating panel by dragging its SE corner to `cursor`, holding its
    /// top-left `anchor` fixed.
    pub fn resize_float(&mut self, idx: usize, anchor: Rect, cursor: Vec2) {
        if let Some(p) = self.panels.get_mut(idx) {
            p.place = Placement::Float {
                x: anchor.x,
                y: anchor.y,
                w: (cursor.x - anchor.x).max(MIN_FLOAT_W),
                h: (cursor.y - anchor.y).max(MIN_FLOAT_H),
            };
        }
    }

    /// Dock panel `idx` to `side` at the default thickness (a dropped float).
    pub fn dock_panel(&mut self, idx: usize, side: Side) {
        if let Some(p) = self.panels.get_mut(idx) {
            p.place = Placement::Dock {
                side,
                size: DEFAULT_DOCK,
            };
        }
    }

    /// Whether panel `idx` is currently floating.
    pub fn is_float(&self, idx: usize) -> bool {
        matches!(
            self.panels.get(idx).map(|p| p.place),
            Some(Placement::Float { .. })
        )
    }

    /// The topmost floating panel whose SE resize handle contains `point`.
    pub fn float_handle_at(&self, point: Vec2) -> Option<usize> {
        for &(idx, rect) in self.solved.rects.iter().rev() {
            if self.is_float(idx) {
                let handle = Rect {
                    x: rect.x + rect.w - FLOAT_HANDLE,
                    y: rect.y + rect.h - FLOAT_HANDLE,
                    w: FLOAT_HANDLE,
                    h: FLOAT_HANDLE,
                };
                if handle.contains(point) {
                    return Some(idx);
                }
            }
        }
        None
    }

    /// The screen edge a dropped panel would dock to, if the cursor is within
    /// [`EDGE_SNAP`] of one (Left/Right take priority over Top/Bottom at corners).
    pub fn edge_near(&self, p: Vec2, screen: (f32, f32)) -> Option<Side> {
        let sw = screen.0 as i16;
        let sh = screen.1 as i16;
        if p.x <= EDGE_SNAP {
            Some(Side::Left)
        } else if p.x >= sw - EDGE_SNAP {
            Some(Side::Right)
        } else if p.y <= EDGE_SNAP {
            Some(Side::Top)
        } else if p.y >= sh - EDGE_SNAP {
            Some(Side::Bottom)
        } else {
            None
        }
    }

    /// The strip a panel dropped on `side` would occupy — for the drop-zone
    /// highlight while dragging.
    pub fn edge_zone(side: Side, screen: (f32, f32)) -> Rect {
        let sw = screen.0 as i16;
        let sh = screen.1 as i16;
        match side {
            Side::Left => Rect {
                x: 0,
                y: 0,
                w: DEFAULT_DOCK,
                h: sh,
            },
            Side::Right => Rect {
                x: sw - DEFAULT_DOCK,
                y: 0,
                w: DEFAULT_DOCK,
                h: sh,
            },
            Side::Top => Rect {
                x: 0,
                y: 0,
                w: sw,
                h: DEFAULT_DOCK,
            },
            Side::Bottom => Rect {
                x: 0,
                y: sh - DEFAULT_DOCK,
                w: sw,
                h: DEFAULT_DOCK,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(kind: PanelKind, place: Placement) -> Panel {
        Panel {
            kind,
            place,
            z: 0,
            open: true,
        }
    }

    fn dock(kind: PanelKind, side: Side, size: i16) -> Panel {
        panel(kind, Placement::Dock { side, size })
    }

    fn manager(panels: Vec<Panel>) -> DockManager {
        DockManager {
            panels,
            ..DockManager::default()
        }
    }

    /// The default layout: three tool panels stacked in the left column (Maps
    /// closed). They share the 84px width and split the height; the world view is
    /// what's left to the right, with one splitter on the boundary.
    #[test]
    fn default_layout_stacks_three_left() {
        let mut dm = DockManager::default();
        dm.recompute((240.0, 136.0));
        let s = &dm.solved;
        // Layers/Paint/Objects (idx 0,1,2) are 84 wide, stacked, full height split.
        assert_eq!(
            s.rect_of(0),
            Some(Rect {
                x: 0,
                y: 0,
                w: 84,
                h: 45
            })
        );
        assert_eq!(
            s.rect_of(1),
            Some(Rect {
                x: 0,
                y: 45,
                w: 84,
                h: 45
            })
        );
        assert_eq!(
            s.rect_of(2),
            Some(Rect {
                x: 0,
                y: 90,
                w: 84,
                h: 46
            })
        );
        // Maps (idx 3) is closed — not placed.
        assert_eq!(s.rect_of(3), None);
        // World is the right remainder; one Left splitter on the seam.
        assert_eq!(
            s.world,
            Rect {
                x: 84,
                y: 0,
                w: 156,
                h: 136
            }
        );
        assert_eq!(s.splitters.len(), 1);
        assert_eq!(s.splitters[0].0, Side::Left);
        assert_eq!(
            s.splitters[0].1,
            Rect {
                x: 83,
                y: 0,
                w: 2,
                h: 136
            }
        );
    }

    /// Each side tiles off the correct edge; Left/Right take full height first,
    /// then Top fills only the width left between them.
    #[test]
    fn sides_tile_in_order() {
        let mut dm = manager(vec![
            dock(PanelKind::Layers, Side::Left, 40),
            dock(PanelKind::Paint, Side::Right, 30),
            dock(PanelKind::Objects, Side::Top, 28),
        ]);
        dm.recompute((240.0, 136.0));
        let s = &dm.solved;
        assert_eq!(
            s.rect_of(0),
            Some(Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 136
            })
        );
        assert_eq!(
            s.rect_of(1),
            Some(Rect {
                x: 210,
                y: 0,
                w: 30,
                h: 136
            })
        );
        // Top spans only the 170px between the left and right docks.
        assert_eq!(
            s.rect_of(2),
            Some(Rect {
                x: 40,
                y: 0,
                w: 170,
                h: 28
            })
        );
        assert_eq!(
            s.world,
            Rect {
                x: 40,
                y: 28,
                w: 170,
                h: 108
            }
        );
    }

    /// A docked side can't eat the whole framebuffer — the world keeps `MIN_WORLD`.
    #[test]
    fn dock_leaves_min_world() {
        let mut dm = manager(vec![dock(PanelKind::Layers, Side::Left, 1000)]);
        dm.recompute((240.0, 136.0));
        assert_eq!(dm.solved.world.w, MIN_WORLD);
        assert_eq!(dm.solved.rect_of(0).unwrap().w, 240 - MIN_WORLD);
    }

    /// The splitter band picks its side, and resizing a side moves every panel on
    /// it together.
    #[test]
    fn splitter_pick_and_resize_whole_side() {
        let mut dm = manager(vec![
            dock(PanelKind::Layers, Side::Left, 84),
            dock(PanelKind::Paint, Side::Left, 84),
        ]);
        dm.recompute((240.0, 136.0));
        assert_eq!(dm.splitter_at(Vec2::new(83, 60)), Some(Side::Left));
        assert_eq!(dm.splitter_at(Vec2::new(120, 60)), None);
        dm.set_side_thickness(Side::Left, 50);
        dm.recompute((240.0, 136.0));
        assert_eq!(dm.solved.rect_of(0).unwrap().w, 50);
        assert_eq!(dm.solved.rect_of(1).unwrap().w, 50);
    }

    /// A floating panel parked off-screen is clamped back into view.
    #[test]
    fn float_clamps_into_screen() {
        let mut dm = manager(vec![panel(
            PanelKind::Maps,
            Placement::Float {
                x: 300,
                y: 200,
                w: 80,
                h: 60,
            },
        )]);
        dm.recompute((240.0, 136.0));
        let r = dm.solved.rect_of(0).unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (160, 76, 80, 60));
        // No docks → the whole screen is world.
        assert_eq!(
            dm.solved.world,
            Rect {
                x: 0,
                y: 0,
                w: 240,
                h: 136
            }
        );
    }

    /// The cursor near an edge resolves to that dock side; the middle is no edge.
    #[test]
    fn edge_near_snaps_at_the_borders() {
        let dm = DockManager::default();
        assert_eq!(
            dm.edge_near(Vec2::new(3, 70), (240.0, 136.0)),
            Some(Side::Left)
        );
        assert_eq!(
            dm.edge_near(Vec2::new(238, 70), (240.0, 136.0)),
            Some(Side::Right)
        );
        assert_eq!(
            dm.edge_near(Vec2::new(120, 2), (240.0, 136.0)),
            Some(Side::Top)
        );
        assert_eq!(
            dm.edge_near(Vec2::new(120, 134), (240.0, 136.0)),
            Some(Side::Bottom)
        );
        assert_eq!(dm.edge_near(Vec2::new(120, 70), (240.0, 136.0)), None);
    }

    /// Tearing a docked panel off floats it, dragging it floats-moves it, and a
    /// drop on an edge docks it there.
    #[test]
    fn tear_off_move_and_redock() {
        let mut dm = manager(vec![dock(PanelKind::Layers, Side::Left, 84)]);
        dm.recompute((240.0, 136.0));
        // Tear off → floating at a chosen origin, keeping the docked size.
        dm.set_float(0, Vec2::new(100, 40), 84, 136);
        assert!(dm.is_float(0));
        dm.recompute((240.0, 136.0));
        // The whole screen is world again (nothing docked).
        assert_eq!(
            dm.solved.world,
            Rect {
                x: 0,
                y: 0,
                w: 240,
                h: 136
            }
        );
        // Its SE handle is pickable; the body is not a handle.
        let r = dm.solved.rect_of(0).unwrap();
        assert_eq!(
            dm.float_handle_at(Vec2::new(r.x + r.w - 2, r.y + r.h - 2)),
            Some(0)
        );
        assert_eq!(dm.float_handle_at(Vec2::new(r.x + 2, r.y + 2)), None);
        // Drop on the right edge re-docks it there.
        dm.dock_panel(0, Side::Right);
        dm.recompute((240.0, 136.0));
        let r = dm.solved.rect_of(0).unwrap();
        assert_eq!((r.x, r.w), (240 - DEFAULT_DOCK, DEFAULT_DOCK));
        assert_eq!(dm.solved.world.x, 0);
    }

    /// Toggling a panel hides then shows it (removing/restoring it from layout).
    #[test]
    fn toggle_hides_and_shows() {
        let mut dm = DockManager::default();
        dm.recompute((240.0, 136.0));
        assert!(dm.open_panel(PanelKind::Layers).is_some());
        dm.toggle_panel(PanelKind::Layers);
        dm.recompute((240.0, 136.0));
        assert!(dm.open_panel(PanelKind::Layers).is_none());
        dm.toggle_panel(PanelKind::Layers);
        dm.recompute((240.0, 136.0));
        assert!(dm.open_panel(PanelKind::Layers).is_some());
    }

    /// Floating panels draw after docked ones, ascending by z (so a raised panel
    /// lands last in the draw list / first under a reverse hit walk).
    #[test]
    fn floats_sorted_after_docks_by_z() {
        let mut a = panel(
            PanelKind::Maps,
            Placement::Float {
                x: 0,
                y: 0,
                w: 40,
                h: 40,
            },
        );
        a.z = 5;
        let mut b = panel(
            PanelKind::Layers,
            Placement::Float {
                x: 0,
                y: 0,
                w: 40,
                h: 40,
            },
        );
        b.z = 2;
        let mut dm = manager(vec![dock(PanelKind::Paint, Side::Left, 84), a, b]);
        dm.recompute((240.0, 136.0));
        // Draw order: dock (idx 0), then floats by z: b (idx 2, z=2), a (idx 1, z=5).
        let order: Vec<usize> = dm.solved.rects.iter().map(|(i, _)| *i).collect();
        assert_eq!(order, vec![0, 2, 1]);
    }
}
