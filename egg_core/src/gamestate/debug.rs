use crate::system::{PrintOptions, SWEETIE_16, StaticSpriteOptions};

use crate::{
    drawstate::{DrawState, LayerId, PALETTE_MAP_IDENTITY},
    map::MapInfo,
    system::{
        ConsoleApi, ConsoleHelper,
        drawing::{Canvas, EdgePolicy, Transform},
        image::{Rgba, RgbaImage},
    },
};

use super::walkaround::WalkaroundState;

const WIDTH: u32 = 32;

pub fn draw_sprite_test(draw_state: &mut DrawState, system: &mut impl ConsoleApi, indice: u32) {
    draw_state.set_palette(&SWEETIE_16);

    let black = Rgba::from_rgb(draw_state.palettes[0][0]);
    let white = Rgba::from_rgb(draw_state.palettes[0][12]);
    let print_opts = PrintOptions {
        color: 12,
        ..PrintOptions::default()
    };

    draw_state.rgba(LayerId::BG).fill(black);

    let palette_map = PALETTE_MAP_IDENTITY;
    for x in 0..(WIDTH as i32) {
        for y in 0..17 {
            draw_state.rgba_canvas[LayerId::BG as usize].spr_indexed(
                &draw_state.indexed_sprites,
                &draw_state.palettes[0],
                &palette_map,
                x + y * (WIDTH as i32) + indice as i32,
                x * 8,
                y * 8,
                StaticSpriteOptions::default(),
            );
        }
    }

    {
        let canvas = &mut draw_state.rgba_canvas[LayerId::BG as usize];
        if system.btn(5) {
            // Raw indexed sprite bytes as colour-mapped pixels.
            let palette = draw_state.palettes[0].as_slice();
            let data = &draw_state.indexed_sprites.data;
            let offset = ((indice % 32) * 8 + (indice / 32) * 2048) as usize;
            for y in 0..(canvas.height() as i32) {
                for x in 0..(canvas.width() as i32) {
                    let idx = match data.get((x + y * 256) as usize + offset) {
                        Some(&i) => i,
                        None => continue,
                    };
                    if let Some(rgb) = palette.get(idx as usize) {
                        canvas.set_pixel(x as u32, y as u32, Rgba::from_rgb(*rgb));
                    }
                }
            }
            system.print_to(canvas, "RAW DATA:", 0, 0, white, print_opts.clone());
        }
        if system.btn(4) {
            for i in 0..255i32 {
                system.print_to(canvas, "PALETTE:", 0, 0, white, print_opts.clone());
                let px = 10 + i % 32;
                let py = 10 + i / 32;
                if px >= 0
                    && py >= 0
                    && (px as u32) < canvas.width()
                    && (py as u32) < canvas.height()
                    && let Some(rgb) = draw_state.palettes[0].get(i as usize)
                {
                    canvas.set_pixel(px as u32, py as u32, Rgba::from_rgb(*rgb));
                }
            }
        }
        if system.btn(6) {
            canvas.stroke_rect(0, 0, 8, 8, white);
            system.print_to(
                canvas,
                &format!("Sprite ID = {indice}"),
                0,
                8,
                white,
                print_opts.clone(),
            );
        }

        let mouse_pos = system.mouse();
        let grid_index = (i32::from(mouse_pos.x / 8), i32::from(mouse_pos.y / 8));
        let mouse_indice = indice as i32 + grid_index.0 + grid_index.1 * WIDTH as i32;
        let (grid_x, grid_y) = (grid_index.0 * 8, grid_index.1 * 8);
        let flip_text = if grid_index.1 == 0 { 15 } else { 0 };
        canvas.stroke_rect(grid_x, grid_y, 8, 8, white);
        system.print_to_centered(
            canvas,
            &format!("ID:{mouse_indice}"),
            grid_x + 4,
            grid_y - 6 + flip_text,
            white,
            print_opts,
        );
    }

    let output = system.output_image();
    output.blit::<RgbaImage>(
        0,
        0,
        &draw_state.rgba(LayerId::BG),
        EdgePolicy::Transparent,
        Transform::IDENTITY,
        |p| p.a() == 0,
    );
}

pub fn step_sprite_test(system: &mut impl ConsoleApi, indice: &mut u32) {
    if system.btn(0) && *indice >= WIDTH {
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
    pub fn draw_map_viewer(
        &self,
        draw_state: &mut DrawState,
        system: &mut impl ConsoleApi,
        walkaround: &WalkaroundState,
    ) {
        if !self.focused {
            return;
        }
        let height = draw_state.rgba(LayerId::BG).height() as i32;
        let c0 = draw_state.colour(0);
        let c12 = draw_state.colour(12);
        let c13 = draw_state.colour(13);
        let c15 = draw_state.colour(15);
        draw_state.rgba(LayerId::BG).fill_rect(0, 0, 70, height, c0);
        draw_state
            .rgba(LayerId::BG)
            .fill_rect(0, 8 + 8 * self.layer_index as i32, 70, 8, c15);

        let (layers, title) = if self.fg {
            (walkaround.current_map.fg_layers.iter().enumerate(), "FG")
        } else {
            (walkaround.current_map.layers.iter().enumerate(), "BG")
        };
        system.print_to(
            draw_state.rgba(LayerId::BG),
            &format!("{title} LAYERS:"),
            0,
            0,
            c13,
            PrintOptions::default().with_color(13),
        );
        for (i, layer) in layers {
            let text = if layer.visible { "" } else { "(Hidden)" };
            system.print_to(
                draw_state.rgba(LayerId::BG),
                &format!("Layer {} {text}", i),
                0,
                8 + 8 * i as i32,
                c12,
                PrintOptions {
                    color: 12,
                    small_text: true,
                    ..PrintOptions::default()
                },
            );
        }
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
}
