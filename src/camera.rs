use crate::Vec2;
use crate::{trace, HEIGHT, WIDTH};

#[derive(Debug)]
pub struct Camera {
    pub pos: Vec2,
    pub bounds: CameraBounds,
}
impl Camera {
    pub const fn new(pos: Vec2, bounds: CameraBounds) -> Self {
        Self { pos, bounds }
    }
    pub const fn const_default() -> Self {
        Camera::new(Vec2::new(0, 0), CameraBounds::bounded((0, 300), (0, 200)))
    }
    pub fn bound(&self, focus_x: Option<i16>, focus_y: Option<i16>) -> Vec2 {
        self.bounds.bound(Vec2::new(
            focus_x.unwrap_or(self.pos.x),
            focus_y.unwrap_or(self.pos.y),
        ))
    }
    pub fn x(&self) -> i32 {
        self.pos.x.into()
    }
    pub fn y(&self) -> i32 {
        self.pos.y.into()
    }
    pub fn center_on(&mut self, x: i16, y: i16) {
        self.pos = self.bound(Some(x - WIDTH as i16 / 2), Some(y - HEIGHT as i16 / 2));
    }
    pub fn from_map_size(w: u8, h: u8, sx: i16, sy: i16) -> Self {
        let (w, h): (i16, i16) = (w.into(), h.into());
        trace!(format!("W: {w}, H: {h}"), 11);
        let (x_offset, y_offset): (i16, i16) = ((WIDTH / 2) as i16, (HEIGHT / 2) as i16);
        let (cx, cy): (i16, i16) = (w * 4 + sx - x_offset, h * 4 + sy - y_offset);
        if w <= 30 && h <= 17 {
            // Area fits inside screen, center and display.
            Camera::new(Vec2::new(cx, cy), CameraBounds::stick(cx, cy))
        } else {
            // Area does not fit inside screen, follow target & add bounds.
            Camera::new(
                Vec2::new(cx, cy),
                CameraBounds {
                    x_bounds: if w >= 30 {
                        CameraRange::Range(sx, sx + w * 8 - WIDTH as i16)
                    } else {
                        CameraRange::Stick(cx)
                    },
                    y_bounds: if h >= 17 {
                        CameraRange::Range(sy, sy + h * 8 - HEIGHT as i16)
                    } else {
                        CameraRange::Stick(cy)
                    },
                },
            )
        }
    }
}
#[derive(Debug, Clone)]
pub struct CameraBounds {
    x_bounds: CameraRange,
    y_bounds: CameraRange,
}
impl CameraBounds {
    pub const fn new(x_bounds: CameraRange, y_bounds: CameraRange) -> Self {
        Self { x_bounds, y_bounds }
    }
    pub const fn stick(x: i16, y: i16) -> Self {
        use CameraRange::*;
        Self::new(Stick(x), Stick(y))
    }
    pub const fn free() -> Self {
        use CameraRange::*;
        Self::new(Free, Free)
    }
    pub const fn bounded(x: (i16, i16), y: (i16, i16)) -> Self {
        use CameraRange::*;
        Self::new(Range(x.0, x.1 - 240), Range(y.0, y.1 - 136))
    }
    pub fn bound(&self, focus: Vec2) -> Vec2 {
        Vec2::new(self.x_bounds.bound(focus.x), self.y_bounds.bound(focus.y))
    }
}
/// Restriction of camera's movement along x or y axes.
/// `Range` clamps to `(min, max)`. `Stick` restricts to a single value.
/// `Free` gives full `i16` range.
#[derive(Debug, Clone)]
pub enum CameraRange {
    Free,
    Stick(i16),
    Range(i16, i16),
}
impl CameraRange {
    pub fn bound(&self, value: i16) -> i16 {
        match self {
            Self::Free => value,
            Self::Stick(x) => *x,
            Self::Range(min, max) => value.clamp(*min, *max),
        }
    }
}
