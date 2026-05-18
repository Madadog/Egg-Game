use tic80_api::{core::{MapOptions, PrintOptions, StaticSpriteOptions}, helpers::SWEETIE_16};

use crate::{
    map::MapInfo,
    system::{ConsoleApi, ConsoleHelper},
};

use super::walkaround::WalkaroundState;

const WIDTH: u32 = 32;

pub fn draw_sprite_test(system: &mut impl ConsoleApi, indice: u32) {
    system.cls(0);
    system.set_palette(SWEETIE_16);
    system.draw_ovr2(|system| {system.cls(0)});
    for x in 0..(WIDTH as i32) {
        for y in 0..17 {
            system.spr(
                x + y * (WIDTH as i32) + indice as i32,
                x * 8,
                y * 8,
                StaticSpriteOptions::default(),
            );
        }
    }
    if system.btn(5) {
        let size = system.screen_size();
        for x in 0..(size.0 as i32) {
            for y in 0..(size.1 as i32) {
                let color = system.get_bitmap_indexed(2)
                    [(x + y * 256) as usize + ((indice % 32) * 8 + (indice / 32) * 2048) as usize];
                system.pix(x, y, color);
            }
        }
        system.print_alloc(
            "RAW DATA:",
            0,
            0,
            PrintOptions {
                color: 12,
                ..PrintOptions::default()
            },
        );
    }
    if system.btn(4) {
        for i in 0..255 {
            system.print_alloc(
                "PALETTE:",
                0,
                0,
                PrintOptions {
                    color: 12,
                    ..PrintOptions::default()
                },
            );
            system.pix(10 + i % 32, 10 + i / 32, i as u8);
        }
    }
    if system.btn(6) {
        system.rectb(0, 0, 8, 8, 12);
        system.print_alloc(
            format!("Sprite ID = {indice}"),
            0,
            8,
            PrintOptions {
                color: 12,
                ..PrintOptions::default()
            },
        );
    }
    let mouse_pos = system.mouse();
    let mouse = system.mouse_delta();
    let grid_index = (i32::from(mouse_pos.x / 8), i32::from(mouse_pos.y / 8));
    let mouse_indice = indice as i32 + grid_index.0 + grid_index.1 * WIDTH as i32;
    let (grid_x, grid_y) = (grid_index.0 * 8, grid_index.1 * 8);
    let flip_text = if grid_index.1 == 0 {
        15
    } else {
        0
    };
    system.rectb(grid_x, grid_y, 8, 8, 12);
    system.print_alloc_centered(
        &format!("ID:{}", mouse_indice),
        grid_x + 4,
        grid_y - 6 + flip_text,
        PrintOptions {
            color: 12,
            ..PrintOptions::default()
        },
    );

    if mouse.left {}
}

pub fn step_sprite_test(system: &mut impl ConsoleApi, indice: &mut u32) {
    if system.btn(0) && *indice >= WIDTH  {
        *indice = indice.saturating_sub(WIDTH);
    }
    if system.btn(1) {
        *indice = indice.saturating_add(WIDTH);
    }
    if system.btn(2) && !(*indice).is_multiple_of(WIDTH) {
        *indice = indice.saturating_sub(1);
    }
    if system.btn(3) && (*indice % WIDTH) < 2 {
        *indice = indice.saturating_add(1);
    }
}

#[derive(Debug, Clone, Default)]
pub struct MapViewer {
    pub focused: bool,
    pub fg: bool,
    pub layer_index: usize,
}
impl MapViewer {
    pub fn draw_map_viewer(&self, system: &mut impl ConsoleApi, walkaround: &WalkaroundState) {
        //walkaround.draw_map();
        if !self.focused {
            return;
        }
        let (_width, height) = system.screen_size();
        system.rect(0, 0, 70, height as i32, 0);

        system.rect(0, 8 + 8 * self.layer_index as i32, 70, 8, 15);

        let (layers, title) = if self.fg {
            (walkaround.current_map.fg_layers.iter().enumerate(), "FG")
        } else {
            (walkaround.current_map.layers.iter().enumerate(), "BG")
        };
        system.print_alloc(
            format!("{title} LAYERS:"),
            0,
            0,
            PrintOptions::default().with_color(13),
        );
        for (i, layer) in layers {
            let text = if layer.visible { "" } else { "(Hidden)" };
            system.print_alloc(
                format!("Layer {} {text}", i),
                0,
                8 + 8 * i as i32,
                PrintOptions {
                    color: 12,
                    small_text: true,
                    ..PrintOptions::default()
                },
            );
        }
        // MapViewer::draw_map_data(
        //     system,
        //     MapOptions {
        //         x: 0,
        //         y: 0,
        //         w: walkaround.current_map.layers[self.layer_index].size.x() as i32,
        //         h: walkaround.current_map.layers[self.layer_index].size.y() as i32,
        //         sx: 0,
        //         sy: 0,
        //         transparent: None,
        //         scale: 1,
        //     },
        //     walkaround.current_map.bank,
        //     self.layer_index,
        // );
    }

    pub fn step_map_viewer(&mut self, system: &mut impl ConsoleApi, map: &mut MapInfo) {
        if system.btnp(0, 0, 0) {
            self.layer_index = self.layer_index.saturating_sub(1);
        }
        if system.btnp(1, 0, 0) {
            self.layer_index = (self.layer_index + 1).min(map.layers.len() - 1);
        }
        if system.btnp(4, 0, 0) {
            let layers = if self.fg {
                map.fg_layers.get_mut(self.layer_index)
            } else {
                map.layers.get_mut(self.layer_index)
            };
            if let Some(layer) = layers {
                layer.visible = !layer.visible;
            }
        }
        if system.btnp(5, 0, 0) {
            self.fg = !self.fg;
        }
    }

    pub fn draw_map_data(
        system: &mut impl ConsoleApi,
        opts: MapOptions,
        bank: usize,
        layer: usize,
    ) {
        for i in 0..opts.w {
            for j in 0..opts.h {
                let (x, y) = (8 * i, 8 * j);
                system.rectb(x, y, 8, 8, 1);
                system.print_alloc(
                    format!("{}", system.map_get(bank, layer, i, i)),
                    x,
                    y,
                    PrintOptions {
                        color: 12,
                        small_text: true,
                        ..PrintOptions::default()
                    },
                );
            }
        }
    }
}
