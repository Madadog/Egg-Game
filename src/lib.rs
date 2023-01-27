pub mod alloc;
mod tic80;
mod rand;
mod tic_helpers;
mod map_data;
mod camera;
mod position;

use tic80::*;
use crate::rand::Pcg32;
use crate::position::{Vec2, Hitbox};
use crate::tic_helpers::*;
use crate::camera::Camera;
use crate::map_data::*;
use once_cell::sync::Lazy;
use std::sync::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub struct Player {
    pub pos: Vec2,
    pub local_hitbox: Hitbox,
    pub hp: u8,
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub walktime: u16,
    pub walking: bool,
}
impl Player {
    pub const fn const_default() -> Self {
        Self {
            pos: Vec2::new(96, 34),
            local_hitbox: Hitbox::new(0,10,7,4),
            hp: 3,
            dir: (0, 1),
            walktime: 0,
            walking: false,
        }
    }
    pub fn sprite_index(&self) -> (i32, Flip, i32) {
        let t = (((self.walktime+19) / 20) % 2) as i32;
        let anim = if self.walktime > 0 {t + 1} else {0};
        if self.dir.1 > 0 { return (768 + anim, Flip::None, t) } // Up
        if self.dir.1 < 0 { return (771 + anim, Flip::None, t) } // Down
        if self.dir.0 > 0 { return (832 + anim, Flip::None, t) } // Right
        return (832 + anim, Flip::Horizontal, t) // Left
    }
    pub fn hitbox(&self) -> Hitbox {
        self.local_hitbox.offset(self.pos)
    }
}
impl Default for Player {
    fn default() -> Self { Self::const_default() }
}

pub struct DebugInfo {
    player_info: bool,
}
impl DebugInfo {
    pub const fn const_default() -> Self {
        DebugInfo { player_info: false }
    }
}

static TIME: RwLock<i32> = RwLock::new(0);
static PLAYER: RwLock<Player> = RwLock::new(Player::const_default());
static POS: RwLock<Vec<(i16, i16)>> = RwLock::new(Vec::new());
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(|| {Pcg32::default()}));
static PAUSE: AtomicBool = AtomicBool::new(false);
static CAMERA: RwLock<Camera> = RwLock::new(Camera::const_default());
static DEBUG_INFO: RwLock<DebugInfo> = RwLock::new(DebugInfo::const_default());
static CURRENT_MAP: RwLock<&MapSet> = RwLock::new(&SUPERMARKET);

// REMINDER: Heap maxes at 8192 u32.

pub fn time() -> i32 {
    *TIME.read().unwrap()
}
pub fn player_mut<'a>() -> RwLockWriteGuard<'a, Player> {
    PLAYER.write().unwrap()
}
pub fn player<'a>() -> RwLockReadGuard<'a, Player> {
    PLAYER.read().unwrap()
}
pub fn debug_info_mut<'a>() -> RwLockWriteGuard<'a, DebugInfo> {
    DEBUG_INFO.write().unwrap()
}
pub fn debug_info<'a>() -> RwLockReadGuard<'a, DebugInfo> {
    DEBUG_INFO.read().unwrap()
}
pub fn camera_mut<'a>() -> RwLockWriteGuard<'a, Camera> {
    CAMERA.write().unwrap()
}
pub fn camera<'a>() -> RwLockReadGuard<'a, Camera> {
    CAMERA.read().unwrap()
}
pub fn cam_x() -> i32 { i32::from(camera().pos.x)}
pub fn cam_y() -> i32 { i32::from(camera().pos.y)}
pub fn rand() -> u32 {
    RNG.write().unwrap().next_u32()
}
pub fn rand_u8() -> u8 {
    (rand() % 256).try_into().unwrap()
}
pub fn is_paused() -> bool {
    PAUSE.load(Ordering::Relaxed)
}
pub fn set_pause(pause: bool) {
    PAUSE.store(pause, Ordering::Relaxed);
}
pub fn current_map<'a>() -> RwLockReadGuard<'a, &'a MapSet<'a>> {
    CURRENT_MAP.read().unwrap()
}
pub fn load_map(map: &'static MapSet<'static>) {
    let map1 = &map.maps[0];
    *camera_mut() = Camera::from_map_size(map1.w as u8, map1.h as u8, map1.sx as i16, map1.sy as i16);
    *CURRENT_MAP.write().unwrap() = map;
}

