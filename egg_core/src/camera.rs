use crate::position::Vec2;

const HEIGHT: i16 = 136;
const WIDTH: i16 = 240;

#[derive(Debug, Clone)]
pub struct Camera {
    pub pos: Vec2,
    pub bounds: CameraBounds,
}
impl Camera {
    pub const fn new(pos: Vec2, bounds: CameraBounds) -> Self {
        Self { pos, bounds }
    }
    pub const fn default() -> Self {
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
        self.pos = self.bound(Some(x - WIDTH / 2), Some(y - HEIGHT / 2));
    }
    pub fn from_map_size(size: Vec2, offset: Vec2) -> Self {
        assert!(size.x.is_positive() && size.y.is_positive());

        let cam_offset = Vec2::new(WIDTH / 2, HEIGHT / 2);
        let center = size * 4 + offset - cam_offset;

        if size.x <= WIDTH / 8 && size.y <= HEIGHT / 8 {
            // Area fits inside screen, center and display.
            Camera::new(
                Vec2::new(center.x, center.y),
                CameraBounds::stick(center.x, center.y),
            )
        } else {
            // Area does not fit inside screen, follow target & add bounds.
            Camera::new(
                Vec2::new(center.x, center.y),
                CameraBounds {
                    x_bounds: if size.x >= WIDTH / 8 {
                        CameraRange::Range(offset.x, offset.x + size.x * 8 - WIDTH)
                    } else {
                        CameraRange::Stick(center.x)
                    },
                    y_bounds: if size.y >= HEIGHT / 8 {
                        CameraRange::Range(offset.y, offset.y + size.y * 8 - HEIGHT)
                    } else {
                        CameraRange::Stick(center.y)
                    },
                },
            )
        }
    }
}

use CameraRange::*;

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
        Self::new(Stick(x), Stick(y))
    }
    pub const fn free() -> Self {
        Self::new(Free, Free)
    }
    pub const fn bounded(x: (i16, i16), y: (i16, i16)) -> Self {
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
