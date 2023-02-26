use crate::{animation::{AnimFrame, Animation}, position::Vec2, tic80_core::SpriteOptions};


#[derive(Debug, Clone)]
pub struct TalkPic {
    pub frames: &'static [AnimFrame<'static>],
}
impl TalkPic {
    pub fn to_anim(self) -> Animation<'static> {
        Animation {
            frames: self.frames,
            ..Animation::const_default()
        }
    }
    pub const fn new(frames: &'static [AnimFrame<'static>]) -> Self {
        Self { frames }
    }
}

const SPR_4X4: SpriteOptions = SpriteOptions {w: 2, h: 2, ..SpriteOptions::transparent_zero()};

pub static Y_NORMAL: TalkPic = TalkPic::new(
    &[
        AnimFrame {
            pos: Vec2::new(3, 9),
            spr_id: 920,
            duration: 30,
            options: SPR_4X4,
            outline_colour: Some(0),
            palette_rotate: 0,
        }
    ]
);
pub static Y_AWAY: TalkPic = TalkPic::new(
    &[
        AnimFrame {
            pos: Vec2::new(3, 9),
            spr_id: 988,
            duration: 30,
            options: SPR_4X4,
            outline_colour: Some(0),
            palette_rotate: 0,
        }
    ]
);
pub static Y_LOOK: TalkPic = TalkPic::new(
    &[
        AnimFrame {
            pos: Vec2::new(4, 11),
            spr_id: 980,
            duration: 30,
            options: SPR_4X4,
            outline_colour: Some(0),
            palette_rotate: 0,
        }
    ]
);