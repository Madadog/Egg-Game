// MIT License

// Copyright (c) 2017 Vadim Grigoruk @nesbox // grigoruk@gmail.com

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

// Constants
pub const WIDTH: i32 = 240;
pub const HEIGHT: i32 = 136;

#[derive(Default, Clone, Debug)]
pub struct MouseInput {
    pub x: i16,
    pub y: i16,
    pub scroll_x: i8,
    pub scroll_y: i8,
    pub left: bool,
    pub middle: bool,
    pub right: bool,
}

// Audio
pub struct MusicOptions {
    pub frame: i32,
    pub row: i32,
    pub repeat: bool,
    pub sustain: bool,
    pub tempo: i32,
    pub speed: i32,
}

impl Default for MusicOptions {
    fn default() -> Self {
        Self {
            frame: -1,
            row: -1,
            repeat: true,
            sustain: false,
            tempo: -1,
            speed: -1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SfxOptions {
    pub note: i32,
    pub octave: i32,
    pub duration: i32,
    pub channel: i32,
    pub volume_left: i32,
    pub volume_right: i32,
    pub speed: i32,
}

impl Default for SfxOptions {
    fn default() -> Self {
        Self {
            note: -1,
            octave: -1,
            duration: -1,
            channel: 0,
            volume_left: 15,
            volume_right: 15,
            speed: 0,
        }
    }
}
pub enum TextureSource {
    Tiles,
    Map,
    VBank1,
}

pub struct TTriOptions<'a> {
    pub texture_src: TextureSource,
    pub transparent: &'a [u8],
    pub z1: f32,
    pub z2: f32,
    pub z3: f32,
    pub depth: bool,
}

impl Default for TTriOptions<'_> {
    fn default() -> Self {
        Self {
            texture_src: TextureSource::Tiles,
            transparent: &[],
            z1: 0.0,
            z2: 0.0,
            z3: 0.0,
            depth: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MapOptions<'a> {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub sx: i32,
    pub sy: i32,
    pub transparent: &'a [u8],
    pub scale: i8,
}

impl<'a> MapOptions<'a> {
    pub const fn new(
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        sx: i32,
        sy: i32,
        transparent: &'a [u8],
        scale: i8,
    ) -> Self {
        Self {
            x,
            y,
            w,
            h,
            sx,
            sy,
            transparent,
            scale,
        }
    }
}

impl Default for MapOptions<'_> {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            w: 30,
            h: 17,
            sx: 0,
            sy: 0,
            transparent: &[],
            scale: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Flip {
    None,
    Horizontal,
    Vertical,
    Both,
}

#[derive(Debug, Clone)]
pub enum Rotate {
    None,
    By90,
    By180,
    By270,
}

#[derive(Debug, Clone)]
pub struct SpriteOptions<'a> {
    pub transparent: &'a [u8],
    pub scale: i32,
    pub flip: Flip,
    pub rotate: Rotate,
    pub w: i32,
    pub h: i32,
}
impl<'a> SpriteOptions<'a> {
    pub const fn default() -> Self {
        Self {
            transparent: &[],
            scale: 1,
            flip: Flip::None,
            rotate: Rotate::None,
            w: 1,
            h: 1,
        }
    }
    pub const fn transparent_zero() -> Self {
        Self {
            transparent: &[0],
            ..Self::default()
        }
    }
}
impl Default for SpriteOptions<'_> {
    fn default() -> Self {
        Self {
            transparent: &[],
            scale: 1,
            flip: Flip::None,
            rotate: Rotate::None,
            w: 1,
            h: 1,
        }
    }
}

// Text Output
// The *_raw functions require a null terminated string reference.
// The *_alloc functions can handle any AsRef<str> type, but require the overhead of allocation.
// The macros will avoid the allocation if passed a string literal by adding the null terminator at compile time.

#[derive(Clone)]
pub struct PrintOptions {
    pub color: i32,
    pub fixed: bool,
    pub scale: i32,
    pub small_text: bool,
}
impl PrintOptions {
    pub fn with_color(self, color: i32) -> Self { Self {color, ..self} }
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            color: 15,
            fixed: false,
            scale: 1,
            small_text: false,
        }
    }
}
pub struct FontOptions<'a> {
    pub transparent: &'a [u8],
    pub char_width: i8,
    pub char_height: i8,
    pub fixed: bool,
    pub scale: i32,
    pub alt_font: bool,
}

impl Default for FontOptions<'_> {
    fn default() -> Self {
        Self {
            transparent: &[],
            char_width: 8,
            char_height: 8,
            fixed: false,
            scale: 1,
            alt_font: false,
        }
    }
}