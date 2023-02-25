use std::sync::RwLock;
use crate::tic80_core::{sys::MouseInput, GAMEPADS, mouse};

pub static GAMEPAD_HELPER: RwLock<[u8; 4]> = RwLock::new([0; 4]);

pub fn step_gamepad_helper() {
    let buttons = unsafe { *GAMEPADS };
    *GAMEPAD_HELPER.write().unwrap() = buttons;
}

pub static MOUSE_HELPER: RwLock<MouseInput> = RwLock::new(MOUSE_INPUT_DEFAULT);

pub fn step_mouse_helper() {
    let input = mouse();
    *MOUSE_HELPER.write().unwrap() = input;
}

pub const MOUSE_INPUT_DEFAULT: MouseInput = MouseInput {
    x: 0,
    y: 0,
    scroll_x: 0,
    scroll_y: 0,
    left: false,
    middle: false,
    right: false,
};

pub fn mem_btn(id: u8) -> bool {
    let controller: usize = (id / 8).min(3).into();
    let id = id % 8;
    let buttons = unsafe { (*GAMEPADS)[controller] };
    (1 << id) & buttons != 0
}
pub fn mem_btnp(id: u8) -> bool {
    let controller: usize = (id / 8).min(3).into();
    let id = id % 8;
    let buttons = unsafe { (*GAMEPADS)[controller] };
    let previous = GAMEPAD_HELPER.read().unwrap()[controller];
    (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
}
/// Returns true if any button was pressed. Ignores button releases.
pub fn any_btnp() -> bool {
    let buttons = unsafe { *GAMEPADS };
    let previous = *GAMEPAD_HELPER.read().unwrap();
    let mut flag = false;
    for (b0, b1) in previous.iter().zip(buttons.iter()) {
        flag |= b0.count_ones() < b1.count_ones();
    }
    flag
}
/// Returns true if any button was pressed or released
pub fn any_btnpr() -> bool {
    let buttons = unsafe { *GAMEPADS };
    let previous = *GAMEPAD_HELPER.read().unwrap();
    buttons != previous
}
pub fn mouse_delta() -> MouseInput {
    let old = MOUSE_HELPER.read().unwrap().clone();
    let new = mouse();
    MouseInput {
        x: new.x - old.x,
        y: new.y - old.y,
        left: new.left && !old.left,
        middle: new.middle && !old.middle,
        right: new.right && !old.right,
        ..new
    }
}