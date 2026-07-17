//! Drawing for dialogue [`Portrait`]s. The portrait itself is pure data
//! ([`egg_world::data::portraits`]); this is the `ui`-side renderer that paints
//! it through [`DrawState`], keeping `data/` free of any dependency on drawing.

use egg_render::geometry::Vec2;
use egg_render::{Flip, SpriteOptions};
use egg_world::data::portraits::Portrait;
use egg_world::draw_state::{DrawState, LayerId, palette_map_rotate};

/// Draw `portrait` onto `layer` at `offset` (its authored offset is added on
/// top): the outline pass over every cell first — so the outline hugs the
/// assembled silhouette rather than boxing each cell — then the palette-rotated
/// fill.
pub fn draw_offset(
    portrait: &Portrait,
    draw_state: &mut DrawState,
    layer: LayerId,
    offset: Vec2,
    outline: Option<u8>,
    flip: Flip,
) {
    let pmap = palette_map_rotate(0);
    let (x_offset, y_offset) = (
        i16::from(if flip.x() {
            8 - portrait.offset.0
        } else {
            portrait.offset.0
        }),
        i16::from(if flip.y() {
            8 - portrait.offset.1
        } else {
            portrait.offset.1
        }),
    );
    let origin = Vec2::new(offset.x + x_offset, offset.y + y_offset);
    if let Some(outline) = outline {
        for (pos, cell) in portrait.sprite.iter_at_flipped(origin, flip) {
            let options = SpriteOptions {
                flip: cell.flip,
                rotate: cell.rotate,
                ..SpriteOptions::transparent_zero()
            };
            draw_state.spr_outline(
                layer,
                cell.spr_id,
                pos.x.into(),
                pos.y.into(),
                options,
                outline,
            );
        }
    }
    for (pos, cell) in portrait.sprite.iter_at_flipped(origin, flip) {
        let options = SpriteOptions {
            flip: cell.flip,
            rotate: cell.rotate,
            ..SpriteOptions::transparent_zero()
        };
        draw_state.spr(
            layer,
            &pmap,
            cell.spr_id,
            pos.x.into(),
            pos.y.into(),
            options,
        );
    }
}
