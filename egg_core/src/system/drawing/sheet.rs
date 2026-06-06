//! TIC-80-style sheet rendering on top of the raster core in the parent
//! [`drawing`](super) module: 8×8 tile addressing, multi-tile sprites,
//! map-region drawing, and the palette/colorkey policy that resolves indexed
//! pixels to colours. The public surface is the inherent methods on
//! [`RgbaImage`] and [`IndexedImage`] below.

use super::image::{IndexedImage, Rgba, RgbaImage};
use super::{Canvas, EdgePolicy, Rotate, Transform};
use crate::system::{MapLayer, MapOptions, StaticSpriteOptions};

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

/// Resolve a source palette index to an output colour: remap through
/// `palette_map` (indices past its end map to themselves, so `&[]` is the
/// identity map), then look the result up in `palette`. `None` when `palette`
/// has no such entry.
#[inline]
fn palette_colour(palette: &[[u8; 3]], palette_map: &[usize], idx: u8) -> Option<Rgba> {
    let mapped = palette_map
        .get(idx as usize)
        .copied()
        .unwrap_or(idx as usize);
    palette.get(mapped).map(|rgb| Rgba::from_rgb(*rgb))
}

#[inline]
fn xform_from_opts(opts: &StaticSpriteOptions<'_>) -> Transform {
    Transform {
        flip_x: opts.flip.x(),
        flip_y: opts.flip.y(),
        rotate: opts.rotate,
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
    // Tiles are square (8×8), so rotation never changes the bounding box.
    let (rw, rh) = (8, 8);
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
/// for each tile with its destination (x, y) and source tile id. Tile ids
/// advance by `tiles_per_row` per sprite row (the sheet's own row stride), and
/// `flip` reverses tile order so multi-tile sprites flip as a whole.
fn for_each_tile<F: FnMut(i32, i32, i32)>(
    id: i32,
    x: i32,
    y: i32,
    tiles_per_row: i32,
    opts: &StaticSpriteOptions<'_>,
    mut draw_one: F,
) {
    let flip_x = opts.flip.x();
    let flip_y = opts.flip.y();
    let scale = opts.scale.max(1);
    let tile_px = 8 * scale;
    for j in 0..opts.h {
        for i in 0..opts.w {
            let ti = if flip_x { opts.w - 1 - i } else { i };
            let tj = if flip_y { opts.h - 1 - j } else { j };
            let tile_id = id + ti + tj * tiles_per_row;
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
    if tpr <= 0 {
        // Empty/unloaded sheet: nothing to draw (and `tile_origin` would
        // divide by zero).
        return;
    }
    for_each_tile(id, x, y, tpr, opts, |tile_id, dx, dy| {
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
    if tpr <= 0 {
        // Empty/unloaded sheet: nothing to draw (and `tile_origin` would
        // divide by zero).
        return;
    }
    for j in 0..opts.h {
        for i in 0..opts.w {
            let (Ok(mx), Ok(my)) = (usize::try_from(opts.x + i), usize::try_from(opts.y + j))
            else {
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
                None
            } else {
                palette_colour(palette, palette_map, idx)
            }
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
            if transparent == Some(idx) {
                None
            } else {
                palette_colour(palette, palette_map, idx)
            }
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
                palette_colour(palette, &[], idx)
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
            if transparent == Some(idx) {
                None
            } else {
                Some(idx)
            }
        });
    }
}
