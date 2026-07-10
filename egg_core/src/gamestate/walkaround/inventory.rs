use crate::{
    Ctx,
    data::eggdata::{GameItems, UseDef},
    data::sound,
    platform::{ConsoleApi, ConsoleHelper, dpad_delta, just_pressed},
    render::{print_to_centered_with_font, print_to_with_font},
    ui::dialogue::Dialogue,
    ui::layout::{NodeId, Ui, UiBuilder},
};

#[derive(Clone, Debug)]
pub struct Inventory {
    pub items: [Option<String>; 8],
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
                Some("ff".into()),
                Some("lm".into()),
                Some("chegg".into()),
                None,
                None,
                None,
                None,
                None,
            ],
        }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }
    pub fn take(&mut self, index: usize) -> Option<String> {
        if let Some(slot) = self.items.get_mut(index) {
            if slot.is_some() { slot.take() } else { None }
        } else {
            None
        }
    }
    /// The item key in slot `index`, or `None` for an empty/out-of-range slot.
    pub fn get(&self, index: usize) -> Option<&str> {
        self.items.get(index).and_then(|s| s.as_deref())
    }
    /// Place item `key` in the first empty slot. Returns `true` if it fit,
    /// `false` if the inventory is full — the caller decides what a full
    /// inventory means (today: nothing happens), so this never panics or drops
    /// the player's existing items.
    pub fn add(&mut self, key: String) -> bool {
        if let Some(slot) = self.items.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(key);
            true
        } else {
            false
        }
    }
    /// The slot contents as the persistent `[Option<String>; 8]` a save stores:
    /// each slot's item key, or `None` for an empty slot. Inverse of
    /// [`load_from_save`](Self::load_from_save).
    pub fn to_save(&self) -> [Option<String>; 8] {
        self.items.clone()
    }
    /// Repopulate the slots from a save's `[Option<String>; 8]` of item keys,
    /// dropping any key the registry no longer knows (an old/garbage save) so
    /// that slot reads back empty rather than referencing a missing item.
    /// Inverse of [`to_save`](Self::to_save).
    pub fn load_from_save(&mut self, saved: &[Option<String>; 8], items: &GameItems) {
        for (out, key) in self.items.iter_mut().zip(saved) {
            *out = match key {
                Some(k) if items.contains(k) => Some(k.clone()),
                _ => None,
            };
        }
    }
}

