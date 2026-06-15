use crate::{
    Ctx,
    data::sound,
    dialogue::Dialogue,
    system::{ConsoleApi, ConsoleHelper, dpad_delta, just_pressed},
    ui::{NodeId, Ui, UiBuilder},
};

/// A persistent item identifier. Stored as a `u8` in [`SaveData::inventory`]
/// (a fixed `[u8; 8]`), so the live inventory can be serialised to and rebuilt
/// from a save. Id `0` is the reserved *empty-slot* sentinel — no real item
/// ever uses it — so a zeroed save array reads back as an empty inventory.
///
/// [`SaveData::inventory`](crate::data::save::SaveData::inventory)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemID(pub u8);
impl ItemID {
    /// The empty-slot sentinel: the id a save stores for a slot holding no item.
    pub const EMPTY: ItemID = ItemID(0);
}

static ITEM_FF: InventoryItem = InventoryItem {
    id: ItemID(1),
    sprite: 513,
    name: "item_ff_name",
    desc: "item_ff_desc",
};
static ITEM_LM: InventoryItem = InventoryItem {
    id: ItemID(2),
    sprite: 514,
    name: "item_lm_name",
    desc: "item_lm_desc",
};
static ITEM_CHEGG: InventoryItem = InventoryItem {
    id: ItemID(3),
    sprite: 524,
    name: "item_chegg_name",
    desc: "item_chegg_desc",
};

/// Every item the game knows about, the single source of truth the registry
/// lookups ([`by_id`], [`by_name`]) scan. Adding an item is a matter of writing
/// one more `static InventoryItem` (with a fresh [`ItemID`]) and listing it here.
static ALL_ITEMS: &[&InventoryItem] = &[&ITEM_FF, &ITEM_LM, &ITEM_CHEGG];

/// Resolve an item by its persistent [`ItemID`] (the `u8` a save stores).
/// `None` for the empty sentinel or an id no item claims — so an old/garbage
/// save id leaves the slot empty rather than crashing. Mirrors
/// [`sound::by_name`](crate::data::sound::by_name) /
/// [`portraits::by_name`](crate::data::portraits::by_name).
pub fn by_id(id: ItemID) -> Option<&'static InventoryItem> {
    ALL_ITEMS.iter().copied().find(|item| item.id == id)
}

/// Resolve an item by its script name (the `name` label key, e.g.
/// `"item_chegg_name"`), for an [`InteractFn`](crate::interact::InteractFn) that
/// names the item it grants. `None` for an unknown name.
pub fn by_name(name: &str) -> Option<&'static InventoryItem> {
    ALL_ITEMS.iter().copied().find(|item| item.name == name)
}

#[derive(Debug)]
pub struct InventoryItem {
    /// This item's persistent id, the `u8` a save stores for it (see [`ItemID`]).
    pub id: ItemID,
    pub sprite: i32,
    pub name: &'static str,
    pub desc: &'static str,
}
impl InventoryItem {
    pub const fn new(id: ItemID, sprite: i32, name: &'static str, desc: &'static str) -> Self {
        Self {
            id,
            sprite,
            name,
            desc,
        }
    }
}

pub struct Inventory {
    pub items: [Option<&'static InventoryItem>; 8],
    pub unlocks: [bool; 4],
}
impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            items: [
                Some(&ITEM_FF),
                Some(&ITEM_LM),
                Some(&ITEM_CHEGG),
                None,
                None,
                None,
                None,
                None,
            ],
            unlocks: [false; 4],
        }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }
    pub fn take(&mut self, index: usize) -> Option<&'static InventoryItem> {
        if let Some(slot) = self.items.get_mut(index) {
            if slot.is_some() { slot.take() } else { None }
        } else {
            None
        }
    }
    /// Place `item` in the first empty slot. Returns `true` if it fit, `false`
    /// if the inventory is full — the caller decides what a full inventory means
    /// (today: nothing happens), so this never panics or drops the player's
    /// existing items.
    pub fn add(&mut self, item: &'static InventoryItem) -> bool {
        if let Some(slot) = self.items.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(item);
            true
        } else {
            false
        }
    }
    /// The slot contents as the persistent `[u8; 8]` a save stores: each slot's
    /// [`ItemID`], or [`ItemID::EMPTY`] (`0`) for an empty slot. Inverse of
    /// [`load_from_save_ids`](Self::load_from_save_ids).
    pub fn to_save_ids(&self) -> [u8; 8] {
        let mut ids = [ItemID::EMPTY.0; 8];
        for (slot, out) in self.items.iter().zip(ids.iter_mut()) {
            if let Some(item) = slot {
                *out = item.id.0;
            }
        }
        ids
    }
    /// Repopulate the slots from a save's `[u8; 8]` of [`ItemID`]s, resolving
    /// each through [`by_id`]. The empty sentinel or an id no item claims (an
    /// old/garbage save) leaves that slot empty. Inverse of
    /// [`to_save_ids`](Self::to_save_ids).
    pub fn load_from_save_ids(&mut self, ids: [u8; 8]) {
        for (out, id) in self.items.iter_mut().zip(ids) {
            *out = by_id(ItemID(id));
        }
    }
}

