use crate::{
    interact::Interactable,
    position::{touches_tile, Hitbox, Vec2},
    tic80::{mget, MapOptions}, camera::CameraBounds,
};

#[derive(Clone)]
pub struct MapSet<'a> {
    pub maps: &'a [MapLayer<'a>],
    pub fg_maps: &'a [MapLayer<'a>],
    pub warps: &'a [Warp],
    pub interactables: &'a [Interactable<'a>],
    pub bg_colour: u8,
    pub palette_rotation: &'a [u8],
    pub music_track: Option<u8>,
    pub bank: u8,
    pub camera_bounds: Option<CameraBounds>,
}

#[derive(Clone)]
pub struct MapLayer<'a> {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub sx: i32,
    pub sy: i32,
    pub transparent: &'a [u8],
    pub scale: i8,
    pub blit_segment: u8,
    pub flag_offset: i32,
}
impl<'a> MapLayer<'a> {
    pub const DEFAULT_MAP: Self = Self {
        x: 0,
        y: 0,
        w: 30,
        h: 17,
        sx: 0,
        sy: 0,
        transparent: &[],
        scale: 1,
        blit_segment: 4,
        flag_offset: 0,
    };
}
impl<'a> From<MapLayer<'a>> for MapOptions<'a> {
    fn from(map: MapLayer<'a>) -> Self {
        MapOptions {
            x: map.x,
            y: map.y,
            w: map.w,
            h: map.h,
            sx: map.sx,
            sy: map.sy,
            transparent: map.transparent,
            scale: map.scale,
        }
    }
}

#[derive(Clone)]
pub struct Warp {
    pub from: Hitbox,
    pub map: Option<&'static MapSet<'static>>,
    pub to: Vec2,
    pub flip: Axis,
}

impl Warp {
    pub const fn new(from: Hitbox, map: Option<&'static MapSet<'static>>, to: Vec2) -> Self {
        Self {
            from,
            map,
            to,
            flip: Axis::None,
        }
    }
    /// Defaults to 8x8 tile, start and end destinations are in 8x8 tile coordinates (i.e. tx1=2 becomes x=16)
    pub const fn new_tile(
        tx1: i16,
        ty1: i16,
        map: Option<&'static MapSet<'static>>,
        tx2: i16,
        ty2: i16,
    ) -> Self {
        Self::new(
            Hitbox::new(tx1 * 8, ty1 * 8, 8, 8),
            map,
            Vec2::new(tx2 * 8, ty2 * 8),
        )
    }
    pub const fn with_flip(self, flip: Axis) -> Self {
        Self { flip, ..self }
    }
    pub fn map(&'static self) -> Option<&'static MapSet<'static>> {
        self.map
    }
}

#[derive(Debug, Clone)]
pub enum Axis {
    None,
    X,
    Y,
    Both,
}
impl Axis {
    pub fn x(&self) -> bool {
        matches!(self, Self::Both | Self::X)
    }
    pub fn y(&self) -> bool {
        matches!(self, Self::Both | Self::Y)
    }
}

pub fn layer_collides(
    point: Vec2,
    layer_hitbox: Hitbox,
    layer_x: i32,
    layer_y: i32,
    spr_flag_offset: i32,
) -> bool {
    if layer_hitbox.touches_point(point) {
        let map_point = Vec2::new(
            (point.x - layer_hitbox.x) / 8 + layer_x as i16,
            (point.y - layer_hitbox.y) / 8 + layer_y as i16,
        );
        let id = mget(map_point.x.into(), map_point.y.into()) + spr_flag_offset;
        touches_tile(
            id as usize,
            Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y),
        )
    } else {
        false
    }
}
