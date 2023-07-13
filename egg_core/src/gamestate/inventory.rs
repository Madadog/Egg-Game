use crate::{
    data::{dialogue_data::*, sound},
    dialogue::{print_width, Dialogue},
    system::{ConsoleApi, ConsoleHelper},
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
            if slot.is_some() {
                slot.take()
            } else {
                None
            }
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
            _ => 2,
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
                system.play_sound(sound::INTERACT.with_note(-17));
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

pub struct InventoryUi {
    pub inventory: Inventory,
    pub state: InventoryUiState,
    pub dialogue: Dialogue,
}
impl InventoryUi {
    pub fn new() -> Self {
        Self {
            inventory: Inventory::new(),
            state: InventoryUiState::PageSelect(0),
            dialogue: Dialogue::const_default(),
        }
    }
    pub fn open(&mut self, system: &mut impl ConsoleApi) {
        system.play_sound(sound::INTERACT.with_note(-12));
        self.state = InventoryUiState::PageSelect(0);
    }
    pub fn click(&mut self, system: &mut impl ConsoleApi) {
        match &mut self.state {
            InventoryUiState::PageSelect(_) => self.state.change(system),
            InventoryUiState::Items(new_index, selected_item) => {
                if let Some((old_index, id)) = selected_item {
                    // Put item back down
                    if old_index == new_index {
                        system.play_sound(sound::INTERACT.with_note(-5));
                        *selected_item = None;
                        return;
                    };

                    // Swap items, pick up swapped item if present.
                    self.inventory.swap(*new_index, *old_index);
                    if let Some(Some(x)) = self.inventory.items.get(*old_index) {
                        system.play_sound(sound::INTERACT.with_note(0));
                        *id = *x;
                    } else {
                        system.play_sound(sound::INTERACT.with_note(-5));
                        *selected_item = None;
                    };
                } else {
                    // Pick up item
                    if let Some(Some(x)) = self.inventory.items.get(*new_index) {
                        system.play_sound(sound::INTERACT);
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
    pub fn draw(&self, system: &mut impl ConsoleApi) {
        use crate::dialogue::DIALOGUE_OPTIONS;
        use tic80_api::core::{PrintOptions, SpriteOptions, HEIGHT, WIDTH};
        system.blit_segment(4);
        let entries = [
            INVENTORY_ITEMS,
            INVENTORY_SHELL,
            INVENTORY_OPTIONS,
            INVENTORY_BACK,
        ];
        let small_text = DIALOGUE_OPTIONS.small_text(system);
        // Entries is fixed-length so this can't fail
        let width = entries
            .iter()
            .map(|x| {
                print_width(system, x, false, small_text)})
            .max()
            .unwrap();
        let side_column = width + 3;
        let column_margin = 2;
        let scale = 2;
        let item_slot_size = scale * 8 + 5;
        let main_width = item_slot_size * 4 + 5;
        let total_width = main_width + side_column + column_margin;
        let total_height = item_slot_size * 2 + 5;
        let x_offset = (WIDTH - total_width) / 2;
        let y_offset = (HEIGHT - total_height) / 2;
        let mut column_colour = 0;
        let mut main_colour = 0;
        match self.state {
            InventoryUiState::PageSelect(_) => column_colour += 2,
            _ => {
                main_colour += 2;
            }
        };
        system.cls(0);
        system.print_alloc_centered(
            crate::data::dialogue_data::INVENTORY_TITLE,
            120,
            37,
            PrintOptions {
                color: 12,
                small_text,
                ..Default::default()
            },
        );
        // draw side selection
        system.rect_outline(
            x_offset,
            y_offset,
            side_column,
            5 + entries.len() as i32 * 8,
            column_colour,
            column_colour + 1,
        );
        system.rect(
            x_offset + 1,
            y_offset + 8 * self.state.page() + 3,
            side_column - 2,
            7,
            column_colour + 1,
        );
        for (i, string) in entries.iter().enumerate() {
            system.print_alloc(
                string,
                x_offset + 2,
                y_offset + i as i32 * 8 + 4,
                PrintOptions {
                    color: 12,
                    small_text,
                    ..Default::default()
                },
            );
        }
        match self.state.page() {
            0 => {
                system.rect_outline(
                    x_offset + side_column + column_margin,
                    y_offset,
                    main_width,
                    total_height,
                    main_colour,
                    main_colour + 1,
                );
                for (i, item) in (0..).zip(self.inventory.items.iter()) {
                    let (sx, sy) = (
                        x_offset + side_column + column_margin + 3 + (i % 4) * item_slot_size,
                        y_offset + 3 + (i / 4) * item_slot_size,
                    );
                    system.rect_outline(
                        sx,
                        sy,
                        item_slot_size - 1,
                        item_slot_size - 1,
                        0,
                        main_colour + 1,
                    );
                    if let Some(item) = item {
                        system.spr(
                            item.sprite,
                            sx + 2,
                            sy + 2,
                            SpriteOptions {
                                scale,
                                transparent: &[0],
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            1 => {
                system.rect_outline(
                    x_offset + side_column + column_margin,
                    y_offset,
                    main_width,
                    item_slot_size + 5,
                    main_colour,
                    main_colour + 1,
                );
                for i in 0..4 {
                    let (sx, sy) = (
                        x_offset + side_column + column_margin + 3 + (i % 4) * item_slot_size,
                        y_offset + 3 + (i / 4) * item_slot_size,
                    );
                    system.rect_outline(
                        sx,
                        sy,
                        item_slot_size - 1,
                        item_slot_size - 1,
                        0,
                        main_colour + 1,
                    );
                    system.spr_blit_segment(
                        1086,
                        sx + 2,
                        sy + 2,
                        SpriteOptions {
                            transparent: &[0],
                            w: 2,
                            h: 2,
                            ..Default::default()
                        },
                        8,
                    );
                }
            }
            _ => (),
        };
        match &self.state {
            InventoryUiState::Items(current_index, selected) => {
                let (sx, sy) = (
                    x_offset
                        + side_column
                        + column_margin
                        + 3
                        + (*current_index as i32 % 4) * item_slot_size,
                    y_offset + 3 + (*current_index as i32 / 4) * item_slot_size,
                );
                system.rectb(sx, sy, item_slot_size - 1, item_slot_size - 1, 12);
                if let Some((selected_index, selected_item)) = selected {
                    let (old_sx, old_sy) = (
                        x_offset
                            + side_column
                            + column_margin
                            + 3
                            + (*selected_index as i32 % 4) * item_slot_size,
                        y_offset + 3 + (*selected_index as i32 / 4) * item_slot_size,
                    );
                    system.rect(
                        old_sx + 1,
                        old_sy + 1,
                        item_slot_size - 3,
                        item_slot_size - 3,
                        0,
                    );
                    system.spr_outline(
                        selected_item.sprite,
                        sx + 2,
                        sy + 2 - 4,
                        SpriteOptions {
                            scale,
                            transparent: &[0],
                            ..Default::default()
                        },
                        12,
                    );
                    system.rect_outline(7, 98, 70, 9, 2, 3);
                    system.print_alloc(
                        selected_item.name,
                        9,
                        100,
                        PrintOptions {
                            small_text,
                            color: 12,
                            ..Default::default()
                        },
                    );
                    let string = &self.dialogue.fit_text(system, selected_item.desc);
                    self.dialogue.draw_dialogue_portrait(
                        system,
                        string,
                        false,
                        selected_item.sprite,
                        3,
                        1,
                        1,
                    );
                } else {
                    if let Some(item) = &self.inventory.items[*current_index] {
                        system.rect_outline(7, 98, 70, 9, 2, 3);
                        system.print_alloc(
                            item.name,
                            9,
                            100,
                            PrintOptions {
                                small_text,
                                color: 12,
                                ..Default::default()
                            },
                        );
                        let string = &self.dialogue.fit_text(system, item.desc);
                        self.dialogue.draw_dialogue_portrait(
                            system,
                            string,
                            false,
                            item.sprite,
                            3,
                            1,
                            1,
                        );
                    }
                }
            }
            InventoryUiState::Eggs(current_index) => {
                let (sx, sy) = (
                    x_offset
                        + side_column
                        + column_margin
                        + 3
                        + (*current_index as i32 % 4) * item_slot_size,
                    y_offset + 3,
                );
                system.rectb(sx, sy, item_slot_size - 1, item_slot_size - 1, 12);
            }
            _ => {}
        };
    }
    pub fn step(&mut self, system: &mut impl ConsoleApi) {
        let (mut dx, mut dy) = (0, 0);
        if system.mem_btnp(0) {
            dy -= 1
        }
        if system.mem_btnp(1) {
            dy += 1
        }
        if system.mem_btnp(2) {
            dx -= 1
        }
        if system.mem_btnp(3) {
            dx += 1
        }
        self.state.arrows(system, dx, dy);
        if system.mem_btnp(4) {
            self.click(system)
        };
        if system.mem_btnp(5) {
            self.state.back(system)
        };
    }
}
