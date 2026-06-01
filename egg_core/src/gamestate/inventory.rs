use crate::{
    data::{dialogue_data::*, sound},
    dialogue::{DIALOGUE_OPTIONS, Dialogue},
    system::{ConsoleApi, ConsoleHelper, just_pressed},
    ui::{self, Content, Decoration, NodeId, Style, Ui, UiBuilder},
};

static ITEM_FF: InventoryItem = InventoryItem {
    sprite: 513,
    name: ITEM_FF_NAME,
    desc: ITEM_FF_DESC,
};
static ITEM_LM: InventoryItem = InventoryItem {
    sprite: 514,
    name: ITEM_LM_NAME,
    desc: ITEM_LM_DESC,
};
static ITEM_CHEGG: InventoryItem = InventoryItem {
    sprite: 524,
    name: ITEM_CHEGG_NAME,
    desc: ITEM_CHEGG_DESC,
};

#[derive(Debug)]
pub struct InventoryItem {
    pub sprite: i32,
    pub name: &'static str,
    pub desc: &'static str,
}
impl InventoryItem {
    pub const fn new(sprite: i32, name: &'static str, desc: &'static str) -> Self {
        Self { sprite, name, desc }
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

/// Build a 20×20 item/egg slot: an outlined box keyed for hit-testing, holding
/// an optional 16×16 sprite inset by 2px (via padding).
fn make_slot(builder: &mut UiBuilder<InvKey>, key: InvKey, outline: u8, sprite: Option<Content>) -> NodeId {
    let children: Vec<NodeId> = match sprite {
        Some(content) => vec![builder.leaf(
            Style { size: ui::size(16.0, 16.0), ..Default::default() },
            content,
            Decoration::default(),
            None,
        )],
        None => Vec::new(),
    };
    builder.container(
        Style { size: ui::size(20.0, 20.0), padding: ui::pad(2.0), ..Default::default() },
        Decoration::outlined(0, outline),
        Some(key),
        &children,
    )
}

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
    /// an [`InvKey`] so a mouse hit resolves straight to the thing under it.
    pub fn build_ui(&self, system: &mut impl ConsoleApi) -> Ui<InvKey> {
        use crate::system::{HEIGHT, PrintOptions, WIDTH};

        // Original fixed dimensions, kept so the panel centres exactly as before
        // (item slot stride was `2*8 + 5 = 21`: a 20px box with a 1px gap).
        const MAIN_W: f32 = 89.0;
        const PANEL_H: f32 = 47.0;

        let small = DIALOGUE_OPTIONS.small_text(system);
        let body_opts = PrintOptions { color: 12, small_text: small, ..Default::default() };
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
        let labels = [INVENTORY_ITEMS, INVENTORY_SHELL, INVENTORY_OPTIONS, INVENTORY_BACK];
        let label_w = labels
            .iter()
            .map(|s| system.text_width(s, body_opts.clone()))
            .max()
            .unwrap_or(0);
        let label_nodes: Vec<NodeId> = labels
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let selected = i as i32 == page;
                b.leaf(
                    Style { size: ui::full_width(8.0), ..Default::default() },
                    Content::Text { text: s.to_string(), color: 12, center: false, small },
                    if selected { Decoration::fill(col_c + 1) } else { Decoration::default() },
                    Some(InvKey::Page(i)),
                )
            })
            .collect();
        let side = b.container(
            Style {
                size: ui::width((label_w + 5) as f32),
                padding: ui::pad_lrtb(2.0, 2.0, 1.0, 1.0),
                ..ui::column(0.0)
            },
            Decoration::outlined(col_c, col_c + 1),
            None,
            &label_nodes,
        );

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
                        let sprite = match item {
                            Some(item) if dragging_from != Some(i) => {
                                Some(Content::Sprite { id: item.sprite, scale: 2, w: 1, h: 1, outline: None })
                            }
                            _ => None,
                        };
                        make_slot(&mut b, InvKey::Slot(i), main_c + 1, sprite)
                    })
                    .collect();
                b.container(
                    Style { size: ui::width(MAIN_W), padding: ui::pad_lrtb(3.0, 2.0, 3.0, 3.0), ..ui::wrap_row(1.0) },
                    Decoration::outlined(main_c, main_c + 1),
                    None,
                    &slots,
                )
            }
            1 => {
                let slots: Vec<NodeId> = (0..4)
                    .map(|i| {
                        let egg = Content::Sprite { id: 534, scale: 1, w: 2, h: 2, outline: None };
                        make_slot(&mut b, InvKey::Egg(i), main_c + 1, Some(egg))
                    })
                    .collect();
                b.container(
                    Style { size: ui::width(MAIN_W), padding: ui::pad_lrtb(3.0, 2.0, 3.0, 3.0), ..ui::wrap_row(1.0) },
                    Decoration::outlined(main_c, main_c + 1),
                    None,
                    &slots,
                )
            }
            n => {
                let hint = if n == 2 { "Open options menu" } else { "Back to world" };
                let hint_w = system.text_width(hint, body_opts.clone());
                let text_node = b.leaf(
                    Style { size: ui::size(hint_w as f32, 8.0), ..Default::default() },
                    Content::Text { text: hint.to_string(), color: 12, center: false, small },
                    Decoration::default(),
                    None,
                );
                let hint_box = b.container(
                    Style { size: ui::size((hint_w + 3) as f32, 10.0), padding: ui::pad_lrtb(2.0, 0.0, 1.0, 0.0), ..Default::default() },
                    Decoration::outlined(col_c, col_c + 1),
                    None,
                    &[text_node],
                );
                // Reserve the full main width so the side column keeps its x, and
                // drop the hint box level with the selected page label.
                b.container(
                    Style { size: ui::width(MAIN_W), padding: ui::pad_lrtb(0.0, 0.0, (n * 8) as f32, 0.0), ..ui::column(0.0) },
                    Decoration::default(),
                    None,
                    &[hint_box],
                )
            }
        };

        // --- Panel (side + main), centred on the 240×136 screen by Taffy. ---
        let panel = b.container(
            Style { size: ui::full_width(PANEL_H), ..ui::row_top(2.0) },
            Decoration::default(),
            None,
            &[side, main],
        );
        let root = b.container(
            Style { size: ui::size(WIDTH as f32, HEIGHT as f32), ..ui::centered() },
            Decoration::default(),
            None,
            &[panel],
        );
        b.finish(root)
    }
    pub fn draw(&self, draw_state: &mut crate::drawstate::DrawState, system: &mut impl ConsoleApi) {
        use crate::drawstate::{LayerId::*, PALETTE_MAP_IDENTITY};
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::image::{Rgba, RgbaImage};
        use crate::system::{PrintOptions, StaticSpriteOptions};

        let small = DIALOGUE_OPTIONS.small_text(system);
        let body_opts = PrintOptions { color: 12, small_text: small, ..Default::default() };
        let black = draw_state.colour(0);
        let white = draw_state.colour(12);
        let c2 = draw_state.colour(2);
        let c3 = draw_state.colour(3);

        // Foreground starts clear each frame; everything here draws onto it.
        draw_state.rgba(FG).fill(Rgba::TRANSPARENT);

        // Title, white with a 1px black shadow.
        system.print_to_centered(draw_state.rgba(FG), INVENTORY_TITLE, 121, 38, black, body_opts.clone());
        system.print_to_centered(draw_state.rgba(FG), INVENTORY_TITLE, 120, 37, white, body_opts.clone());

        // Lay out and draw the whole panel in one pass...
        let ui = self.build_ui(system);
        ui.draw(draw_state, system, FG);

        // ...then overlay the state-specific bits using the laid-out rects.
        match &self.state {
            InventoryUiState::Items(current, selected) => {
                if let Some(slot) = ui.rect(InvKey::Slot(*current)) {
                    draw_state
                        .rgba(FG)
                        .stroke_rect(slot.x.into(), slot.y.into(), slot.w.into(), slot.h.into(), white);
                    if let Some((_, item)) = selected {
                        // Picked-up item floats 4px above its cursor slot, outlined.
                        draw_state.spr_with_outline(
                            FG,
                            &PALETTE_MAP_IDENTITY,
                            item.sprite,
                            i32::from(slot.x) + 2,
                            i32::from(slot.y) + 2 - 4,
                            StaticSpriteOptions { scale: 2, transparent: &[0], ..Default::default() },
                            12,
                        );
                    }
                }
                let name = match selected {
                    Some((_, item)) => Some(item.name),
                    None => self.inventory.items[*current].map(|item| item.name),
                };
                if let Some(name) = name {
                    draw_state.rgba(FG).outlined_rect(7, 98, 70, 9, c2, c3);
                    system.print_to(draw_state.rgba(FG), name, 9, 100, white, body_opts.clone());
                }
            }
            InventoryUiState::Eggs(current) => {
                if let Some(slot) = ui.rect(InvKey::Egg(*current)) {
                    draw_state
                        .rgba(FG)
                        .stroke_rect(slot.x.into(), slot.y.into(), slot.w.into(), slot.h.into(), white);
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
                let string = self.dialogue.fit_text(system, item.desc);
                self.dialogue
                    .draw_dialogue_portrait(draw_state, FG, system, &string, false, item.sprite, 3, 1, 1);
            }
        }

        // Composite background then foreground into the output image.
        let output = system.output_image();
        output.blit::<RgbaImage>(
            0,
            0,
            &draw_state.rgba_canvas[BG as usize],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
        output.blit::<RgbaImage>(
            0,
            0,
            &draw_state.rgba_canvas[FG as usize],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
    }
    pub fn step(&mut self, system: &mut impl ConsoleApi) {
        // --- Mouse: hover moves the cursor, left-click acts, right-click backs out. ---
        let ui = self.build_ui(system);
        let mouse = system.mouse();
        let mut mouse_clicked = false;
        if let Some(key) = ui.hit(mouse.pos()) {
            match key {
                InvKey::Page(i) => {
                    if mouse.moved() {
                        self.state = InventoryUiState::PageSelect(i as i32);
                    }
                    if just_pressed(mouse.left) {
                        self.state = InventoryUiState::PageSelect(i as i32);
                        self.state.change(system);
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
                        self.click(system);
                        mouse_clicked = true;
                    }
                }
                InvKey::Egg(i) => {
                    if mouse.moved() {
                        self.state = InventoryUiState::Eggs(i);
                    }
                    if just_pressed(mouse.left) {
                        self.state = InventoryUiState::Eggs(i);
                        self.click(system);
                        mouse_clicked = true;
                    }
                }
            }
        }
        if just_pressed(mouse.right) {
            self.state.back(system);
        }

        // Keyboard / gamepad navigation
        let pad = system.controller();
        let (mut dx, mut dy) = (0, 0);
        if just_pressed(pad.up) {
            dy -= 1
        }
        if just_pressed(pad.down) {
            dy += 1
        }
        if just_pressed(pad.left) {
            dx -= 1
        }
        if just_pressed(pad.right) {
            dx += 1
        }
        self.state.arrows(system, dx, dy);
        if just_pressed(pad.a) && !mouse_clicked {
            self.click(system)
        };
        if just_pressed(pad.b) {
            self.state.back(system)
        };
    }
}
