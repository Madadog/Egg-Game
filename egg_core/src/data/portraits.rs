use crate::position::Vec2;
use crate::system::ConsoleApi;
use tic80_api::core::StaticSpriteOptions;

#[derive(Debug, Clone)]
pub enum PicContainer {
    Pic4x4(&'static Portrait),
}
impl PicContainer {
    pub fn draw_offset(&self, system: &mut impl ConsoleApi, offset: Vec2) {
        match self {
            Self::Pic4x4(x) => x.draw_offset(system, offset),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Portrait {
    spr_ids: [i16; 4],
    offset: (i8, i8),
}
impl Portrait {
    pub const fn new(spr_ids: [i16; 4], offset: (i8, i8)) -> Self {
        Self { spr_ids, offset }
    }
    pub const fn new_single(spr_id: i16, offset: (i8, i8)) -> Self {
        // Y axis stride is 32 for now...
        let spr_ids = [spr_id, spr_id + 1, spr_id + 32, spr_id + 33];
        Self { spr_ids, offset }
    }
    pub fn draw_offset(&self, system: &mut impl ConsoleApi, offset: Vec2) {
        for (i, id) in self.spr_ids.iter().enumerate() {
            let i = i as i32;
            let (x, y): (i32, i32) = (
                i32::from(self.offset.0) + i32::from(offset.x) + (i % 2) * 8,
                i32::from(self.offset.1) + i32::from(offset.y) + (i / 2) * 8,
            );
            system.draw_outline(
                (*id).into(),
                x,
                y,
                StaticSpriteOptions::transparent_zero(),
                1,
            );
        }
        system.palette_map_rotate(1);
        for (i, id) in self.spr_ids.iter().enumerate() {
            let i = i as i32;
            let (x, y): (i32, i32) = (
                i32::from(self.offset.0) + i32::from(offset.x) + (i % 2) * 8,
                i32::from(self.offset.1) + i32::from(offset.y) + (i / 2) * 8,
            );
            system.spr((*id).into(), x, y, StaticSpriteOptions::transparent_zero());
        }
        system.palette_map_rotate(0);
    }
    pub const fn to(&'static self) -> PicContainer {
        PicContainer::Pic4x4(self)
    }
}

pub const Y_NORMAL: Portrait = Portrait {
    spr_ids: [920, 921, 952, 953],
    offset: (8, 13),
};
pub const Y_LOOK: Portrait = Portrait {
    spr_ids: [980, 981, 1012, 1013],
    offset: (8, 15),
};
pub const Y_CLOSE: Portrait = Portrait {
    spr_ids: [982, 983, 1012, 1013],
    offset: (8, 15),
};
pub const Y_OOF: Portrait = Portrait {
    spr_ids: [1014, 1015, 1012, 1013],
    offset: (8, 15),
};
pub const Y_NO: Portrait = Portrait {
    spr_ids: [984, 985, 1016, 1013],
    offset: (8, 15),
};
pub const Y_YELL: Portrait = Portrait {
    spr_ids: [986, 987, 1018, 1019],
    offset: (3, 11),
};
pub const Y_AWAY: Portrait = Portrait::new_single(988, (8, 13));
pub const Y_SMUG: Portrait = Portrait::new_single(990, (3, 7));
pub const Y_FRUS: Portrait = Portrait::new_single(926, (3, 7));
pub const Y_HMM: Portrait = Portrait::new_single(924, (3, 7));
pub const Y_REGRET: Portrait = Portrait::new_single(922, (8, 13));
pub const HORROR: Portrait = Portrait::new_single(661, (10, 10));
