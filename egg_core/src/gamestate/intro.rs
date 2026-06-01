use crate::data::dialogue_data::GAME_TITLE;
use crate::data::sound::music::MusicTrack;
use crate::drawstate::{DrawState, LayerId::*, fade_colour_into, fade_palette_into};
use crate::gamestate::menu::draw_title_indexed;
use crate::system::drawing::{Canvas, EdgePolicy};
use crate::system::{ConsoleApi, HEIGHT, SWEETIE_16, WIDTH};

pub fn draw_animation(t: u16, draw_state: &mut DrawState, system: &mut impl ConsoleApi) -> bool {
    let steps: &[u16] = &[0, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.map(|x| t - steps[x.saturating_sub(1)]);
    let mut screen_offset = [0i8; 2];

    if let Some(local_time) = local_time {
        match index {
            Some(0) => {
                // Black BG, Oblong sun + starfield on FG.
                draw_state.indexed_canvas[BG as usize].fill(0);
                system.music(Some(&MusicTrack::INTRO));
                let fg = &mut draw_state.indexed(FG);
                fg.fill(0);
                fg.stroke_circle(90, 38, 4, 4);
                fg.stroke_circle(90, 36, 3, 4);
                fg.fill_circle(90, 38, 3, 12);
                fg.fill_circle(90, 36, 2, 12);
                let (fw, fh) = (fg.width(), fg.height());
                for _ in 0..420 {
                    let x = system.rng().next_u32() as i32 % WIDTH;
                    let y = system.rng().next_u32() as i32 % HEIGHT;
                    if x >= 0 && y >= 0 && (x as u32) < fw && (y as u32) < fh {
                        fg.set_pixel(x as u32, y as u32, 12);
                    }
                }
            }
            Some(1) => {
                // Growing circle with palette fading in from black.
                let max_time = 700.0 - 60.0;
                fade_palette_into(
                    &mut draw_state.palettes[0],
                    &[[0; 3]; 16],
                    &SWEETIE_16,
                    local_time * 2,
                );
                let t = (local_time as f32 / max_time).powf(0.02);
                let size = 200.0 / (max_time + 1.0 - t * max_time).powi(2).max(1.0);
                let t = size as i32;
                if let Some(slot) = draw_state.palettes[0].get_mut(15) {
                    *slot = [0x0F; 3];
                }
                let fg = &mut draw_state.indexed(FG);
                fg.fill_circle(120, 68, t, 15);
                fg.stroke_circle(120, 68, t, 2);
                let (horizontal, vertical) = (
                    (system.rng().next_u32() % 2) as i8 - 1,
                    (system.rng().next_u32() % 2) as i8 - 1,
                );
                if local_time > 400 {
                    if local_time < 450 {
                        if local_time % 3 == 0 {
                            screen_offset = [horizontal, vertical];
                        }
                    } else {
                        screen_offset = [horizontal, vertical];
                    }
                }
            }
            Some(2) => {
                // Blackout fading to title screen.
                // palette[15] fades to palette[0] otherwise
                // the transition to the title is rough
                fade_palette_into(
                    &mut draw_state.palettes[0],
                    &[[0x0F; 3]; 16],
                    &SWEETIE_16,
                    local_time * 10,
                );
                if let Some(slot) = draw_state.palettes[0].get_mut(15) {
                    fade_colour_into(slot, [0x0F; 3], [26, 28, 44], local_time * 10);
                }
                draw_state.indexed(BG).fill(15);
                draw_state.indexed(FG).fill(0);
                let fg = &mut draw_state.indexed_canvas[FG as usize];
                draw_title_indexed(
                    fg,
                    &draw_state.indexed_sprites,
                    system,
                    120,
                    53,
                    GAME_TITLE,
                    t as i32,
                );
            }
            _ => (),
        }
        compose_intro_layers(draw_state, system, screen_offset);
        true
    } else {
        // Intro complete: set save flag, reset palette, show title.
        system.music(None);
        system.memory().intro_anim_seen = true;
        draw_state.set_palette(&SWEETIE_16);
        draw_state.indexed(BG).fill(0);
        draw_state.indexed(FG).fill(0);
        let fg = &mut draw_state.indexed_canvas[FG as usize];
        draw_title_indexed(
            fg,
            &draw_state.indexed_sprites,
            system,
            120,
            53,
            GAME_TITLE,
            t as i32,
        );
        compose_intro_layers(draw_state, system, [0, 0]);
        false
    }
}

fn compose_intro_layers(draw_state: &DrawState, system: &mut impl ConsoleApi, offset: [i8; 2]) {
    let palette = draw_state.palettes[0].as_slice();
    let output = system.output_image();
    draw_state.indexed_canvas[BG as usize].draw_to_rgba(
        output,
        0,
        0,
        palette,
        &[],
        EdgePolicy::Transparent,
    );
    // Clamp on the offset FG so screen-shake doesn't expose a transparent
    // seam at the trailing edge.
    draw_state.indexed_canvas[FG as usize].draw_to_rgba(
        output,
        offset[0] as i32,
        offset[1] as i32,
        palette,
        &[0],
        EdgePolicy::Clamp,
    );
}
