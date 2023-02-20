// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
// 
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
// 
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
// 
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

use crate::tic80::*;

pub fn palette_map_swap(from: u8, to: u8) {
    let from: i32 = (from % 16).into();
    assert!(from >= 0);
    unsafe { poke4(PALETTE_MAP as i32 * 2 + from, to % 16) }
}

pub fn palette_map_set_all(to: u8) {
    for i in 0..=15 {
        unsafe { poke4(PALETTE_MAP as i32 * 2 + i, to % 16) }
    }
}

pub fn set_palette_map(map: [u8; 16]) {
    for (i, item) in map.into_iter().enumerate() {
        unsafe { poke4(PALETTE_MAP as i32 * 2 + i as i32, item % 16) }
    }
}

pub fn palette_map_reset() {
    for i in 0..=15 {
        unsafe { poke4(PALETTE_MAP as i32 * 2 + i, i as u8) }
    }
}

pub fn palette_map_rotate(amount: u8) {
    for i in 0..=15 {
        unsafe { poke4(PALETTE_MAP as i32 * 2 + i, i as u8 + amount) }
    }
}

pub fn get_palette_map() -> [u8; 16] {
    let mut palette_map = [0; 16];
    for (i, x) in palette_map.iter_mut().enumerate() {
        unsafe { *x = peek4(PALETTE_MAP as i32 * 2 + i as i32) }
    }
    palette_map
}

pub fn set_palette_colour(index: u8, rgb: [u8; 3]) {
    let index: usize = (index % 16).into();
    for (i, colour) in rgb.into_iter().enumerate() {
        unsafe { (*PALETTE)[index * 3 + i] = colour }
    }
}

pub fn set_palette(colours: [[u8; 3]; 16]) {
    for (i, colour) in colours.iter().enumerate() {
        set_palette_colour(i as u8, *colour);
    }
}

pub fn get_palette() -> [[u8; 3]; 16] {
    let mut palette = [[0; 3]; 16];
    for (from, to) in palette
        .iter_mut()
        .flatten()
        .zip(unsafe { (*PALETTE).iter() })
    {
        *from = *to;
    }
    palette
}

/// Lerps between 2 colour palettes. `amount` is an interpolation amount, ranging from `0..=256`.
pub fn fade_palette(from: [[u8; 3]; 16], to: [[u8; 3]; 16], amount: u16) {
    let amount = amount.min(256);
    for (index, (colour1, colour2)) in from.iter().zip(to.iter()).enumerate() {
        let mut rgb = [0; 3];
        for (j, (component1, component2)) in colour1.iter().zip(colour2.iter()).enumerate() {
            rgb[j] =
                ((*component1 as u16 * (256 - amount) + *component2 as u16 * amount) >> 8) as u8;
        }
        set_palette_colour(index as u8, rgb);
    }
}
pub fn fade_palette_colour(index: u8, from: [u8; 3], to: [u8; 3], amount: u16) {
    let amount = amount.min(256);
    let index: usize = (index % 16).into();
    let mut rgb = [0; 3];
    for (j, (component1, component2)) in from.iter().zip(to.iter()).enumerate() {
        rgb[j] = ((*component1 as u16 * (256 - amount) + *component2 as u16 * amount) >> 8) as u8;
    }
    set_palette_colour(index as u8, rgb);
}

pub fn set_border(colour: u8) {
    unsafe { *BORDER_COLOR = colour }
}

pub fn screen_offset(horizontal: i8, vertical: i8) {
    unsafe {
        (*SCREEN_OFFSET)[0] = horizontal as u8;
        (*SCREEN_OFFSET)[1] = vertical as u8;
    }
}

pub fn draw_ovr<T: Fn()>(draw: T) {
    unsafe {
        vbank(1);
    }
    draw();
    unsafe {
        vbank(0);
    }
}

pub fn get_pmem(address: usize) -> u8 {
    let address = address.min(1023);
    unsafe { (*PERSISTENT_RAM)[address] }
}

pub fn set_pmem(address: usize, value: u8) {
    let address = address.min(1023);
    unsafe { (*PERSISTENT_RAM)[address] = value }
}

