use crate::{
    position::Vec2,
    tic80_core::{spr, SpriteOptions}, tic80_helpers::{draw_outline, palette_map_rotate, spr_outline},
};

#[derive(Debug, Clone)]
pub enum PicContainer {
    Pic4x4(&'static Pic4x4),
    PicSingle(&'static PicSingle),
}
impl PicContainer {
    pub fn draw_offset(&self, offset: Vec2) {
        match self {
            Self::Pic4x4(x) => x.draw_offset(offset),
            Self::PicSingle(x) => x.draw_offset(offset),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pic4x4 {
    spr_ids: [i16; 4],
    offset: (i8, i8),
}
impl Pic4x4 {
    pub fn draw_offset(&self, offset: Vec2) {
        for (i, id) in self.spr_ids.iter().enumerate() {
            let i = i as i32;
            let (x, y): (i32, i32) = (
                i32::from(self.offset.0) + i32::from(offset.x) + (i % 2) * 8,
                i32::from(self.offset.1) + i32::from(offset.y) + (i / 2) * 8,
            );
            draw_outline((*id).into(), x, y, SpriteOptions::transparent_zero(), 1);
        }
        palette_map_rotate(1);
        for (i, id) in self.spr_ids.iter().enumerate() {
            let i = i as i32;
            let (x, y): (i32, i32) = (
                i32::from(self.offset.0) + i32::from(offset.x) + (i % 2) * 8,
                i32::from(self.offset.1) + i32::from(offset.y) + (i / 2) * 8,
            );
            spr((*id).into(), x, y, SpriteOptions::transparent_zero());
        }
        palette_map_rotate(0);
    }
    pub const fn to(&'static self) -> PicContainer {
        PicContainer::Pic4x4(self)
    }
}

#[derive(Debug, Clone)]
pub struct PicSingle {
    spr_id: i16,
    offset: (i8, i8),
}
impl PicSingle {
    pub fn draw_offset(&self, offset: Vec2) {
        let (x, y): (i32, i32) = (
            i32::from(self.offset.0) + i32::from(offset.x),
            i32::from(self.offset.1) + i32::from(offset.y),
        );
        palette_map_rotate(1);
        spr_outline(
            self.spr_id.into(),
            x,
            y,
            SpriteOptions {
                w: 2,
                h: 2,
                ..SpriteOptions::transparent_zero()
            },
            1,
        );
        palette_map_rotate(0);
    }
    pub const fn to(&'static self) -> PicContainer {
        PicContainer::PicSingle(self)
    }
}

pub const Y_NORMAL: Pic4x4 = Pic4x4 {
    spr_ids: [920, 921, 952, 953],
    offset: (8, 13),
};
pub const Y_LOOK: Pic4x4 = Pic4x4 {
    spr_ids: [980, 981, 1012, 1013],
    offset: (8, 15),
};
pub const Y_CLOSE: Pic4x4 = Pic4x4 {
    spr_ids: [982, 983, 1012, 1013],
    offset: (8, 15),
};
pub const Y_OOF: Pic4x4 = Pic4x4 {
    spr_ids: [1014, 1015, 1012, 1013],
    offset: (8, 15),
};
pub const Y_NO: Pic4x4 = Pic4x4 {
    spr_ids: [984, 985, 1016, 1013],
    offset: (8, 15),
};
pub const Y_YELL: Pic4x4 = Pic4x4 {
    spr_ids: [986, 987, 1018, 1019],
    offset: (3, 11),
};
pub const Y_AWAY: PicSingle = PicSingle {
    spr_id: 988,
    offset: (8, 13),
};
pub const Y_SMUG: PicSingle = PicSingle {
    spr_id: 990,
    offset: (3, 7),
};
pub const Y_FRUS: PicSingle = PicSingle {
    spr_id: 926,
    offset: (3, 7),
};
pub const Y_HMM: PicSingle = PicSingle {
    spr_id: 924,
    offset: (3, 7),
};
pub const Y_REGRET: PicSingle = PicSingle {
    spr_id: 922,
    offset: (8, 13),
};
pub const HORROR: PicSingle = PicSingle {
    spr_id: 661,
    offset: (10, 10),
};
