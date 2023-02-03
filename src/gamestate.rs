use crate::{ANIMATIONS, DIALOGUE, TIME};
use crate::{current_map, load_map, player, player_mut, camera, camera_mut, rand};
use crate::position::{Hitbox, Vec2, touches_tile};
use crate::tic80::*;
use crate::map_data::*;
use crate::interact::Interaction;
use crate::{trace, print};

use crate::tic_helpers::{palette_map_reset, palette_map_rotate, blit_segment, spr_outline, draw_ovr, SWEETIE_16, fade_palette, set_palette, set_palette_colour, screen_offset, get_pmem, set_pmem, print_raw_centered, fade_palette_colour};
use crate::{cam_x, cam_y, debug_info, mem_btn, mem_btnp, frames, mouse_delta, any_btnp};

pub enum GameState {
    Instructions(u16),
    Walkaround,
    Animation(u16),
    MainMenu,
    Options,
}
impl GameState {
    pub fn run(&mut self) {
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if *i > 60 && any_btnp() {
                    *self = Self::Walkaround;
                }
                draw_instructions();
            },
            Self::Walkaround => {
                step_walkaround();
                draw_walkaround();
            },
            Self::Animation(x) => {
                if get_pmem(0) != 0 {
                    *self = Self::MainMenu;
                    return
                };
                if mem_btn(4) {*x += 1;}
                if draw_animation(*x) {
                    *x += 1;
                } else {
                    *self = Self::MainMenu;
                }
            }
            Self::MainMenu => {
                match step_main_menu() {
                    Some(MainMenuOption::Play) => *self = Self::Instructions(0),
                    Some(MainMenuOption::Options) => *self = Self::Options,
                    None => {}
                };
                draw_main_menu();
            }
            Self::Options => {
                if step_options() {
                    draw_options();
                } else {
                    *self = Self::MainMenu;
                }
            }
        }
    }
}

pub fn step_walkaround() {
    for (anim, interact) in ANIMATIONS.write().unwrap().iter_mut()
        .zip(current_map().interactables.iter()) {
            if let Some(sprite) = &interact.sprite {
                anim.0 += 1;//timer
                if anim.0 > sprite.frames[anim.1].length {
                    anim.0 = 0;
                    anim.1 += 1;//index
                    if anim.1 >= sprite.frames.len() {
                        anim.1 = 0;
                    }
                }
            }
        }
        
        if keyp(28, -1, -1) { load_map(&SUPERMARKET); }
        if keyp(29, -1, -1) { load_map(&SUPERMARKET_HALL); }
        if keyp(30, -1, -1) { load_map(&TEST_PEN); }
        {
            let fixed = DIALOGUE.read().unwrap().fixed;
            let small_text = DIALOGUE.read().unwrap().small_text;
            if keyp(35, -1, -1) {
                DIALOGUE.write().unwrap().set_options(!fixed, small_text);
            }
            if keyp(36, -1, -1) {
                DIALOGUE.write().unwrap().set_options(fixed, !small_text);
            }
        }
        
        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;
        if matches!(DIALOGUE.write().unwrap().text, None) {
            if mem_btn(0) { dy -= 1; }
            if mem_btn(1) { dy += 1; }
            if mem_btn(2) { dx -= 1; }
            if mem_btn(3) { dx += 1; }
        } else {
            DIALOGUE.write().unwrap().tick(1);
            if mem_btn(4) { DIALOGUE.write().unwrap().tick(2); }
            if mem_btnp(5, 0, -1) { DIALOGUE.write().unwrap().skip(); }
        }
        if mem_btnp(4, -1, -1) && DIALOGUE.read().unwrap().is_done() {
            interact = true;
            if matches!(DIALOGUE.write().unwrap().text, Some(_)) {
                interact = false;
                DIALOGUE.write().unwrap().close();
            }
        }
        
        // Player position + intended movement
        let player_hitbox = player().hitbox();
        let delta_hitbox = player_hitbox.offset_xy(dx, dy);
        let interact_hitbox = player_hitbox.offset_xy(
            player().dir.0.into(),
            player().dir.1.into()
        );
        
        // Face direction
        if dx != 0 || dy != 0 {
            player_mut().dir.1 = dy as i8;
            player_mut().dir.0 = dx as i8;
        }
        
        // Collide
        let points_dx = player_hitbox.dx_corners(dx);
        let points_dy = player_hitbox.dy_corners(dy);
        let point_diag = player_hitbox.dd_corner(Vec2::new(dx, dy));
        let mut diagonal_collision = false;
        let layer_collision = |point: Vec2, layer_hitbox: Hitbox, layer_x: i32, layer_y: i32| {
            if layer_hitbox.touches_point(point) {
                let map_point = Vec2::new(
                    (point.x - layer_hitbox.x)/8 + layer_x as i16,
                                          (point.y - layer_hitbox.y)/8 + layer_y as i16
                );
                let id = mget(map_point.x.into(), map_point.y.into());
                touches_tile(id as usize, Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y))
            } else {
                false
            }
        };
        for layer in current_map().maps.iter() {
            let layer_hitbox = Hitbox::new(layer.sx as i16, layer.sy as i16,
                                           layer.w as i16 * 8, layer.h as i16 * 8);
            if layer_hitbox.touches(delta_hitbox) {
                if let Some(points_dx) = points_dx {
                    points_dx.into_iter().for_each(|point| {
                        if layer_collision(point, layer_hitbox, layer.x, layer.y) { dx=0; }
                    });
                };
                if let Some(points_dy) = points_dy {
                    points_dy.into_iter().for_each(|point| {
                        if layer_collision(point, layer_hitbox, layer.x, layer.y) { dy=0; }
                        
                    });
                }
                if let Some(point_diag) = point_diag {
                    if layer_collision(point_diag, layer_hitbox, layer.x, layer.y) { diagonal_collision=true; }
                }
            }
        }
        if diagonal_collision && dx != 0 && dy != 0 { dx=0; }
        // Apply motion
        {
            let mut player = player_mut();
            if dx != 0 || dy != 0 {
                player.pos.x += dx as i16;
                player.pos.y += dy as i16;
                player.walktime = player.walktime.wrapping_add(1);
                player.walking = true;
            } else {
                player.walktime = 0;
                player.walking = false;
            };
        }
        
        let mut warp_target = None;
        for warp in current_map().warps.iter() {
            if player().hitbox().touches(warp.from)
                || (interact && interact_hitbox.touches(warp.from)) {
                    warp_target = Some(warp.clone());
                    break;
                }
        }
        if let Some(target) = warp_target {
            player_mut().pos = target.to;
            if let Some(new_map) = target.map {
                load_map(new_map);
            }
        } else if interact {
            for item in current_map().interactables.iter() {
                if interact_hitbox.touches(item.hitbox) {
                    match &item.interaction {
                        Interaction::Text(x) => {
                            trace!(x, 12);
                            DIALOGUE.write().unwrap().set_text(x);
                        },
                        x => {trace!(format!("{:?}", x), 12);},
                    }
                }
            }
        }
        
        camera_mut().center_on(player().pos.x+4, player().pos.y+8);
}

