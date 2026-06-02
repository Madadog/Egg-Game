use super::image::{IndexedImage, Rgba, RgbaImage};
use super::{Flip, MapOptions, StaticSpriteOptions};
use crate::system::types::MapLayer;

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

/// Number of 8-pixel tiles in one row of a sprite sheet.
#[inline]
const fn tiles_per_row(sheet_width: u32) -> i32 {
    (sheet_width / 8) as i32
}

/// Source-coordinate top-left of the 8×8 tile at `id` within a sheet of
/// `tiles_per_row` columns. TIC-80 convention: tiles are laid out left-to-right,
/// top-to-bottom.
#[inline]
fn tile_origin(id: i32, tiles_per_row: i32) -> (i32, i32) {
    let row = id.div_euclid(tiles_per_row);
    let col = id.rem_euclid(tiles_per_row);
    (col * 8, row * 8)
}

#[inline]
fn xform_from_opts(opts: &StaticSpriteOptions<'_>) -> Transform {
    let (flip_x, flip_y) = match opts.flip {
        Flip::None => (false, false),
        Flip::Horizontal => (true, false),
        Flip::Vertical => (false, true),
        Flip::Both => (true, true),
    };
    let rotate = match opts.rotate {
        crate::system::types::Rotate::None => Rotate::None,
        crate::system::types::Rotate::By90 => Rotate::By90,
        crate::system::types::Rotate::By180 => Rotate::By180,
        crate::system::types::Rotate::By270 => Rotate::By270,
    };
    Transform {
        flip_x,
        flip_y,
        rotate,
        scale: opts.scale.max(1) as u32,
    }
}

/// Blit one 8×8 tile from a sprite sheet at the given destination position,
/// with the transform from `opts`. `convert` maps a source pixel to either a
/// destination pixel or `None` (transparent).
fn blit_tile<D, S, F>(
    dest: &mut D,
    source: &S,
    src_tx: i32,
    src_ty: i32,
    dx: i32,
    dy: i32,
    xform: Transform,
    convert: F,
) where
    D: Canvas + ?Sized,
    S: Canvas,
    F: Fn(S::Pixel) -> Option<D::Pixel>,
{
    let scale = xform.scale.max(1) as i32;
    let (rw, rh) = match xform.rotate {
        Rotate::None | Rotate::By180 => (8, 8),
        Rotate::By90 | Rotate::By270 => (8, 8),
    };
    let tw = rw * scale;
    let th = rh * scale;
    let dw = dest.width() as i32;
    let dh = dest.height() as i32;
    let x0 = dx.max(0);
    let y0 = dy.max(0);
    let x1 = (dx + tw).min(dw);
    let y1 = (dy + th).min(dh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let sw = source.width() as i32;
    let sh = source.height() as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let (u, v) = ((x - dx) / scale, (y - dy) / scale);
            let (a, b) = match xform.rotate {
                Rotate::None => (u, v),
                Rotate::By90 => (v, rw - 1 - u),
                Rotate::By180 => (rw - 1 - u, rh - 1 - v),
                Rotate::By270 => (rh - 1 - v, u),
            };
            let a = if xform.flip_x { 7 - a } else { a };
            let b = if xform.flip_y { 7 - b } else { b };
            let sx = src_tx + a;
            let sy = src_ty + b;
            if sx < 0 || sy < 0 || sx >= sw || sy >= sh {
                continue;
            }
            if let Some(p) = convert(source.get_pixel(sx as u32, sy as u32)) {
                dest.set_pixel(x as u32, y as u32, p);
            }
        }
    }
}

/// Walks the `opts.w` × `opts.h` tile grid starting at `id`, invoking `draw_one`
/// for each tile with its destination (x, y) and source tile id. Honours
/// `flip` so multi-tile sprites flip their tile order too.
fn for_each_tile<F: FnMut(i32, i32, i32)>(
    id: i32,
    x: i32,
    y: i32,
    opts: &StaticSpriteOptions<'_>,
    mut draw_one: F,
) {
    let flip_x = matches!(opts.flip, Flip::Horizontal | Flip::Both);
    let flip_y = matches!(opts.flip, Flip::Vertical | Flip::Both);
    let scale = opts.scale.max(1);
    let tile_px = 8 * scale;
    for j in 0..opts.h {
        for i in 0..opts.w {
            let ti = if flip_x { opts.w - 1 - i } else { i };
            let tj = if flip_y { opts.h - 1 - j } else { j };
            let tile_id = id + ti + tj * 32;
            let dx = x + i * tile_px;
            let dy = y + j * tile_px;
            draw_one(tile_id, dx, dy);
        }
    }
}

/// Draw a multi-tile sprite from the indexed sheet `source` onto any `Canvas`,
/// mapping each source index through `convert` (`None` = transparent).
fn draw_sprite<D, F>(
    dest: &mut D,
    source: &IndexedImage,
    id: i32,
    x: i32,
    y: i32,
    opts: &StaticSpriteOptions<'_>,
    convert: F,
) where
    D: Canvas,
    F: Fn(u8) -> Option<D::Pixel>,
{
    let xform = xform_from_opts(opts);
    let tpr = tiles_per_row(source.width());
    for_each_tile(id, x, y, opts, |tile_id, dx, dy| {
        let (tx, ty) = tile_origin(tile_id, tpr);
        blit_tile(dest, source, tx, ty, dx, dy, xform, &convert);
    });
}

