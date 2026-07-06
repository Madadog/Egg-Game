use crate::{SCANCODE_COUNT, ScanCode};
use egg_render::geometry::Vec2;

/// Mouse state holding `[current, previous]` for every field, so movement and
/// button edges are always well-defined. Index `0` is the current frame, index
/// `1` the previous. Call [`MouseInput::shift`] once per frame before writing
/// the new current values.
#[derive(Default, Clone, Copy, Debug)]
pub struct MouseInput {
    pub x: [i16; 2],
    pub y: [i16; 2],
    pub scroll_x: [i8; 2],
    pub scroll_y: [i8; 2],
    pub left: [bool; 2],
    pub middle: [bool; 2],
    pub right: [bool; 2],
}

impl MouseInput {
    /// Current cursor position.
    pub fn pos(&self) -> Vec2 {
        Vec2::new(self.x[0], self.y[0])
    }
    /// Cursor position on the previous frame.
    pub fn previous_pos(&self) -> Vec2 {
        Vec2::new(self.x[1], self.y[1])
    }
    /// Whether the cursor moved since last frame.
    pub fn moved(&self) -> bool {
        self.x[0] != self.x[1] || self.y[0] != self.y[1]
    }
    /// Roll the current values into the previous slot, making room for this
    /// frame's values to be written into the current (index `0`) slot.
    pub fn step(&mut self) {
        self.x[1] = self.x[0];
        self.y[1] = self.y[0];
        self.scroll_x[1] = self.scroll_x[0];
        self.scroll_y[1] = self.scroll_y[0];
        self.left[1] = self.left[0];
        self.middle[1] = self.middle[0];
        self.right[1] = self.right[0];
    }
}

/// Whether a `[current, previous]` button is held this frame.
pub fn pressed(button: [bool; 2]) -> bool {
    button[0]
}

/// Whether a `[current, previous]` button was just pressed this frame — down
/// now, up last frame (rising edge).
pub fn just_pressed(button: [bool; 2]) -> bool {
    button[0] && !button[1]
}

/// Gamepad state holding `[current, previous]` for every button, mirroring
/// [`MouseInput`]. Index `0` is the current frame, `1` the previous. Buttons
/// follow the TIC-80 layout: directions (`up`/`down`/`left`/`right`) then the
/// `a`/`b`/`x`/`y` face buttons. Read edges with the shared [`pressed`] and
/// [`just_pressed`] helpers, exactly as with the mouse buttons.
#[derive(Default, Clone, Copy, Debug)]
pub struct Controller {
    pub up: [bool; 2],
    pub down: [bool; 2],
    pub left: [bool; 2],
    pub right: [bool; 2],
    pub a: [bool; 2],
    pub b: [bool; 2],
    pub x: [bool; 2],
    pub y: [bool; 2],
}

impl Controller {
    /// All eight buttons in TIC-80 index order: up, down, left, right, A, B, X, Y.
    fn buttons(&self) -> [[bool; 2]; 8] {
        [
            self.up, self.down, self.left, self.right, self.a, self.b, self.x, self.y,
        ]
    }
    /// Whether any button is held this frame.
    pub fn any_pressed(&self) -> bool {
        self.buttons().into_iter().any(pressed)
    }
    /// Whether any button had a rising edge this frame (down now, up last frame).
    pub fn any_just_pressed(&self) -> bool {
        self.buttons().into_iter().any(just_pressed)
    }
    /// Whether any button changed state since last frame (press or release).
    pub fn changed(&self) -> bool {
        self.buttons().iter().any(|b| b[0] != b[1])
    }
    /// Release buttons, update last frame state (for `just_pressed`). Call once per frame.
    pub fn step(&mut self) {
        for b in [
            &mut self.up,
            &mut self.down,
            &mut self.left,
            &mut self.right,
            &mut self.a,
            &mut self.b,
            &mut self.x,
            &mut self.y,
        ] {
            b[1] = b[0];
            b[0] = false;
        }
    }
}

