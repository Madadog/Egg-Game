use std::collections::HashMap;

use crate::system::MapOptions;
use crate::{
    camera::CameraBounds,
    data::{
        map_data::{MapIndex, legacy_map},
        sound::{SfxData, music::MusicTrack},
        tmj::{TiledMap, TiledMapLayer},
    },
    interact::Interactable,
    position::{Collider, Hitbox, Vec2, touches_tile},
    system::drawing::image::IndexedImage,
};
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

/// Every loaded Tiled map, keyed by file stem (`"bank1"`, `"office"`, …).
/// This is the single owner of live tile data — legacy maps are windows into
/// the big `bank1`/`bank2` surfaces, "modern" maps (those with an object
/// layer) are self-contained — replacing the lossy tile copies the console
/// used to keep. Draw, collision and the editor all read (and the editor
/// writes) through here.
#[derive(Debug, Default)]
pub struct MapStore {
    maps: HashMap<String, TiledMap>,
}
impl MapStore {
    pub fn insert(&mut self, name: impl Into<String>, map: TiledMap) {
        self.maps.insert(name.into(), map);
    }
    pub fn get(&self, name: &str) -> Option<&TiledMap> {
        self.maps.get(name)
    }
    pub fn get_mut(&mut self, name: &str) -> Option<&mut TiledMap> {
        self.maps.get_mut(name)
    }
    /// All loaded map names, sorted for stable menu/UI listings.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.maps.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
    /// Whether `name` is a "modern" map — one that carries an object layer
    /// (interactables/warps live in the map file, not in code).
    pub fn is_modern(&self, name: &str) -> bool {
        self.get(name).is_some_and(|map| {
            map.layers
                .iter()
                .any(|layer| matches!(layer, TiledMapLayer::ObjectLayer(_)))
        })
    }
}

/// Resolve a map name to its load metadata. Tries the hardcoded legacy table
/// first; a name that parses as a number is re-resolved through the legacy
/// name table (the one place old numeric saves and numeric `to_map`
/// properties in existing `.tmj` files are translated); otherwise a "modern"
/// map's [`MapInfo`] is built straight from its own layers. `None` when the
/// name matches nothing. `indexed_sprites` is only read for the sprite art the
/// modern collision layer derives its colliders from (the sheet that lives on
/// [`crate::drawstate::DrawState`]).
pub fn map_by_name(
    indexed_sprites: &IndexedImage,
    name: &str,
    maps: &MapStore,
) -> Option<MapInfo> {
    if let Some(map) = legacy_map(name) {
        return Some(map);
    }
    if let Ok(index) = name.parse::<usize>() {
        return legacy_map(MapIndex(index).name());
    }
    if maps.is_modern(name) {
        return Some(modern_map_info(indexed_sprites, name, maps.get(name)?));
    }
    None
}

