use crate::{position::{Vec2, Hitbox, touches_tile}, tic80::{mget, MapOptions}, interact::Interactable};


#[derive(Clone)]
pub struct MapSet<'a> {
    pub maps: &'a [MapOptions<'a>],
    pub fg_maps: &'a [MapOptions<'a>],
    pub warps: &'a [Warp],
    pub interactables: &'a [Interactable<'a>],
    pub bg_colour: u8,
    pub palette_rotation: &'a [u8],
    pub music_track: Option<u8>,
    pub bank: u8,
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
        Self { from, map, to, flip: Axis::None }
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
        Self {flip, ..self}
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
        match self {
            Self::Both | Self::X => true,
            _ => false,
        }
    }
    pub fn y(&self) -> bool {
        match self {
            Self::Both | Self::Y => true,
            _ => false,
        }
    }
}

pub fn layer_collides(point: Vec2, layer_hitbox: Hitbox, layer_x: i32, layer_y: i32) -> bool {
    if layer_hitbox.touches_point(point) {
        let map_point = Vec2::new(
            (point.x - layer_hitbox.x) / 8 + layer_x as i16,
            (point.y - layer_hitbox.y) / 8 + layer_y as i16,
        );
        let id = mget(map_point.x.into(), map_point.y.into());
        touches_tile(
            id as usize,
            Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y),
        )
    } else {
        false
    }
}