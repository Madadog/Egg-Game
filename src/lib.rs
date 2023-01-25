pub mod alloc;
mod tic80;
mod rand;
mod tic_helpers;
mod map_data;
mod camera;
mod position {
    #[derive(Debug, Clone, Copy)]
    pub struct Vec2 {
        pub x: i16,
        pub y: i16,
    }
    impl Vec2 {
        pub const fn new(x: i16, y: i16) -> Self {
            Vec2 {x, y}
        }
    }
}

#[derive(Debug)]
pub struct Player {
    pub pos: Vec2,
    pub hp: u8,
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub walktime: u16,
    pub walking: bool,
}
impl Player {
    pub const fn const_default() -> Self {
        Self {
            pos: Vec2::new(96, 24),
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

use tic80::*;
use crate::rand::Pcg32;
use crate::position::Vec2;
use crate::tic_helpers::*;
use crate::camera::Camera;
use crate::map_data::*;
use once_cell::sync::Lazy;
use std::sync::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use std::sync::atomic::{AtomicBool, Ordering};

static TIME: RwLock<i32> = RwLock::new(0);
static PLAYER: RwLock<Player> = RwLock::new(Player::const_default());
static POS: RwLock<Vec<(i16, i16)>> = RwLock::new(Vec::new());
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(|| {Pcg32::default()}));
static PAUSE: AtomicBool = AtomicBool::new(false);
static CAMERA: RwLock<Camera> = RwLock::new(Camera::const_default());
static DEBUG_INFO: RwLock<DebugInfo> = RwLock::new(DebugInfo::const_default());
static CURRENT_MAP: RwLock<MapSet> = RwLock::new(SUPERMARKET);

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

#[inline]
fn step_game() {
    if POS.read().unwrap().len() <= 100 {
        POS.write().unwrap().push((0, 0)); POS.write().unwrap().push((100, 100));
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
    {
        let mut player = player_mut();
        if dx != 0 || dy != 0 {
            player.dir.1 = dy;
            player.dir.0 = dx;
            player.pos.x += dx as i16;
            player.pos.y += dy as i16;
            player.walktime = player.walktime.wrapping_add(1);
            player.walking = true;
        } else {
            player.walktime = 0;
            player.walking = false;
        };
    }
    camera_mut().center_on(player().pos.x+4, player().pos.y+8);

    *TIME.write().unwrap() += 1;
}
fn draw_game() {
    // draw bg
    palette_map_reset();
    if time() % 300 > 285 {
        set_border( (rand()%16) as u8);
    }
    cls(0);
    blit_segment(4);
    for layer in CURRENT_MAP.read().unwrap().maps.iter() {
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
    
    // blit_segment(2);
    for (i, (x, y)) in POS.write().unwrap().iter_mut().enumerate() {
        *x += (rand()%9-4) as i16;
        *y += (rand()%9-4) as i16;
        *x = (*x).max(-7);
        *y = (*y).max(-7);
        palette_map_swap(rand_u8(), rand_u8());
        //palette_map_rotate(i as u8);
        spr_outline(
            513 + (i%3) as i32,
            *x as i32,
            *y as i32,
            SpriteOptions {
                w: 1,
                h: 1,
                transparent: &[0],
                scale: 1,
                ..Default::default()
            },
            0,
        );
    }
    // draw fg
    palette_map_reset();
    print!("HELLO WORLD!", 84, 84, PrintOptions::default());
    print!(format!("There are {} things.", POS.read().unwrap().len()), 84, 94, PrintOptions::default());
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

#[export_name = "TIC"]
pub fn tic() {
    if keyp(16, -1, -1) {
        set_pause(!is_paused());
    }
    if is_paused() { return }
    if keyp(4, -1, -1) {
        let p = debug_info().player_info;
        debug_info_mut().player_info = !p;
    }
    step_game();
    draw_game();
}
