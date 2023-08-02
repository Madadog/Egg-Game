use tic80_api::core::MusicOptions;
use tic80_api::helpers::SWEETIE_16;

use crate::data::dialogue_data::GAME_TITLE;
use crate::data::save;
use crate::gamestate::menu::draw_title;
use crate::system::ConsoleApi;
use crate::system::ConsoleHelper;

pub fn draw_animation(t: u16, system: &mut impl ConsoleApi) -> bool {
    let steps: &[u16] = &[0, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.map(|x| t - steps[x.saturating_sub(1)]);
    if let Some(local_time) = local_time {
        match index {
            Some(1) => {
                let max_time = 700.0 - 60.0;
                system.fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
                system.draw_ovr2(|system| {
                    system.fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
                    // This saves a couple kilobytes of wasm binary compared to using powf(0.02)
                    let t = (local_time as f32 / max_time)
                        .sqrt()
                        .sqrt()
                        .sqrt()
                        .sqrt()
                        .sqrt()
                        .sqrt();
                    let size = 200.0 / (max_time + 1.0 - t * max_time).powi(2).max(1.0);
                    let t = size as i32;
                    system.set_palette_colour(15, [0x0F; 3]);
                    system.circ(120, 68, t, 15);
                    system.circb(120, 68, t, 2);
                    let (horizontal, vertical) = (
                        (system.rng().next_u32() % 2) as i8 - 1,
                        (system.rng().next_u32() % 2) as i8 - 1,
                    );
                    if local_time > 400 {
                        if local_time < 450 {
                            if local_time % 3 == 0 {
                                system.screen_offset(horizontal, vertical);
                            }
                        } else {
                            system.screen_offset(horizontal, vertical);
                        }
                    }
                });
                true
            }
            Some(2) => {
                system.fade_palette_colour(15, [0x0F; 3], [26, 28, 44], local_time * 10);
                system.cls(15);
                system.draw_ovr2(|system| {
                    system.screen_offset(0, 0);
                    system.cls(0);
                    system.fade_palette([[0x0F; 3]; 16], SWEETIE_16, local_time * 10);
                    draw_title(system, 120, 53, GAME_TITLE, t as i32);
                });
                true
            }
            Some(0) => {
                system.cls(0);
                system.music(3, MusicOptions::default());
                system.draw_ovr2(|system| {
                    system.cls(0);
                    system.circb(90, 38, 4, 4);
                    system.circb(90, 36, 3, 4);
                    system.circ(90, 38, 3, 12);
                    system.circ(90, 36, 2, 12);
                    for _ in 0..420 {
                        let (x, y) = (
                            system.rng().next_u32() as i32 % 240,
                            system.rng().next_u32() as i32 % 136,
                        );
                        system.pix(
                            x,
                            y,
                            12,
                        );
                    }
                });
                true
            }
            _ => std::process::abort(),
        }
    } else {
        system.music(
            -1,
            MusicOptions {
                frame: 1,
                ..Default::default()
            },
        );
        // Intro has played, skip it on next boot.
        system.memory().set(save::INTRO_ANIM_SEEN);
        system.screen_offset(0, 0);
        system.set_palette(SWEETIE_16);
        system.cls(0);
        draw_title(system, 120, 53, GAME_TITLE, t as i32);
        system.draw_ovr2(|system| system.cls(0));
        false
    }
}
