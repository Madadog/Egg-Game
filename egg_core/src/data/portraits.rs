use crate::draw_state::{DrawState, LayerId, palette_map_rotate};
use crate::geometry::Vec2;
use crate::render::SpriteOptions;

#[derive(Debug, Clone)]
pub struct Portrait {
    spr_ids: [i32; 4],
    offset: (i8, i8),
}
impl Portrait {
    pub const fn new(spr_ids: [i32; 4], offset: (i8, i8)) -> Self {
        Self { spr_ids, offset }
    }
    pub const fn new_single(spr_id: i32, offset: (i8, i8)) -> Self {
        // Y axis stride is 32 for now...
        let spr_ids = [spr_id, spr_id + 1, spr_id + 32, spr_id + 33];
        Self { spr_ids, offset }
    }
    pub fn draw_offset(&self, draw_state: &mut DrawState, layer: LayerId, offset: Vec2) {
        let pmap = palette_map_rotate(1);
        let xy = |i: i32| -> (i32, i32) {
            (
                i32::from(self.offset.0) + i32::from(offset.x) + (i % 2) * 8,
                i32::from(self.offset.1) + i32::from(offset.y) + (i / 2) * 8,
            )
        };
        for (id, i) in self.spr_ids.iter().zip(0..) {
            let (x, y) = xy(i);
            draw_state.spr_outline(layer, *id, x, y, SpriteOptions::transparent_zero(), 1);
        }
        for (id, i) in self.spr_ids.iter().zip(0..) {
            let (x, y) = xy(i);
            draw_state.spr(layer, &pmap, *id, x, y, SpriteOptions::transparent_zero());
        }
    }
}

/// Resolve a portrait by its script name (lowercased identifier, e.g. `"horror"`).
pub fn by_name(name: &str) -> Option<Portrait> {
    Some(match name {
        "y_normal" => Y_NORMAL,
        "y_look" => Y_LOOK,
        "y_close" => Y_CLOSE,
        "y_oof" => Y_OOF,
        "y_no" => Y_NO,
        "y_yell" => Y_YELL,
        "y_away" => Y_AWAY,
        "y_smug" => Y_SMUG,
        "y_frus" => Y_FRUS,
        "y_hmm" => Y_HMM,
        "y_regret" => Y_REGRET,
        "horror" => HORROR,
        _ => return None,
    })
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
