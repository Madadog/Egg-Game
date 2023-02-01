use crate::{ANIMATIONS, DIALOGUE, TIME};
use crate::{current_map, load_map, player, player_mut, camera, camera_mut};
use crate::position::{Hitbox, Vec2, touches_tile};
use crate::tic80::*;
use crate::map_data::*;
use crate::interact::Interaction;
use crate::{trace, print};

use crate::tic_helpers::{palette_map_reset, palette_map_rotate, blit_segment, spr_outline};
use crate::{cam_x, cam_y, debug_info, mem_btn, mem_btnp};

pub enum GameState {
    Popup,
    Walkaround,
}
impl GameState {
    pub fn run(&mut self) {
        match self {
            Self::Popup => {
                if mem_btn(0) || mem_btn(1) || mem_btn(2) || mem_btn(3) || mem_btn(4) || mem_btn(5) {
                    *self = Self::Walkaround;
                }
                draw_popup();
            },
            Self::Walkaround => {
                step_walkaround();
                draw_walkaround();
            },
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
        
        unsafe {trace!(format!("{:b},{:b},{:b},{:b}",(*GAMEPADS)[0],(*GAMEPADS)[1],(*GAMEPADS)[2],(*GAMEPADS)[3]), 12);}
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
            trace!("mem_btnp(4)",12);
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
        
        *TIME.write().unwrap() += 1;
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
                    small_font,
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
                       small_font: true,
                   color: 11,
                   ..Default::default()
                   }
            );
            print!(format!("Camera: {:#?}", camera()), 64, 0,
                   PrintOptions {
                       small_font: true,
                   color: 11,
                   ..Default::default()
                   }
            );
        }
}

pub fn draw_popup() {
    cls(1);
    print_raw("Arrow keys: Move around.\nZ: Interact.\nX: Skip text.\n\nRemember to sleep regularly.",
              4,
              20,
              PrintOptions {
                  color: 12,
              ..Default::default()
              }
    );
}
