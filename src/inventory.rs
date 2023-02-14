use crate::dialogue_data::{ITEM_FF_NAME, ITEM_FF_DESC};

static ITEM_FF: InventoryItem = InventoryItem {sprite: 514, name: ITEM_FF_NAME, desc: ITEM_FF_DESC};

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

pub struct Inventory {
    pub items: [Option<i32>; 8],
    pub unlocks: [bool; 4],
}
impl Inventory {
    pub const fn new() -> Self {
        Self {
            // items: [None; 8],
            items: [Some(513), Some(514), None, None, None, None, None, None],
            unlocks: [false; 4],
        }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }
    pub fn take(&mut self, index: usize) -> Option<Option<i32>> {
        self.items.get_mut(index).take().copied()
    }
}

pub enum InventoryUiState {
    PageSelect(i32),
    Items(usize, Option<(usize, i32)>),
    Eggs(usize),
    Close,
}
impl InventoryUiState {
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
    pub fn click(&mut self, inventory: &mut Inventory) {
        match self {
            Self::PageSelect(_) => self.change(),
            Self::Items(new_index, selected_item) => {
                if let Some((old_index, id)) = selected_item {
                    // Put item back down
                    if old_index == new_index {
                        *selected_item = None;
                        return
                    };
                    // Swap items, pick up swapped item if present. 
                    inventory.swap(*new_index, *old_index);
                    if let Some(Some(x)) = inventory.items.get(*old_index) {
                        *id = *x;
                    } else {
                        *selected_item = None;
                    };
                } else {
                    // Pick up item
                    if let Some(Some(x)) = inventory.items.get(*new_index) {
                        *selected_item = Some((*new_index, *x));
                    };
                }
            },
            _ => ()
        }
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

pub struct InventoryUi {
    pub inventory: Inventory,
    pub state: InventoryUiState,
}
impl InventoryUi {
    pub const fn new() -> Self {
        Self {
            inventory: Inventory::new(),
            state: InventoryUiState::PageSelect(0),
        }
    }
    pub fn open(&mut self) {
        self.state = InventoryUiState::PageSelect(0);
    }
    pub fn draw(&self) {
        use crate::tic80::{rect, rectb, cls, print_raw, spr, PrintOptions, WIDTH, HEIGHT, SpriteOptions};
        use crate::tic_helpers::{rect_outline, spr_outline};
        let side_column = 32;
        let column_margin = 2;
        // let main_width = WIDTH - 53 - side_column - column_margin;
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
        // draw side selection
        rect_outline(x_offset, y_offset, side_column, 21, column_colour, column_colour+1);
        rect(x_offset+1,y_offset+8*self.state.page()+3,side_column-2,7,column_colour+1);
        print_raw("Items\0", x_offset+2, y_offset+4, PrintOptions {color: 12, ..Default::default()});
        print_raw("Eggs\0", x_offset+2, y_offset+8+4, PrintOptions {color: 12, ..Default::default()});
        match self.state.page() {
            0 => {
                rect_outline(x_offset + side_column + column_margin,y_offset, main_width, total_height, main_colour, main_colour+1);
                for (i, item) in (0..).zip(self.inventory.items.iter()) {
                    let (sx, sy) = (
                        x_offset + side_column + column_margin + 3 + (i%4)*item_slot_size,
                        y_offset + 3 + (i/4)*item_slot_size,
                    );
                    rect_outline(sx, sy, item_slot_size-1, item_slot_size-1, 0, main_colour+1);
                    if let Some(id) = item {
                        spr(*id, sx+2, sy+2, SpriteOptions {scale, transparent: &[0], ..Default::default()});
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
        match self.state {
            InventoryUiState::Items(current_index, selected) => {
                let (sx, sy) = (
                    x_offset + side_column + column_margin + 3 + (current_index as i32%4)*item_slot_size,
                    y_offset + 3 + (current_index as i32/4)*item_slot_size,
                );
                rectb(sx, sy, item_slot_size-1, item_slot_size-1, 12);
                if let Some((selected_index, selected_id)) = selected {
                    let (old_sx, old_sy) = (
                        x_offset + side_column + column_margin + 3 + (selected_index as i32%4)*item_slot_size,
                        y_offset + 3 + (selected_index as i32/4)*item_slot_size,
                    );
                    rect(old_sx+1, old_sy+1, item_slot_size-3, item_slot_size-3, 0);
                    if current_index != selected_index {
                        if let Some(id) = self.inventory.items[current_index] {
                            spr_outline(id, sx+2, sy+2, SpriteOptions {scale, transparent: &[0], ..Default::default()}, 12);
                        }
                    }
                    spr_outline(selected_id, sx+2, sy+2-4, SpriteOptions {scale, transparent: &[0], ..Default::default()}, 12);
                } else {
                    if let Some(id) = self.inventory.items[current_index] {
                        spr_outline(id, sx+2, sy+2, SpriteOptions {scale, transparent: &[0], ..Default::default()}, 12);
                    }
                }
            }
            _ => {}
        };
        // draw items slot
        // rect_outline(x_offset + side_column + column_margin,y_offset, main_width, total_height, main_colour, main_colour+1);
        // for (i, item) in (0..).zip(self.inventory.items.iter()) {
        //     let (sx, sy) = (
        //         x_offset + side_column + column_margin + 3 + (i%4)*item_slot_size,
        //         y_offset + 3 + (i/4)*item_slot_size,
        //     );
        //     let (colour, outline_colour) = if let Some(index) = item_index {
        //         if i == index as i32 {(0, 12)} else {(0, main_colour+1)}
        //     } else { (0, main_colour+1) };
        //     rect_outline(sx, sy, item_slot_size-1, item_slot_size-1, colour, outline_colour);
        //     if let Some(id) = item {
        //         if let Some(old_item) = old_item {
        //             if i as usize == old_item.0 {continue}
        //         }
        //         if old_item.is_none() && item_index.is_some() && i == item_index.unwrap() as i32 {
        //             spr_outline(*id, sx+2, sy+2, SpriteOptions {scale, transparent: &[0], ..Default::default()}, 12);
        //         } else {
        //             spr(*id, sx+2, sy+2, SpriteOptions {scale, transparent: &[0], ..Default::default()});
        //         }
        //     };
        //     if old_item.is_some() && item_index.is_some() && i == item_index.unwrap() as i32 {
        //         spr_outline(old_item.unwrap().1, sx+2, sy-9, SpriteOptions {scale, transparent: &[0], ..Default::default()}, 12);
        //     }
        // }
    }
    pub fn step(&mut self) {
        use crate::mem_btnp;
        let (mut dx, mut dy) = (0, 0);
        if mem_btnp(0) { dy -= 1 }
        if mem_btnp(1) { dy += 1 }
        if mem_btnp(2) { dx -= 1 }
        if mem_btnp(3) { dx += 1 }
        self.state.arrows(dx, dy);
        if mem_btnp(4) { self.state.click(&mut self.inventory) };
        if mem_btnp(5) { self.state.back() };
    }
}