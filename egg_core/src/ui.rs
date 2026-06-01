//! A small, immediate-mode UI layer over the [Taffy](https://docs.rs/taffy)
//! flexbox engine.
//!
//! The console draws to a fixed 240×136 indexed/RGBA canvas with hand-written
//! pixel coordinates. This module replaces that manual arithmetic for menu-like
//! UIs: you describe a tree of styled boxes (text, sprites, containers), Taffy
//! computes an absolute pixel [`Rect`] for each, and then you get two passes for
//! free — [`Ui::draw`] (render decoration + content) and [`Ui::hit`] (mouse
//! pick). Every interactive box carries a caller-chosen key `K`, so hit-testing
//! returns exactly the element the mouse is over.
//!
//! It is *immediate mode*: rebuild the tree each frame with [`UiBuilder`]. The
//! trees here are tiny (<30 nodes) so this is effectively free at 64 fps, and it
//! keeps the existing per-frame `step`/`draw` split unchanged — both passes just
//! rebuild the same layout.
//!
//! Leaf sizes are supplied up front (measure text with [`ConsoleHelper::text_width`],
//! sprites are `w*8*scale` px), so Taffy's measure-closure is never needed.

use taffy::geometry::Rect as TaffyRect;
use taffy::prelude::{
    AlignItems, AvailableSpace, Dimension, Display, FlexDirection, FlexWrap, JustifyContent,
    LengthPercentage, Size, TaffyTree, auto, length,
};

use crate::drawstate::{DrawState, LayerId, PALETTE_MAP_IDENTITY};
use crate::position::Vec2;
use crate::system::drawing::Canvas;
use crate::system::{ConsoleApi, ConsoleHelper, HEIGHT, PrintOptions, StaticSpriteOptions, WIDTH};

/// Re-exported so consumers can write `Style { .. }` literals (with the
/// [`row`]/[`column`]/[`size`]/[`pad`] helpers) and node-building helpers
/// without depending on `taffy` directly.
pub use taffy::prelude::{NodeId, Style};

/// What a leaf node renders. Containers use [`Content::None`] and rely on their
/// [`Decoration`] plus their children.
pub enum Content {
    /// Nothing — a pure layout/decoration box.
    None,
    /// A single line of text. `color` is a palette index; `center` draws it
    /// centred on the node (otherwise left-aligned at the node's top-left).
    Text {
        text: String,
        color: u8,
        center: bool,
        small: bool,
    },
    /// A sprite from the default indexed sheet. `w`/`h` are in 8px tiles,
    /// `scale` is an integer upscale, `outline` optionally draws a 1px border.
    Sprite {
        id: i32,
        scale: i32,
        w: i32,
        h: i32,
        outline: Option<u8>,
    },
}

/// Optional box decoration drawn behind/around a node's [`Rect`]. Both fields
/// are palette indices, resolved to colours at draw time — so the same tree can
/// be built for hit-testing (no `DrawState` needed) and for drawing.
#[derive(Default, Clone, Copy)]
pub struct Decoration {
    pub fill: Option<u8>,
    pub outline: Option<u8>,
}

impl Decoration {
    pub fn fill(c: u8) -> Self {
        Self {
            fill: Some(c),
            outline: None,
        }
    }
    pub fn outlined(fill: u8, outline: u8) -> Self {
        Self {
            fill: Some(fill),
            outline: Some(outline),
        }
    }
}

struct NodeData<K> {
    key: Option<K>,
    content: Content,
    deco: Decoration,
}

/// An absolute, integer pixel rectangle in the 240×136 screen space. (Not
/// [`crate::position::Hitbox`], whose `new` panics on zero-size boxes — layout
/// containers are routinely empty.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

impl Rect {
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }
    pub fn center_x(&self) -> i16 {
        self.x + self.w / 2
    }
    pub fn center_y(&self) -> i16 {
        self.y + self.h / 2
    }
}

