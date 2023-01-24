pub mod alloc;
mod tic80;
mod rand;
mod position {
    //#[derive(Debug)]
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

pub struct Player {
    pub pos: Vec2,
    pub hp: u8,
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub walktime: u8,
}
impl Player {
    pub const fn cdefault() -> Self {
        Self {
            pos: Vec2::new(96, 24),
            hp: 3,
            dir: (0, 1),
            walktime: 0,
        }
    }
    pub fn sprite_index(&self) -> (i32, Flip) {
        if self.dir.1 > 0 { return (768, Flip::None) } // Up
        if self.dir.1 < 0 { return (771, Flip::None) } // Down
        if self.dir.0 > 0 { return (832, Flip::None) } // Right
        return (832, Flip::Horizontal) // Left
    }
}
impl Default for Player {
    fn default() -> Self {
        Self {
            pos: Vec2::new(96, 24),
            hp: 3,
            dir: (0, 1),
            walktime: 0,
        }
    }
}


use tic80::*;
use crate::rand::Pcg32;
use crate::position::Vec2;
use crate::tic_helpers::*;
use once_cell::sync::Lazy;
use std::sync::{RwLock, RwLockWriteGuard, RwLockReadGuard};
use std::sync::atomic::{AtomicBool, Ordering};

static TIME: RwLock<i32> = RwLock::new(0);
static PLAYER: RwLock<Player> = RwLock::new(Player::cdefault());
//static mut POS: [(u8, u8); 5000] = [(0, 0); 5000];
static POS: RwLock<Vec<(i16, i16)>> = RwLock::new(Vec::new());
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(|| {Pcg32::default()}));
static PAUSE: AtomicBool = AtomicBool::new(false);

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
pub fn rand() -> u32 {
    RNG.write().unwrap().next_u32()
}
mod tic_helpers;

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
    if dx != 0 || dy != 0 {
        player_mut().dir.1 = dy;
        player_mut().dir.0 = dx;
        player_mut().pos.x += dx as i16;
        player_mut().pos.y += dy as i16;
    }

    *TIME.write().unwrap() += 1;
}
fn draw_game() {

}

#[export_name = "TIC"]
pub fn tic() {
    if keyp(16, -1, -1) {
        PAUSE.store(!PAUSE.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    if PAUSE.load(Ordering::Relaxed) { return }
    step_game();
    palette_map_reset();
    set_border( (rand()%16) as u8);
    cls(0);
    blit_segment(4);
    map(MapOptions {
        x: 60,
        y: 17,
        ..Default::default()
    });
    palette_map_rotate(1);
    let player_sprite = player().sprite_index();
    spr(
        player_sprite.0,
        player().pos.x.into(),
        player().pos.y.into(),
        SpriteOptions {
            w: 1,
            h: 2,
            transparent: &[0],
            scale: 1,
            flip: player_sprite.1,
            ..Default::default()
        },
    );
    palette_map_reset();
    // blit_segment(2);
    for (i, (x, y)) in POS.write().unwrap().iter_mut().enumerate() {
        *x += (rand()%9-4) as i16;
        *y += (rand()%9-4) as i16;
        *x = (*x).max(-7);
        *y = (*y).max(-7);
        palette_map_swap(rand() as i32, rand() as u8);
        spr(
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
        );
    }
    print!("HELLO WORLD!", 84, 84, PrintOptions::default());
    print!(format!("There are {} things.", POS.read().unwrap().len()), 84, 94, PrintOptions::default());
}
