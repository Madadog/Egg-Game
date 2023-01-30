use crate::{Vec2, SpriteOptions};

pub struct AnimFrame<'a> {
    pub pos: Vec2,
    pub id: u16,
    pub length: u16,
    pub options: SpriteOptions<'a>,
}
impl<'a> AnimFrame<'a> {
    pub const fn new(pos: Vec2, id: u16, length: u16, options: SpriteOptions<'a>) -> Self { Self { pos, id, length, options } }
    pub fn const_default() -> Self {
        Self {
            pos: Vec2::new(0, 0),
            id: 0,
            length: 1,
            options: SpriteOptions::transparent_zero(),
        }
    }
}

pub struct Animation<'a> {
    pub tick: u16,
    pub index: usize,
    pub frames: &'a [AnimFrame<'a>],
}
impl<'a> Animation<'a> {
    pub const fn const_default() -> Self {
        Self { tick: 0, index: 0, frames: &[] }
    }
    pub fn current_frame(&self) -> &AnimFrame<'a> {
        &self.frames[self.index]
    }
    pub fn advance(&mut self) {
        if self.tick >= self.current_frame().length {
            self.index += 1;
            if self.index == self.frames.len() { self.index = 0; }
            self.tick = 0;
        } else {
            self.tick += 1;
        }
    }
}
