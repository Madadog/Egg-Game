mod alloc;


use egg_core::gamestate::walkaround::WalkaroundState;
use egg_core::gamestate::GameState;
use egg_core::data::map_data::*;
use egg_core::position::{Hitbox, Vec2};
use egg_core::rand::Pcg32;
use egg_core::tic80_helpers::SyncHelper;
use once_cell::sync::Lazy;
use egg_core::packed::{PackedI16, PackedU8};
use std::fmt::format;
use std::sync::atomic::{AtomicBool, Ordering, AtomicI32, AtomicU8, AtomicUsize};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use tic80_api::core::*;
use tic80_api::helpers::input_manager;

pub struct DebugInfo {
    pub player_info: AtomicBool,
    pub map_info: AtomicBool,
    pub memory_info: AtomicBool,
    pub memory_index: AtomicUsize,
}
impl DebugInfo {
    pub const fn const_default() -> Self {
        DebugInfo {
            player_info: AtomicBool::new(false),
            map_info: AtomicBool::new(false),
            memory_info: AtomicBool::new(false),
            memory_index: AtomicUsize::new(0),
        }
    }
    pub fn player_info(&self) -> bool {
        self.player_info.load(Ordering::SeqCst)
    }
    pub fn map_info(&self) -> bool {
        self.map_info.load(Ordering::SeqCst)
    }
    pub fn memory_info(&self) -> bool {
        self.memory_info.load(Ordering::SeqCst)
    }
    pub fn memory_index(&self) -> usize {
        self.memory_index.load(Ordering::SeqCst)
    }
    pub fn set_player_info(&self, new: bool) {
        self.player_info.store(new, Ordering::SeqCst);
    }
    pub fn set_map_info(&self, new: bool) {
        self.map_info.store(new, Ordering::SeqCst);
    }
    pub fn set_memory_info(&self, new: bool) {
        self.memory_info.store(new, Ordering::SeqCst);
    }
    pub fn set_memory_index(&self, new: usize) {
        self.memory_index.store(new, Ordering::SeqCst);
    }
}

static WALKAROUND_STATE: RwLock<WalkaroundState> = RwLock::new(WalkaroundState::new());
static TIME: AtomicI32 = AtomicI32::new(0);
static PAUSE: AtomicBool = AtomicBool::new(false);
static RNG: RwLock<Lazy<Pcg32>> = RwLock::new(Lazy::new(Pcg32::default));
static DEBUG_INFO: DebugInfo = DebugInfo::const_default();
static GAMESTATE: RwLock<GameState> = RwLock::new(GameState::Animation(0));
static BG_COLOUR: AtomicU8 = AtomicU8::new(0);
static SYNC_HELPER: SyncHelper = SyncHelper::new();

// REMINDER: Heap maxes at 8192 u32.

pub fn frames() -> i32 {
    TIME.load(Ordering::SeqCst)
}
pub fn rand() -> u32 {
    if let Ok(mut rng) = RNG.write() {
        rng.next_u32()
    } else {
        0
    }
}
pub fn rand_u8() -> u8 {
    (rand() % 256) as u8
}
pub fn is_paused() -> bool {
    PAUSE.load(Ordering::Relaxed)
}
pub fn set_pause(pause: bool) {
    PAUSE.store(pause, Ordering::Relaxed);
}

pub fn run_gamestate() {
    if let Ok(mut state) = GAMESTATE.write() {
        state.run()
    }
}

#[export_name = "BOOT"]
pub fn boot() {
    std::panic::set_hook(Box::new(|x| {
        trace!(format!("{x}"), 2);
    }));
    if let Ok(mut walkaround) = WALKAROUND_STATE.write() {
        walkaround.load_map(BEDROOM)
    }
}

#[export_name = "TIC"]
pub fn tic() {
    SYNC_HELPER.step();
    TIME.fetch_add(1, Ordering::SeqCst);

    if keyp(16, -1, -1) {
        set_pause(!is_paused());
        print!(
            "Paused",
            100,
            62,
            PrintOptions {
                color: 12,
                ..Default::default()
            }
        );
    }
    if is_paused() {
        return;
    }
    if keyp(4, -1, -1) {
        let p = DEBUG_INFO.player_info();
        DEBUG_INFO.set_player_info(!p);
    }
    if keyp(13, -1, -1) {
        let p = DEBUG_INFO.map_info();
        DEBUG_INFO.set_map_info(!p);
    }
    if keyp(14, -1, -1) {
        let p = DEBUG_INFO.memory_info();
        DEBUG_INFO.set_memory_info(!p);
    }

    run_gamestate();
    if key(6) {
        run_gamestate();
        print_raw(
            "Fast-Forward\0",
            100,
            62,
            PrintOptions {
                color: 12,
                ..Default::default()
            },
        );
    }

    if DEBUG_INFO.memory_info.load(Ordering::SeqCst) {
        for i in 0i32..((163840 / 2).min(240 * 136)) {
            let j = (i as usize + DEBUG_INFO.memory_index()).min(163839) as i32;
            let x = unsafe { *((0x18000 + j) as *mut u8) };
            let (l, u) = (x % 16, x >> 4);
            pix((i * 2) % 240, i / 240, l);
            pix((i * 2 + 1) % 240, i / 240, u);
        }
        let acc = MEM_USAGE.load(Ordering::SeqCst);
        print_raw(
            &format!(
                "{acc}/160kB used (heap). [n] to close.\n[up] and [down] to scroll.\nDisplaying address offset = {}\0",
                DEBUG_INFO.memory_index()
            ),
            1,
            1,
            PrintOptions::default().with_color(12),
        );
        if input_manager::mem_btn(0) {
            let x = (DEBUG_INFO.memory_index() + 240 * 8).min(163840);
            DEBUG_INFO.set_memory_index(x);
        }
        if input_manager::mem_btn(1) {
            let x = DEBUG_INFO.memory_index().saturating_sub(240 * 8);
            DEBUG_INFO.set_memory_index(x);
        }
        if DEBUG_INFO.memory_index() == 163840 {
            print_raw("End.\0", 1, 120, PrintOptions::default().with_color(12));
        }
    }
    input_manager::step_gamepad_helper();
    input_manager::step_mouse_helper();
}
