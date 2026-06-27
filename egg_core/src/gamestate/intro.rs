use crate::Ctx;
use crate::data::sound::music::MusicTrack;
use crate::draw_state::{DrawState, LayerId::*, fade_colour_into, fade_palette_into};
use crate::gamestate::GameMode;
use crate::gamestate::menu::draw_title_indexed;
use crate::render::{Canvas, EdgePolicy};
use crate::platform::{ConsoleApi, ConsoleHelper, SWEETIE_16, pressed};

/// The startup intro animation: a frame counter ticking through
/// [`draw_animation`] until it finishes (or is skipped with B), then it hands off
/// to the title's main menu.
#[derive(Debug, Default)]
pub struct IntroAnimation {
    frame: u16,
}
impl IntroAnimation {
    pub fn step(&mut self, ctx: &mut Ctx<impl ConsoleApi>) -> Option<GameMode> {
        // Already played this save — skip straight to the menu.
        if ctx.save.intro_anim_seen {
            return Some(GameMode::MainMenu);
        }
        // Hold B to fast-forward past the intro.
        if pressed(ctx.system.controller().b) {
            self.frame = self.frame.saturating_add(1000);
        }
        if draw_animation(self.frame, ctx) {
            self.frame += 1;
            None
        } else {
            Some(GameMode::MainMenu)
        }
    }
}

pub fn draw_animation(t: u16, ctx: &mut Ctx<impl ConsoleApi>) -> bool {
    let steps: &[u16] = &[0, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.map(|x| t - steps[x.saturating_sub(1)]);
    let mut screen_offset = [0i8; 2];

    if let Some(local_time) = local_time {
        match index {
            Some(0) => {
                // Black BG, Oblong sun + starfield on FG.
                ctx.draw.set_palette(&[[0; 3]; 16]);
                ctx.draw.indexed_canvas[BG as usize].fill(0);
                ctx.system.music(Some(&MusicTrack::named("intro")));
                let fg = &mut ctx.draw.indexed(FG);
                fg.fill(0);
                // Centre the whole composition on the canvas (the oblong sun sits
                // 30px left of centre), so the intro re-centres with the
                // framebuffer size rather than hugging the top-left.
                let (fw, fh) = (fg.width(), fg.height());
                let (cx, cy) = (fw as i32 / 2, fh as i32 / 2);
                fg.stroke_circle(cx - 30, cy - 30, 4, 4);
                fg.stroke_circle(cx - 30, cy - 32, 3, 4);
                fg.fill_circle(cx - 30, cy - 30, 3, 12);
                fg.fill_circle(cx - 30, cy - 32, 2, 12);
                for _ in 0..420 {
                    let x = ctx.rng.next_u32() as i32 % (fw as i32);
                    let y = ctx.rng.next_u32() as i32 % (fh as i32);
                    if x >= 0 && y >= 0 && (x as u32) < fw && (y as u32) < fh {
                        fg.set_pixel(x as u32, y as u32, 12);
                    }
                }
            }
            Some(1) => {
                // Growing circle with palette fading in from black.
                let max_time = 700.0 - 60.0;
                fade_palette_into(
                    &mut ctx.draw.palettes[0],
                    &[[0; 3]; 16],
                    &SWEETIE_16,
                    local_time * 2,
                );
                let t = (local_time as f32 / max_time).powf(0.02);
                let size = 200.0 / (max_time + 1.0 - t * max_time).powi(2).max(0.1);
                let t = size as i32;
                if let Some(slot) = ctx.draw.palettes[0].get_mut(15) {
                    *slot = [0x0F; 3];
                }
                let fg = &mut ctx.draw.indexed(FG);
                let (cx, cy) = (fg.width() as i32 / 2, fg.height() as i32 / 2);
                fg.fill_circle(cx, cy, t, 15);
                fg.stroke_circle(cx, cy, t, 2);
                let (horizontal, vertical) = (
                    (ctx.rng.next_u32() % 2) as i8 - 1,
                    (ctx.rng.next_u32() % 2) as i8 - 1,
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
                    &mut ctx.draw.palettes[0],
                    &[[0x0F; 3]; 16],
                    &SWEETIE_16,
                    local_time * 10,
                );
                if let Some(slot) = ctx.draw.palettes[0].get_mut(15) {
                    fade_colour_into(slot, [0x0F; 3], [26, 28, 44], local_time * 10);
                }
                ctx.draw.indexed(BG).fill(15);
                ctx.draw.indexed(FG).fill(0);
                let fg = &mut ctx.draw.indexed_canvas[FG as usize];
                let ty = fg.height() as i32 / 2 - 15;
                draw_title_indexed(
                    fg,
                    &ctx.draw.indexed_sprites,
                    ctx.font,
                    ctx.script,
                    ty,
                    &ctx.script.label("game_title"),
                    t as i32,
                );
            }
            _ => (),
        }
        compose_intro_layers(ctx.draw, ctx.system, screen_offset);
        true
    } else {
        // Intro complete: set save flag, reset palette, show title.
        ctx.system.music(None);
        ctx.save.intro_anim_seen = true;
        ctx.draw.set_palette(&SWEETIE_16);
        ctx.draw.indexed(BG).fill(0);
        ctx.draw.indexed(FG).fill(0);
        let fg = &mut ctx.draw.indexed_canvas[FG as usize];
        let ty = fg.height() as i32 / 2 - 15;
        draw_title_indexed(
            fg,
            &ctx.draw.indexed_sprites,
            ctx.font,
            ctx.script,
            ty,
            &ctx.script.label("game_title"),
            t as i32,
        );
        compose_intro_layers(ctx.draw, ctx.system, [0, 0]);
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