pub fn draw_walkaround() {
    // draw bg
    palette_map_reset();
    cls(1);
    blit_segment(4);
    for (i, layer) in current_map().maps.iter().enumerate() {
        if i == 0 {palette_map_rotate(1)} else {palette_map_rotate(0)}
        let mut layer = layer.clone();
        layer.sx -= cam_x();
        layer.sy -= cam_y();
        if debug_info().map_info {
            rectb(layer.sx, layer.sy, layer.w * 8, layer.h * 8, 9);
        }
        map(layer);
    }
    // draw sprites from least to greatest y
    palette_map_rotate(1);
    let player_sprite = player().sprite_index();
    let (player_x, player_y): (i32, i32) = (player().pos.x.into(), player().pos.y.into());
    spr_outline(
        player_sprite.0,
        player_x-cam_x(),
                player_y - player_sprite.2-cam_y(),
                SpriteOptions {
                    w: 1,
                h: 2,
                transparent: &[0],
                scale: 1,
                flip: player_sprite.1,
                ..Default::default()
                },
                1,
    );
    palette_map_reset();
    
    for (item, time) in current_map().interactables.iter()
        .zip(ANIMATIONS.read().unwrap().iter()) {
            if let Some(anim) = &item.sprite {
                spr_outline(
                    anim.frames[time.1].id.into(),
                            anim.frames[time.1].pos.x as i32 + item.hitbox.x as i32 - cam_x(),
                            anim.frames[time.1].pos.y as i32 + item.hitbox.y as i32 - cam_y(),
                            anim.frames[time.1].options.clone(),
                            1,
                );
            }
        }
        
        // draw fg
        palette_map_reset();
        {
            let print_timer = DIALOGUE.read().unwrap().timer;
            let font_fixed = DIALOGUE.read().unwrap().fixed;
            let small_font = DIALOGUE.read().unwrap().small_text;
            if let Some(text) = &DIALOGUE.read().unwrap().text {
                let w = 200;
                let h = 24;
                rect((WIDTH - w)/2, (HEIGHT - h) - 4, w, h, 2);
                rectb((WIDTH - w)/2, (HEIGHT - h) - 4, w, h, 3);
                print_alloc(&text[..(print_timer)], (WIDTH - w)/2+3, (HEIGHT - h) - 4 + 3, PrintOptions {
                    color: 12,
                    small_text: small_font,
                    fixed: font_fixed,
                    ..Default::default()
                });
            }
        }
        if debug_info().map_info {
            for warp in current_map().warps.iter() {
                warp.from
                .offset_xy(-cam_x() as i16, -cam_y() as i16)
                .draw(12);
            }
            player().hitbox().offset_xy(-cam_x() as i16, -cam_y() as i16).draw(12);
            for item in current_map().interactables.iter() {
                item.hitbox.offset_xy(-cam_x() as i16, -cam_y() as i16).draw(14);
            }
        }
        if debug_info().player_info {
            print!(format!("Player: {:#?}", player()), 0, 0,
                   PrintOptions {
                       small_text: true,
                   color: 11,
                   ..Default::default()
                   }
            );
            print!(format!("Camera: {:#?}", camera()), 64, 0,
                   PrintOptions {
                       small_text: true,
                   color: 11,
                   ..Default::default()
                   }
            );
        }
}