#[derive(Clone, Debug)]
pub enum InventoryUiState {
    PageSelect(i32),
    Items(usize, Option<(usize, String)>),
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
        system.play_sound(sound::click());
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
                system.play_sound(sound::interact());
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
                    system.play_sound(sound::click());
                };
                *i = (*i + dy % 3).clamp(0, 3);
                if dx == 1 {
                    self.change(system)
                };
            }
            Self::Items(i, sel) => {
                // The Use(8)/Drop(9) buttons exist only while an item is held;
                // normalise a stale button cursor back into the grid the moment
                // it's gone (a defensive fixup — the click paths already reset it).
                let holding = sel.is_some();
                if !holding && *i >= 8 {
                    *i = 3;
                }
                if *i >= 8 {
                    // On the button strip: left/right slides between the two
                    // buttons (and off Use back into the grid); up/down returns to
                    // the grid. Reachable only while holding, so no bounds worry.
                    match (dx, dy) {
                        (1, _) => *i = 9,
                        (-1, _) => *i = if *i == 9 { 8 } else { 3 },
                        (_, d) if d != 0 => *i = 3,
                        _ => {}
                    }
                    return;
                }
                if (*i == 0 || *i == 4) && dx == -1 {
                    self.back(system);
                    return;
                };
                // Right off the right column (3 or 7) steps onto the Use button
                // while holding; otherwise it's clamped so the top-right slot
                // doesn't wrap to the bottom-left.
                if holding && (*i == 3 || *i == 7) && dx == 1 {
                    *i = 8;
                    return;
                }
                let dx = if *i == 3 || *i == 7 { dx.min(0) } else { dx };
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
    /// The "Use" button — placing the held item here fires its use-effect. Shown
    /// only while an item is held (see [`InventoryUi::build_buttons`]); the dpad
    /// cursor reaches it as index 8.
    Use,
    /// The "Drop" button — placing the held item here discards it. Shown only
    /// while an item is held; the dpad cursor reaches it as index 9.
    Drop,
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

#[derive(Clone, Debug)]
pub struct InventoryUi {
    pub inventory: Inventory,
    pub state: InventoryUiState,
    pub dialogue: Dialogue,
    /// A "Use" the player placed on the Use button this frame: the authored
    /// effect of the used item, staged here for the walkaround to drain and fire
    /// (`step_inventory`) once the bag has closed, so the dialogue/cutscene plays
    /// in the world rather than under the frozen overlay. `None` the rest of the
    /// time.
    pub pending_use: Option<UseDef>,
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
            // Start closed — the bag opens on the bag button (see `open`), not on
            // walkaround entry.
            state: InventoryUiState::Close,
            dialogue: Dialogue::default(),
            pending_use: None,
        }
    }
    pub fn open(&mut self, system: &mut impl ConsoleApi) {
        system.play_sound(sound::interact());
        self.state = InventoryUiState::PageSelect(0);
    }
    /// Whether the bag overlay is currently up. The walkaround consults this to
    /// decide whether to run the overlay step (and composite it over the world);
    /// `Close` is the one state that means "not open".
    pub fn is_open(&self) -> bool {
        !matches!(self.state, InventoryUiState::Close)
    }
    /// The per-overlay freeze seam: whether an open overlay pauses the walkaround
    /// sim. The bag freezes it (so opening the inventory stops the world the way
    /// the map editor does), so this is `true`. A future non-pausing overlay
    /// (e.g. a HUD that coexists with a moving world) would return `false`,
    /// taking the fall-through path where the world still steps.
    pub fn pauses(&self) -> bool {
        true
    }
    pub fn click(&mut self, system: &mut impl ConsoleApi) {
        match &mut self.state {
            InventoryUiState::PageSelect(_) => self.state.change(system),
            InventoryUiState::Items(new_index, selected_item) => {
                if let Some((old_index, key)) = selected_item {
                    // Put item back down
                    if old_index == new_index {
                        system.play_sound(sound::item_down());
                        *selected_item = None;
                        return;
                    };

                    // Swap items, pick up swapped item if present.
                    self.inventory.swap(*new_index, *old_index);
                    if let Some(Some(x)) = self.inventory.items.get(*old_index) {
                        system.play_sound(sound::item_swap());
                        *key = x.clone();
                    } else {
                        system.play_sound(sound::item_down());
                        *selected_item = None;
                    };
                } else {
                    // Pick up item
                    if let Some(Some(x)) = self.inventory.items.get(*new_index) {
                        system.play_sound(sound::item_up());
                        *selected_item = Some((*new_index, x.clone()));
                    } else {
                        system.play_sound(sound::deny());
                    };
                }
            }
            InventoryUiState::Eggs(_index) => {
                system.play_sound(sound::deny());
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
        use crate::render::PrintOptions;

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
            .map(|s| ctx.text_width(s, body_opts.clone()))
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
                    .map(|(i, slot_key)| {
                        // The slot we're currently dragging from is left empty;
                        // the floating item is drawn over the cursor in `draw`.
                        // Only a known item (in the registry) draws a sprite.
                        let child = match slot_key {
                            Some(key) if dragging_from != Some(i) => {
                                ctx.items.get(key).map(|def| {
                                    b.sprite(def.sprite, 1, 1).scale(2).size(16.0, 16.0).id()
                                })
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
                let hint_w = ctx.text_width(hint, body_opts.clone());
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
    /// Build the floating "Use"/"Drop" button strip and its screen placement, or
    /// `None` when no item is held — the buttons exist only while dragging one.
    ///
    /// A detached mini-row hung off the panel's right edge and vertically centred
    /// against it (the strip right of the centred panel is free on the base
    /// screen), so it never shifts or resizes the centred panel on pickup. Laid
    /// out at the origin and *placed* via the returned `(x, y)` offset with the
    /// `Ui::*_at` helpers — so both hit-testing (`step`) and drawing (`draw`)
    /// feed off the same laid-out rects and always agree (the panel's own
    /// build-once pattern). Anchored to the shared panel's top-right slot rect,
    /// so it tracks the panel at any framebuffer size.
    fn build_buttons<S: ConsoleApi>(
        &self,
        ctx: &Ctx<S>,
        panel: &Ui<InvKey>,
    ) -> Option<(Ui<InvKey>, i16, i16)> {
        use crate::render::PrintOptions;

        // Only while an item is held (which only happens on the Items page).
        if !matches!(self.state, InventoryUiState::Items(_, Some(_))) {
            return None;
        }
        let small = ctx.save.small_text_on;
        let body_opts = PrintOptions {
            color: 12,
            small_text: small,
            ..Default::default()
        };
        // Each button is an outlined box (the slots' palette convention) wrapping
        // its label, sized to the text plus a small pad — like the panel's hint box.
        let button = |b: &mut UiBuilder<InvKey>, label: &str, key: InvKey| -> NodeId {
            let w = ctx.text_width(label, body_opts.clone());
            let text = b.text(label).small(small).size(w as f32, 8.0).id();
            b.boxed([text])
                .size((w + 4) as f32, 11.0)
                .pad_lrtb(2.0, 2.0, 1.0, 0.0)
                .outlined(0, 1)
                .key(key)
                .id()
        };
        let mut b = UiBuilder::new();
        let use_btn = button(&mut b, &ctx.script.label("inventory_use"), InvKey::Use);
        let drop_btn = button(&mut b, &ctx.script.label("inventory_drop"), InvKey::Drop);
        let row = b.row(2.0, [use_btn, drop_btn]).id();
        let (sw, sh) = ctx.draw.size();
        let ui = b.finish(row, (sw as f32, sh as f32));

        // Place right of the panel, vertically centred against it. The panel is
        // screen-centred, so its vertical centre is the screen's; the top-right
        // slot's right edge plus the main box's 2px right pad is the panel's right
        // edge (a small gap beyond it clears the panel outline).
        let bh = ui.rect(InvKey::Use).map(|r| r.h).unwrap_or(11);
        let panel_right = panel
            .rect(InvKey::Slot(3))
            .map(|s| s.x + s.w + 2)
            .unwrap_or(sw as i16 * 3 / 4);
        let bx = panel_right + 3;
        let by = sh as i16 / 2 - bh / 2;
        Some((ui, bx, by))
    }
    pub fn draw(&self, ctx: &mut Ctx<impl ConsoleApi>) {
        use crate::draw_state::{LayerId::*, PALETTE_MAP_IDENTITY};
        use crate::render::image::{Rgba, RgbaImage};
        use crate::render::{Canvas, EdgePolicy, PrintOptions, SpriteOptions, Transform};

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
        print_to_centered_with_font(ctx.font, 
            ctx.draw.rgba(FG),
            &inventory_title,
            cx + 1,
            title_y + 1,
            black,
            body_opts.clone(),
        );
        print_to_centered_with_font(ctx.font, 
            ctx.draw.rgba(FG),
            &inventory_title,
            cx,
            title_y,
            white,
            body_opts.clone(),
        );

        // Lay out and draw the whole panel in one pass...
        let ui = self.build_ui(&*ctx);
        // ...plus the floating Use/Drop strip while an item is held (None
        // otherwise), placed off the panel's right edge. Same laid-out rects the
        // cursor highlight below and `step`'s hit-test read.
        let buttons = self.build_buttons(&*ctx, &ui);
        ui.draw(ctx.draw, ctx.font, FG);
        if let Some((btns, bx, by)) = &buttons {
            btns.draw_at(*bx, *by, ctx.draw, ctx.font, FG);
        }

        // Unlock emblems: each shell whose story flag is set shows its icon
        // (sprite `596 + slot index`) centred on its 16×16 egg. `rect` resolves
        // the egg slots only on the Eggs page, so this is a no-op elsewhere.
        let shell_unlocks = ctx.save.shell_flags();
        for (i, unlocked) in shell_unlocks.iter().enumerate() {
            if *unlocked && let Some(slot) = ui.rect(InvKey::Egg(i)) {
                ctx.draw.spr(
                    FG,
                    &PALETTE_MAP_IDENTITY,
                    596 + i as i32,
                    i32::from(slot.x) + (i32::from(slot.w) - 8) / 2,
                    i32::from(slot.y) + (i32::from(slot.h) - 6) / 2,
                    SpriteOptions {
                        w: 1,
                        h: 1,
                        transparent: Some(0),
                        ..Default::default()
                    },
                );
            }
        }

        // ...then overlay the state-specific bits using the laid-out rects.
        match &self.state {
            InventoryUiState::Items(current, selected) => {
                // The cursor may sit on a slot (`0..8`) or, while holding, on the
                // Use(8)/Drop(9) button — resolve its rect from whichever tree laid
                // it out, so the highlight and floating sprite follow onto a button.
                let cursor = if *current < 8 {
                    ui.rect(InvKey::Slot(*current))
                } else {
                    buttons.as_ref().and_then(|(bu, bx, by)| {
                        let key = if *current == 8 { InvKey::Use } else { InvKey::Drop };
                        bu.rect_at(*bx, *by, key)
                    })
                };
                if let Some(slot) = cursor {
                    ctx.draw.rgba(FG).stroke_rect(
                        slot.x.into(),
                        slot.y.into(),
                        slot.w.into(),
                        slot.h.into(),
                        white,
                    );
                    if let Some((_, key)) = selected {
                        // Picked-up item floats 4px above its cursor slot, outlined.
                        if let Some(def) = ctx.items.get(key) {
                            ctx.draw.spr_with_outline(
                                FG,
                                &PALETTE_MAP_IDENTITY,
                                def.sprite,
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
                }
                let key = match selected {
                    Some((_, key)) => Some(key.clone()),
                    None => self.inventory.get(*current).map(str::to_string),
                };
                if let Some(key) = key {
                    let name = ctx.item_name(&key);
                    // Glued to the description box's portrait (the item dialogue
                    // below): the portrait's left edge is `(cw - width)/2 - 13`
                    // (x 7 at the base width) and the box is bottom-anchored, so a
                    // 38px bottom margin gives y 98 at the base height. Tracking the
                    // same expressions keeps the name tab on the box in BOTH axes.
                    let (cw, ch) = ctx.draw.size();
                    let nx = (cw - self.dialogue.width as i32) / 2 - 13;
                    let ny = ch - 38;
                    ctx.draw.rgba(FG).outlined_rect(nx, ny, 70, 9, c2, c3);
                    print_to_with_font(ctx.font, 
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
            let key = match selected {
                Some((_, key)) => Some(key.clone()),
                None => self.inventory.get(*current).map(str::to_string),
            };
            // Only a known item (sprite in the registry) draws its portrait.
            let resolved = key
                .as_deref()
                .and_then(|k| ctx.items.get(k).map(|d| (d.sprite, k.to_string())));
            if let Some((sprite, key)) = resolved {
                let desc = ctx.item_desc(&key);
                let string = self.dialogue.fit_text(ctx.font, small, &desc);
                self.dialogue.draw_dialogue_portrait(
                    ctx.draw, FG, ctx.font, small, &string, false, sprite, 3, 1, 1,
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
    /// Drop (discard) the held item, or the one under the cursor if none is held,
    /// removing it from the inventory entirely. No-op outside the Items page.
    pub fn drop_item(&mut self, system: &mut impl ConsoleApi) {
        let target = match &self.state {
            InventoryUiState::Items(current, selected) => selected
                .as_ref()
                .map(|(origin, _)| *origin)
                .unwrap_or(*current),
            _ => return,
        };
        if self.inventory.take(target).is_some() {
            system.play_sound(sound::item_down());
            if let InventoryUiState::Items(_, selected) = &mut self.state {
                *selected = None;
            }
        } else {
            system.play_sound(sound::deny());
        }
    }
    /// Activate the bag button the held item was placed on. Drop discards the
    /// item (the existing [`drop_item`](Self::drop_item) path) and returns the
    /// cursor to the origin slot; Use looks up the item's authored `on_use` and,
    /// if any, stages it in [`pending_use`](Self::pending_use), puts the held item
    /// back down (using never consumes it) and closes the bag so the effect plays
    /// in the world — otherwise a deny buzz, still holding, bag still open. Kept
    /// out of [`click`](Self::click) because Use needs the item registry on `ctx`,
    /// which `click` (mouse/A both) doesn't take. No-op unless an item is held.
    fn activate_button(&mut self, ctx: &mut Ctx<impl ConsoleApi>, key: InvKey) {
        match key {
            InvKey::Drop => {
                // The origin slot the held item came from, so the cursor lands
                // back on it once the button (index >= 8) is gone.
                let origin = match &self.state {
                    InventoryUiState::Items(cur, sel) => {
                        sel.as_ref().map(|(o, _)| *o).unwrap_or(*cur)
                    }
                    _ => return,
                };
                self.drop_item(ctx.system);
                if let InventoryUiState::Items(i @ 8.., None) = &mut self.state {
                    *i = origin;
                }
            }
            InvKey::Use => {
                let Some(key) = (match &self.state {
                    InventoryUiState::Items(_, Some((_, key))) => Some(key.clone()),
                    _ => None,
                }) else {
                    return;
                };
                match ctx.items.get(&key).and_then(|d| d.on_use.clone()) {
                    // No authored use: deny, keep holding, bag stays open.
                    None => ctx.system.play_sound(sound::deny()),
                    Some(def) => {
                        self.pending_use = Some(def);
                        if let InventoryUiState::Items(_, selected) = &mut self.state {
                            *selected = None;
                        }
                        self.state = InventoryUiState::Close;
                    }
                }
            }
            _ => {}
        }
    }
    pub fn step(&mut self, ctx: &mut Ctx<impl ConsoleApi>) {
        // --- Mouse: hover moves the cursor, left-click acts, right-click backs out. ---
        let ui = self.build_ui(&*ctx);
        // The floating Use/Drop strip, if an item is held. Hit-tested first — it
        // sits outside the panel, so it never overlaps a slot.
        let buttons = self.build_buttons(&*ctx, &ui);
        let mouse = ctx.input.mouse;
        let mut mouse_clicked = false;
        let hit = buttons
            .as_ref()
            .and_then(|(bu, bx, by)| bu.hit_at(*bx, *by, mouse.pos()))
            .or_else(|| ui.hit(mouse.pos()));
        if let Some(key) = hit {
            match key {
                InvKey::Use | InvKey::Drop => {
                    // Hover slides the cursor onto the button (index 8/9, keeping
                    // the held item), left-click places it there to activate.
                    let target = if key == InvKey::Use { 8 } else { 9 };
                    let drag = match &self.state {
                        InventoryUiState::Items(_, sel) => sel.clone(),
                        _ => None,
                    };
                    if mouse.moved() {
                        self.state = InventoryUiState::Items(target, drag.clone());
                    }
                    if just_pressed(mouse.left) {
                        self.state = InventoryUiState::Items(target, drag);
                        self.activate_button(ctx, key);
                        mouse_clicked = true;
                    }
                }
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
                        InventoryUiState::Items(_, sel) => sel.clone(),
                        _ => None,
                    };
                    if mouse.moved() {
                        self.state = InventoryUiState::Items(i, drag.clone());
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
        let pad = ctx.input.controller();
        let (dx, dy) = dpad_delta(&pad, just_pressed);
        self.state.arrows(ctx.system, dx.into(), dy.into());
        if just_pressed(pad.a) && !mouse_clicked {
            // A on the held item over a button (cursor 8/9) activates it;
            // anywhere else it's the normal pick-up / put-down / swap.
            match &self.state {
                InventoryUiState::Items(i, Some(_)) if *i >= 8 => {
                    let key = if *i == 8 { InvKey::Use } else { InvKey::Drop };
                    self.activate_button(ctx, key);
                }
                _ => self.click(ctx.system),
            }
        };
        if just_pressed(pad.b) {
            self.state.back(ctx.system)
        };
        // X discards the held / hovered item.
        if just_pressed(pad.x) {
            self.drop_item(ctx.system)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::test_console::TestConsole;

    /// An Items state holding a copy of `origin`'s item, cursor at `cursor`.
    fn held(cursor: usize) -> InventoryUiState {
        InventoryUiState::Items(cursor, Some((0, "ff".to_string())))
    }

    /// The dpad reaches the Use(8)/Drop(9) buttons only while an item is held,
    /// and can always escape them back into the grid; an empty-handed cursor can
    /// never land on them (right off the right column just clamps, as before).
    #[test]
    fn dpad_reaches_and_escapes_buttons_only_while_holding() {
        let mut sys = TestConsole::new();

        // Empty-handed: right off the top-right slot (3) clamps — no button.
        let mut s = InventoryUiState::Items(3, None);
        s.arrows(&mut sys, 1, 0);
        assert!(
            matches!(s, InventoryUiState::Items(3, None)),
            "no button reachable without a held item",
        );

        // Holding: right off 3 steps onto Use(8), then onto Drop(9)...
        let mut s = held(3);
        s.arrows(&mut sys, 1, 0);
        assert!(matches!(s, InventoryUiState::Items(8, _)), "3 -> Use");
        s.arrows(&mut sys, 1, 0);
        assert!(matches!(s, InventoryUiState::Items(9, _)), "Use -> Drop");
        // ...left walks Drop -> Use -> back into the grid's right column.
        s.arrows(&mut sys, -1, 0);
        assert!(matches!(s, InventoryUiState::Items(8, _)), "Drop -> Use");
        s.arrows(&mut sys, -1, 0);
        assert!(matches!(s, InventoryUiState::Items(3, _)), "Use -> grid");

        // Up/down off a button also returns to the grid (both escapable).
        let mut s = held(8);
        s.arrows(&mut sys, 0, 1);
        assert!(matches!(s, InventoryUiState::Items(3, _)), "down off Use -> grid");

        // The bottom-right slot (7) reaches the buttons too.
        let mut s = held(7);
        s.arrows(&mut sys, 1, 0);
        assert!(matches!(s, InventoryUiState::Items(8, _)), "7 -> Use");
    }

    /// A button cursor (8/9) left with nothing held is normalised back into the
    /// grid on the next dpad tick, so once the held item is gone the cursor can
    /// never be stranded off the slots.
    #[test]
    fn stale_button_cursor_normalises_into_grid() {
        let mut sys = TestConsole::new();
        let mut s = InventoryUiState::Items(9, None);
        s.arrows(&mut sys, 0, 0);
        assert!(
            matches!(s, InventoryUiState::Items(3, None)),
            "8/9 with no held item -> grid",
        );
    }
}