/// Cardinal D-pad delta from a controller — each axis in `-1..=1` (right/down
/// positive). `edge` selects held ([`pressed`]) vs rising-edge
/// ([`just_pressed`]) reads.
pub fn dpad_delta(pad: &Controller, edge: impl Fn([bool; 2]) -> bool) -> (i16, i16) {
    let axis = |neg, pos| edge(pos) as i16 - edge(neg) as i16;
    (axis(pad.left, pad.right), axis(pad.up, pad.down))
}

/// A whole frame's accumulated input: the four gamepads, the keyboard edge
/// state, and the characters typed. The host fills one of these per window each
/// frame and threads it into the engine as data (via `Ctx::input`),
/// so the host — not the console — decides which window's input a step sees.
#[derive(Clone, Debug)]
pub struct EggInput {
    pub controllers: [Controller; 4],
    pub keyboard: [bool; SCANCODE_COUNT],
    pub previous_keyboard: [bool; SCANCODE_COUNT],
    /// Consecutive fixed steps each scancode has been held (0 while up), advanced
    /// in [`refresh`](Self::refresh) — drives [`key_repeat`](Self::key_repeat).
    pub held: [u16; SCANCODE_COUNT],
    pub mouse: MouseInput,
    pub typed_chars: Vec<char>,
}
impl Default for EggInput {
    fn default() -> Self {
        Self::new()
    }
}

