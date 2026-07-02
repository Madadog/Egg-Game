/// Keyboard scancodes. Values 1..=65 follow the TIC-80 numbering; values
/// after that (Escape, F1..F12) are extensions specific to this console.
///
/// Use these instead of bare numbers when calling [`EggInput::key`] /
/// [`EggInput::keyp`] so the meaning of each key press stays obvious.
///
/// [`EggInput::key`]: crate::platform::EggInput::key
/// [`EggInput::keyp`]: crate::platform::EggInput::keyp
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ScanCode {
    A = 1,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0 = 27,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Minus = 37,
    Equals,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Apostrophe,
    Grave,
    Comma,
    Period,
    Slash,
    Space = 48,
    Tab,
    Return,
    Backspace,
    Delete,
    Insert,
    PageUp,
    PageDown,
    Home,
    End,
    Up = 58,
    Down,
    Left,
    Right,
    CapsLock = 62,
    Ctrl,
    Shift,
    Alt,
    Escape = 66,
    F1 = 67,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

pub const SCANCODE_COUNT: usize = 78;

impl ScanCode {
    /// 1-based TIC-80-compatible scancode number.
    pub const fn number(self) -> u8 {
        self as u8
    }
    /// 0-based index into the `keyboard` array.
    pub const fn index(self) -> usize {
        (self as u8 - 1) as usize
    }
}
