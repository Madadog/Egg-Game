use crate::tic80::*;

/// Ported straight from TIC80's poke4 function.
/// I hope this is safe.
unsafe fn rust_poke4(index: i32, value: u8) {
    // #define POKE_N(P,I,V,A,B,C,D) do        \
    // {                                       \
    //     u8* val = (u8*)(P) + ((I) >> (A));  \
    //     u8 offset = ((I) & (B)) << (C);     \
    //     *val &= ~((D) << offset);           \
    //     *val |= ((V) & (D)) << offset;      \
    // } while(0)
    /*#define PEEK_N(P,I,A,B,C,D) ( ( ((u8*)(P))[((I) >> (A))] >> ( ((I) & (B)) << (C) ) ) & (D) )
    
    inline void tic_tool_poke4(void* addr, u32 index, u8 value)
    {
        POKE_N(addr, index, value, 1,1,2,15);
    }*/
    
    // Clamp to TIC80 reserved RAM.
    let index = index.clamp(0, 98322*2);
    let val: *mut u8 = (index as usize >> 1) as *mut u8;
    let offset: u8 = (((index) & (1)) as u8) << (2);
    unsafe {
        *val &= !((15) << offset);
        *val |= ((value) & (15)) << offset;
    }
}

pub fn palette_map_swap(from: u8, to: u8) {
    let from: i32 = (from % 16).into();
    assert!(from >= 0);
    unsafe { rust_poke4(PALETTE_MAP as i32 * 2 + from, to % 16) }
}

pub fn palette_map_set_all(to: u8) {
    for i in 0..=15 {
        unsafe { rust_poke4(PALETTE_MAP as i32 * 2 + i, to % 16) }
    }
}

pub fn set_palette_map(map: [u8; 16]) {
    for (i, item) in map.into_iter().enumerate() {
        unsafe { rust_poke4(PALETTE_MAP as i32 * 2 + i as i32, item % 16) }
    }
}

pub fn palette_map_reset() {
    for i in 0..=15 {
        unsafe { rust_poke4(PALETTE_MAP as i32 * 2 + i, i as u8) }
    }
}

pub fn palette_map_rotate(amount: u8) {
    for i in 0..=15 {
        unsafe { rust_poke4(PALETTE_MAP as i32 * 2 + i, i as u8 + amount) }
    }
}

pub fn get_palette_map() -> [u8; 16] {
    let mut palette_map = [0; 16];
    for (i, x) in palette_map.iter_mut().enumerate() {
        unsafe { *x = peek4(PALETTE_MAP as i32 * 2 + i as i32) }
    }
    palette_map
}

pub fn palette(index: u8, rgb: [u8; 3]) {
    let index: usize = (index % 16).into();
    for (i, colour) in rgb.into_iter().enumerate() {
        unsafe { (*PALETTE)[index + i] = colour}
    }
}

pub fn set_border(colour: u8) {
    unsafe { *BORDER_COLOR = colour }
}

pub fn screen_offset(horizontal: u8, vertical: u8) {
    unsafe {
        (*SCREEN_OFFSET)[0] = horizontal;
        (*SCREEN_OFFSET)[1] = vertical;
    }
}

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
    unsafe { poke(0x3FFC, value) }
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

pub const SWEETIE_16: [[u8; 3]; 16] = [
    [26, 28, 44],// #1a1c2c
    [93, 39, 93],// #5d275d
    [177, 62, 83],// #b13e53
    [239, 125, 87],// #ef7d57
    [255, 205, 117],// #ffcd75
    [167, 240, 112],// #a7f070
    [56, 183, 100],// #38b764
    [37, 113, 121],// #257179
    [41, 54, 111],// #29366f
    [59, 93, 201],// #3b5dc9
    [65, 166, 246],// #41a6f6
    [115, 239, 247],// #73eff7
    [244, 244, 244],// #f4f4f4
    [148, 176, 194],// #94b0c2
    [86, 108, 134],// #566c86
    [51, 60, 87],// #333c57
];
