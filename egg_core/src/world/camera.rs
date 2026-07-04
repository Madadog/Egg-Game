use crate::geometry::Vec2;
use CameraRange::*;

// Base resolution, used only by the `const fn` constructors (`default`/`bounded`)
// that build the placeholder camera before a map loads. Live cameras receive the
// runtime viewport size (which can grow in "mirror window" mode) via `center_on`
// and `from_map_size`, so a bigger framebuffer reveals more of the map.
const WIDTH: i16 = crate::platform::WIDTH as i16;
const HEIGHT: i16 = crate::platform::HEIGHT as i16;

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
    /// Centre the viewport on `(x, y)` for a `w`×`h` pixel screen.
    pub fn center_on(&mut self, x: i16, y: i16, w: i16, h: i16) {
        self.pos = self.bound(Some(x - w / 2), Some(y - h / 2));
    }
    /// Build a camera framing a map area of `size` tiles at pixel `offset`, sized
    /// for a `w`×`h` pixel screen. A larger screen frames more of the map.
    pub fn from_map_size(size: Vec2, offset: Vec2, w: i16, h: i16) -> Self {
        assert!(size.x.is_positive() && size.y.is_positive());

        let cam_offset = Vec2::new(w / 2, h / 2);
        let center = size * 4 + offset - cam_offset;

        if size.x <= w / 8 && size.y <= h / 8 {
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
                    x_bounds: if size.x >= w / 8 {
                        CameraRange::Range(offset.x, offset.x + size.x * 8 - w)
                    } else {
                        CameraRange::Stick(center.x)
                    },
                    y_bounds: if size.y >= h / 8 {
                        CameraRange::Range(offset.y, offset.y + size.y * 8 - h)
                    } else {
                        CameraRange::Stick(center.y)
                    },
                },
            )
        }
    }
}

/// Progress of a screen shake: `left` of `total` frames remain, at up to
/// ±`amplitude` px. The offset pattern is a fixed 4-phase cycle keyed on
/// `left` — fully deterministic, so a scrubber re-sim reproduces it. Shared by
/// the cutscene `shake` verb and the dialogue `#shake` directive: the owner
/// holds an `Option<Shake>`, arms it with [`begin`](Self::begin), advances it
/// once per frame with [`tick`](Self::tick), and adds [`offset`](Self::offset)
/// to whatever focus it centres the camera on (bounds still clamp, so a shake
/// at a map edge is absorbed rather than showing past the map).
#[derive(Debug, Clone)]
pub struct Shake {
    total: u32,
    left: u32,
    amplitude: i16,
}
impl Shake {
    /// A shake running `frames` frames at up to ±`amplitude` px — or `None`
    /// for zero frames, so a data-authored `0` is simply spent already.
    pub fn begin(frames: u32, amplitude: i16) -> Option<Self> {
        (frames > 0).then_some(Self {
            total: frames,
            left: frames,
            amplitude,
        })
    }
    /// This frame's focus offset: a right/up/left/down cycle whose amplitude
    /// tapers linearly to nothing as `left` runs out (ceiling division, so the
    /// tail stays at ±1 rather than flatlining early on small amplitudes).
    pub fn offset(&self) -> Vec2 {
        let (amp, left, total) = (self.amplitude as i64, self.left as i64, self.total as i64);
        let a = ((amp * left + total - 1) / total) as i16;
        match self.left % 4 {
            0 => Vec2::new(a, 0),
            1 => Vec2::new(0, -a),
            2 => Vec2::new(-a, 0),
            _ => Vec2::new(0, a),
        }
    }
    /// Advance `slot` by one frame; a spent shake drops to `None`. Call exactly
    /// once per frame on each live slot.
    pub fn tick(slot: &mut Option<Shake>) {
        if let Some(shake) = slot {
            shake.left -= 1;
            if shake.left == 0 {
                *slot = None;
            }
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
        Self::new(Stick(x), Stick(y))
    }
    pub const fn free() -> Self {
        Self::new(Free, Free)
    }
    pub const fn bounded(x: (i16, i16), y: (i16, i16)) -> Self {
        Self::new(Range(x.0, x.1 - WIDTH), Range(y.0, y.1 - HEIGHT))
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
