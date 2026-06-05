//! Format-agnostic raster core: the [`Canvas`] trait (pixel access, `blit`,
//! immediate-mode primitives) and the discrete [`Transform`] applied during
//! blits. Knows nothing about sprite sheets, maps, or palettes — that layer
//! lives in [`sheet`].

use super::image::{IndexedImage, Rgba, RgbaImage};

pub mod sheet;

/// How `blit` treats destination pixels outside the natural projection of the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgePolicy {
    /// Pixels outside `src` are left untouched.
    #[default]
    Transparent,
    /// Edge pixels of `src` are held outwards to fill the whole destination.
    Clamp,
}

/// 90-degree rotation steps applied to the source before blitting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotate {
    #[default]
    None,
    By90,
    By180,
    By270,
}

/// Discrete transform applied to `src` during a blit: flip, 90-degree rotate,
/// integer upscale. Order is flip -> rotate -> scale, and `(dx, dy)` anchors
/// the top-left of the transformed bounding box on the destination (TIC-80
/// convention: a rotated sprite occupies the rotated bbox starting at (dx, dy)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transform {
    pub flip_x: bool,
    pub flip_y: bool,
    pub rotate: Rotate,
    pub scale: u32,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        flip_x: false,
        flip_y: false,
        rotate: Rotate::None,
        scale: 1,
    };
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A 2D drawable surface. Implementors supply pixel access; primitives and
/// blit are default methods that share the same clipping/rasterisation logic
/// across pixel formats (RGBA, palette index, etc.).
pub trait Canvas {
    type Pixel: Copy;

    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn get_pixel(&self, x: u32, y: u32) -> Self::Pixel;
    fn set_pixel(&mut self, x: u32, y: u32, colour: Self::Pixel);

    /// Blit `src` at (`dx`, `dy`) with optional flip/rotate/scale. Source
    /// pixels for which `is_transparent` returns true are skipped — the
    /// caller supplies that test since it's pixel-format-specific (RGBA
    /// alpha == 0, palette index == colorkey, etc.).
    fn blit<S: Canvas<Pixel = Self::Pixel>>(
        &mut self,
        dx: i32,
        dy: i32,
        src: &S,
        edge: EdgePolicy,
        xform: Transform,
        is_transparent: impl Fn(Self::Pixel) -> bool,
    ) {
        self.blit_with(dx, dy, src, edge, xform, |p| {
            if is_transparent(p) { None } else { Some(p) }
        });
    }

    /// Cross-format blit: same geometry as `blit`, but `convert` maps source
    /// pixels to destination pixels, returning `None` for transparent.
    fn blit_with<S, F>(
        &mut self,
        dx: i32,
        dy: i32,
        src: &S,
        edge: EdgePolicy,
        xform: Transform,
        convert: F,
    ) where
        S: Canvas,
        F: Fn(S::Pixel) -> Option<Self::Pixel>,
    {
        let sw = src.width() as i32;
        let sh = src.height() as i32;
        let dw = self.width() as i32;
        let dh = self.height() as i32;
        let scale = xform.scale.max(1) as i32;
        // Post-rotate, pre-scale extent.
        let (rw, rh) = match xform.rotate {
            Rotate::None | Rotate::By180 => (sw, sh),
            Rotate::By90 | Rotate::By270 => (sh, sw),
        };
        // Final footprint on destination.
        let tw = rw * scale;
        let th = rh * scale;
        let (x0, y0, x1, y1) = match edge {
            EdgePolicy::Transparent => (dx.max(0), dy.max(0), (dx + tw).min(dw), (dy + th).min(dh)),
            EdgePolicy::Clamp => (0, 0, dw, dh),
        };
        for y in y0..y1 {
            for x in x0..x1 {
                // Inverse-map dest -> src: undo translate, scale, rotate, flip.
                let (u, v) = ((x - dx) / scale, (y - dy) / scale);
                let (a, b) = match xform.rotate {
                    Rotate::None => (u, v),
                    Rotate::By90 => (v, rw - 1 - u),
                    Rotate::By180 => (rw - 1 - u, rh - 1 - v),
                    Rotate::By270 => (rh - 1 - v, u),
                };
                let sx = if xform.flip_x { sw - 1 - a } else { a };
                let sy = if xform.flip_y { sh - 1 - b } else { b };
                let sx = sx.clamp(0, sw - 1) as u32;
                let sy = sy.clamp(0, sh - 1) as u32;
                if let Some(pixel) = convert(src.get_pixel(sx, sy)) {
                    self.set_pixel(x as u32, y as u32, pixel);
                }
            }
        }
    }

    // --- Immediate-mode primitives ---