/// Draw a region of `layer` (sampling the indexed sheet `source`) onto any
/// `Canvas`, mapping each source index through `convert` (`None` = transparent).
fn draw_map<D, F>(
    dest: &mut D,
    layer: &MapLayer,
    source: &IndexedImage,
    mut opts: MapOptions,
    convert: F,
) where
    D: Canvas,
    F: Fn(u8) -> Option<D::Pixel>,
{
    let dw = dest.width() as i32;
    let dh = dest.height() as i32;
    if opts.sx + opts.w * 8 < 0 || opts.sy + opts.h * 8 < 0 || opts.sx >= dw || opts.sy >= dh {
        return;
    }
    // Crop whole off-screen tiles. Use truncated division (Rust's `/`) so a
    // partial tile at sx=-1 keeps drawing — `div_euclid` would round away from
    // zero and crop a whole tile.
    if opts.sx <= 0 {
        let x_tiles = -(opts.sx / 8);
        opts.sx += x_tiles * 8;
        opts.x += x_tiles;
        opts.w -= x_tiles;
    }
    if opts.sy <= 0 {
        let y_tiles = -(opts.sy / 8);
        opts.sy += y_tiles * 8;
        opts.y += y_tiles;
        opts.h -= y_tiles;
    }
    let tpr = tiles_per_row(source.width());
    for j in 0..opts.h {
        for i in 0..opts.w {
            let Ok(mx) = usize::try_from(opts.x + i) else {
                continue;
            };
            let Ok(my) = usize::try_from(opts.y + j) else {
                continue;
            };
            let Some(tile_id) = MapLayer::get(layer, mx, my) else {
                continue;
            };
            let (tx, ty) = tile_origin(tile_id as i32, tpr);
            let dx = opts.sx + i * 8;
            let dy = opts.sy + j * 8;
            blit_tile(dest, source, tx, ty, dx, dy, Transform::IDENTITY, &convert);
        }
    }
}

impl RgbaImage {
    /// Draw an indexed sprite from `source` onto this canvas at (`x`, `y`).
    /// `palette_map` is applied to each source pixel index before `palette`
    /// lookup. Indices listed in `opts.transparent` are skipped.
    pub fn spr_indexed(
        &mut self,
        source: &IndexedImage,
        palette: &[[u8; 3]],
        palette_map: &[usize],
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions<'_>,
    ) {
        let transparent = opts.transparent;
        draw_sprite(self, source, id, x, y, &opts, |idx| {
            if transparent.contains(&idx) {
                return None;
            }
            let mapped = palette_map
                .get(idx as usize)
                .copied()
                .unwrap_or(idx as usize);
            palette.get(mapped).map(|rgb| Rgba::from_rgb(*rgb))
        });
    }

    /// Draw a 1-pixel outline of `id` by stamping it four times in cardinal
    /// directions with every palette entry mapped to `outline_colour`.
    pub fn spr_outline(
        &mut self,
        source: &IndexedImage,
        palette: &[[u8; 3]],
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions<'_>,
        outline_colour: u8,
    ) {
        let outline_map = [outline_colour as usize; 16];
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            self.spr_indexed(
                source,
                palette,
                &outline_map,
                id,
                x + dx,
                y + dy,
                opts.clone(),
            );
        }
    }

    /// Draw a region of `layer` onto this canvas, sampling `source` for each
    /// tile and looking colours up through `palette_map` + `palette`.
    pub fn map_draw_indexed(
        &mut self,
        layer: &MapLayer,
        source: &IndexedImage,
        palette: &[[u8; 3]],
        palette_map: &[usize],
        opts: MapOptions,
    ) {
        let transparent = opts.transparent;
        draw_map(self, layer, source, opts, |idx| {
            if let Some(t) = transparent
                && idx == t
            {
                return None;
            }
            let mapped = palette_map
                .get(idx as usize)
                .copied()
                .unwrap_or(idx as usize);
            palette.get(mapped).map(|rgb| Rgba::from_rgb(*rgb))
        });
    }
}

impl IndexedImage {
    /// Composite this indexed image onto `target` at (`dx`, `dy`) by looking
    /// each index up in `palette`. Indices listed in `transparent` are
    /// skipped (target pixel left untouched). `edge` controls what happens
    /// to destination pixels outside the natural projection of the source —
    /// `Clamp` repeats the source's edge pixels, which is the right choice
    /// when an offset (e.g. screen shake) would otherwise leave seams.
    pub fn draw_to_rgba(
        &self,
        target: &mut RgbaImage,
        dx: i32,
        dy: i32,
        palette: &[[u8; 3]],
        transparent: &[u8],
        edge: EdgePolicy,
    ) {
        target.blit_with(dx, dy, self, edge, Transform::IDENTITY, |idx| {
            if transparent.contains(&idx) {
                None
            } else {
                palette
                    .get(usize::from(idx))
                    .map(|rgb| Rgba::from_rgb(*rgb))
            }
        });
    }

    /// Draw an indexed sprite from `source` onto this canvas at (`x`, `y`).
    /// Indices listed in `opts.transparent` are skipped; all other indices are
    /// copied through unchanged (no palette lookup — that's a compositing-time
    /// concern).
    pub fn spr(
        &mut self,
        source: &IndexedImage,
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions<'_>,
    ) {
        let transparent = opts.transparent;
        draw_sprite(self, source, id, x, y, &opts, |idx| {
            if transparent.contains(&idx) {
                None
            } else {
                Some(idx)
            }
        });
    }

    /// Draw a region of `layer` onto this canvas using `source` for tile pixels.
    pub fn map_draw(&mut self, layer: &MapLayer, source: &IndexedImage, opts: MapOptions) {
        let transparent = opts.transparent;
        draw_map(self, layer, source, opts, |idx| {
            if let Some(t) = transparent
                && idx == t
            {
                None
            } else {
                Some(idx)
            }
        });
    }
}