pub enum InventoryUiState {
    PageSelect(i32),
    Items(usize, Option<(usize, &'static InventoryItem)>),
    Eggs(usize),
    Options,
    Close,
}
impl InventoryUiState {
    pub fn page(&self) -> i32 {
        match self {
            Self::PageSelect(x) => *x,
            Self::Items(_, _) => 0,
            Self::Eggs(_) => 1,
            Self::Options => 2,
            Self::Close => 3,
        }
    }
    pub fn change(&mut self, system: &mut impl ConsoleApi) {
        system.play_sound(sound::CLICK);
        match self {
            Self::PageSelect(0) => *self = Self::Items(0, None),
            Self::PageSelect(1) => *self = Self::Eggs(0),
            Self::PageSelect(2) => *self = Self::Options,
            Self::PageSelect(3) => *self = Self::Close,
            _ => *self = Self::PageSelect(self.page()),
        };
    }
    pub fn back(&mut self, system: &mut impl ConsoleApi) {
        match self {
            Self::PageSelect(_) => {
                system.play_sound(sound::INTERACT);
                *self = Self::Close
            }
            Self::Close => (),
            _ => self.change(system),
        }
    }
    pub fn arrows(&mut self, system: &mut impl ConsoleApi, dx: i32, dy: i32) {
        match self {
            Self::PageSelect(i) => {
                if dx != 0 || dy != 0 {
                    system.play_sound(sound::CLICK);
                };
                *i = (*i + dy % 3).clamp(0, 3);
                if dx == 1 {
                    self.change(system)
                };
            }
            Self::Items(i, _) => {
                if (*i == 0 || *i == 4) && dx == -1 {
                    self.back(system);
                    return;
                };
                let dx = if *i == 3 { dx.min(0) } else { dx };
                let new = *i as i32 + dx + dy * 4;
                if (0..8).contains(&new) {
                    *i = new as usize;
                };
            }
            Self::Eggs(i) => {
                if *i == 0 && dx == -1 {
                    self.back(system);
                    return;
                };
                *i = (*i as i32 + dx).clamp(0, 3) as usize;
            }
            _ => (),
        }
    }
}

/// Identifies the interactive boxes of the inventory layout, so a mouse hit
/// resolves to exactly the page label, item slot, or egg slot under the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvKey {
    /// One of the four side-column page labels (0=Items, 1=Eggs, 2=Options, 3=Back).
    Page(usize),
    /// An item slot on the Items page (`0..8`).
    Slot(usize),
    /// An egg slot on the Eggs page (`0..4`).
    Egg(usize),
}

/// A 20×20 item/egg slot: an outlined box keyed for hit-testing, wrapping an
/// optional pre-built 16×16 sprite `child` inset 2px by the padding.
fn slot(b: &mut UiBuilder<InvKey>, key: InvKey, outline: u8, child: Option<NodeId>) -> NodeId {
    b.boxed(child)
        .size(20.0, 20.0)
        .pad(2.0)
        .outlined(0, outline)
        .key(key)
        .id()
}

/// The Taffy panel's fixed height (px): the side column + slot grid. Centred
/// vertically on the framebuffer, so the title and the item-name box position
/// themselves relative to it.
const PANEL_H: f32 = 47.0;

pub struct InventoryUi {
    pub inventory: Inventory,
    pub state: InventoryUiState,
    pub dialogue: Dialogue,
}
impl Default for InventoryUi {
    fn default() -> Self {
        Self::new()
    }
}

impl InventoryUi {
    pub fn new() -> Self {
        Self {
            inventory: Inventory::new(),
            state: InventoryUiState::PageSelect(0),
            dialogue: Dialogue::default(),
        }
    }
    pub fn open(&mut self, system: &mut impl ConsoleApi) {
        system.play_sound(sound::INTERACT);
        self.state = InventoryUiState::PageSelect(0);
    }
    pub fn click(&mut self, system: &mut impl ConsoleApi) {
        match &mut self.state {
            InventoryUiState::PageSelect(_) => self.state.change(system),
            InventoryUiState::Items(new_index, selected_item) => {
                if let Some((old_index, id)) = selected_item {
                    // Put item back down
                    if old_index == new_index {
                        system.play_sound(sound::ITEM_DOWN);
                        *selected_item = None;
                        return;
                    };

                    // Swap items, pick up swapped item if present.
                    self.inventory.swap(*new_index, *old_index);
                    if let Some(Some(x)) = self.inventory.items.get(*old_index) {
                        system.play_sound(sound::ITEM_SWAP);
                        *id = *x;
                    } else {
                        system.play_sound(sound::ITEM_DOWN);
                        *selected_item = None;
                    };
                } else {
                    // Pick up item
                    if let Some(Some(x)) = self.inventory.items.get(*new_index) {
                        system.play_sound(sound::ITEM_UP);
                        *selected_item = Some((*new_index, *x));
                    } else {
                        system.play_sound(sound::DENY);
                    };
                }
            }
            InventoryUiState::Eggs(_index) => {
                system.play_sound(sound::DENY);
            }
            _ => (),
        }
    }
    /// Lay out the inventory panel — a centred row of the side page-column and
    /// a page-specific main area — with Taffy. Rebuilt each frame and used for
    /// both hit-testing (`step`) and drawing (`draw`). Every label/slot carries
    /// an [`InvKey`] so a mouse hit resolves straight to the thing under it. A
    /// pure read-only builder: it only reads `ctx` (the save's small-text flag,
    /// the screen size, font metrics, the script), so it takes `&Ctx` rather
    /// than the old `&mut ConsoleApi` (which it needed only for `memory()`).
    pub fn build_ui<S: ConsoleApi>(&self, ctx: &Ctx<S>) -> Ui<InvKey> {
        use crate::system::PrintOptions;

        // Original fixed dimensions, kept so the panel centres exactly as before
        // (item slot stride was `2*8 + 5 = 21`: a 20px box with a 1px gap).
        // `PANEL_H` is a module const so `draw` can place the title above it.
        const MAIN_W: f32 = 89.0;

        let small = ctx.save.small_text_on;
        let body_opts = PrintOptions {
            color: 12,
            small_text: small,
            ..Default::default()
        };
        // Centre against the render target (the framebuffer drawn into), so the
        // panel re-centres at any window size — and the hit-test pass in `step`
        // and the draw pass agree on the layout.
        let (sw, sh) = ctx.draw.size();
        let screen = (sw as f32, sh as f32);
        let page = self.state.page();
        let page_select = matches!(self.state, InventoryUiState::PageSelect(_));
        // While choosing a page the side column is highlighted (palette +2); once
        // inside a page the main area is highlighted instead.
        let col_c: u8 = if page_select { 2 } else { 0 };
        let main_c: u8 = if page_select { 0 } else { 2 };
        let dragging_from = match &self.state {
            InventoryUiState::Items(_, Some((old, _))) => Some(*old),
            _ => None,
        };

        let mut b = UiBuilder::new();

        // --- Side column: the four page labels. ---
        let labels = [
            ctx.script.label("inventory_items"),
            ctx.script.label("inventory_shell"),
            ctx.script.label("inventory_options"),
            ctx.script.label("inventory_back"),
        ];
        let label_w = labels
            .iter()
            .map(|s| ctx.system.text_width(s, body_opts.clone()))
            .max()
            .unwrap_or(0);
        let label_nodes: Vec<NodeId> = labels
            .iter()
            .enumerate()
            .map(|(i, s)| {
                b.text(s.as_str())
                    .small(small)
                    .full_width(8.0)
                    .fill_if(i as i32 == page, col_c + 1)
                    .key(InvKey::Page(i))
                    .id()
            })
            .collect();
        let side = b
            .column(0.0, label_nodes)
            .width((label_w + 5) as f32)
            .pad_lrtb(2.0, 2.0, 1.0, 1.0)
            .outlined(col_c, col_c + 1)
            .id();

        // --- Main area: a slot grid (Items/Eggs) or a hint box (Options/Back). ---
        let main = match page {
            0 => {
                let slots: Vec<NodeId> = self
                    .inventory
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        // The slot we're currently dragging from is left empty;
                        // the floating item is drawn over the cursor in `draw`.
                        let child = match item {
                            Some(item) if dragging_from != Some(i) => {
                                Some(b.sprite(item.sprite, 1, 1).scale(2).size(16.0, 16.0).id())
                            }
                            _ => None,
                        };
                        slot(&mut b, InvKey::Slot(i), main_c + 1, child)
                    })
                    .collect();
                b.wrap_row(1.0, slots)
                    .width(MAIN_W)
                    .pad_lrtb(3.0, 2.0, 3.0, 3.0)
                    .outlined(main_c, main_c + 1)
                    .id()
            }
            1 => {
                let slots: Vec<NodeId> = (0..4)
                    .map(|i| {
                        let egg = b.sprite(534, 2, 2).size(16.0, 16.0).id();
                        slot(&mut b, InvKey::Egg(i), main_c + 1, Some(egg))
                    })
                    .collect();
                b.wrap_row(1.0, slots)
                    .width(MAIN_W)
                    .pad_lrtb(3.0, 2.0, 3.0, 3.0)
                    .outlined(main_c, main_c + 1)
                    .id()
            }
            n => {
                let hint = if n == 2 {
                    "Open options menu"
                } else {
                    "Back to world"
                };
                let hint_w = ctx.system.text_width(hint, body_opts.clone());
                let text_node = b.text(hint).small(small).size(hint_w as f32, 8.0).id();
                let hint_box = b
                    .boxed([text_node])
                    .size((hint_w + 3) as f32, 10.0)
                    .pad_lrtb(2.0, 0.0, 1.0, 0.0)
                    .outlined(col_c, col_c + 1)
                    .id();
                // Reserve the full main width so the side column keeps its x, and
                // drop the hint box level with the selected page label.
                b.column(0.0, [hint_box])
                    .width(MAIN_W)
                    .pad_lrtb(0.0, 0.0, (n * 8) as f32, 0.0)
                    .id()
            }
        };