impl EggInput {
    pub fn new() -> Self {
        Self {
            controllers: [Controller::default(); 4],
            keyboard: [false; SCANCODE_COUNT],
            previous_keyboard: [false; SCANCODE_COUNT],
            held: [0; SCANCODE_COUNT],
            mouse: MouseInput::default(),
            typed_chars: Vec::with_capacity(8),
        }
    }
    pub fn press_key(&mut self, key: ScanCode) {
        if let Some(down) = self.keyboard.get_mut(key.index()) {
            *down = true;
        }
    }
    pub fn push_char(&mut self, c: char) {
        self.typed_chars.push(c);
    }
    pub fn refresh(&mut self) {
        // Advance the per-key hold counters from the frame that just ended — the
        // `keyboard` array still holds it here, before the clear below.
        for (held, &down) in self.held.iter_mut().zip(&self.keyboard) {
            *held = if down { held.saturating_add(1) } else { 0 };
        }
        self.previous_keyboard = self.keyboard;
        self.mouse.step();
        for controller in &mut self.controllers {
            controller.step();
        }
        self.keyboard = [false; SCANCODE_COUNT];
        self.typed_chars.clear();
    }
    pub fn key_chars(&self) -> &[char] {
        &self.typed_chars
    }
    /// Index a per-scancode array by `key`, yielding the type's default (`false` /
    /// `0`) for an out-of-range scancode. `ScanCode::index()` is always in range,
    /// so this just keeps every lookup panic-free behind one helper.
    fn at<T: Copy + Default>(array: &[T], key: ScanCode) -> T {
        array.get(key.index()).copied().unwrap_or_default()
    }
    /// Whether `key` is down this frame.
    pub fn key(&self, key: ScanCode) -> bool {
        Self::at(&self.keyboard, key)
    }
    /// Whether `key` was down on the previous frame.
    fn was_down(&self, key: ScanCode) -> bool {
        Self::at(&self.previous_keyboard, key)
    }
    /// Fixed steps `key` has been held (0 while up).
    fn held_steps(&self, key: ScanCode) -> u16 {
        Self::at(&self.held, key)
    }
    /// True only on the frame `key` goes down (down now, up last frame).
    pub fn keyp(&self, key: ScanCode) -> bool {
        self.key(key) && !self.was_down(key)
    }
    /// Edge-or-repeat: true on the initial press, then — while still held — again
    /// every `rate` fixed steps after an initial `delay` (both in fixed steps).
    /// `delay`/`rate` are per-call so different consumers can tune their cadence.
    pub fn key_repeat(&self, key: ScanCode, delay: u16, rate: u16) -> bool {
        if !self.key(key) {
            return false;
        }
        let held = self.held_steps(key);
        if held == 0 {
            return true;
        }
        held >= delay && (held - delay).is_multiple_of(rate.max(1))
    }
    /// Player one's [`Controller`], mirroring the `mouse` field. Returns a copy;
    /// read it with the shared [`pressed`]/[`just_pressed`] helpers, e.g.
    /// `just_pressed(input.controller().a)`.
    pub fn controller(&self) -> Controller {
        self.controllers[0]
    }
    /// Whether any button on any controller was just pressed this frame. Ignores
    /// button releases.
    pub fn any_btnp(&self) -> bool {
        self.controllers.iter().any(Controller::any_just_pressed)
    }
    /// Whether any button on any controller was pressed or released this frame.
    pub fn any_btnpr(&self) -> bool {
        self.controllers.iter().any(Controller::changed)
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    /// `key_repeat` fires on the press frame, then — once held past `delay` — every
    /// `rate` fixed steps, and never while the key is up. One frame = `refresh()`
    /// (advances the hold counter from last frame, clears `keyboard`) then a press.
    #[test]
    fn key_repeat_fires_on_press_then_after_delay_at_rate() {
        let mut input = EggInput::new();
        let k = ScanCode::Backspace;
        let (delay, rate) = (3u16, 2u16);

        let mut fired = Vec::new();
        for frame in 0..10 {
            input.refresh();
            input.press_key(k);
            if input.key_repeat(k, delay, rate) {
                fired.push(frame);
            }
        }
        // Initial press at 0, then held reaches `delay` (3) and repeats every `rate`.
        assert_eq!(fired, vec![0, 3, 5, 7, 9]);

        // A held key that's no longer pressed this frame never repeats…
        input.refresh();
        assert!(!input.key_repeat(k, delay, rate));
        // …and after release the counter resets, so a fresh press fires again.
        input.refresh();
        input.press_key(k);
        assert!(input.key_repeat(k, delay, rate));
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;

    #[test]
    fn edges_and_movement() {
        let mut m = MouseInput {
            x: [5, 5],
            y: [9, 7],
            ..Default::default()
        };
        assert_eq!(m.pos(), Vec2::new(5, 9));
        assert!(m.moved()); // y differs from last frame

        m.y = [7, 7];
        assert!(!m.moved());

        m.left = [true, false];
        assert!(pressed(m.left));
        assert!(just_pressed(m.left));

        m.left = [true, true];
        assert!(pressed(m.left)); // still held...
        assert!(!just_pressed(m.left)); // ...but not a new press

        m.left = [false, true];
        assert!(!pressed(m.left));
        assert!(!just_pressed(m.left));
    }

    #[test]
    fn shift_rolls_current_into_previous() {
        let mut m = MouseInput {
            x: [3, 0],
            left: [true, false],
            ..Default::default()
        };
        m.step();
        assert_eq!(m.x, [3, 3]);
        assert_eq!(m.left, [true, true]);
    }
}

#[cfg(test)]
mod controller_tests {
    use super::*;

    #[test]
    fn edges_and_aggregates() {
        let mut c = Controller {
            a: [true, false],
            ..Default::default()
        };
        assert!(pressed(c.a));
        assert!(just_pressed(c.a));
        assert!(c.any_pressed());
        assert!(c.any_just_pressed());
        assert!(c.changed());

        c.a = [true, true];
        assert!(pressed(c.a)); // still held...
        assert!(!just_pressed(c.a)); // ...but not a new press
        assert!(!c.changed());
    }

    #[test]
    fn shift_rolls_current_and_clears() {
        let mut c = Controller {
            up: [true, false],
            ..Default::default()
        };
        c.step();
        // Previous holds last frame's press; current resets to released.
        assert_eq!(c.up, [false, true]);
    }
}