/// Build the runtime [`MapInfo`] for a modern (Tiled) map: tile layer 0 is
/// the collision layer (drawn invisible, its colliders derived per-tile from
/// the sprite art), the remaining tile layers split into bg/fg by the Tiled
/// layer-name `fg` prefix, and interactables/warps come from the object
/// layer. `source_layer` is each layer's index in `TiledMap::layers` — object
/// layers occupy indices too ([`TiledMap::get`] returns `None` for them), so
/// the numbering stays aligned with the file.
fn modern_map_info(indexed_sprites: &IndexedImage, name: &str, map: &TiledMap) -> MapInfo {
    let (width, height) = match map.layers.first() {
        Some(TiledMapLayer::TileLayer(layer)) => (
            layer.width.try_into().unwrap(),
            layer.height.try_into().unwrap(),
        ),
        _ => (0, 0),
    };
    let mut collision_layer = LayerInfo {
        origin: Vec2::new(0, 0),
        size: Vec2::new(width, height),
        offset: Vec2::new(0, 0),
        source_layer: 0,
        transparent: Some(0),
        visible: false,
        ..LayerInfo::DEFAULT_LAYER
    };
    let mut colliders = Vec::new();
    for j in 0..collision_layer.size.y {
        for i in 0..collision_layer.size.x {
            let tile = map.get(0, i as usize, j as usize).unwrap_or(0);
            colliders.push(Collider::from_sprite(indexed_sprites, tile));
        }
    }
    collision_layer.colliders = colliders;

    let mut layers = vec![collision_layer];
    let mut fg_layers = Vec::new();
    for (i, layer) in map.layers.iter().enumerate().skip(1) {
        let TiledMapLayer::TileLayer(layer) = layer else {
            continue;
        };
        let info = LayerInfo {
            origin: Vec2::new(0, 0),
            size: Vec2::new(
                layer.width.try_into().unwrap(),
                layer.height.try_into().unwrap(),
            ),
            offset: Vec2::new(0, 0),
            source_layer: i,
            transparent: Some(0),
            ..LayerInfo::DEFAULT_LAYER
        };
        if layer.name.to_lowercase().starts_with("fg") {
            fg_layers.push(info);
        } else {
            layers.push(info);
        }
    }
    let (interactables, warps) = map.parse_objects();
    MapInfo {
        layers,
        fg_layers,
        interactables,
        warps,
        source: name.to_string(),
        ..Default::default()
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
    /// Name of the [`MapStore`] map the layers window into: `"bank1"`/`"bank2"`
    /// for the legacy windowed maps, the map's own name for modern maps.
    /// Empty (the default) means no tile source — draw and collision guard on
    /// the lookup miss.
    pub source: String,
    pub camera_bounds: Option<CameraBounds>,
}
impl MapInfo {
    pub fn draw_bg_indexed(
        &self,
        draw_state: &mut crate::drawstate::DrawState,
        layer: crate::drawstate::LayerId,
        map: &TiledMap,
        offset: Vec2,
        debug: bool,
    ) {
        for l in &self.layers {
            l.draw_indexed(draw_state, layer, map, offset, debug);
        }
    }
    pub fn draw_fg_indexed(
        &self,
        draw_state: &mut crate::drawstate::DrawState,
        layer: crate::drawstate::LayerId,
        map: &TiledMap,
        offset: Vec2,
        debug: bool,
    ) {
        for l in &self.fg_layers {
            l.draw_indexed(draw_state, layer, map, offset, debug);
        }
    }
}

/// Layers defined by map metadata. References external data stored in the
/// [`MapStore`].
#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub origin: Vec2,
    pub size: Vec2,
    pub offset: Vec2,
    pub transparent: Option<u8>,
    /// (rotate_palette, shift_sprite_flags)
    pub rotate_and_shift_flags: (u8, u8),
    pub visible: bool,
    pub source_layer: usize,
    pub colliders: Vec<Collider>,
    // pub display_mode: BG, FG, Object
}
impl LayerInfo {
    pub const DEFAULT_LAYER: Self = Self {
        origin: Vec2::new(0, 0),
        size: Vec2::new(30, 17),
        offset: Vec2::new(0, 0),
        transparent: None,
        rotate_and_shift_flags: (0, 0),
        visible: true,
        source_layer: 0,
        colliders: Vec::new(),
    };
    pub const fn new(x: i16, y: i16, w: i16, h: i16) -> Self {
        let mut layer = Self::DEFAULT_LAYER;
        layer.origin = Vec2::new(x, y);
        layer.size = Vec2::new(w, h);
        layer
    }
    pub const fn with_offset(mut self, sx: i16, sy: i16) -> Self {
        self.offset = Vec2::new(sx, sy);
        self
    }
    pub const fn with_trans(mut self, transparent: &'static [u8]) -> Self {
        self.transparent = Some(transparent[0]);
        self
    }
    pub const fn with_rot_and_shift_flags(mut self, rot: u8, sprite_flag_shift: u8) -> Self {
        self.rotate_and_shift_flags = (rot, sprite_flag_shift);
        self
    }
    pub fn palette_rotate(&self) -> u8 {
        self.rotate_and_shift_flags.0
    }
    pub fn shift_sprite_flags(&self) -> bool {
        self.rotate_and_shift_flags.1 != 0
    }
    pub fn draw_indexed(
        &self,
        draw_state: &mut crate::drawstate::DrawState,
        layer: crate::drawstate::LayerId,
        map: &TiledMap,
        offset: Vec2,
        debug: bool,
    ) {
        use crate::drawstate::palette_map_rotate;
        use crate::system::drawing::Canvas;
        if !self.visible {
            return;
        }
        let Some(TiledMapLayer::TileLayer(map_layer)) = map.layers.get(self.source_layer) else {
            return;
        };
        let palette_map = palette_map_rotate(self.palette_rotate().into());
        let mut options: MapOptions = self.clone().into();
        options.sx -= i32::from(offset.x);
        options.sy -= i32::from(offset.y);
        if debug {
            let c9 = draw_state.colour(9);
            draw_state.rgba_canvas[layer as usize].stroke_rect(
                options.sx,
                options.sy,
                options.w * 8,
                options.h * 8,
                c9,
            );
        }
        draw_state.map_draw(layer, map_layer, &palette_map, options);
    }
    pub fn hitbox(&self) -> Hitbox {
        Hitbox::new(
            self.offset.x,
            self.offset.y,
            self.size.x * 8,
            self.size.y * 8,
        )
    }
}
impl From<LayerInfo> for MapOptions {
    fn from(map: LayerInfo) -> Self {
        MapOptions {
            x: map.origin.x.into(),
            y: map.origin.y.into(),
            w: map.size.x.into(),
            h: map.size.y.into(),
            sx: map.offset.x.into(),
            sy: map.offset.y.into(),
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
    pub from: (Vec2, Vec2),
    /// Destination map name (`None` = same map). Resolved via [`map_by_name`],
    /// so numeric strings from old `.tmj` files keep working.
    pub map: Option<String>,
    pub to: Vec2,
    pub flip: Axis,
    pub mode: WarpMode,
    pub sound: Option<SfxData>,
}

impl Warp {
    pub fn new(from: Hitbox, map: Option<&str>, to: Vec2) -> Self {
        let from = (Vec2::new(from.x, from.y), Vec2::new(from.w, from.h));
        let to = Vec2::new(to.x, to.y);
        Self {
            from,
            map: map.map(str::to_string),
            to,
            flip: Axis::None,
            mode: WarpMode::Interact,
            sound: None,
        }
    }
    /// Defaults to 8x8 tile, start and end destinations are in 8x8 tile coordinates (i.e. tx1=2 becomes x=16)
    pub fn new_tile(tx1: i16, ty1: i16, map: Option<&str>, tx2: i16, ty2: i16) -> Self {
        Self::new(
            Hitbox::new(tx1 * 8, ty1 * 8, 8, 8),
            map,
            Vec2::new(tx2 * 8, ty2 * 8),
        )
    }
    pub fn with_flip(self, flip: Axis) -> Self {
        Self { flip, ..self }
    }
    pub fn with_mode(self, mode: WarpMode) -> Self {
        Self { mode, ..self }
    }
    pub fn with_sound(self, sound: SfxData) -> Self {
        Self {
            sound: Some(sound),
            ..self
        }
    }
    pub fn hitbox(&self) -> Hitbox {
        Hitbox::new(self.from.0.x, self.from.0.y, self.from.1.x, self.from.1.y)
    }
    pub fn target(&self) -> Vec2 {
        Vec2::new(self.to.x, self.to.y)
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

/// Whether `point` collides with `layer` at this map position. `sprite_flags`
/// is the per-tile flag table ([`crate::drawstate::DrawState::sprite_flags`]),
/// consulted by tile id; the layer's own bitmap colliders are the second source.
pub fn layer_collides_flags(
    sprite_flags: &[u8],
    point: Vec2,
    layer: &LayerInfo,
    tiles: &TiledMap,
) -> bool {
    let layer_hitbox = layer.hitbox();
    if layer_hitbox.touches_point(point) {
        let map_point = Vec2::new(
            (point.x - layer_hitbox.x) / 8 + layer.origin.x,
            (point.y - layer_hitbox.y) / 8 + layer.origin.y,
        );
        let spr_flag_offset = if layer.shift_sprite_flags() { 256 } else { 0 };
        let id = tiles
            .get(0, map_point.x as usize, map_point.y as usize)
            .unwrap_or(0)
            + spr_flag_offset;
        let mget_collision = touches_tile(
            *sprite_flags.get(id).unwrap_or(&0),
            Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y),
        );
        let bitmap_collision = layer
            .colliders
            .get((map_point.x % layer.size.x) as usize + (map_point.y * layer.size.x) as usize)
            .map(|collider| collider.get(point.x as usize, point.y as usize))
            .unwrap_or_default();
        mget_collision || bitmap_collision
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::tmj::{ObjectLayer, TileLayer};
    use crate::system::test_console::TestConsole;

    /// A tiny self-contained modern map: one 4×4 tile layer (the collision
    /// layer) plus an empty object layer (which is what marks it as modern).
    fn synthetic_modern_map() -> TiledMap {
        TiledMap {
            width: 4,
            height: 4,
            layers: vec![
                TiledMapLayer::TileLayer(TileLayer {
                    width: 4,
                    height: 4,
                    data: vec![0; 16],
                    name: "Collision".to_string(),
                }),
                TiledMapLayer::ObjectLayer(ObjectLayer {
                    name: "Object Layer 1".to_string(),
                    objects: Vec::new(),
                }),
            ],
            tilesets: Vec::new(),
        }
    }

    /// Legacy names resolve through the hardcoded table, with their tile
    /// source pointing at the right bank surface.
    #[test]
    fn map_by_name_resolves_legacy_name() {
        let console = TestConsole::new();
        let store = MapStore::default();
        let town =
            map_by_name(&console.indexed_sprites, "town", &store).expect("town is a legacy map");
        assert_eq!(town.source, "bank2");
        assert_eq!(town.fg_layers.len(), 1);
        assert!(map_by_name(&console.indexed_sprites, "no_such_map", &store).is_none());
    }

    /// Numeric strings (old saves / numeric `to_map` properties) fall back to
    /// the legacy index → name mapping: "4" is the bedroom.
    #[test]
    fn map_by_name_resolves_numeric_fallback() {
        let console = TestConsole::new();
        let store = MapStore::default();
        let bedroom =
            map_by_name(&console.indexed_sprites, "4", &store).expect("4 is a legacy index");
        assert_eq!(bedroom.source, "bank1");
        // The bedroom's room layer windows into bank1 at (30, 0).
        assert_eq!(bedroom.layers[0].origin, Vec2::new(30, 0));
        assert_eq!(
            bedroom.warps[0].map.as_deref(),
            Some("house_stairwell"),
            "resolved the same map the bedroom() builder describes"
        );
    }

    /// Modern names build their MapInfo from the map's own layers: layer 0
    /// becomes the invisible collision layer with one collider per tile.
    #[test]
    fn map_by_name_builds_modern_map() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        store.insert("lab", synthetic_modern_map());
        assert!(store.is_modern("lab"));
        let lab = map_by_name(&console.indexed_sprites, "lab", &store).expect("lab is in the store");
        assert_eq!(lab.source, "lab");
        assert_eq!(lab.layers.len(), 1, "collision layer only");
        assert!(!lab.layers[0].visible);
        assert_eq!(lab.layers[0].colliders.len(), 16);
        assert!(lab.fg_layers.is_empty());
    }
}
