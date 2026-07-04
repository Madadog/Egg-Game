//! Drawing for dialogue [`Portrait`]s. The portrait itself is pure data
//! ([`crate::data::portraits`]); this is the `ui`-side renderer that paints it
//! through [`DrawState`], keeping `data/` free of any dependency on drawing.

use crate::data::portraits::Portrait;
use crate::draw_state::{DrawState, LayerId, palette_map_rotate};
use crate::geometry::Vec2;
use crate::render::SpriteOptions;

/// Draw `portrait` onto `layer` at `offset` (its authored offset is added on
/// top): the outline pass over every cell first — so the outline hugs the
/// assembled silhouette rather than boxing each cell — then the palette-rotated
/// fill.
pub fn draw_offset(portrait: &Portrait, draw_state: &mut DrawState, layer: LayerId, offset: Vec2) {
    let pmap = palette_map_rotate(1);
    let origin = Vec2::new(
        offset.x + i16::from(portrait.offset.0),
        offset.y + i16::from(portrait.offset.1),
    );
    for (pos, id) in portrait.sprite.iter_at(origin) {
        draw_state.spr_outline(
            layer,
            id,
            pos.x.into(),
            pos.y.into(),
            SpriteOptions::transparent_zero(),
            1,
        );
    }
    for (pos, id) in portrait.sprite.iter_at(origin) {
        draw_state.spr(
            layer,
            &pmap,
            id,
            pos.x.into(),
            pos.y.into(),
            SpriteOptions::transparent_zero(),
        );
    }
}
