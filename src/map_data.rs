use crate::{MapOptions, SpriteOptions};
use crate::position::{Hitbox, Vec2};
use crate::interact::{Interactable, Interaction};
use crate::animation::*;

pub(crate) const DEFAULT_MAP: MapOptions = MapOptions {
    x: 60,
    y: 17,
    w: 30,
    h: 17,
    transparent: &[],
    sx: 0,
    sy: 0,
    scale: 1,
};
pub(crate) const DEFAULT_MAP_SET: MapSet = MapSet {
    maps: &[],
    warps: &[],
    interactables: &[],
};

#[derive(Clone)]
pub struct MapSet<'a> {
    pub maps: &'a [MapOptions<'a>],
    pub warps: &'a [Warp<'a>],
    pub interactables: &'a [Interactable<'a>],
}

#[derive(Clone)]
pub struct Warp<'a> {
    pub from: Hitbox,
    pub map: Option<&'a MapSet<'a>>,
    pub to: Vec2,
}

impl<'a> Warp<'a> {
    pub const fn new(from: Hitbox, map: Option<&'a MapSet<'a>>, to: Vec2) -> Self { Self { from, map, to } }
    /// Defaults to 8x8 tile, start and end destinations are in 8x8 tile coordinates (i.e. tx1=2 becomes x=16)
    pub const fn new_tile(tx1: i16, ty1: i16, map: Option<&'a MapSet<'a>>, tx2: i16, ty2: i16) -> Self {
        Self::new(Hitbox::new(tx1*8, ty1*8, 8, 8), map, Vec2::new(tx2*8, ty2*8))
    }
}

pub static SUPERMARKET: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {//bg
            x: 60,
            y: 17,
            w: 26,
            h: 12,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {//fruit stand
            x: 61,
            y: 29,
            w: 3,
            h: 2,
            transparent: &[0],
            sx: 2*8,
            sy: 8*8,
            scale: 1,
        },
        MapOptions {//vending machines
            x: 70,
            y: 29,
            w: 4,
            h: 5,
            transparent: &[0],
            sx: 19*8,
            sy: 4*8,
            scale: 1,
        },
        MapOptions {//counter
            x: 60,
            y: 31,
            w: 8,
            h: 3,
            transparent: &[0],
            sx: 5*8,
            sy: 4*8,
            scale: 1,
        },
        MapOptions {//top vending machine
            x: 68,
            y: 29,
            w: 2,
            h: 3,
            transparent: &[0],
            sx: 13*8,
            sy: 5*4,
            scale: 1,
        },
    ],
    warps: &[Warp::new_tile(17,4, Some(&SUPERMARKET_HALL),9,4),
             Warp::new_tile(8,4, Some(&SUPERMARKET_HALL),3,4)],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(13*8, 5*4, 8*2, 8*3),
            interaction: Interaction::Text("There's no money in the coin return slot."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(2*8, 8*8, 8*3, 8*2),
            interaction: Interaction::Text("\"Reduced-Price Produce: Fresh out of season.\""),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(4*8, 5*8, 8, 20),
            interaction: Interaction::Text("It's yellow outside."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(19*8, 5*8, 8, 15),
            interaction: Interaction::Text("If you blow on the glass, it fogs up, revealing all the fingerprints left by prior employees."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(20*8, 6*8, 8, 15),
            interaction: Interaction::Text("A note on the front says \"Out of Order\", followed by a smiley face. A sickening contrast."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(21*8, 7*8, 8, 16),
            interaction: Interaction::Text("The blurb reads \"It's SodaTime!\". The machine is filled to the brim with cans of motor oil."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(11*8, 10*8, 3*8, 8),
            interaction: Interaction::Text("Looks like the creator didn't put too much effort into this part of the map."),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub static SUPERMARKET_HALL: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {//bg
            x: 86,
            y: 17,
            w: 13,
            h: 7,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {//closet
            x: 87,
            y: 24,
            w: 3,
            h: 4,
            transparent: &[0],
            sx: 5*8,
            sy: 0,
            scale: 1,
        },
        MapOptions {//diagonal door
            x: 86,
            y: 24,
            w: 1,
            h: 3,
            transparent: &[0],
            sx: 11*8,
            sy: 2*8,
            scale: 1,
        },
    ],
    warps: &[Warp::new_tile(9,6, Some(&SUPERMARKET),17,4),
             Warp::new_tile(3,6, Some(&SUPERMARKET),8,4),
             Warp::new_tile(4,2, Some(&SUPERMARKET_STOREROOM),2,3)],
    interactables: &[
        Interactable {
            hitbox: Hitbox::new(11*8, 4*8, 8, 8),
            interaction: Interaction::Text("This is an emergency exit. It's not an emergency right now. Ergo, you cannot use the emergency exit."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(8*8, 3*8, 8, 8),
            interaction: Interaction::Text("Looks like it's still under construction."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(11*4, 0, 2*8, 7*4),
            interaction: Interaction::Text("There's a single bottle of floor cleaner. Not a mop in sight, though."),
            sprite: None,
        },
        Interactable {
            hitbox: Hitbox::new(1*8, 3*8, 12, 16),
            interaction: Interaction::Text("Looks like this window has been recently painted over."),
            sprite: None,
        },
    ],
    ..DEFAULT_MAP_SET
};

pub static SUPERMARKET_STOREROOM: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            x:86, y:28,
            w:9, h:6,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {
            x:93, y:24,
            w:5, h:4,
            transparent: &[0],
            sx: 2*8,
            ..DEFAULT_MAP
        },
    ],
    warps: &[Warp::new_tile(2,5, Some(&SUPERMARKET_HALL),4,2)],
    interactables: &[Interactable {
        hitbox: Hitbox::new(53, 28, 8, 10),
        interaction: Interaction::Text("It's floating."),
        sprite: Some(Animation {
            frames: &[AnimFrame::new(Vec2::new(0,0), 524, 30, SpriteOptions::transparent_zero()),
                      AnimFrame::new(Vec2::new(0,-1), 524, 30, SpriteOptions::transparent_zero()),],
            ..Animation::const_default()
        }),
    },
    Interactable {
        hitbox: Hitbox::new(16, 0, 5*8, 4*7),
        interaction: Interaction::Text("They're all out of Keratin Krunch."),
        sprite: None,
    }],
    ..DEFAULT_MAP_SET
};
