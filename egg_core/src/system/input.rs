use crate::position::Vec2;

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
