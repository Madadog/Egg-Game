use crate::{
    drawstate::{DrawState, LayerId, PALETTE_MAP_IDENTITY},
    system::{
        ConsoleApi, ConsoleHelper, PrintOptions, SWEETIE_16, StaticSpriteOptions, pressed,
        drawing::{Canvas, EdgePolicy, Transform},
        drawing::image::{Rgba, RgbaImage},
    },
};

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

    let pad = system.controller();
    {
        let canvas = &mut draw_state.rgba_canvas[LayerId::BG as usize];
        if pressed(pad.b) {
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
        if pressed(pad.a) {
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
        if pressed(pad.x) {
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

        let mouse_pos = system.mouse().pos();
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
    let pad = system.controller();
    if pressed(pad.up) && *indice >= WIDTH {
        *indice = indice.saturating_sub(WIDTH);
    }
    if pressed(pad.down) {
        *indice = indice.saturating_add(WIDTH);
    }
    if pressed(pad.left) && !(*indice).is_multiple_of(WIDTH) {
        *indice = indice.saturating_sub(1);
    }
    if pressed(pad.right) && (*indice % WIDTH) < 2 {
        *indice = indice.saturating_add(1);
    }
}