/// Valid values:
///
/// 0000 SYS GFX
/// 0001 FONT
///
/// 0010 4bpp BG Page 0
/// 0011 4bpp FG Page 0
///
/// 0100 2bpp BG Page 0
/// 0101 2bpp BG Page 1
/// 0110 2bpp FG Page 0
/// 0111 2bpp FG Page 1
///
/// 1000 1bpp BG Page 0
/// 1001 1bpp BG Page 1
/// 1010 1bpp BG Page 2
/// 1011 1bpp BG Page 3
/// 1100 1bpp FG Page 0
/// 1101 1bpp FG Page 1
/// 1110 1bpp FG Page 2
/// 1111 1bpp FG Page 3
pub fn blit_segment(value: u8) {
    unsafe { *BLIT_SEGMENT = value }
}
pub fn get_blit_segment() -> u8 {
    unsafe { *BLIT_SEGMENT }
}
pub fn spr_blit_segment(id: i32, x: i32, y: i32, opts: SpriteOptions, blit_seg: u8) {
    let old = get_blit_segment();
    blit_segment(blit_seg);
    spr(id, x, y, opts);
    blit_segment(old);
}

pub fn spr_outline(id: i32, x: i32, y: i32, sprite_options: SpriteOptions, outline_colour: u8) {
    let old_map = get_palette_map();
    palette_map_set_all(outline_colour);
    spr(id, x + 1, y, sprite_options.clone());
    spr(id, x - 1, y, sprite_options.clone());
    spr(id, x, y + 1, sprite_options.clone());
    spr(id, x, y - 1, sprite_options.clone());
    set_palette_map(old_map);
    spr(id, x, y, sprite_options);
}

pub fn rect_outline(x: i32, y: i32, w: i32, h: i32, fill: u8, outline: u8) {
    rect(x, y, w, h, fill);
    rectb(x, y, w, h, outline);
}

pub fn print_raw_centered(string: &str, x: i32, y: i32, options: PrintOptions) {
    let string_width = print_raw(string, 999, 999, options.clone());
    print_raw(string, x - string_width / 2, y, options);
}

pub struct SyncHelper {
    synced: bool,
    last_bank: u8,
}

impl SyncHelper {
    pub const fn new() -> Self {
        SyncHelper { synced: false, last_bank: 0 }
    }
    pub fn step(&mut self) {
        self.synced = false;
    }
    /// Sync can only be called once per frame. Returns result to indicate failure or success.
    /// Mask lets you switch out sections of cart data:
    /// * all     = 0    -- 0
    /// * tiles   = 1<<0 -- 1
    /// * sprites = 1<<1 -- 2
    /// * map     = 1<<2 -- 4
    /// * sfx     = 1<<3 -- 8
    /// * music   = 1<<4 -- 16
    /// * palette = 1<<5 -- 32
    /// * flags   = 1<<6 -- 64
    /// * screen  = 1<<7 -- 128 (as of 0.90)
    pub fn sync(&mut self, mask: i32, bank: u8) -> Result<(), ()> {
        if self.synced {Err(())}
        else {
            self.synced = true;
            self.last_bank = bank;
            unsafe { sync(mask, bank, false) };
            Ok(())
        }
    }
    pub fn is_synced(&self) -> bool {self.synced}
    pub fn last_bank(&self) -> u8 {self.last_bank}
}

pub const SWEETIE_16: [[u8; 3]; 16] = [
    [26, 28, 44],    // #1a1c2c
    [93, 39, 93],    // #5d275d
    [177, 62, 83],   // #b13e53
    [239, 125, 87],  // #ef7d57
    [255, 205, 117], // #ffcd75
    [167, 240, 112], // #a7f070
    [56, 183, 100],  // #38b764
    [37, 113, 121],  // #257179
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [115, 239, 247], // #73eff7
    [244, 244, 244], // #f4f4f4
    [148, 176, 194], // #94b0c2
    [86, 108, 134],  // #566c86
    [51, 60, 87],    // #333c57
];
pub const NIGHT_16: [[u8; 3]; 16] = [
    [10, 10, 10],    // #0a0a0a
    [26, 28, 44],    // #1a1c2c
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [115, 239, 247], // #73eff7
    [167, 240, 112], // #a7f070
    [56, 183, 100],  // #38b764
    [37, 113, 121],  // #257179
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [244, 244, 244], // #f4f4f4
    [115, 239, 247], // #73eff7
    [148, 176, 194], // #94b0c2
    [86, 108, 134],  // #566c86
];