    /// Fills a horizontal run of pixels. Coordinates are clipped to the image.
    fn hline(&mut self, x: i32, y: i32, width: i32, colour: Self::Pixel) {
        if y < 0 || y >= self.height() as i32 || width <= 0 {
            return;
        }
        let x0 = x.max(0);
        let x1 = (x + width).min(self.width() as i32);
        if x0 >= x1 {
            return;
        }
        for px in x0..x1 {
            self.set_pixel(px as u32, y as u32, colour);
        }
    }
    /// Fills a vertical run of pixels. Coordinates are clipped to the image.
    fn vline(&mut self, x: i32, y: i32, height: i32, colour: Self::Pixel) {
        if x < 0 || x >= self.width() as i32 || height <= 0 {
            return;
        }
        let y0 = y.max(0);
        let y1 = (y + height).min(self.height() as i32);
        for py in y0..y1 {
            self.set_pixel(x as u32, py as u32, colour);
        }
    }
    /// Fills a solid rectangle. Coordinates are clipped to the image.
    fn fill_rect(&mut self, x: i32, y: i32, width: i32, height: i32, colour: Self::Pixel) {
        for j in 0..height {
            self.hline(x, y + j, width, colour);
        }
    }
    /// Draws a 1-pixel rectangle border. Coordinates are clipped to the image.
    fn stroke_rect(&mut self, x: i32, y: i32, width: i32, height: i32, colour: Self::Pixel) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.hline(x, y, width, colour);
        self.hline(x, y + height - 1, width, colour);
        self.vline(x, y, height, colour);
        self.vline(x + width - 1, y, height, colour);
    }
    /// Draws a filled rectangle with an outline.
    fn outlined_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        fill_colour: Self::Pixel,
        outline_colour: Self::Pixel,
    ) {
        self.fill_rect(x + 1, y + 1, width - 2, height - 2, fill_colour);
        self.stroke_rect(x, y, width, height, outline_colour);
    }
    /// Bresenham line between two integer endpoints.
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, colour: Self::Pixel) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            if x >= 0 && y >= 0 && (x as u32) < self.width() && (y as u32) < self.height() {
                self.set_pixel(x as u32, y as u32, colour);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
    /// Filled circle (midpoint algorithm).
    fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, colour: Self::Pixel) {
        if radius < 0 {
            return;
        }
        let mut x = radius;
        let mut y = 0;
        let mut err = 1 - x;
        while x >= y {
            self.hline(cx - x, cy + y, 2 * x + 1, colour);
            self.hline(cx - x, cy - y, 2 * x + 1, colour);
            self.hline(cx - y, cy + x, 2 * y + 1, colour);
            self.hline(cx - y, cy - x, 2 * y + 1, colour);
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }
    /// Outlined circle (midpoint algorithm).
    fn stroke_circle(&mut self, cx: i32, cy: i32, radius: i32, colour: Self::Pixel) {
        if radius < 0 {
            return;
        }
        let mut x = radius;
        let mut y = 0;
        let mut err = 1 - x;
        fn plot<C: Canvas + ?Sized>(canvas: &mut C, px: i32, py: i32, colour: C::Pixel) {
            if px >= 0 && py >= 0 && (px as u32) < canvas.width() && (py as u32) < canvas.height() {
                canvas.set_pixel(px as u32, py as u32, colour);
            }
        }
        while x >= y {
            plot(self, cx + x, cy + y, colour);
            plot(self, cx - x, cy + y, colour);
            plot(self, cx + x, cy - y, colour);
            plot(self, cx - x, cy - y, colour);
            plot(self, cx + y, cy + x, colour);
            plot(self, cx - y, cy + x, colour);
            plot(self, cx + y, cy - x, colour);
            plot(self, cx - y, cy - x, colour);
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }
}

impl Canvas for RgbaImage {
    type Pixel = Rgba;
    fn width(&self) -> u32 {
        self.width()
    }
    fn height(&self) -> u32 {
        self.height()
    }
    fn get_pixel(&self, x: u32, y: u32) -> Rgba {
        self.get_pixel(x, y)
    }
    fn set_pixel(&mut self, x: u32, y: u32, colour: Rgba) {
        self.set_pixel(x, y, colour);
    }
}

impl Canvas for IndexedImage {
    type Pixel = u8;
    fn width(&self) -> u32 {
        self.width()
    }
    fn height(&self) -> u32 {
        self.height()
    }
    fn get_pixel(&self, x: u32, y: u32) -> u8 {
        self.get_pixel(x, y)
    }
    fn set_pixel(&mut self, x: u32, y: u32, colour: u8) {
        self.set_pixel(x, y, colour);
    }
}
