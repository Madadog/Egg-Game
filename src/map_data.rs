use crate::MapOptions;
use crate::position::{Hitbox, Vec2};

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

pub(crate) const FRUIT_STAND: MapOptions = MapOptions {
    x: 61,
    y: 29,
    w: 3,
    h: 2,
    transparent: &[0],
    sx: 2*8,
    sy: 8*8,
    scale: 1,
};

#[derive(Clone)]
pub struct MapSet<'a> {
    pub maps: &'a [MapOptions<'a>],
    pub warps: &'a [Warp<'a>],
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
        MapOptions {
            x: 60,
            y: 17,
            w: 26,
            h: 12,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {
            x: 61,
            y: 29,
            w: 3,
            h: 2,
            transparent: &[0],
            sx: 2*8,
            sy: 8*8,
            scale: 1,
        },
        MapOptions {
            x: 68,
            y: 29,
            w: 4,
            h: 5,
            transparent: &[0],
            sx: 19*8,
            sy: 4*8,
            scale: 1,
        },
    ],
    warps: &[Warp::new_tile(17,4, Some(&SUPERMARKET_HALL),8,4)],
};

pub static SUPERMARKET_HALL: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            x: 86,
            y: 17,
            w: 13,
            h: 6,
            transparent: &[0],
            ..DEFAULT_MAP
        },
        MapOptions {
            x: 87,
            y: 23,
            w: 3,
            h: 4,
            transparent: &[0],
            sx: 5*8,
            sy: 0,
            scale: 1,
        },
        MapOptions {
            x: 86,
            y: 23,
            w: 1,
            h: 3,
            transparent: &[0],
            sx: 11*8,
            sy: 2*8,
            scale: 1,
        },
    ],
    warps: &[Warp::new_tile(8,6, Some(&SUPERMARKET),17,4)],
};
