use crate::{
    camera::CameraBounds,
    data::{
        map_data::MapIndex,
        sound::{self, music::MusicTrack, SfxData},
    },
    interact::{Interactable, StaticInteractable},
    packed::{PackedI16, PackedU8},
    position::{touches_tile, Hitbox, Vec2},
    system::{ConsoleApi, ConsoleHelper},
};
use tic80_api::core::MapOptions;
/*
pub enum TileMapCollision {
    None,
    Collision,
}

pub enum TileMapInteraction {
    None,
    Interaction(Interactable),
    Warp(usize),
}

pub trait TileMap {
    fn get(&self, x: i32, y: i32) -> (TileMapCollision, TileMapInteraction);
    fn draw(&self, console: &mut impl ConsoleApi);
    fn step(&mut self, console: &impl ConsoleApi);
}*/

#[derive(Clone, Debug)]
pub struct StaticMapInfo<'a> {
    pub layers: &'a [LayerInfo],
    pub fg_layers: &'a [LayerInfo],
    pub warps: &'a [Warp],
    pub interactables: &'a [StaticInteractable<'a>],
    pub bg_colour: u8,
    pub music_track: Option<MusicTrack>,
    pub bank: usize,
    pub camera_bounds: Option<CameraBounds>,
}
impl<'a> StaticMapInfo<'a> {
    pub fn draw_bg(&self, system: &mut impl ConsoleApi, bank: usize, offset: Vec2, debug: bool) {
        self.layers
            .iter()
            .for_each(|layer| layer.draw_tic80(system, bank, offset, debug))
    }
    pub fn draw_fg(&self, system: &mut impl ConsoleApi, bank: usize, offset: Vec2, debug: bool) {
        self.fg_layers
            .iter()
            .for_each(|layer| layer.draw_tic80(system, bank, offset, debug))
    }
}

/// Metadata necessary to load a map into Walkaround.
#[derive(Clone, Debug, Default)]
pub struct MapInfo {
    pub layers: Vec<LayerInfo>,
    pub fg_layers: Vec<LayerInfo>,
    pub warps: Vec<Warp>,
    pub interactables: Vec<Interactable>,
    pub bg_colour: u8,
    pub music_track: Option<MusicTrack>,
    pub bank: usize,
    pub camera_bounds: Option<CameraBounds>,
}
impl MapInfo {
    pub fn draw_bg(&self, system: &mut impl ConsoleApi, bank: usize, offset: Vec2, debug: bool) {
        self.layers
            .iter()
            .for_each(|layer| layer.draw_tic80(system, bank, offset, debug))
    }
    pub fn draw_fg(&self, system: &mut impl ConsoleApi, bank: usize, offset: Vec2, debug: bool) {
        self.fg_layers
            .iter()
            .for_each(|layer| layer.draw_tic80(system, bank, offset, debug))
    }
}
impl From<StaticMapInfo<'static>> for MapInfo {
    fn from(value: StaticMapInfo) -> Self {
        MapInfo {
            layers: value.layers.into(),
            fg_layers: value.fg_layers.into(),
            warps: value.warps.into(),
            interactables: value
                .interactables
                .iter()
                .map(|x| x.clone().into())
                .collect(),
            bg_colour: value.bg_colour,
            music_track: value.music_track,
            bank: value.bank,
            camera_bounds: value.camera_bounds,
            ..Default::default()
        }
    }
}

/// Layers defined by map metadata. Separate from `TiledMap` layers.
#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub origin: PackedI16,
    pub size: PackedI16,
    pub offset: PackedI16,
    pub transparent: Option<u8>,
    /// (blit_segment, rotate_palette, shift_sprite_flags, UNUSED)
    pub blit_rotate_and_flags: PackedU8,
    // pub source_bank: usize,
    // pub source_layer: usize,
    // pub visible: bool,
    // pub display_mode: BG, FG, Object
}
impl LayerInfo {
    pub const DEFAULT_MAP: Self = Self {
        origin: PackedI16::from_i16(0, 0),
        size: PackedI16::from_i16(30, 17),
        offset: PackedI16::from_i16(0, 0),
        transparent: None,
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
            transparent: Some(transparent[0]),
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
    pub fn draw_tic80(&self, system: &mut impl ConsoleApi, bank: usize, offset: Vec2, debug: bool) {
        system.palette_map_rotate(self.palette_rotate().into());
        system.blit_segment(self.blit_segment());
        let mut options: MapOptions = self.clone().into();
        options.sx -= i32::from(offset.x);
        options.sy -= i32::from(offset.y);
        if debug {
            system.rectb(options.sx, options.sy, options.w * 8, options.h * 8, 9);
        }
        system.map_draw(bank, 0, options);
    }
}
impl<'a> From<LayerInfo> for MapOptions {
    fn from(map: LayerInfo) -> Self {
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
    pub sound: Option<SfxData>,
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
            sound: None,
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
    pub const fn with_sound(self, sound: SfxData) -> Self {
        Self {
            sound: Some(sound),
            ..self
        }
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

pub fn layer_collides_flags(
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


pub fn layer_collides(
    system: &mut impl ConsoleApi,
    point: Vec2,
    layer_hitbox: Hitbox,
    layer_x: i32,
    layer_y: i32,
    bank: usize,
    layer: usize,
) -> bool {
    if layer_hitbox.touches_point(point) {
        let map_point = Vec2::new(
            (point.x - layer_hitbox.x) / 8 + layer_x as i16,
            (point.y - layer_hitbox.y) / 8 + layer_y as i16,
        );
        let id = system.map_get(bank, layer, map_point.x.into(), map_point.y.into());
        touches_tile(
            id.try_into().unwrap(),
            Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y),
        )
    } else {
        false
    }
}