/// Builds a Taffy tree of keyed, decorated nodes, then [`UiBuilder::finish`]es
/// into a laid-out [`Ui`].
pub struct UiBuilder<K> {
    tree: TaffyTree<NodeData<K>>,
}

impl<K: Copy + PartialEq> UiBuilder<K> {
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::<NodeData<K>>::new(),
        }
    }

    /// Add a leaf node (no children).
    pub fn leaf(
        &mut self,
        style: Style,
        content: Content,
        deco: Decoration,
        key: Option<K>,
    ) -> NodeId {
        self.tree
            .new_leaf_with_context(style, NodeData { key, content, deco })
            .expect("taffy new_leaf")
    }

    /// Add a container node wrapping `children`.
    pub fn container(
        &mut self,
        style: Style,
        deco: Decoration,
        key: Option<K>,
        children: &[NodeId],
    ) -> NodeId {
        let node = self
            .tree
            .new_with_children(style, children)
            .expect("taffy new_with_children");
        self.tree
            .set_node_context(
                node,
                Some(NodeData {
                    key,
                    content: Content::None,
                    deco,
                }),
            )
            .expect("taffy set_node_context");
        node
    }

    /// Compute layout from `root` and resolve every node to an absolute [`Rect`].
    pub fn finish(mut self, root: NodeId) -> Ui<K> {
        self.tree
            .compute_layout(
                root,
                Size {
                    width: AvailableSpace::Definite(WIDTH as f32),
                    height: AvailableSpace::Definite(HEIGHT as f32),
                },
            )
            .expect("taffy compute_layout");
        let mut resolved = Vec::new();
        resolve(&self.tree, root, 0, 0, &mut resolved);
        Ui {
            tree: self.tree,
            resolved,
        }
    }
}

impl<K: Copy + PartialEq> Default for UiBuilder<K> {
    fn default() -> Self {
        Self::new()
    }
}

struct Resolved<K> {
    key: Option<K>,
    rect: Rect,
    node: NodeId,
}

/// Depth-first pre-order walk accumulating absolute offsets. Pre-order is the
/// natural back-to-front paint order (a container draws before its children).
fn resolve<K: Copy>(
    tree: &TaffyTree<NodeData<K>>,
    node: NodeId,
    ox: i32,
    oy: i32,
    out: &mut Vec<Resolved<K>>,
) {
    let layout = tree.layout(node).expect("taffy layout");
    // Taffy rounds layout to integers by default, so these casts are exact.
    let x = ox + layout.location.x as i32;
    let y = oy + layout.location.y as i32;
    let rect = Rect {
        x: x as i16,
        y: y as i16,
        w: layout.size.width as i16,
        h: layout.size.height as i16,
    };
    let key = tree.get_node_context(node).and_then(|d| d.key);
    out.push(Resolved { key, rect, node });
    for child in tree.children(node).expect("taffy children") {
        resolve(tree, child, x, y, out);
    }
}

/// A laid-out UI: an absolute [`Rect`] per node, queryable by key and renderable
/// in one pass.
pub struct Ui<K: Copy + PartialEq> {
    tree: TaffyTree<NodeData<K>>,
    resolved: Vec<Resolved<K>>,
}

impl<K: Copy + PartialEq> Ui<K> {
    /// The topmost keyed node under `point`, if any. Iterates front-to-back so
    /// nested children win over their containers.
    pub fn hit(&self, point: Vec2) -> Option<K> {
        self.resolved
            .iter()
            .rev()
            .find_map(|r| r.key.filter(|_| r.rect.contains(point)))
    }

    /// The absolute rect of the first node carrying `key`.
    pub fn rect(&self, key: K) -> Option<Rect> {
        self.resolved
            .iter()
            .find(|r| r.key == Some(key))
            .map(|r| r.rect)
    }

