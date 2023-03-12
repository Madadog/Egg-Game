use crate::data::dialogue_data::GAME_TITLE;
use crate::gamestate::menu::draw_title;
use crate::rand;
use crate::data::save;
use crate::tic80_core::*;
use crate::tic80_helpers::*;

pub fn draw_animation(t: u16) -> bool {
    let steps: &[u16] = &[0, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.map(|x| t - steps[x.saturating_sub(1)]);
    match index {
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
                    pix(rand() as i32 % 240, rand() as i32 % 136, 12)
                }
            });
            true
        }
        Some(1) => {
            let local_time = local_time.unwrap_or_else(|| std::process::abort());
            let max_time = 700.0 - 60.0;
            fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
            draw_ovr(|| {
                fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
                // powf(0.02) is very code-dense, so we approximate it...
                let t = (local_time as f32 / max_time).sqrt().sqrt().sqrt().sqrt().sqrt().sqrt();
                let size = 200.0 / (max_time + 1.0 - t * max_time).powi(2).max(1.0);
                let t = size as i32;
                set_palette_colour(15, [0x0F; 3]);
                circ(120, 68, t, 15);
                circb(120, 68, t, 2);
                if local_time > 400 {
                    if local_time < 450 {
                        if local_time % 3 == 0 {
                            screen_offset((rand() % 2 - 1) as i8, (rand() % 2 - 1) as i8);
                        }
                    } else {
                        screen_offset((rand() % 2 - 1) as i8, (rand() % 2 - 1) as i8);
                    }
                }
            });
            true
        }
        Some(2) => {
            fade_palette_colour(15, [0x0F; 3], [26, 28, 44], local_time.unwrap() * 10);
            cls(15);
            draw_ovr(|| {
                screen_offset(0, 0);
                cls(0);
                fade_palette([[0x0F; 3]; 16], SWEETIE_16, local_time.unwrap() * 10);
                draw_title(120, 53, GAME_TITLE);
            });
            true
        }
        _ => {
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
            draw_title(120, 53, GAME_TITLE);
            draw_ovr(|| cls(0));
            false
        }
    }
}
