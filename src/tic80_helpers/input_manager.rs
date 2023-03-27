use crate::{
    tic80_core::{mouse, sys::MouseInput, GAMEPADS},
    trace,
};
use std::sync::RwLock;

pub static GAMEPAD_HELPER: RwLock<[u8; 4]> = RwLock::new([0; 4]);

pub fn step_gamepad_helper() {
    let buttons = unsafe { *GAMEPADS };
    if let Ok(mut old_buttons) = GAMEPAD_HELPER.write() {
        *old_buttons = buttons;
    }
}

pub static MOUSE_HELPER: RwLock<MouseInput> = RwLock::new(MOUSE_INPUT_DEFAULT);

pub fn step_mouse_helper() {
    let input = mouse();
    if let Ok(mut old_mouse) = MOUSE_HELPER.write() {
        *old_mouse = input;
    }
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
    if let Ok(old_gamepad) = GAMEPAD_HELPER.read() {
        let previous = old_gamepad[controller];
        (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
    } else {
        trace!("mem_btnp failed", 12);
        false
    }
}
/// Returns true if any button was pressed. Ignores button releases.
pub fn any_btnp() -> bool {
    let buttons = unsafe { *GAMEPADS };
    if let Ok(previous) = GAMEPAD_HELPER.read() {
        let mut flag = false;
        for (b0, b1) in previous.iter().zip(buttons.iter()) {
            flag |= b0.count_ones() < b1.count_ones();
        }
        flag
    } else {
        trace!("any_btnp failed", 12);
        false
    }
}
/// Returns true if any button was pressed or released
pub fn any_btnpr() -> bool {
    let buttons = unsafe { *GAMEPADS };
    if let Ok(previous) = GAMEPAD_HELPER.read() {
        buttons != *previous
    } else {
        trace!("any_btnpr failed", 12);
        false
    }
}
pub fn mouse_delta() -> MouseInput {
    if let Ok(old) = MOUSE_HELPER.read() {
        let new = mouse();
        MouseInput {
            x: new.x - old.x,
            y: new.y - old.y,
            left: new.left && !old.left,
            middle: new.middle && !old.middle,
            right: new.right && !old.right,
            ..new
        }
    } else {
        trace!("mouse_delta failed", 12);
        MOUSE_INPUT_DEFAULT
    }
}