    /// Render every node's decoration then content onto `layer`, back-to-front.
    pub fn draw(&self, draw_state: &mut DrawState, system: &mut impl ConsoleApi, layer: LayerId) {
        for r in &self.resolved {
            let Some(data) = self.tree.get_node_context(r.node) else {
                continue;
            };
            draw_deco(draw_state, layer, r.rect, data.deco);
            match &data.content {
                Content::None => {}
                Content::Text {
                    text,
                    color,
                    center,
                    small,
                } => {
                    let colour = draw_state.colour(*color);
                    let opts = PrintOptions {
                        color: *color as i32,
                        small_text: *small,
                        ..Default::default()
                    };
                    // 1px top margin so glyphs sit just below the box/highlight
                    // edge, matching the original hand-laid menus.
                    let ty = i32::from(r.rect.y) + 1;
                    let canvas = draw_state.rgba(layer);
                    if *center {
                        system.print_to_centered(
                            canvas,
                            text,
                            r.rect.center_x() as i32,
                            ty,
                            colour,
                            opts,
                        );
                    } else {
                        system.print_to(canvas, text, r.rect.x as i32, ty, colour, opts);
                    }
                }
                Content::Sprite {
                    id,
                    scale,
                    w,
                    h,
                    outline,
                } => {
                    let opts = StaticSpriteOptions {
                        transparent: &[0],
                        scale: *scale,
                        w: *w,
                        h: *h,
                        ..Default::default()
                    };
                    let (x, y) = (r.rect.x as i32, r.rect.y as i32);
                    match outline {
                        Some(oc) => draw_state.spr_with_outline(
                            layer,
                            &PALETTE_MAP_IDENTITY,
                            *id,
                            x,
                            y,
                            opts,
                            *oc,
                        ),
                        None => draw_state.spr(layer, &PALETTE_MAP_IDENTITY, *id, x, y, opts),
                    }
                }
            }
        }
    }
}

/// Paint a [`Decoration`] (fill and/or 1px outline) over `rect`.
fn draw_deco(draw_state: &mut DrawState, layer: LayerId, rect: Rect, deco: Decoration) {
    let (x, y, w, h) = (rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32);
    match (deco.fill, deco.outline) {
        (Some(f), Some(o)) => {
            let cf = draw_state.colour(f);
            let co = draw_state.colour(o);
            draw_state.rgba(layer).outlined_rect(x, y, w, h, cf, co);
        }
        (Some(f), None) => {
            let cf = draw_state.colour(f);
            draw_state.rgba(layer).fill_rect(x, y, w, h, cf);
        }
        (None, Some(o)) => {
            let co = draw_state.colour(o);
            draw_state.rgba(layer).stroke_rect(x, y, w, h, co);
        }
        (None, None) => {}
    }
}

// --- Style helpers (return concrete `Style` so type inference for the generic
// `Style<S = DefaultCheapStr>` is anchored) -----------------------------------

/// A horizontal flex container with `gap` px between children.
pub fn row(gap: f32) -> Style {
    Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        gap: Size {
            width: length(gap),
            height: length(0.0),
        },
        ..Default::default()
    }
}

/// A vertical flex container with `gap` px between children.
pub fn column(gap: f32) -> Style {
    Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        gap: Size {
            width: length(0.0),
            height: length(gap),
        },
        ..Default::default()
    }
}

/// A horizontal flex container whose children keep their natural heights and
/// align to the top edge, rather than stretching to the tallest sibling.
pub fn row_top(gap: f32) -> Style {
    Style {
        align_items: Some(AlignItems::FlexStart),
        ..row(gap)
    }
}

/// A horizontal flex container that wraps onto new rows — give it a fixed width
/// ([`size`]) and fixed-size children to get a grid.
pub fn wrap_row(gap: f32) -> Style {
    Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        gap: Size {
            width: length(gap),
            height: length(gap),
        },
        ..Default::default()
    }
}

/// Centre this container's single child in both axes (used to centre a panel on
/// the screen).
pub fn centered() -> Style {
    Style {
        display: Display::Flex,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        ..Default::default()
    }
}