        // --- Panel (side + main), centred on the 240×136 screen by Taffy. ---
        let panel = b.row_top(2.0, [side, main]).full_width(PANEL_H).id();
        let root = b.centered(panel).size(screen.0, screen.1).id();
        b.finish(root, screen)
    }
    pub fn draw(&self, ctx: &mut Ctx<impl ConsoleApi>) {
        use crate::drawstate::{LayerId::*, PALETTE_MAP_IDENTITY};
        use crate::system::drawing::image::{Rgba, RgbaImage};
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::{PrintOptions, SpriteOptions};

        let small = ctx.save.small_text_on;
        let body_opts = PrintOptions {
            color: 12,
            small_text: small,
            ..Default::default()
        };
        let black = ctx.draw.colour(0);
        let white = ctx.draw.colour(12);
        let c2 = ctx.draw.colour(2);
        let c3 = ctx.draw.colour(3);

        // Foreground starts clear each frame; everything here draws onto it.
        ctx.draw.rgba(FG).fill(Rgba::TRANSPARENT);

        // Title, white with a 1px black shadow. Centred on the framebuffer width
        // and kept its canonical 7px gap above the (vertically-centred) grid, so
        // it tracks the panel instead of floating off when the window grows.
        let inventory_title = ctx.label("inventory_title");
        let (cw, ch) = ctx.draw.size();
        let cx = cw / 2;
        let title_y = (ch - PANEL_H as i32) / 2 - 7;
        ctx.system.print_to_centered(
            ctx.draw.rgba(FG),
            &inventory_title,
            cx + 1,
            title_y + 1,
            black,
            body_opts.clone(),
        );
        ctx.system.print_to_centered(
            ctx.draw.rgba(FG),
            &inventory_title,
            cx,
            title_y,
            white,
            body_opts.clone(),
        );

        // Lay out and draw the whole panel in one pass...
        let ui = self.build_ui(&*ctx);
        ui.draw(ctx.draw, ctx.system, FG);

        // ...then overlay the state-specific bits using the laid-out rects.
        match &self.state {
            InventoryUiState::Items(current, selected) => {
                if let Some(slot) = ui.rect(InvKey::Slot(*current)) {
                    ctx.draw.rgba(FG).stroke_rect(
                        slot.x.into(),
                        slot.y.into(),
                        slot.w.into(),
                        slot.h.into(),
                        white,
                    );
                    if let Some((_, item)) = selected {
                        // Picked-up item floats 4px above its cursor slot, outlined.
                        ctx.draw.spr_with_outline(
                            FG,
                            &PALETTE_MAP_IDENTITY,
                            item.sprite,
                            i32::from(slot.x) + 2,
                            i32::from(slot.y) + 2 - 4,
                            SpriteOptions {
                                scale: 2,
                                transparent: Some(0),
                                ..Default::default()
                            },
                            12,
                        );
                    }
                }
                let name = match selected {
                    Some((_, item)) => Some(item.name),
                    None => self.inventory.items[*current].map(|item| item.name),
                };
                if let Some(name) = name {
                    let name = ctx.label(name);
                    // Glued to the description box's portrait (the item dialogue
                    // below): the portrait's left edge is `(cw - width)/2 - 13`
                    // (x 7 at the base width) and the box is bottom-anchored, so a
                    // 38px bottom margin gives y 98 at the base height. Tracking the
                    // same expressions keeps the name tab on the box in BOTH axes.
                    let (cw, ch) = ctx.draw.size();
                    let nx = (cw - self.dialogue.width as i32) / 2 - 13;
                    let ny = ch - 38;
                    ctx.draw.rgba(FG).outlined_rect(nx, ny, 70, 9, c2, c3);
                    ctx.system.print_to(
                        ctx.draw.rgba(FG),
                        &name,
                        nx + 2,
                        ny + 2,
                        white,
                        body_opts.clone(),
                    );
                }
            }
            InventoryUiState::Eggs(current) => {
                if let Some(slot) = ui.rect(InvKey::Egg(*current)) {
                    ctx.draw.rgba(FG).stroke_rect(
                        slot.x.into(),
                        slot.y.into(),
                        slot.w.into(),
                        slot.h.into(),
                        white,
                    );
                }
            }
            _ => {}
        }

        // Description portrait for the held or hovered item.
        if let InventoryUiState::Items(current, selected) = &self.state {
            let item = match selected {
                Some((_, item)) => Some(*item),
                None => self.inventory.items[*current],
            };
            if let Some(item) = item {
                let desc = ctx.label(item.desc);
                let string = self.dialogue.fit_text(ctx.system, small, &desc);
                self.dialogue.draw_dialogue_portrait(
                    ctx.draw,
                    FG,
                    ctx.system,
                    small,
                    &string,
                    false,
                    item.sprite,
                    3,
                    1,
                    1,
                );
            }
        }

        // Composite background then foreground into the output image.
        let output = ctx.system.output_image();
        output.blit::<RgbaImage>(
            0,
            0,
            &ctx.draw.rgba_canvas[BG as usize],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
        output.blit::<RgbaImage>(
            0,
            0,
            &ctx.draw.rgba_canvas[FG as usize],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
    }
    pub fn step(&mut self, ctx: &mut Ctx<impl ConsoleApi>) {
        // --- Mouse: hover moves the cursor, left-click acts, right-click backs out. ---
        let ui = self.build_ui(&*ctx);
        let mouse = ctx.system.mouse();
        let mut mouse_clicked = false;
        if let Some(key) = ui.hit(mouse.pos()) {
            match key {
                InvKey::Page(i) => {
                    if mouse.moved() {
                        self.state = InventoryUiState::PageSelect(i as i32);
                    }
                    if just_pressed(mouse.left) {
                        self.state = InventoryUiState::PageSelect(i as i32);
                        self.state.change(ctx.system);
                        mouse_clicked = true;
                    }
                }
                InvKey::Slot(i) => {
                    let drag = match &self.state {
                        InventoryUiState::Items(_, sel) => *sel,
                        _ => None,
                    };
                    if mouse.moved() {
                        self.state = InventoryUiState::Items(i, drag);
                    }
                    if just_pressed(mouse.left) {
                        self.state = InventoryUiState::Items(i, drag);
                        self.click(ctx.system);
                        mouse_clicked = true;
                    }
                }
                InvKey::Egg(i) => {
                    if mouse.moved() {
                        self.state = InventoryUiState::Eggs(i);
                    }
                    if just_pressed(mouse.left) {
                        self.state = InventoryUiState::Eggs(i);
                        self.click(ctx.system);
                        mouse_clicked = true;
                    }
                }
            }
        }
        if just_pressed(mouse.right) {
            self.state.back(ctx.system);
        }

        // Keyboard / gamepad navigation
        let pad = ctx.system.controller();
        let (dx, dy) = dpad_delta(&pad, just_pressed);
        self.state.arrows(ctx.system, dx.into(), dy.into());
        if just_pressed(pad.a) && !mouse_clicked {
            self.click(ctx.system)
        };
        if just_pressed(pad.b) {
            self.state.back(ctx.system)
        };
    }
}
