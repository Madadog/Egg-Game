use crate::{dialogue_data::*, dialogue::{draw_dialogue_portrait}, tic_helpers::print_raw_centered};

static ITEM_FF: InventoryItem = InventoryItem {sprite: 513, name: ITEM_FF_NAME, desc: ITEM_FF_DESC};
static ITEM_LM: InventoryItem = InventoryItem {sprite: 514, name: ITEM_LM_NAME, desc: ITEM_LM_DESC};
static ITEM_CHEGG: InventoryItem = InventoryItem {sprite: 524, name: ITEM_CHEGG_NAME, desc: ITEM_CHEGG_DESC};

#[derive(Debug)]
pub struct InventoryItem<'a> {
    pub sprite: i32,
    pub name: &'a str,
    pub desc: &'a str,
}
impl<'a> InventoryItem<'a> {
    pub const fn new(sprite: i32, name: &'static str, desc: &'static str) -> Self {
        Self { sprite, name, desc }
    }
}

pub struct Inventory<'a> {
    pub items: [Option<&'a InventoryItem<'static>>; 8],
    pub unlocks: [bool; 4],
}
impl<'a> Inventory<'a> {
    pub fn new() -> Self {
        Self {
            items: [Some(&ITEM_FF), Some(&ITEM_LM), Some(&ITEM_CHEGG), None, None, None, None, None],
            unlocks: [false; 4],
        }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }
    pub fn take(&mut self, index: usize) -> Option<&'a InventoryItem<'a>> {
        let x = self.items.get_mut(index).unwrap();
        if x.is_some() {
            let item = x.unwrap();
            *x = None;
            Some(item)
        } else {
            None
        }
    }
}

pub enum InventoryUiState<'a> {
    PageSelect(i32),
    Items(usize, Option<(usize, &'a InventoryItem<'static>)>),
    Eggs(usize),
    Close,
}
impl<'a> InventoryUiState<'a> {
    pub fn page(&self) -> i32 {
        match self {
            Self::PageSelect(x) => *x,
            Self::Items(_, _) => 0,
            Self::Eggs(_) => 1,
            _ => 2,
        }
    }
    pub fn change(&mut self) {
        match self {
            Self::PageSelect(0) => *self = Self::Items(0, None),
            Self::PageSelect(1) => *self = Self::Eggs(0),
            _ => *self = Self::PageSelect(self.page()),
        };
    }
    pub fn back(&mut self) {
        match self {
            Self::PageSelect(_) => *self = Self::Close,
            Self::Close => (),
            _ => self.change(),
        }
    }
    pub fn arrows(&mut self, dx: i32, dy: i32) {
        match self {
            Self::PageSelect(x) => {
                *x = (*x+dy%2).clamp(0, 1);
                if dx == 1 {self.change()};
            },
            Self::Items(x, _) => {
                if (*x == 0 || *x == 4) && dx == -1 {self.back(); return};
                let dx = if *x == 3 {dx.min(0)} else {dx};
                let new = *x as i32 + dx + dy * 4;
                if new >= 0 && new < 8 { *x = new as usize; };
            },
            Self::Eggs(x) => {if *x == 0 && dx == -1 {self.back(); return};},
            _ => (),
        }
    }
}

