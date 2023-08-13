use crate::{
    camera::CameraBounds,
    interact::Interactable,
    data::{map_data::MapIndex, sound::music::MusicTrack},
    packed::{PackedI16, PackedU8},
    position::{touches_tile, Hitbox, Vec2}, system::{ConsoleApi, ConsoleHelper},
};
use tic80_api::core::MapOptions;

#[derive(Clone, Debug)]
pub struct MapSet<'a> {
    pub maps: &'a [MapLayer<'a>],
    pub fg_maps: &'a [MapLayer<'a>],
    pub warps: &'a [Warp],
    pub interactables: &'a [Interactable<'a>],
    pub bg_colour: u8,
    pub music_track: Option<MusicTrack>,
    pub bank: u8,
    pub camera_bounds: Option<CameraBounds>,
}
impl<'a> MapSet<'a> {
    pub fn draw_bg(&self, system: &mut impl ConsoleApi, offset: Vec2, debug: bool) {
        self.maps.iter().for_each(|layer| layer.draw_tic80(system, offset, debug))
    }
    pub fn draw_fg(&self, system: &mut impl ConsoleApi, offset: Vec2, debug: bool) {
        self.fg_maps.iter().for_each(|layer| layer.draw_tic80(system, offset, debug))
    }
}

#[derive(Clone, Debug)]
pub struct MapLayer<'a> {
    pub origin: PackedI16,
    pub size: PackedI16,
    pub offset: PackedI16,
    pub transparent: &'a [u8],
    /// (blit_segment, rotate_palette, shift_sprite_flags, UNUSED)
    pub blit_rotate_and_flags: PackedU8,
}
impl<'a> MapLayer<'a> {
    pub const DEFAULT_MAP: Self = Self {
        origin: PackedI16::from_i16(0, 0),
        size: PackedI16::from_i16(30, 17),
        offset: PackedI16::from_i16(0, 0),
        transparent: &[],
        blit_rotate_and_flags: PackedU8::from_u8((4, 0, 0, 0)),
    };
    pub const fn new(x: i16, y: i16, w: i16, h: i16) -> Self {
        Self {
            origin: PackedI16::from_i16(x, y),
            size: PackedI16::from_i16(w, h),
            ..Self::DEFAULT_MAP
        }
    }
    pub const fn with_offset(self, sx: i16, sy: i16) -> Self {
        Self {
            offset: PackedI16::from_i16(sx, sy),
            ..self
        }
    }
    pub const fn with_trans(self, transparent: &'static [u8]) -> Self {
        Self {
            transparent,
            ..self
        }
    }
    pub const fn with_blit_rot_flags(self, blit: u8, rot: u8, sprite_flag_shift: u8) -> Self {
        Self {
            blit_rotate_and_flags: PackedU8::from_u8((blit, rot, sprite_flag_shift, 0)),
            ..self
        }
    }
    pub fn size(&self) -> Vec2 {
        let size = self.size.to_i16();
        Vec2::new(size.0, size.1)
    }
    pub fn offset(&self) -> Vec2 {
        let offset = self.offset.to_i16();
        Vec2::new(offset.0, offset.1)
    }
    pub fn blit_segment(&self) -> u8 {
        self.blit_rotate_and_flags.to_u8().0
    }
    pub fn palette_rotate(&self) -> u8 {
        self.blit_rotate_and_flags.to_u8().1
    }
    pub fn shift_sprite_flags(&self) -> bool {
        self.blit_rotate_and_flags.to_u8().2 != 0
    }
    pub fn draw_tic80(&self, system: &mut impl ConsoleApi, offset: Vec2, debug: bool) {
        system.palette_map_rotate(self.palette_rotate());
        system.blit_segment(self.blit_segment());
        let mut options: MapOptions = self.clone().into();
        options.sx -= i32::from(offset.x);
        options.sy -= i32::from(offset.y);
        if debug {
            system.rectb(options.sx, options.sy, options.w * 8, options.h * 8, 9);
        }
        system.map(options);
    }
}
impl<'a> From<MapLayer<'a>> for MapOptions<'a> {
    fn from(map: MapLayer<'a>) -> Self {
        MapOptions {
            x: map.origin.x().into(),
            y: map.origin.y().into(),
            w: map.size.x().into(),
            h: map.size.y().into(),
            sx: map.offset.x().into(),
            sy: map.offset.y().into(),
            transparent: map.transparent,
            scale: 1,
        }
    }
}

/// Defines how a warp is interacted with.
#[derive(Clone, Debug)]
pub enum WarpMode {
    /// Automatically used when touched.
    Auto,
    /// Requires the player to manually interact with the door
    /// if the "Automatic doors" setting is disabled.
    Interact,
}

#[derive(Clone, Debug)]
pub struct Warp {
    pub from: (PackedI16, PackedI16),
    pub map: Option<MapIndex>,
    pub to: PackedI16,
    pub flip: Axis,
    pub mode: WarpMode,
}

impl Warp {
    pub const fn new(from: Hitbox, map: Option<MapIndex>, to: Vec2) -> Self {
        let from = (
            PackedI16::from_i16(from.x, from.y),
            PackedI16::from_i16(from.w, from.h),
        );
        let to = PackedI16::from_i16(to.x, to.y);
        Self {
            from,
            map,
            to,
            flip: Axis::None,
            mode: WarpMode::Interact,
        }
    }
    /// Defaults to 8x8 tile, start and end destinations are in 8x8 tile coordinates (i.e. tx1=2 becomes x=16)
    pub const fn new_tile(tx1: i16, ty1: i16, map: Option<MapIndex>, tx2: i16, ty2: i16) -> Self {
        Self::new(
            Hitbox::new(tx1 * 8, ty1 * 8, 8, 8),
            map,
            Vec2::new(tx2 * 8, ty2 * 8),
        )
    }
    pub const fn with_flip(self, flip: Axis) -> Self {
        Self { flip, ..self }
    }
    pub const fn with_mode(self, mode: WarpMode) -> Self {
        Self { mode, ..self }
    }
    pub fn map(&'static self) -> Option<MapIndex> {
        self.map
    }
    pub fn hitbox(&self) -> Hitbox {
        Hitbox::new(
            self.from.0.x(),
            self.from.0.y(),
            self.from.1.x(),
            self.from.1.y(),
        )
    }
    pub fn target(&self) -> Vec2 {
        Vec2::new(self.to.x(), self.to.y())
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
    system: &mut impl ConsoleApi,
    point: Vec2,
    layer_hitbox: Hitbox,
    layer_x: i32,
    layer_y: i32,
    spr_flag_offset: bool,
) -> bool {
    if layer_hitbox.touches_point(point) {
        let map_point = Vec2::new(
            (point.x - layer_hitbox.x) / 8 + layer_x as i16,
            (point.y - layer_hitbox.y) / 8 + layer_y as i16,
        );
        let spr_flag_offset = if spr_flag_offset { 256 } else { 0 };
        let id = system.mget(map_point.x.into(), map_point.y.into()) + spr_flag_offset;
        touches_tile(
            *system.get_sprite_flags().get(id as usize).unwrap_or(&0),
            Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y),
        )
    } else {
        false
    }
}
