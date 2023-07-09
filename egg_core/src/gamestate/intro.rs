use crate::data::dialogue_data::GAME_TITLE;
use crate::data::save;
use crate::gamestate::menu::draw_title;
use crate::rand::Lcg64Xsh32;
use tic80_api::core::*;
use tic80_api::helpers::*;

pub fn draw_animation(t: u16, rng: &mut Lcg64Xsh32) -> bool {
    let steps: &[u16] = &[0, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.map(|x| t - steps[x.saturating_sub(1)]);
    if let Some(local_time) = local_time {
        match index {
            Some(1) => {
                let max_time = 700.0 - 60.0;
                fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
                draw_ovr(|| {
                    fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
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
                    set_palette_colour(15, [0x0F; 3]);
                    circ(120, 68, t, 15);
                    circb(120, 68, t, 2);
                    let (horizontal, vertical) = ((rng.next_u32() % 2 - 1) as i8, (rng.next_u32() % 2 - 1) as i8);
                    if local_time > 400 {
                        if local_time < 450 {
                            if local_time % 3 == 0 {
                                screen_offset(horizontal, vertical);
                            }
                        } else {
                            screen_offset(horizontal, vertical);
                        }
                    }
                });
                true
            }
            Some(2) => {
                fade_palette_colour(15, [0x0F; 3], [26, 28, 44], local_time * 10);
                cls(15);
                draw_ovr(|| {
                    screen_offset(0, 0);
                    cls(0);
                    fade_palette([[0x0F; 3]; 16], SWEETIE_16, local_time * 10);
                    draw_title(120, 53, GAME_TITLE, t as i32);
                });
                true
            }
            Some(0) => {
                cls(0);
                set_palette([[0; 3]; 16]);
                music(3, MusicOptions::default());
                draw_ovr(|| {
                    cls(0);
                    set_palette([[0; 3]; 16]);
                    circb(90, 38, 4, 4);
                    circb(90, 36, 3, 4);
                    circ(90, 38, 3, 12);
                    circ(90, 36, 2, 12);
                    for _ in 0..420 {
                        pix(rng.next_u32() as i32 % 240, rng.next_u32() as i32 % 136, 12)
                    }
                });
                true
            }
            _ => {
                std::process::abort()
            }
        }
    } else {
        music(
            -1,
            MusicOptions {
                frame: 1,
                ..Default::default()
            },
        );
        // Intro has played, skip it on next boot.
        save::INTRO_ANIM_SEEN.set_true();
        screen_offset(0, 0);
        set_palette(SWEETIE_16);
        cls(0);
        draw_title(120, 53, GAME_TITLE, t as i32);
        draw_ovr(|| cls(0));
        false
    }
}
