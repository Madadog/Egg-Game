pub(crate) use crate::MapOptions;
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

pub struct MapSet<'a> {
    pub maps: &'a [MapOptions<'a>],
}

pub const SUPERMARKET: MapSet<'static> = MapSet {
    maps: &[
        MapOptions {
            x: 60,
            y: 17,
            w: 25,
            h: 12,
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
    ]
};
