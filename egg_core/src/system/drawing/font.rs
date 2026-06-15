//! Bitmap font: a glyph atlas plus cached glyph widths, and the free
//! functions that measure and render text with it.

use super::Canvas;
use super::image::RgbaImage;
use crate::system::types::PrintOptions;

/// An 8×8 bitmap font: an [`RgbaImage`] laid out as a 16×16 grid of glyphs
/// (indexed by `char as u8`) plus the precomputed visual width of every
/// glyph. Caching the widths lets [`text_width`] measure a string without
/// rasterising it to a throwaway canvas. The image is read alpha-only and a
/// glyph's width is its rightmost opaque column + 1.
#[derive(Clone, Debug)]
pub struct Font {
    image: RgbaImage,
    /// Visual width of each glyph, indexed by `char as u8`.
    glyph_widths: [u8; 256],
}

impl Font {
    /// Wrap `image` and precompute every glyph's width.
    pub fn new(image: RgbaImage) -> Self {
        let mut font = Self {
            image,
            glyph_widths: [0; 256],
        };
        font.refresh();
        font
    }

    /// A blank 128×128 font (every glyph zero-width). Fill it via
    /// [`Font::image_mut`] then call [`Font::refresh`] to rebuild the widths.
    pub fn blank() -> Self {
        Self::new(RgbaImage::new(128, 128))
    }

    /// The backing glyph atlas.
    pub fn image(&self) -> &RgbaImage {
        &self.image
    }

    /// Mutable access to the backing pixels. Call [`Font::refresh`] afterwards
    /// so the cached glyph widths match the new pixels.
    pub fn image_mut(&mut self) -> &mut RgbaImage {
        &mut self.image
    }

    /// Recompute every glyph's cached width from the current pixels.
    pub fn refresh(&mut self) {
        for (index, width) in self.glyph_widths.iter_mut().enumerate() {
            *width = glyph_visual_width(&self.image, index as u8);
        }
    }

    /// Visual width of `char`'s glyph in pixels (rightmost opaque column + 1).
    pub fn glyph_width(&self, char: char) -> i32 {
        self.glyph_widths[char as u8 as usize] as i32
    }
}

/// Scan the 8×8 glyph for `char_index` and return its visual width: the
/// rightmost opaque column + 1, or 0 if the glyph is entirely transparent.
fn glyph_visual_width(image: &RgbaImage, char_index: u8) -> u8 {
    let char_index = char_index as usize;
    let glyph_x = (char_index % 16) * 8;
    let glyph_y = (char_index / 16) * 8;
    let mut width = 0;
    for j in 0..8 {
        for i in 0..8 {
            let font_index = (glyph_x + i) + (glyph_y + j) * 128;
            if image.alpha_at_index(font_index) != 0 {
                width = width.max(i as u8 + 1);
            }
        }
    }
    width
}

/// Measure the maximum line width of `text` rendered with `font`, using the
/// font's cached glyph widths (no rasterisation). Useful for centering /
/// wrapping.
pub fn text_width(font: &Font, text: &str, opts: PrintOptions) -> i32 {
    layout(font, text, 0, 0, &opts, |_, _, _| {})
}

/// Render `text` onto `target` using the supplied `font`. Free-function
/// variant of [`ConsoleHelper::print_to`] for callers that already hold a
/// `&Font` reference (e.g. when split-borrowing the console's font and
/// output_image at the same time). To measure text without drawing it, use
/// [`text_width`].
///
/// [`ConsoleHelper::print_to`]: crate::system::ConsoleHelper::print_to
pub fn print_to_with_font<C: Canvas>(
    font: &Font,
    target: &mut C,
    text: &str,
    x: i32,
    y: i32,
    colour: C::Pixel,
    opts: PrintOptions,
) {
    layout(font, text, x, y, &opts, |glyph, dx, dy| {
        draw_letter_to(font, target, glyph, dx, dy, colour);
    });
}

/// Walk `text` one glyph at a time, advancing the pen and tracking the maximum
/// line width. `place` is invoked with each visible glyph and its pen position,
/// so the same layout drives both measuring ([`text_width`], which passes a
/// no-op) and rendering ([`print_to_with_font`]). Advances come from the
/// font's cached glyph widths.
fn layout(
    font: &Font,
    text: &str,
    x: i32,
    y: i32,
    opts: &PrintOptions,
    mut place: impl FnMut(char, i32, i32),
) -> i32 {
    let mut max_width = 0;
    let mut dx = x;
    let mut dy = y;
    for char in text.chars() {
        match char as u8 {
            10 => {
                dx = x;
                dy += 6;
            }
            32 => {
                dx += if opts.small_text { 3 } else { 4 };
            }
            _ => {
                let glyph = if opts.small_text {
                    (char as u8 + 128) as char
                } else {
                    char
                };
                place(glyph, dx, dy);
                dx += font.glyph_width(glyph) + 1;
            }
        }
        max_width = max_width.max(dx - x);
    }
    max_width
}

/// Draw one 8×8 glyph from `font` onto `target` at (`x`, `y`) using `colour`
/// for every non-transparent font pixel.
fn draw_letter_to<C: Canvas>(
    font: &Font,
    target: &mut C,
    char: char,
    x: i32,
    y: i32,
    colour: C::Pixel,
) {
    let char_index = char as u8 as usize;
    let glyph_x = (char_index % 16) * 8;
    let glyph_y = (char_index / 16) * 8;
    let target_w = target.width() as i32;
    let target_h = target.height() as i32;
    let image = font.image();
    for j in 0..8 {
        for i in 0..8 {
            let font_index = (glyph_x + i as usize) + (glyph_y + j as usize) * 128;
            if image.alpha_at_index(font_index) == 0 {
                continue;
            }
            let px = x + i;
            let py = y + j;
            if px < 0 || py < 0 || px >= target_w || py >= target_h {
                continue;
            }
            target.set_pixel(px as u32, py as u32, colour);
        }
    }
}