pub struct InventoryUi<'a> {
    pub inventory: Inventory<'a>,
    pub state: InventoryUiState<'a>,
}
impl<'a> InventoryUi<'a> {
    pub fn new() -> Self {
        Self {
            inventory: Inventory::new(),
            state: InventoryUiState::PageSelect(0),
        }
    }
    pub fn open(&mut self) {
        self.state = InventoryUiState::PageSelect(0);
    }
    pub fn click(&mut self) {
        match &mut self.state {
            InventoryUiState::PageSelect(_) => self.state.change(),
            InventoryUiState::Items(new_index, selected_item) => {
                if let Some((old_index, id)) = selected_item {
                    // Put item back down
                    if old_index == new_index {
                        *selected_item = None;
                        return
                    };
                    // Swap items, pick up swapped item if present. 
                    self.inventory.swap(*new_index, *old_index);
                    if let Some(Some(x)) = self.inventory.items.get(*old_index) {
                        *id = *x;
                    } else {
                        *selected_item = None;
                    };
                } else {
                    // Pick up item
                    if let Some(Some(x)) = self.inventory.items.get(*new_index) {
                        *selected_item = Some((*new_index, *x));
                    };
                }
            },
            _ => ()
        }
    }
    pub fn draw(&self) {
        use crate::tic80::{rect, rectb, cls, print_raw, spr, PrintOptions, WIDTH, HEIGHT, SpriteOptions};
        use crate::tic_helpers::{rect_outline, spr_outline};
        let side_column = 32;
        let column_margin = 2;
        let scale = 2;
        let item_slot_size = scale*8+5;
        let main_width = item_slot_size*4+5;
        let total_width = main_width + side_column + column_margin;
        let total_height = item_slot_size*2+5;
        let x_offset = (WIDTH - total_width)/2;
        let y_offset = (HEIGHT - total_height)/2;
        let mut column_colour = 0;
        let mut main_colour = 0;
        match self.state {
            InventoryUiState::PageSelect(_) => {column_colour += 2},
            _ => {main_colour += 2;},
        };
        cls(0);
        print_raw_centered(crate::dialogue_data::INVENTORY_TITLE, 120, 37, PrintOptions {color: 12, ..Default::default()});
        // draw side selection
        rect_outline(x_offset, y_offset, side_column, 21, column_colour, column_colour+1);
        rect(x_offset+1,y_offset+8*self.state.page()+3,side_column-2,7,column_colour+1);
        print_raw("Items\0", x_offset+2, y_offset+4, PrintOptions {color: 12, ..Default::default()});
        print_raw("Shell\0", x_offset+2, y_offset+8+4, PrintOptions {color: 12, ..Default::default()});
        match self.state.page() {
            0 => {
                rect_outline(x_offset + side_column + column_margin,y_offset, main_width, total_height, main_colour, main_colour+1);
                for (i, item) in (0..).zip(self.inventory.items.iter()) {
                    let (sx, sy) = (
                        x_offset + side_column + column_margin + 3 + (i%4)*item_slot_size,
                        y_offset + 3 + (i/4)*item_slot_size,
                    );
                    rect_outline(sx, sy, item_slot_size-1, item_slot_size-1, 0, main_colour+1);
                    if let Some(item) = item {
                        spr(item.sprite, sx+2, sy+2, SpriteOptions {scale, transparent: &[0], ..Default::default()});
                    }
                }
            },
            1 => {
                rect_outline(x_offset + side_column + column_margin,y_offset, main_width, item_slot_size+5, main_colour, main_colour+1);
                for i in 0..4 {
                    let (sx, sy) = (
                        x_offset + side_column + column_margin + 3 + (i%4)*item_slot_size,
                        y_offset + 3 + (i/4)*item_slot_size,
                    );
                    rect_outline(sx, sy, item_slot_size-1, item_slot_size-1, 0, main_colour+1);
                }
            },
            _ => (),
        };
        match &self.state {
            InventoryUiState::Items(current_index, selected) => {
                use crate::DIALOGUE;
                use crate::print;
                let (sx, sy) = (
                    x_offset + side_column + column_margin + 3 + (*current_index as i32%4)*item_slot_size,
                    y_offset + 3 + (*current_index as i32/4)*item_slot_size,
                );
                rectb(sx, sy, item_slot_size-1, item_slot_size-1, 12);
                if let Some((selected_index, selected_item)) = selected {
                    let (old_sx, old_sy) = (
                        x_offset + side_column + column_margin + 3 + (*selected_index as i32%4)*item_slot_size,
                        y_offset + 3 + (*selected_index as i32/4)*item_slot_size,
                    );
                    rect(old_sx+1, old_sy+1, item_slot_size-3, item_slot_size-3, 0);
                    spr_outline(selected_item.sprite, sx+2, sy+2-4, SpriteOptions {scale, transparent: &[0], ..Default::default()}, 12);
                    rect_outline(7 , 98, 70, 9, 2, 3);
                    print!(selected_item.name, 9, 100, PrintOptions::default().with_color(12));
                    draw_dialogue_portrait(&DIALOGUE.read().unwrap().fit_text(selected_item.desc), false, selected_item.sprite, 3, 1, 1);
                } else {
                    if let Some(item) = &self.inventory.items[*current_index] {
                        rect_outline(7, 98, 70, 9, 2, 3);
                        print!(item.name, 9, 100, PrintOptions::default().with_color(12));
                        draw_dialogue_portrait(&DIALOGUE.read().unwrap().fit_text(item.desc), false, item.sprite, 3, 1, 1);
                    }
                }
            }
            _ => {}
        };
    }
    pub fn step(&mut self) {
        use crate::mem_btnp;
        let (mut dx, mut dy) = (0, 0);
        if mem_btnp(0) { dy -= 1 }
        if mem_btnp(1) { dy += 1 }
        if mem_btnp(2) { dx -= 1 }
        if mem_btnp(3) { dx += 1 }
        self.state.arrows(dx, dy);
        if mem_btnp(4) { self.click() };
        if mem_btnp(5) { self.state.back() };
    }
}