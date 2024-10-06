use tic80_api::core::{PrintOptions, SpriteOptions, StaticSpriteOptions};

use crate::system::ConsoleApi;

use super::walkaround::WalkaroundState;

const WIDTH: u32 = 32;
const WIDTHX10: u32 = WIDTH * 10;

pub fn draw_sprite_test(system: &mut impl ConsoleApi, indice: u32) {
    system.cls(0);
    for x in 0..(WIDTH as i32) {
        for y in 0..16 {
            system.spr(x+y*(WIDTH as i32)+indice as i32, x * 8, y * 8, StaticSpriteOptions::default());
        }
    }
    if system.btn(4) {
        for i in 0..255 {
            system.print_alloc("PALETTE:", 0, 0, PrintOptions {
                color: 12,
                ..PrintOptions::default()
            });
            system.pix(10+i%32, 10+i/32, i as u8);
        }
    }
}

pub fn step_sprite_test(system: &mut impl ConsoleApi, indice: &mut u32) {
    if system.btn(0) {
        *indice = indice.saturating_sub(WIDTH);
    }
    if system.btn(1) {
        *indice = indice.saturating_add(WIDTH);
    }
    if system.btnp(2, 0, 0) {
        *indice = indice.saturating_sub(1);
    }
    if system.btnp(3,0,0) {
        *indice = indice.saturating_add(1);
    }
}

pub struct MapViewer {
    layer_index: usize,
}
impl MapViewer {
    pub fn draw_map_viewer(&self, system: &mut impl ConsoleApi, walkaround: &mut WalkaroundState) {
        system.map_draw()
    }

    pub fn step_map_viewer(&mut self, system: &mut impl ConsoleApi, walkaround: &mut WalkaroundState) {
        system.map_draw()
    }
}