/// A fixed pixel size for `Style.size`.
pub fn size(w: f32, h: f32) -> Size<Dimension> {
    Size {
        width: length(w),
        height: length(h),
    }
}

/// A fixed width with automatic (stretch/content) height for `Style.size`.
pub fn width(w: f32) -> Size<Dimension> {
    Size {
        width: length(w),
        height: auto(),
    }
}

/// An automatic width (stretches to the parent's cross size) with fixed height —
/// the shape of a full-width list row.
pub fn full_width(h: f32) -> Size<Dimension> {
    Size {
        width: auto(),
        height: length(h),
    }
}

/// Uniform padding on all four sides, for `Style.padding`.
pub fn pad(p: f32) -> TaffyRect<LengthPercentage> {
    TaffyRect {
        left: length(p),
        right: length(p),
        top: length(p),
        bottom: length(p),
    }
}

/// Per-side padding (left, right, top, bottom), for `Style.padding`.
pub fn pad_lrtb(l: f32, r: f32, t: f32, b: f32) -> TaffyRect<LengthPercentage> {
    TaffyRect {
        left: length(l),
        right: length(r),
        top: length(t),
        bottom: length(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A menu-shaped column (full-width rows under a top pad) resolves to the
    /// expected absolute rects, and hit-testing picks the row under a point —
    /// this is exactly what gives the menus mouse control.
    #[test]
    fn column_layout_and_hit_testing() {
        let mut b: UiBuilder<usize> = UiBuilder::new();
        let rows: Vec<_> = (0..3)
            .map(|i| {
                b.leaf(
                    Style {
                        size: full_width(8.0),
                        ..Default::default()
                    },
                    Content::None,
                    Decoration::default(),
                    Some(i),
                )
            })
            .collect();
        let root = b.container(
            Style {
                size: size(240.0, 136.0),
                padding: pad_lrtb(0.0, 0.0, 40.0, 0.0),
                ..column(0.0)
            },
            Decoration::default(),
            None,
            &rows,
        );
        let ui = b.finish(root);

        // Rows stretch to the full 240px width and stack 8px apart from y=40.
        for i in 0..3 {
            let r = ui.rect(i).expect("row rect");
            assert_eq!((r.x, r.w, r.h), (0, 240, 8), "row {i} box");
            assert_eq!(r.y, 40 + i as i16 * 8, "row {i} y");
        }

        // A point resolves to the row containing it...
        assert_eq!(ui.hit(Vec2::new(120, 41)), Some(0));
        assert_eq!(ui.hit(Vec2::new(10, 49)), Some(1));
        assert_eq!(ui.hit(Vec2::new(239, 58)), Some(2));
        // ...and the unkeyed root padding / off-panel area hits nothing.
        assert_eq!(ui.hit(Vec2::new(120, 5)), None);
        assert_eq!(ui.hit(Vec2::new(120, 135)), None);
    }

    /// `centered()` places a fixed-size panel in the middle of the viewport,
    /// replacing the old manual `(WIDTH - total) / 2` offset arithmetic.
    #[test]
    fn centered_panel_is_centered() {
        let mut b: UiBuilder<()> = UiBuilder::new();
        let panel = b.leaf(
            Style {
                size: size(100.0, 40.0),
                ..Default::default()
            },
            Content::None,
            Decoration::default(),
            None,
        );
        let root = b.container(
            Style {
                size: size(240.0, 136.0),
                ..centered()
            },
            Decoration::default(),
            None,
            &[panel],
        );
        let ui = b.finish(root);
        // Resolved list is [root, panel]; the panel is centred: (240-100)/2, (136-40)/2.
        let panel_rect = ui.resolved.last().unwrap().rect;
        assert_eq!((panel_rect.x, panel_rect.y), (70, 48));
        assert_eq!((panel_rect.w, panel_rect.h), (100, 40));
    }
}
