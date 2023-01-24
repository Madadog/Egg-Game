use crate::tic80::*;
pub fn palette_map_swap(from: i32, to: u8) {
    let from = from % 16;
    unsafe { poke4(0x3FF0 * 2 + from, to) }
}
pub fn palette_map_reset() {
    for i in 0..=15 {
        unsafe { poke4(0x3FF0 * 2 + i, i as u8) }
    }
}
pub fn palette_map_rotate(amount: u8) {
    for i in 0..=15 {
        unsafe { poke4(0x3FF0 * 2 + i, i as u8 + amount) }
    }
}
pub fn palette(index: i32, rgb: [u8; 3]) {
    let index = index % 16;
    for (i, colour) in rgb.into_iter().enumerate() {
        unsafe { poke(0x3FC0 + index + i as i32, colour) }
    }
}
pub fn set_border(colour: u8) {
    unsafe { poke(0x3FF8, colour) }
}
pub fn screen_offset(horizontal: u8, vertical: u8) {
    unsafe {
        poke(0x3FF9, horizontal);
        poke(0x3FFA, vertical);
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
