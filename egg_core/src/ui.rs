//! A small, immediate-mode UI layer over the [Taffy](https://docs.rs/taffy)
//! flexbox engine.
//!
//! The console draws to indexed/RGBA canvases with hand-written pixel
//! coordinates. This module replaces that manual arithmetic for menu-like
//! UIs: you describe a tree of styled boxes (text, sprites, containers), Taffy
//! computes an absolute pixel [`Rect`] for each against the live framebuffer
//! size (240×136 is just the base resolution), and then you get two passes for
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
use crate::system::{ConsoleApi, ConsoleHelper, PrintOptions, SpriteOptions};

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

/// An absolute, integer pixel rectangle in screen space — the live framebuffer
/// the layout was computed against, not the fixed 240×136 base resolution. (Not
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

    // --- Fluent node constructors --------------------------------------------
    //
    // These return a [`Node`] that accumulates style/decoration/key through
    // chained, defaulted modifiers and commits to the tree on [`Node::id`]. They
    // sit on top of [`leaf`](Self::leaf)/[`container`](Self::container) so a call
    // site reads as the node's shape rather than a `Style { .. }` literal.

    /// Start a [`Node`] with the given content and leaf/container role.
    fn node(&mut self, content: Content, container: bool) -> Node<'_, K> {
        Node {
            builder: self,
            style: Style::default(),
            content,
            deco: Decoration::default(),
            key: None,
            children: Vec::new(),
            container,
        }
    }

    /// A container [`Node`] with a preset flex `style`, wrapping `children`.
    fn stack(&mut self, style: Style, children: impl IntoIterator<Item = NodeId>) -> Node<'_, K> {
        let children: Vec<NodeId> = children.into_iter().collect();
        let mut node = self.node(Content::None, true);
        node.style = style;
        node.children = children;
        node
    }

    /// A single line of text: palette colour 12, left-aligned, large font —
    /// override via [`color`](Node::color)/[`small`](Node::small)/[`center`](Node::center).
    pub fn text(&mut self, text: impl Into<String>) -> Node<'_, K> {
        self.node(
            Content::Text { text: text.into(), color: 12, center: false, small: false },
            false,
        )
    }

    /// A `w`×`h`-tile sprite from the default sheet at scale 1 — override via
    /// [`scale`](Node::scale)/[`sprite_outline`](Node::sprite_outline).
    pub fn sprite(&mut self, id: i32, w: i32, h: i32) -> Node<'_, K> {
        self.node(Content::Sprite { id, scale: 1, w, h, outline: None }, false)
    }

    /// An empty full-width box of fixed `height` — vertical spacing in a column.
    pub fn spacer(&mut self, height: f32) -> Node<'_, K> {
        let mut node = self.node(Content::None, false);
        node.style.size = full_width(height);
        node
    }

    /// A horizontal flex row, `gap` px between `children`.
    pub fn row(&mut self, gap: f32, children: impl IntoIterator<Item = NodeId>) -> Node<'_, K> {
        self.stack(row(gap), children)
    }

    /// A horizontal row whose children keep their natural heights, top-aligned.
    pub fn row_top(&mut self, gap: f32, children: impl IntoIterator<Item = NodeId>) -> Node<'_, K> {
        self.stack(row_top(gap), children)
    }

    /// A vertical flex column, `gap` px between `children`.
    pub fn column(&mut self, gap: f32, children: impl IntoIterator<Item = NodeId>) -> Node<'_, K> {
        self.stack(column(gap), children)
    }

    /// A wrapping row — give it a fixed [`width`](Node::width) and fixed-size
    /// children to get a grid.
    pub fn wrap_row(&mut self, gap: f32, children: impl IntoIterator<Item = NodeId>) -> Node<'_, K> {
        self.stack(wrap_row(gap), children)
    }

    /// Centre `child` in both axes (used to centre a panel on screen).
    pub fn centered(&mut self, child: NodeId) -> Node<'_, K> {
        self.stack(centered(), [child])
    }

    /// A bare flex box wrapping `children` — default layout, for slots and
    /// single-child wrappers that only carry size/decoration.
    pub fn boxed(&mut self, children: impl IntoIterator<Item = NodeId>) -> Node<'_, K> {
        self.stack(Style::default(), children)
    }

    /// Compute layout from `root` and resolve every node to an absolute [`Rect`].
    /// `avail` is the screen size (px) the root lays out within — pass the live
    /// [`ConsoleApi::width`]/[`height`](crate::system::ConsoleApi::height) so the
    /// UI fills the framebuffer at any resolution.
    pub fn finish(mut self, root: NodeId, avail: (f32, f32)) -> Ui<K> {
        self.tree
            .compute_layout(
                root,
                Size {
                    width: AvailableSpace::Definite(avail.0),
                    height: AvailableSpace::Definite(avail.1),
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

/// A node under construction, returned by the [`UiBuilder`] constructors
/// ([`text`](UiBuilder::text), [`row`](UiBuilder::row), …). Configure it with
/// chained, defaulted modifiers, then [`id`](Self::id) inserts it into the tree
/// and yields its [`NodeId`] for use as a parent's child:
///
/// ```ignore
/// let row = b.text("Items").full_width(8.0).fill_if(selected, 1).key(k).id();
/// ```
///
/// Text/sprite modifiers are no-ops on the wrong node kind, so chains stay flat.
pub struct Node<'a, K: Copy + PartialEq> {
    builder: &'a mut UiBuilder<K>,
    style: Style,
    content: Content,
    deco: Decoration,
    key: Option<K>,
    children: Vec<NodeId>,
    container: bool,
}

impl<K: Copy + PartialEq> Node<'_, K> {
    /// Fixed `w`×`h` px.
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.style.size = size(w, h);
        self
    }
    /// Fixed width, automatic (content/stretch) height.
    pub fn width(mut self, w: f32) -> Self {
        self.style.size = width(w);
        self
    }
    /// Automatic (stretch) width, fixed height — a full-width row.
    pub fn full_width(mut self, h: f32) -> Self {
        self.style.size = full_width(h);
        self
    }
    /// Uniform padding on all four sides.
    pub fn pad(mut self, p: f32) -> Self {
        self.style.padding = pad(p);
        self
    }
    /// Per-side padding (left, right, top, bottom).
    pub fn pad_lrtb(mut self, l: f32, r: f32, t: f32, b: f32) -> Self {
        self.style.padding = pad_lrtb(l, r, t, b);
        self
    }

    /// Fill the box with palette colour `c`.
    pub fn fill(mut self, c: u8) -> Self {
        self.deco.fill = Some(c);
        self
    }
    /// Fill only when `cond` — the common "highlight the selected entry" case.
    pub fn fill_if(self, cond: bool, c: u8) -> Self {
        if cond { self.fill(c) } else { self }
    }
    /// A 1px box outline in palette colour `c`.
    pub fn outline(mut self, c: u8) -> Self {
        self.deco.outline = Some(c);
        self
    }
    /// Fill and outline in one call.
    pub fn outlined(self, fill: u8, outline: u8) -> Self {
        self.fill(fill).outline(outline)
    }

    /// Text colour (palette index); no-op on non-text nodes. Defaults to 12.
    pub fn color(mut self, c: u8) -> Self {
        if let Content::Text { color, .. } = &mut self.content {
            *color = c;
        }
        self
    }
    /// Select the small font; no-op on non-text nodes.
    pub fn small(mut self, small: bool) -> Self {
        if let Content::Text { small: s, .. } = &mut self.content {
            *s = small;
        }
        self
    }
    /// Centre the text within the node; no-op on non-text nodes.
    pub fn center(mut self) -> Self {
        if let Content::Text { center, .. } = &mut self.content {
            *center = true;
        }
        self
    }

    /// Integer upscale for a sprite node; no-op on non-sprite nodes. Defaults to 1.
    pub fn scale(mut self, scale: i32) -> Self {
        if let Content::Sprite { scale: s, .. } = &mut self.content {
            *s = scale;
        }
        self
    }
    /// Draw a 1px outline around the sprite's pixels (e.g. to flag a selection);
    /// no-op on non-sprite nodes. Pass a `then_some`-style `Option` to toggle it.
    pub fn sprite_outline(mut self, outline: Option<u8>) -> Self {
        if let Content::Sprite { outline: o, .. } = &mut self.content {
            *o = outline;
        }
        self
    }

    /// Tag the node so a mouse hit over it resolves to `k`.
    pub fn key(mut self, k: K) -> Self {
        self.key = Some(k);
        self
    }

    /// Insert the configured node into the tree and return its [`NodeId`].
    pub fn id(self) -> NodeId {
        let Node { builder, style, content, deco, key, children, container } = self;
        if container {
            builder.container(style, deco, key, &children)
        } else {
            builder.leaf(style, content, deco, key)
        }
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
                    let opts = SpriteOptions {
                        transparent: Some(0),
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
        let ui = b.finish(root, (240.0, 136.0));

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
        let ui = b.finish(root, (240.0, 136.0));
        // Resolved list is [root, panel]; the panel is centred: (240-100)/2, (136-40)/2.
        let panel_rect = ui.resolved.last().unwrap().rect;
        assert_eq!((panel_rect.x, panel_rect.y), (70, 48));
        assert_eq!((panel_rect.w, panel_rect.h), (100, 40));
    }
}