pub fn draw_instructions() {
    cls(0);
    let string = crate::dialogue_data::INSTRUCTIONS;
    rect(7,15,226,100,1);
    rectb(7,15,226,100,2);
    print_raw(string,
              11,
              21,
              PrintOptions {
                  color: 0,
              ..Default::default()
              }
    );
    print_raw(string,
              10,
              20,
              PrintOptions {
                  color: 12,
              ..Default::default()
              }
    );
}

pub fn draw_animation(t: u16) -> bool {
    let steps: &[u16] = &[0, 1, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.and_then(|x| Some(t - steps[x.saturating_sub(1)]));
    match index {
        Some(0) => {
            cls(0);
            set_palette([[0; 3]; 16]);
            // fade_palette(SWEETIE_16, [[0; 3]; 16], 256/50 * t);
            true
        }
        Some(1) => {
            music(3, MusicOptions::default());
            draw_ovr(|| {
                set_palette([[0; 3]; 16]);
                circb(90, 38, 4, 4);
                circb(90, 36, 3, 4);
                circ(90, 38, 3, 12);
                circ(90, 36, 2, 12);
                for _ in 0..420 {
                    pix(rand() as i32 % 240,
                        rand() as i32 % 136,
                        12,)
                }
            });
            true
        },
        Some(2) => {
            let local_time = local_time.unwrap();
            let max_time = 700.0 - 60.0;
            fade_palette([[0; 3]; 16], SWEETIE_16, local_time*2);
            draw_ovr(|| {
                fade_palette([[0; 3]; 16], SWEETIE_16, local_time*2);
                let t = ((local_time as f32 /max_time)).powf(0.02);
                let size = 200.0 / (max_time + 1.0 - t * max_time).powi(2).max(1.0);
                let t = size as i32;
                set_palette_colour(15, [0x0F;3]);
                circ(120, 68, t, 15);
                circb(120, 68, t, 2);
                if local_time > 400 {
                    if local_time < 450 {
                        if local_time % 3 == 0 {
                            screen_offset((rand()%2 - 1) as i8, (rand()%2 - 1) as i8);
                        }
                    } else {
                        screen_offset((rand()%2 - 1) as i8, (rand()%2 - 1) as i8);
                    }
                }
            });
            true
        },
        Some(3) => {
            screen_offset(0, 0);
            fade_palette_colour(15, [0x0F;3], [26, 28, 44], local_time.unwrap()*10);
            cls(15);
            draw_ovr(|| {
                cls(0);
                fade_palette([[0x0F; 3]; 16], SWEETIE_16, local_time.unwrap()*10);
                draw_title(120, 50)
            });
            true
        },
        _ => {
            music(-1, MusicOptions {
                frame: 1, ..Default::default()
            });
            set_pmem(0, 1);
            screen_offset(0, 0);
            set_palette(SWEETIE_16);
            cls(0);
            draw_title(120, 50);
            draw_ovr(|| cls(0));
            false
        }
    }
}

enum MainMenuOption {
    Play,
    Options,
}

fn step_main_menu() -> Option<MainMenuOption> {
    use crate::MAINMENU;    
    let (menu_index, clicked) = step_menu(2, 88);
    if mem_btnp(4, -1, -1) || clicked {
        *MAINMENU.write().unwrap() = 0;
        match menu_index {
            0 => return Some(MainMenuOption::Play),
            1 => return Some(MainMenuOption::Options),
            _ => {},
        };
    }
    None
}

pub fn draw_main_menu() {
    use crate::MAINMENU;
    use crate::dialogue_data::{MENU_OPTIONS, MENU_PLAY};
    cls(0);
    
    draw_title(120, 50);
    
    let strings = [MENU_PLAY, MENU_OPTIONS];
    let current_option = *MAINMENU.read().unwrap();
    draw_menu(&strings, 120, 88, current_option);
}

pub fn draw_menu(entries: &[&str], x: i32, y: i32, current_option: usize) {
    for (i, string) in entries.iter().enumerate() {
        let color = if i == current_option {4} else {3};
        if i == current_option {
            rect(0,y+i as i32*8-1,240,8,1);
        }
        print_raw_centered(string, x, y+i as i32*8, PrintOptions {
            color,
            ..DIALOGUE.read().unwrap().get_options()
        });
    }
}

pub fn step_menu(entries: usize, y: i16) -> (usize, bool) {
    use crate::MAINMENU;
    let old_index = *MAINMENU.read().unwrap();
    
    let mouse_pos = Vec2::new(mouse().x, mouse().y);
    let mouse_delta = mouse_delta();
    let mut clicked = false;
    for i in 0..entries {
        if Hitbox::new(0, y+8*i as i16, 240, 8).touches_point(mouse_pos) {
            *MAINMENU.write().unwrap() = i as usize;
            clicked = mouse_delta.left;
        }
    }
    if mem_btnp(0, -1, -1) {
        *MAINMENU.write().unwrap() = old_index.saturating_sub(1);
    }
    if mem_btnp(1, -1, -1) {
        *MAINMENU.write().unwrap() = old_index.saturating_add(1).min(entries-1);
    }
    
    (*MAINMENU.read().unwrap(), clicked)
}

pub fn draw_title(x: i32, y: i32) {
    use crate::dialogue_data::GAME_TITLE;
    let title_width = print_raw(GAME_TITLE, 999, 999, PrintOptions {scale: 1, ..Default::default()});
    print_raw_centered(GAME_TITLE, x, y+23, PrintOptions {
        scale: 1,
        color: 2,
        ..Default::default()
    });
    
    rect(120-title_width/2, y+19, title_width-1, 2, 2);
    
    blit_segment(8);
    spr(1086, 120-8, y+((frames() as i32/30)%2), SpriteOptions {
        transparent: &[0],
        scale: 1,
        w: 2,
        h: 2,
        ..Default::default()
    });
    blit_segment(4);
}

fn step_options() -> bool {
    use crate::RESET_PROTECTOR;
    let (menu_index, clicked) = step_menu(4, 40);
    if menu_index != 3 {*RESET_PROTECTOR.write().unwrap() = 0;};
    if mem_btnp(4, -1, -1) || clicked {
        match menu_index {
            0 => {return false},
            1 => {DIALOGUE.write().unwrap().toggle_small_text();},
            2 => {DIALOGUE.write().unwrap().toggle_fixed()},
            3 => {
                if *RESET_PROTECTOR.read().unwrap() == 0 {
                    *RESET_PROTECTOR.write().unwrap() += 1;
                } else {
                    *crate::MAINMENU.write().unwrap() = 0;
                    *RESET_PROTECTOR.write().unwrap() = 0;
                    unsafe {
                        for byte in (*PERSISTENT_RAM).iter_mut() {
                            *byte = 0;
                        }
                    }
                    return false;
                };
            },
            _ => {},
        };
    }
    true
}

pub fn draw_options() {
    cls(0);
    use crate::{MAINMENU, RESET_PROTECTOR};
    use crate::dialogue_data::{MENU_BACK,OPTIONS_FONT_SIZE,OPTIONS_FONT_FIXED,OPTIONS_RESET,OPTIONS_RESET_SURE,
        OPTIONS_LOSE_DATA
    };
    let reset_string = if *RESET_PROTECTOR.read().unwrap() == 0 {
        OPTIONS_RESET
    } else {
        OPTIONS_RESET_SURE
    };
    let strings = [MENU_BACK, OPTIONS_FONT_SIZE, OPTIONS_FONT_FIXED, reset_string];
    let current_option = *MAINMENU.read().unwrap();
    if current_option == 3 {
        rect(60, 10, 120, 11, 2);
        print_raw_centered(OPTIONS_LOSE_DATA, 120, 13, PrintOptions {
            color: 12,
            ..DIALOGUE.read().unwrap().get_options()
        });
    }
    draw_menu(&strings, 120, 40, current_option);
}