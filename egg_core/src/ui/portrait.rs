//! Drawing for dialogue [`Portrait`]s. The portrait itself is pure data
//! ([`crate::data::portraits`]); this is the `ui`-side renderer that paints it
//! through [`DrawState`], keeping `data/` free of any dependency on drawing.

use crate::data::portraits::Portrait;
use crate::draw_state::{DrawState, LayerId, palette_map_rotate};
use crate::geometry::Vec2;
use crate::render::SpriteOptions;

/// Draw `portrait` onto `layer` at `offset` (its authored offset is added on
/// top): the outline pass first, then the palette-rotated fill.
pub fn draw_offset(portrait: &Portrait, draw_state: &mut DrawState, layer: LayerId, offset: Vec2) {
    let pmap = palette_map_rotate(1);
    let xy = |i: i32| -> (i32, i32) {
        (
            i32::from(portrait.offset.0) + i32::from(offset.x) + (i % 2) * 8,
            i32::from(portrait.offset.1) + i32::from(offset.y) + (i / 2) * 8,
        )
    };
    for (id, i) in portrait.spr_ids.iter().zip(0..) {
        let (x, y) = xy(i);
        draw_state.spr_outline(layer, *id, x, y, SpriteOptions::transparent_zero(), 1);
    }
    for (id, i) in portrait.spr_ids.iter().zip(0..) {
        let (x, y) = xy(i);
        draw_state.spr(layer, &pmap, *id, x, y, SpriteOptions::transparent_zero());
    }
}