fn step_game() {
    if POS.read().unwrap().len() <= 100 {
        POS.write().unwrap().push((0, 0)); POS.write().unwrap().push((100, 100));
    }
    
    if keyp(28, -1, -1) {
        load_map(&SUPERMARKET);
    }
    if keyp(29, -1, -1) {
        load_map(&SUPERMARKET_HALL);
    }

    let (mut dx, mut dy) = (0, 0);
    if btn(0) {
        dy -= 1;
    }
    if btn(1) {
        dy += 1;
    }
    if btn(2) {
        dx -= 1;
    }
    if btn(3) {
        dx += 1;
    }
    let player_hitbox = player().hitbox();
    let delta_hitbox = player_hitbox.offset_xy(dx, dy);
    let points_dx = player_hitbox.dx_corners(dx);
    let points_dy = player_hitbox.dy_corners(dy);
    let point_diag = player_hitbox.dd_corner(Vec2::new(dx, dy));
    for layer in current_map().maps.iter() {
        let layer_hitbox = Hitbox::new(layer.sx as i16, layer.sy as i16,
                                    layer.w as i16 * 8, layer.h as i16 * 8);
        if layer_hitbox.touches(delta_hitbox) {
            if let Some(points_dx) = points_dx {
                for point in points_dx {
                    let p_dx = Vec2::new(
                        (point.x - layer_hitbox.x)/8 + layer.x as i16,
                        (point.y - layer_hitbox.y)/8 + layer.y as i16
                    );
                    let id_x = mget(p_dx.x.into(), p_dx.y.into());
                    if fget(id_x, 0) {dx=0;}
                }
            }
            if let Some(points_dy) = points_dy {
                for point in points_dy {
                    let p_dy = Vec2::new(
                        (point.x - layer_hitbox.x)/8 + layer.x as i16,
                        (point.y - layer_hitbox.y)/8 + layer.y as i16
                    );
                    let id_y = mget(p_dy.x.into(), p_dy.y.into());
                    if fget(id_y, 0) {dy=0;}
                }
            }
            if let Some(point_diag) = point_diag {
                let p_diag = Vec2::new(
                    (point_diag.x - layer_hitbox.x)/8 + layer.x as i16,
                    (point_diag.y - layer_hitbox.y)/8 + layer.y as i16
                );
                let id_d = mget(p_diag.x.into(), p_diag.y.into());
                if dx != 0 && dy != 0 && fget(id_d, 0) {
                    dx=0;
                    dy=0;
                }
            }
        }
    }
    {
        let mut player = player_mut();
        if dx != 0 || dy != 0 {
            player.dir.1 = dy as i8;
            player.dir.0 = dx as i8;
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
        if player().hitbox().touches(warp.from) {
            warp_target = Some(warp.clone());
            break;
        }
    }
    if let Some(target) = warp_target {
        player_mut().pos = target.to;
        if let Some(new_map) = target.map {
            load_map(new_map);
        }
    }
    
    camera_mut().center_on(player().pos.x+4, player().pos.y+8);

    *TIME.write().unwrap() += 1;
}

fn draw_game() {
    // draw bg
    if time() % 300 > 285 {
        set_border( (rand()%16) as u8);
    }
    palette_map_reset();
    cls(1);
    blit_segment(4);
    for (i, layer) in current_map().maps.iter().enumerate() {
        if i == 0 {palette_map_rotate(1)} else {palette_map_rotate(0)}
        let mut layer = layer.clone();
        layer.sx -= cam_x();
        layer.sy -= cam_y();
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

    // draw fg
    palette_map_reset();
    if debug_info().player_info {
        for warp in current_map().warps.iter() {
            warp.from
                .offset_xy(-cam_x() as i16, -cam_y() as i16)
                .draw(12);
        }
        print!(format!("There are {} things.", POS.read().unwrap().len()), 84, 94, PrintOptions::default());
        print!(format!("Player: {:#?}", player()), 0, 0,
            PrintOptions {
                small_font: true,
                color: 11,
                ..Default::default()
            }
        );
        player().hitbox().offset_xy(-cam_x() as i16, -cam_y() as i16).draw(12);
        print!(format!("Camera: {:#?}", camera()), 64, 0,
               PrintOptions {
                   small_font: true,
               color: 11,
               ..Default::default()
               }
        );
        unsafe {(*FRAMEBUFFER)[1] = 0x12}
    }
}

#[export_name = "TIC"]
pub fn tic() {
    if keyp(16, -1, -1) {
        set_pause(!is_paused());
        print!("Paused", 100, 62, PrintOptions {color: 12, ..Default::default()});
    }
    if is_paused() { return }
    if keyp(4, -1, -1) {
        let p = debug_info().player_info;
        debug_info_mut().player_info = !p;
    }
    step_game();
    draw_game();
}
