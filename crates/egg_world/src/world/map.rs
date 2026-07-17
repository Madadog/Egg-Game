use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::data::metasprite::{MetaCell, MetaSprite};
use crate::data::save::SaveData;
use crate::data::sound::{SfxData, music::MusicTrack};
use crate::data::tiled::{ImageLayer, TiledMap, TiledMapLayer};
use crate::draw_state::BgColour;
use egg_render::geometry::{Collider, Hitbox, Vec2};
use crate::draw_state::DrawParams;
use egg_render::{MapOptions, SpriteOptions};
use egg_render::image::{IndexedImage, RgbaImage};
use crate::world::animation::AnimFrame;
use crate::world::camera::CameraBounds;
use crate::world::interact::{InteractFn, Interaction};

/// Alpha at or above which a painted collision-mask pixel counts as **solid**.
///
/// A collision image layer ([`ImageLayer::is_collision`]) is sliced into 8×8
/// bitmap [`Collider`]s by thresholding each pixel's alpha against this value:
/// `alpha >= PAINTED_SOLID_ALPHA` ⇒ that pixel blocks movement. The threshold
/// (rather than `alpha > 0`) gives the artist a tolerance band for the soft,
/// antialiased edges a brush leaves — a feathered edge fading from opaque to
/// transparent becomes solid up to its halfway point and passable beyond, so the
/// collision boundary tracks the visual centre of the stroke rather than its
/// faint fringe. 128 is the midpoint of the 0–255 range.
pub const PAINTED_SOLID_ALPHA: u8 = 128;
/*
pub enum TileMapCollision {
    None,
    Collision,
}

pub enum TileMapInteraction {
    None,
    Interaction(MapObject),
    Warp(usize),
}

pub trait TileMap {
    fn get(&self, x: i32, y: i32) -> (TileMapCollision, TileMapInteraction);
    fn draw(&self, console: &mut impl ConsoleApi);
    fn step(&mut self, console: &impl ConsoleApi);
}*/

/// Every loaded Tiled map, keyed by file stem (`"office"`, `"town"`, …). This
/// is the single owner of live tile data: every map is self-contained — its
/// content lives in its own `.tmj` — and resolves through [`map_by_name`] /
/// [`modern_map_info`]. Draw, collision and the editor all read (and the editor
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
    /// Drop a map from the store. The maps directory is scanned to find what
    /// loads, so the editor's `delete_map` also retires the on-disk `.tmj`
    /// (`ConsoleApi::remove_file`) — without that it would resurrect on the next
    /// boot/hot-reload scan.
    pub fn remove(&mut self, name: &str) -> Option<TiledMap> {
        self.maps.remove(name)
    }
    /// Re-key a map from `old` to `new` (a no-op if `old` is absent).
    pub fn rename(&mut self, old: &str, new: impl Into<String>) {
        if let Some(map) = self.maps.remove(old) {
            self.maps.insert(new.into(), map);
        }
    }
    /// Whether a map of this name is loaded (used to dedup new/duplicate names).
    pub fn contains(&self, name: &str) -> bool {
        self.maps.contains_key(name)
    }
    /// All loaded map names, sorted for stable menu/UI listings.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.maps.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
    /// Whether `name` is a "modern" map — one whose content lives in the map
    /// file rather than in code. True when it carries an **object layer** (its
    /// map objects are authored in Tiled) *or* an **image layer** (a painted
    /// background/collision-mask map). The image-layer arm is what lets a
    /// pure-painted map — image + mask, possibly with no tile *and* no object
    /// layer — still resolve through [`map_by_name`] and build via
    /// [`modern_map_info`].
    pub fn is_modern(&self, name: &str) -> bool {
        self.get(name).is_some_and(|map| {
            map.layers.iter().any(|layer| {
                matches!(
                    layer,
                    TiledMapLayer::ObjectLayer(_) | TiledMapLayer::ImageLayer(_)
                )
            })
        })
    }
}

/// Resolve a map name to its load metadata: a **modern** map in the store
/// ([`MapStore::is_modern`]) builds via [`modern_map_info`]; any other name —
/// an unknown one, or a stale numeric string from an old save / `to_map`
/// property — returns `None`. Every map is modern now (each lives in its own
/// `.tmj`), so the store is the sole source. `indexed_sprites` is only read for
/// the sprite art the modern collision layer derives its colliders from (the
/// sheet that lives on [`crate::draw_state::DrawState`]).
pub fn map_by_name(indexed_sprites: &IndexedImage, name: &str, maps: &MapStore) -> Option<MapInfo> {
    if maps.is_modern(name) {
        return Some(modern_map_info(indexed_sprites, name, maps.get(name)?));
    }
    None
}

/// Build the runtime [`MapInfo`] for a modern (Tiled) map. The layer stack is a
/// single ordered pass over [`TiledMap::layers`] (so file order = draw order),
/// folding tile *and* image layers into one bg/fg split:
/// - **tile layer 0** (the first layer, when it's a tile layer) is the
///   *collision layer*: drawn invisible, its per-tile colliders derived from the
///   sprite art ([`Collider::from_sprite`]);
/// - other **tile layers** draw normally, split into bg/fg by the Tiled
///   layer-name `fg` prefix;
/// - a **collision image layer** ([`ImageLayer::is_collision`]) is data, never
///   drawn, its alpha sliced into bitmap colliders ([`painted_collision_layer`]);
/// - a plain **image layer** draws like a tile layer, also bg/fg by the `fg`
///   prefix, blit at its pixel offset;
/// - **object layers** contribute the map objects (warps + interactions).
///
/// `source_layer` is each layer's index in [`TiledMap::layers`] — object layers
/// (and any layer kind) occupy indices too, so the numbering stays aligned with
/// the file and [`LayerInfo::draw_indexed`] / collision can fetch the right
/// source layer by index.
///
/// ## Painted maps
///
/// This is what lets a map be *all paint and no tiles*: a single visible image
/// layer (the background art) + a `collision`-marked image layer (the mask) + an
/// object layer is a complete, playable map with an empty sprite sheet behind it.
/// With no tile layer present there is no tile collision layer and no sheet
/// dependency at all — collision comes entirely from the painted mask's alpha,
/// and the map's dimensions fall back to [`TiledMap::width`]/`height` (below) so
/// the camera still sizes correctly.
fn modern_map_info(indexed_sprites: &IndexedImage, name: &str, map: &TiledMap) -> MapInfo {
    // Map dimensions: the first tile layer's size if there is one (historical
    // behaviour, bit-identical), else the map's own declared size — so a
    // pure-painted map with no tile layer still has a sane size for the camera.
    let (width, height) = match map.layers.first() {
        Some(TiledMapLayer::TileLayer(layer)) => (
            // `unwrap_or(0)` like the painted arm below: a map wider/taller than
            // i16::MAX is malformed, but degrade to an empty (size-0) layer rather
            // than panicking on the conversion.
            layer.width.try_into().unwrap_or(0),
            layer.height.try_into().unwrap_or(0),
        ),
        _ => (
            map.width.try_into().unwrap_or(0),
            map.height.try_into().unwrap_or(0),
        ),
    };

    let mut layers = Vec::new();
    let mut fg_layers = Vec::new();
    let mut sprite_layers = Vec::new();
    let mut seen_collision_tiles = false;
    for (i, layer) in map.layers.iter().enumerate() {
        match layer {
            // The first tile layer is the collision layer: invisible, colliders
            // from the sprite art. Later tile layers draw, routed by their plane.
            TiledMapLayer::TileLayer(tile_layer) => {
                if !seen_collision_tiles {
                    seen_collision_tiles = true;
                    layers.push(collision_tile_layer(indexed_sprites, map, i, width, height));
                    continue;
                }
                let info = LayerInfo {
                    origin: Vec2::new(0, 0),
                    size: Vec2::new(
                        tile_layer.width.try_into().unwrap_or(0),
                        tile_layer.height.try_into().unwrap_or(0),
                    ),
                    offset: Vec2::new(tile_layer.offsetx as i16, tile_layer.offsety as i16),
                    source_layer: i,
                    transparent: Some(0),
                    palette_rotate: tile_layer.palette_rotate(),
                    ..LayerInfo::DEFAULT_LAYER
                };
                // A tile layer picks its plane from the `plane` property (else the
                // `fg` name-prefix); a `sprite`-plane layer y-sorts against
                // entities rather than drawing flat.
                match tile_layer.plane() {
                    Plane::Bg => layers.push(info),
                    Plane::Sprite => sprite_layers.push(info),
                    Plane::Fg => fg_layers.push(info),
                }
            }
            // A collision image layer is invisible data; a plain one draws. Image
            // layers stay on the bg/fg name convention (no `sprite` plane — that
            // needs the tile-layer `plane` property; see [`Plane`]).
            TiledMapLayer::ImageLayer(image) => {
                if image.is_collision() {
                    layers.push(painted_collision_layer(image, i));
                } else {
                    let info = image_draw_layer(image, i);
                    match Plane::from_name(&image.name) {
                        Plane::Fg => fg_layers.push(info),
                        _ => layers.push(info),
                    }
                }
            }
            TiledMapLayer::ObjectLayer(_) => {}
        }
    }

    let sprite_components = sprite_components_of(map, &sprite_layers);
    let objects = map.parse_objects();
    MapInfo {
        layers,
        fg_layers,
        sprite_layers,
        sprite_components,
        objects,
        bg_colour: map.bg_colour().unwrap_or_default(),
        camera_bounds: map.camera_stick().map(|(x, y)| CameraBounds::stick(x, y)),
        // The `music` property names a track by file stem; the host resolves it
        // against the music directory at play time (an unknown name no-ops there,
        // just as a dangling warp `to_map` no-ops against the map store). The
        // optional `music_speed` rides along as the playback-rate multiplier.
        music_track: map
            .music()
            .map(|name| MusicTrack::named(name).with_speed(map.music_speed())),
        source: name.to_string(),
    }
}

/// Decompose every [`Plane::Sprite`] layer into y-sorting [`SpriteComponent`]s
/// (see [`layer_sprite_components`]), in `sprite_layers` order. Components from
/// different layers never merge — each layer flood-fills on its own — so the
/// list is just the per-layer results concatenated.
fn sprite_components_of(map: &TiledMap, sprite_layers: &[LayerInfo]) -> Vec<SpriteComponent> {
    sprite_layers
        .iter()
        .flat_map(|layer| layer_sprite_components(map, layer))
        .collect()
}

/// Flood-fill one sprite-plane layer's non-empty cells (tile id ≠ 0, i.e. the
/// same cells that draw) into 4-connected [`SpriteComponent`]s. Each cell's world
/// position and tile id are resolved through the exact math
/// [`LayerInfo::draw_indexed`] uses (layer pixel offset + grid × 8; the tile id
/// used directly as a sprite index), and the component's `baseline` is the bottom
/// pixel edge of its lowest occupied row (offset included). The layer's
/// `transparent` and `palette_rotate` ride onto every component so a cell draws
/// identically to the flat layer.
fn layer_sprite_components(map: &TiledMap, layer: &LayerInfo) -> Vec<SpriteComponent> {
    let Some(TiledMapLayer::TileLayer(tile_layer)) = map.layers.get(layer.source_layer) else {
        return Vec::new();
    };
    let w = layer.size.x.max(0) as usize;
    let h = layer.size.y.max(0) as usize;
    if w == 0 || h == 0 {
        return Vec::new();
    }
    // A cell holds a tile there iff its (sheet-local) id is non-zero — id 0 is the
    // empty/transparent cell the layer skips when drawing.
    let tile_at = |x: usize, y: usize| tile_layer.get(x, y).filter(|&id| id != 0);

    let mut visited = vec![false; w * h];
    let mut components = Vec::new();
    for start_y in 0..h {
        for start_x in 0..w {
            if visited[start_y * w + start_x] || tile_at(start_x, start_y).is_none() {
                continue;
            }
            // Depth-first flood over the 4-connected blob at this seed.
            visited[start_y * w + start_x] = true;
            let mut stack = vec![(start_x, start_y)];
            let mut world_cells: Vec<(Vec2, i32)> = Vec::new();
            let mut lowest_row = start_y;
            while let Some((x, y)) = stack.pop() {
                let id = tile_at(x, y).unwrap_or(0);
                world_cells.push((
                    Vec2::new(
                        layer.offset.x.saturating_add((x as i16).saturating_mul(8)),
                        layer.offset.y.saturating_add((y as i16).saturating_mul(8)),
                    ),
                    id as i32,
                ));
                lowest_row = lowest_row.max(y);
                let mut push = |nx: usize, ny: usize, stack: &mut Vec<(usize, usize)>| {
                    let idx = ny * w + nx;
                    if !visited[idx] && tile_at(nx, ny).is_some() {
                        visited[idx] = true;
                        stack.push((nx, ny));
                    }
                };
                if x > 0 {
                    push(x - 1, y, &mut stack);
                }
                if x + 1 < w {
                    push(x + 1, y, &mut stack);
                }
                if y > 0 {
                    push(x, y - 1, &mut stack);
                }
                if y + 1 < h {
                    push(x, y + 1, &mut stack);
                }
            }
            // Baseline: the bottom edge of the lowest occupied row, map-absolute.
            let baseline = i32::from(layer.offset.y) + (lowest_row as i32 + 1) * 8;
            // The metasprite origin is the blob's bounding-box top-left, so the
            // component reads as "this sprite, placed here in the world".
            let origin = Vec2::new(
                world_cells.iter().map(|(p, _)| p.x).min().unwrap_or(0),
                world_cells.iter().map(|(p, _)| p.y).min().unwrap_or(0),
            );
            let sprite = MetaSprite {
                cells: world_cells
                    .into_iter()
                    .map(|(world, spr_id)| MetaCell::new(world - origin, spr_id))
                    .collect(),
            };
            components.push(SpriteComponent {
                source_layer: layer.source_layer,
                origin,
                sprite,
                baseline,
                transparent: layer.transparent,
                palette_rotate: layer.palette_rotate,
            });
        }
    }
    components
}

/// The invisible collision [`LayerInfo`] for tile layer `source` (the map's
/// first tile layer): sized `width`×`height` tiles, one [`Collider`] per cell
/// derived from that cell's tile art ([`Collider::from_sprite`]). This is the
/// historical "tile layer 0 is collision" layer, unchanged.
fn collision_tile_layer(
    indexed_sprites: &IndexedImage,
    map: &TiledMap,
    source: usize,
    width: i16,
    height: i16,
) -> LayerInfo {
    let mut colliders = Vec::with_capacity((width as usize) * (height as usize));
    for j in 0..height {
        for i in 0..width {
            let tile = map.get(source, i as usize, j as usize).unwrap_or(0);
            colliders.push(Collider::from_sprite(indexed_sprites, tile));
        }
    }
    LayerInfo {
        origin: Vec2::new(0, 0),
        size: Vec2::new(width, height),
        offset: Vec2::new(0, 0),
        source_layer: source,
        transparent: Some(0),
        visible: false,
        colliders,
        ..LayerInfo::DEFAULT_LAYER
    }
}

/// A drawable [`LayerInfo`] for a plain (non-collision) image layer at index
/// `source`: marked [`LayerKind::Image`] so [`LayerInfo::draw_indexed`] blits
/// its pixels, sized to the image's tile footprint and placed at the layer's
/// pixel offset. Carries `visible` from the layer; no colliders. A layer whose
/// `pixels` never arrived still gets a `LayerInfo` (size 0×0 if unknown) and
/// simply draws nothing.
fn image_draw_layer(image: &ImageLayer, source: usize) -> LayerInfo {
    let (w, h) = image_tile_size(image);
    LayerInfo {
        origin: Vec2::new(0, 0),
        size: Vec2::new(w, h),
        offset: Vec2::new(image.offsetx as i16, image.offsety as i16),
        source_layer: source,
        visible: image.visible,
        kind: LayerKind::Image,
        ..LayerInfo::DEFAULT_LAYER
    }
}

/// The invisible collision [`LayerInfo`] derived from a `collision`-marked image
/// layer at index `source`: its alpha sliced into 8×8 bitmap [`Collider`]s, one
/// per cell, **solid where alpha ≥ [`PAINTED_SOLID_ALPHA`]**. The grid is sized
/// to the image (rounded up to whole tiles) and placed at the layer's pixel
/// `offset`, so a cell's world position respects `offsetx`/`offsety` exactly the
/// way drawing would — the collision lines up with where the mask *would* paint.
///
/// Pixels attach at host install time, before any map is entered, so deriving
/// here is safe. If the pixels never arrived (missing/failed PNG) the layer
/// derives **empty** (logged once) rather than panicking — the map is simply
/// passable where the mask would have been.
fn painted_collision_layer(image: &ImageLayer, source: usize) -> LayerInfo {
    let (w, h) = image_tile_size(image);
    let colliders = match &image.pixels {
        Some(pixels) => painted_colliders(pixels, w, h),
        None => {
            log::warn!(
                "collision image layer {:?} has no pixels (PNG missing or failed to load); deriving empty collision",
                image.name
            );
            Vec::new()
        }
    };
    LayerInfo {
        origin: Vec2::new(0, 0),
        size: Vec2::new(w, h),
        offset: Vec2::new(image.offsetx as i16, image.offsety as i16),
        source_layer: source,
        visible: false,
        colliders,
        kind: LayerKind::Image,
        ..LayerInfo::DEFAULT_LAYER
    }
}

/// An image layer's footprint in **whole tiles** (8px), rounded up so a stray
/// last column/row of pixels still gets a collider cell. `(0, 0)` when the
/// pixels haven't been attached (size unknown until the PNG is decoded) — the
/// resulting layer then draws nothing and collides nothing.
fn image_tile_size(image: &ImageLayer) -> (i16, i16) {
    match &image.pixels {
        Some(pixels) => (
            pixels.width().div_ceil(8) as i16,
            pixels.height().div_ceil(8) as i16,
        ),
        None => (0, 0),
    }
}

/// Slice an RGBA image into a `w`×`h` grid of 8×8 [`Collider`]s (row-major,
/// matching [`layer_collides`]'s `x + y * size.x` indexing): each cell is
/// solid at the pixels whose alpha ≥ [`PAINTED_SOLID_ALPHA`]. Pixels past the
/// image edge (when it isn't a whole multiple of 8) are treated as transparent.
fn painted_colliders(pixels: &RgbaImage, w: i16, h: i16) -> Vec<Collider> {
    let (iw, ih) = (pixels.width(), pixels.height());
    let mut colliders = Vec::with_capacity((w as usize) * (h as usize));
    for cell_y in 0..h {
        for cell_x in 0..w {
            let mut collider = Collider::default();
            for py in 0..8u32 {
                for px in 0..8u32 {
                    let x = cell_x as u32 * 8 + px;
                    let y = cell_y as u32 * 8 + py;
                    if x < iw && y < ih && pixels.get_pixel(x, y).a() >= PAINTED_SOLID_ALPHA {
                        collider.set(px as usize, py as usize, true);
                    }
                }
            }
            colliders.push(collider);
        }
    }
    colliders
}

/// Which draw plane a map layer occupies relative to the entities (player,
/// creatures, pickups) that y-sort by their feet:
/// - [`Bg`](Self::Bg) — drawn entirely *under* every entity (the historical
///   default), before the sorted sprite pass;
/// - [`Sprite`](Self::Sprite) — its tiles y-sort *against* entities, so the
///   player can pass both in front of and behind them (furniture/props);
/// - [`Fg`](Self::Fg) — drawn entirely *over* every entity, after the sorted
///   pass.
///
/// A layer opts in via a `plane` custom property (`"bg"`/`"sprite"`/`"fg"`);
/// absent, it falls back to the historical name convention — an `fg` name-prefix
/// (case-insensitive) means [`Fg`](Self::Fg), else [`Bg`](Self::Bg). The
/// property, when present, wins. `Sprite` is only reachable via the property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Plane {
    /// Under every entity (the default).
    #[default]
    Bg,
    /// Y-sorted against entities by each connected component's baseline.
    Sprite,
    /// Over every entity.
    Fg,
}
impl Plane {
    /// The lowercase `plane`-property spelling, shared by the codec (round-trip)
    /// and the editor's plane-cycle button. Inverse of [`from_property`](Self::from_property).
    pub fn name(self) -> &'static str {
        match self {
            Plane::Bg => "bg",
            Plane::Sprite => "sprite",
            Plane::Fg => "fg",
        }
    }
    /// Parse a `plane` property value (case-insensitive), or `None` for an
    /// unrecognised string (the caller then falls back to the name convention).
    pub fn from_property(value: &str) -> Option<Self> {
        Some(match value.to_ascii_lowercase().as_str() {
            "bg" => Plane::Bg,
            "sprite" => Plane::Sprite,
            "fg" => Plane::Fg,
            _ => return None,
        })
    }
    /// The plane a layer named `name` falls back to when it carries no `plane`
    /// property: [`Fg`](Self::Fg) for the `fg` name-prefix (case-insensitive),
    /// else [`Bg`](Self::Bg). Never [`Sprite`](Self::Sprite) — that needs the
    /// explicit property.
    pub fn from_name(name: &str) -> Self {
        if name.to_lowercase().starts_with("fg") {
            Plane::Fg
        } else {
            Plane::Bg
        }
    }
    /// The next plane in the BG → Sprite → FG → BG cycle (the editor's three-way
    /// toggle).
    pub fn cycle(self) -> Self {
        match self {
            Plane::Bg => Plane::Sprite,
            Plane::Sprite => Plane::Fg,
            Plane::Fg => Plane::Bg,
        }
    }
}

/// A 4-connected blob of non-empty cells on a [`Plane::Sprite`] layer, drawn as
/// one y-sorted unit. Every cell shares the component's [`baseline`](Self::baseline)
/// as its sort key, so the whole prop sorts against entities by its lowest edge —
/// the player passes behind it above that line and in front of it below.
///
/// Components never span layers: each sprite-plane layer flood-fills
/// independently, which is the authoring escape hatch for making two touching
/// props sort separately (put them on separate sprite layers).
#[derive(Clone, Debug)]
pub struct SpriteComponent {
    /// The [`LayerInfo::source_layer`] of the sprite-plane layer this component
    /// came from, so the draw loop can honour that layer's `visible` flag live —
    /// the eye toggle flips `visible` without a reload, so visibility can't be
    /// baked into the derive.
    pub source_layer: usize,
    /// Map-absolute pixel position of the blob's bounding-box top-left — where
    /// [`sprite`](Self::sprite) is placed in the world (pre-camera; subtract
    /// the camera at draw time).
    pub origin: Vec2,
    /// The blob as a [`MetaSprite`]: one cell per occupied tile (pixel offset
    /// from [`origin`](Self::origin), sheet-local tile id as the sprite id), in
    /// flood-fill order. Each cell is resolved at load through the exact math
    /// [`LayerInfo::draw_indexed`] uses, so drawing it as a 1×1 sprite is
    /// bit-identical to the layer drawing it as a tile.
    pub sprite: MetaSprite,
    /// The bottom pixel edge of the component's lowest row, in **map-absolute**
    /// pixels (layer offset included). The y-sort key is `baseline − camera.y`,
    /// so it lands in the same camera-relative space as [`DrawParams::bottom`](crate::draw_state::DrawParams::bottom).
    pub baseline: i32,
    /// Colour key skipped when a cell draws (the layer's `transparent`).
    pub transparent: Option<u8>,
    /// Per-layer palette rotation applied when a cell draws.
    pub palette_rotate: u8,
}
impl SpriteComponent {
    /// This component's y-sort key at camera y `cam_y`: its map-absolute
    /// `baseline` shifted into the camera-relative space
    /// [`DrawParams::bottom`](crate::draw_state::DrawParams::bottom) returns, so an
    /// entity and a component sort by the same measure.
    pub fn sort_key(&self, cam_y: i32) -> i32 {
        self.baseline - cam_y
    }
    /// One 1×1-tile [`DrawParams`] per cell, positioned camera-relative. Drawn,
    /// each is bit-identical to the sprite-plane layer drawing that cell as a
    /// tile (same sheet index, transparency, palette rotation, destination).
    pub fn cell_params(&self, cam_x: i32, cam_y: i32) -> impl Iterator<Item = DrawParams> + '_ {
        let transparent = self.transparent;
        let palette_rotate = self.palette_rotate;
        self.sprite.iter_at(self.origin).map(move |(pos, cell)| {
            DrawParams::new(
                cell.spr_id,
                i32::from(pos.x) - cam_x,
                i32::from(pos.y) - cam_y,
                SpriteOptions {
                    transparent,
                    flip: cell.flip,
                    rotate: cell.rotate,
                    ..SpriteOptions::default()
                },
                None,
                palette_rotate,
            )
        })
    }
}

/// Metadata necessary to load a map into Walkaround.
#[derive(Clone, Debug, Default)]
pub struct MapInfo {
    pub layers: Vec<LayerInfo>,
    pub fg_layers: Vec<LayerInfo>,
    /// Y-sorting draw layers ([`Plane::Sprite`]): the editor lists/edits these
    /// like the bg/fg lists, but they don't draw as a flat pass — their tiles
    /// are grouped into [`sprite_components`](Self::sprite_components) that sort
    /// against entities.
    pub sprite_layers: Vec<LayerInfo>,
    /// The connected-component decomposition of [`sprite_layers`](Self::sprite_layers),
    /// derived at load and re-derived through the editor's reload seam. Draw-only
    /// (no colliders).
    pub sprite_components: Vec<SpriteComponent>,
    /// The map's triggerable objects (warps + interactions) in one ordered
    /// list — the walk loop scans them in vector order, so order is gameplay.
    pub objects: Vec<MapObject>,
    pub bg_colour: BgColour,
    pub music_track: Option<MusicTrack>,
    /// Name of the [`MapStore`] map these layers draw from — the map's own name.
    /// Empty (the default) means no tile source — draw and collision guard on
    /// the lookup miss.
    pub source: String,
    pub camera_bounds: Option<CameraBounds>,
}
impl MapInfo {
    pub fn draw_bg_indexed(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
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
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        map: &TiledMap,
        offset: Vec2,
        debug: bool,
    ) {
        for l in &self.fg_layers {
            l.draw_indexed(draw_state, layer, map, offset, debug);
        }
    }
    /// The sprite-plane components whose source layer is currently **visible**.
    /// The editor's eye toggle flips a [`sprite_layers`](Self::sprite_layers)
    /// entry's `visible` without a reload, so `draw_world` filters here at draw
    /// time rather than baking visibility into the derived components (mirroring
    /// [`LayerInfo::draw_indexed`]'s `!visible` early-return). A component with no
    /// matching layer — which shouldn't occur — counts as visible.
    pub fn visible_sprite_components(&self) -> impl Iterator<Item = &SpriteComponent> {
        self.sprite_components.iter().filter(move |c| {
            self.sprite_layers
                .iter()
                .find(|l| l.source_layer == c.source_layer)
                .is_none_or(|l| l.visible)
        })
    }

    /// Draw the sprite-plane layers **flat** (as plain tiles, in layer order),
    /// for the static editor previews that render a map without the live
    /// entities to y-sort against (warp-destination placement, the path
    /// recorder). The live world instead draws these through the y-sorted
    /// [`sprite_components`](Self::sprite_components) in `draw_world`.
    pub fn draw_sprite_indexed(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        map: &TiledMap,
        offset: Vec2,
        debug: bool,
    ) {
        for l in &self.sprite_layers {
            l.draw_indexed(draw_state, layer, map, offset, debug);
        }
    }
    /// Indices (into [`objects`](Self::objects)) of the warp objects whose
    /// trigger hitbox `hitbox` overlaps. A player whose hitbox sits here stands
    /// on those warps' triggers — a [`WarpMode::Auto`] warp re-fires instantly
    /// (a teleport loop), a [`WarpMode::Interact`] warp drops the player inside a
    /// door they can immediately re-enter. The reusable core of warp-destination
    /// validation, shared by the editor's placement guard and the authored-map
    /// sanity test; callers read `self.objects[i]` for the warp's mode.
    pub fn warps_overlapping(&self, hitbox: Hitbox) -> impl Iterator<Item = usize> + '_ {
        self.objects
            .iter()
            .enumerate()
            .filter(move |(_, o)| {
                matches!(o.effect, ObjectEffect::Warp(_)) && hitbox.touches(o.hitbox)
            })
            .map(|(i, _)| i)
    }
    /// Would a player with `player_hitbox` (its *local*, origin-relative box)
    /// landing at `to` overlap a warp on this map? `Some(index)` of the first
    /// offending warp, `None` if the spot is clear. `to` is in this — the
    /// **destination** — map's pixels; resolve the destination map from the
    /// warp's `map` field first. Thin wrapper over
    /// [`warps_overlapping`](Self::warps_overlapping) that positions the player
    /// box at the landing, so callers pass the raw warp target.
    pub fn warp_landing_conflict(&self, to: Vec2, player_hitbox: Hitbox) -> Option<usize> {
        self.warps_overlapping(player_hitbox.offset(to)).next()
    }
}

/// What a [`LayerInfo`] draws from. The runtime layer list is *heterogeneous*:
/// tile layers and image layers share one `Vec<LayerInfo>` so their **relative
/// file order is preserved** (a painted background under a tile layer, or an
/// `fg`-prefixed image above sprites, both land in the right place), and the
/// kind only decides *how* a layer draws — every other field (offset, size,
/// visibility, colliders) means the same thing for both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerKind {
    /// Tiles drawn from the shared sheet via the map's [`TileLayer`] at
    /// `source_layer` — the historical layer, and the `Default`.
    #[default]
    Tiles,
    /// A single bitmap blit from the map's [`ImageLayer`](crate::data::tiled::ImageLayer)
    /// at `source_layer` (its `pixels`), at this layer's pixel `offset`. See
    /// [`modern_map_info`] for the painted-map model.
    Image,
}

/// Layers defined by map metadata. References external data stored in the
/// [`MapStore`].
#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub origin: Vec2,
    pub size: Vec2,
    pub offset: Vec2,
    pub transparent: Option<u8>,
    /// Per-layer palette rotation: every palette index is shifted by this much
    /// (wrapping at 16) when the layer draws (see [`palette_rotate`](Self::palette_rotate)).
    pub palette_rotate: u8,
    pub visible: bool,
    pub source_layer: usize,
    pub colliders: Vec<Collider>,
    /// Whether this layer draws from a tile layer or an image layer (see
    /// [`LayerKind`]). The tile and image variants share one list so file order
    /// — and thus draw order — is preserved across both.
    pub kind: LayerKind,
}
impl LayerInfo {
    pub const DEFAULT_LAYER: Self = Self {
        origin: Vec2::new(0, 0),
        size: Vec2::new(30, 17),
        offset: Vec2::new(0, 0),
        transparent: None,
        palette_rotate: 0,
        visible: true,
        source_layer: 0,
        colliders: Vec::new(),
        kind: LayerKind::Tiles,
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
    pub const fn with_palette_rotate(mut self, rotate: u8) -> Self {
        self.palette_rotate = rotate;
        self
    }
    pub fn palette_rotate(&self) -> u8 {
        self.palette_rotate
    }
    pub fn draw_indexed(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        map: &TiledMap,
        offset: Vec2,
        debug: bool,
    ) {
        use crate::draw_state::palette_map_rotate;
        use egg_render::Canvas;
        if !self.visible {
            return;
        }
        // Image layers blit their bitmap; tile layers draw from the sheet. Both
        // honour the same camera offset and the same `fg`/visible conventions.
        if self.kind == LayerKind::Image {
            self.draw_image(draw_state, layer, map, offset);
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

    /// Blit an image layer's bitmap into the same canvas tile layers draw to.
    ///
    /// The picture is placed 1:1 at `(offsetx − camera.x, offsety − camera.y)`
    /// with the engine's standard **binary transparency** (a source pixel with
    /// `alpha == 0` is skipped, every other pixel drawn opaque) — deliberately
    /// **no scaling, no repeat, no alpha blending** (all out of scope), and the
    /// layer's `opacity` is ignored. Silently does nothing if the pixels were
    /// never attached (missing/failed PNG) or the source layer isn't an image
    /// layer.
    fn draw_image(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        map: &TiledMap,
        offset: Vec2,
    ) {
        use egg_render::{Canvas, EdgePolicy, Transform};
        let Some(TiledMapLayer::ImageLayer(image)) = map.layers.get(self.source_layer) else {
            return;
        };
        let Some(pixels) = &image.pixels else {
            return;
        };
        let dx = i32::from(self.offset.x) - i32::from(offset.x);
        let dy = i32::from(self.offset.y) - i32::from(offset.y);
        draw_state.rgba_canvas[layer as usize].blit::<RgbaImage>(
            dx,
            dy,
            pixels,
            EdgePolicy::Transparent,
            Transform::default(),
            |p| p.a() == 0,
        );
    }
    /// The layer's pixel-space rectangle (offset + size×8). A non-positive size
    /// — an image layer whose pixels never arrived (0×0 tiles) — yields an empty
    /// hitbox via [`Hitbox::empty_at`] rather than tripping `Hitbox::new`'s
    /// positive-size assert; it then touches nothing, so the layer is harmlessly
    /// skipped by every collision/draw guard that hit-tests it.
    pub fn hitbox(&self) -> Hitbox {
        if self.size.x.is_positive() && self.size.y.is_positive() {
            Hitbox::new(
                self.offset.x,
                self.offset.y,
                self.size.x * 8,
                self.size.y * 8,
            )
        } else {
            Hitbox::empty_at(self.offset.x, self.offset.y)
        }
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

/// How a [`MapObject`] is triggered — the *authored occasion* half of the firing
/// decision (the map author's intent for this object), independent of the effect
/// kind and of any player preference:
/// - [`Touch`](Self::Touch) — fires only when the player's body overlaps the
///   hitbox (a step-on trigger);
/// - [`Press`](Self::Press) — fires only when the player presses the interact
///   button while facing into the hitbox;
/// - [`Any`](Self::Any) — fires on either path;
/// - [`Enter`](Self::Enter) — fires once when the *map loads*, not on any player
///   contact; the map-enter hook. Its hitbox is ignored (the whole map is its
///   trigger), so it's how a room opens a story beat on arrival. Only meaningful
///   for a cutscene interaction (see [`crate::world::interact::Interaction::Cutscene`]);
///   on a warp or other effect it never fires (the enter pass only launches
///   cutscenes, and [`allows_touch`](Self::allows_touch)/[`allows_press`](Self::allows_press)
///   are both false, so the touch/press scan skips it too).
///
/// This is orthogonal to the [`Gate`] (the *whether* axis — flag conditions):
/// `Trigger` says *when* an object may fire, `Gate` says *whether* it may fire
/// this save. They compose — e.g. an `Enter` cutscene gated `unless seen` that
/// `sets seen` is a one-shot on-enter beat.
///
/// Defaults preserve the historical effect-driven behaviour and are set by the
/// constructors, not by `Default`: warps default to [`Any`](Self::Any) (a door
/// you can walk into or press), interactions to [`Press`](Self::Press) (a sign
/// you must face and read); [`Enter`](Self::Enter) is never a default (only
/// authored). See [`MapObject`] for how this composes with the effect kind, the
/// warp [`WarpMode`], and warp narration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Trigger {
    /// Fires on body-touch only.
    Touch,
    /// Fires on a facing-direction interact press only.
    #[default]
    Press,
    /// Fires on either body-touch or a facing press.
    Any,
    /// Fires once when the map is loaded (the map-enter hook), ignoring the
    /// hitbox and all player contact. Only launches a cutscene interaction.
    Enter,
}
impl Trigger {
    /// Whether this trigger fires on body-touch.
    pub fn allows_touch(self) -> bool {
        matches!(self, Self::Touch | Self::Any)
    }
    /// Whether this trigger fires on a facing interact press.
    pub fn allows_press(self) -> bool {
        matches!(self, Self::Press | Self::Any)
    }
    /// The lowercase wire/UI spelling. The single source of truth shared by the
    /// `.tmj` codec and the editor; the inverse is the codec's `parse_trigger`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Touch => "touch",
            Self::Press => "press",
            Self::Any => "any",
            Self::Enter => "enter",
        }
    }
    /// The trigger an effect of `effect`'s kind defaults to when none is
    /// authored: warps to [`Any`](Self::Any), interactions to
    /// [`Press`](Self::Press). The single source of truth shared by
    /// [`MapObject::new`] and the `.tmj` codec (which serialises a trigger only
    /// when it differs from this default, keeping existing files byte-stable).
    pub fn default_for(effect: &ObjectEffect) -> Self {
        match effect {
            ObjectEffect::Warp(_) => Self::Any,
            ObjectEffect::Interact(_) => Self::Press,
        }
    }

    /// Whether an **interaction** with this trigger fires, given whether the
    /// player is touching the hitbox, whether they were already inside it last
    /// frame (`was_inside`), and whether they pressed-and-faced it (`probed`).
    ///
    /// The touch path is **edge-triggered** (fires only on *entering* the
    /// hitbox), so a step-on dialogue plays once rather than every frame the
    /// player stands in it. The press path is level-triggered as usual. Warps
    /// don't use this asymmetry — their teleport exits the hitbox immediately, so
    /// they re-evaluate touch every frame (see [`Self::warp_fires`]).
    pub fn interaction_fires(self, touched: bool, was_inside: bool, probed: bool) -> bool {
        (self.allows_touch() && touched && !was_inside) || (self.allows_press() && probed)
    }

    /// Whether a **warp** with this trigger fires, composing the authored trigger
    /// with the player's manual-doors preference and the warp's [`WarpMode`].
    ///
    /// The touch path is level-triggered (a warp re-evaluates touch every frame
    /// because teleporting exits the hitbox) and is **suppressed** only when the
    /// player opted into manual doors (`manual_doors`) *and* this is an
    /// `Interact`-mode warp; `Auto`-mode warps always keep their touch path. The
    /// press path is never suppressed.
    pub fn warp_fires(
        self,
        touched: bool,
        probed: bool,
        mode: &WarpMode,
        manual_doors: bool,
    ) -> bool {
        let touch_suppressed = manual_doors && matches!(mode, WarpMode::Interact);
        (self.allows_touch() && touched && !touch_suppressed) || (self.allows_press() && probed)
    }
}

/// The *whether* half of a [`MapObject`]'s firing decision: a flag gate that
/// blocks or allows the object independently of its [`Trigger`] (the *when/how*
/// half). All three fields name a story flag from the same vocabulary dialogue
/// uses (`#flag`-declared, stored in [`SaveData::flags`](crate::data::save::SaveData::flags),
/// toggled by dialogue `#set` and read by `#if`), so an object condition and a
/// dialogue branch can gate on the very same flag:
/// - [`if_flag`](Self::if_flag) — fires only while this flag is **set** (the
///   `if` property);
/// - [`unless_flag`](Self::unless_flag) — fires only while this flag is **clear**
///   (the `unless` property);
/// - [`sets`](Self::sets) — a flag **set when the object fires** (the `sets`
///   property), the one-shot side effect.
///
/// The common one-shot is `unless X` + `sets X`: the object fires once, sets `X`,
/// and its own gate then holds it off forever — persisted through the normal save
/// flags. The two gates can name different flags than `sets` for open-ended
/// prerequisite/side-effect chains. Default (every field `None`) is no gate: the
/// object always fires, exactly as before this axis existed, so unauthored
/// objects and old maps are unaffected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gate {
    /// Fires only when this flag is set. `None` = no requirement.
    pub if_flag: Option<String>,
    /// Fires only when this flag is clear. `None` = no bar.
    pub unless_flag: Option<String>,
    /// A flag set (true) when the object fires — the one-shot latch. `None` = no
    /// side effect.
    pub sets: Option<String>,
}
impl Gate {
    /// Whether an object with this gate may fire against the live `save`: its
    /// `if` flag (if any) must be set and its `unless` flag (if any) clear. An
    /// empty gate always allows. The `sets` side effect is applied separately at
    /// fire time (see [`MapObject`]'s firing sites), not here.
    pub fn allows(&self, save: &SaveData) -> bool {
        self.if_flag.as_deref().is_none_or(|f| save.flag(f))
            && self.unless_flag.as_deref().is_none_or(|f| !save.flag(f))
    }
    /// The authored `.tmj` properties this gate serialises to, in `if`, `unless`,
    /// `sets` order — only the set fields, so an ungated object emits nothing and
    /// its file stays byte-stable. The inverse of the gate parse in
    /// [`TiledObject::gate`](crate::data::tiled). Each pair is `(property name,
    /// flag value)`.
    pub fn properties(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("if", &self.if_flag),
            ("unless", &self.unless_flag),
            ("sets", &self.sets),
        ]
        .into_iter()
        .filter_map(|(name, flag)| flag.as_deref().map(|value| (name, value)))
    }
}

/// A triggerable object placed on a map: a hitbox, the effect it fires, the
/// trigger axis that decides *how* it fires, and an optional animated sprite
/// drawn at its location. Unifies the old separate "warp" and "interactable"
/// object kinds into one list (see [`MapInfo::objects`]).
///
/// Three orthogonal knobs compose into whether (and how) an object fires, in the
/// walk loop's object pass:
/// - **`trigger`** ([`Trigger`]) is *authored geometry* — the map author's
///   intent for this object: touch-only, press-only, or either. It is the only
///   knob that decides the body-touch vs. facing-press paths.
/// - the warp **[`WarpMode`]** is *player preference*: it modulates **only a
///   warp's touch path**, and only when the player has opted into manual doors
///   (`SaveData::manual_doors`) — an `Interact`-mode door then stops opening on
///   touch, but its press path and every `Auto`-mode door are unaffected. It
///   never touches interactions.
/// - warp **narration** ([`Warp::narration`]) is *orthogonal*: it doesn't change
///   when a warp fires, only what happens at fire time (show dialogue first, warp
///   once it closes).
///
/// Defaults preserve historical behaviour: the constructors set `trigger` from
/// the effect kind ([`Trigger::default_for`]) — warps [`Trigger::Any`],
/// interactions [`Trigger::Press`] — so with no authored triggers and default
/// save settings every map behaves exactly as before this axis existed.
#[derive(Clone, Debug)]
pub struct MapObject {
    pub hitbox: Hitbox,
    pub effect: ObjectEffect,
    /// How this object fires (touch / press / either). Set by the constructors
    /// from the effect kind; override with [`with_trigger`](Self::with_trigger).
    pub trigger: Trigger,
    pub sprite: Option<Vec<AnimFrame>>,
    /// This object's stable identity within its map — Tiled's per-object id
    /// ([`TiledObject::id`](crate::data::tiled::TiledObject::id)), carried through
    /// the parse so a removable object can be recorded durably in the save's
    /// `taken` set (a positional index would shift when a sibling is added or
    /// removed). `None` for a runtime/editor-created object that has no id yet;
    /// the map writer then assigns it a fresh one above every existing id on the
    /// next save, so survivors never renumber.
    pub id: Option<usize>,
    /// Whether interacting with this object *consumes* it: a pickup that vanishes
    /// once taken and stays gone. On interaction the engine records it (by
    /// [`id`](Self::id)) in the save's `taken` set (see
    /// `take_object`). The object
    /// then stays in the map *data* — so the editor can still show and edit it —
    /// but the walk loop skips it at use-time: its interaction won't fire and its
    /// sprite won't draw while its `<map>#<id>` key is in `taken`.
    /// Authored as a `removable` object property; only meaningful for interaction
    /// objects (warps fire on touch and are never "taken").
    pub removable: bool,
    /// The flag gate deciding *whether* this object fires this save (the `if` /
    /// `unless` conditions) and what flag it sets when it does (`sets`) — the
    /// one-shot machinery. Orthogonal to [`trigger`](Self::trigger) (which decides
    /// *when/how*): every object kind reads it, checked at each firing site. The
    /// default [`Gate`] (all `None`) is no gate, so an unauthored object always
    /// fires. Authored as `if` / `unless` / `sets` object properties.
    pub gate: Gate,
}

/// What a [`MapObject`] does when triggered: warp the player, or run an
/// [`Interaction`] (dialogue / one-off function / nothing).
#[derive(Clone, Debug)]
pub enum ObjectEffect {
    Warp(Warp),
    Interact(Interaction),
}

impl MapObject {
    /// A map object whose `trigger` is the default for `effect`'s kind
    /// ([`Trigger::default_for`]): warps fire on touch-or-press, interactions on
    /// press only. Override afterwards with [`with_trigger`](Self::with_trigger).
    pub fn new(hitbox: Hitbox, effect: ObjectEffect, sprite: Option<Vec<AnimFrame>>) -> Self {
        let trigger = Trigger::default_for(&effect);
        Self {
            hitbox,
            effect,
            trigger,
            sprite,
            id: None,
            removable: false,
            gate: Gate::default(),
        }
    }
    /// Set this object's stable Tiled [`id`](Self::id) (its identity within the
    /// map). `None` clears it — a runtime/editor-created object with no durable
    /// id yet, which the map writer assigns on save.
    pub fn with_id(mut self, id: Option<usize>) -> Self {
        self.id = id;
        self
    }
    /// Mark whether interacting with this object consumes it (see
    /// [`removable`](Self::removable)).
    pub fn with_removable(mut self, removable: bool) -> Self {
        self.removable = removable;
        self
    }
    /// A warp object: its `hitbox` is the trigger region, `warp` the destination.
    /// Defaults to [`Trigger::Any`] (walk into it or press), per [`MapObject::new`].
    pub fn warp(hitbox: Hitbox, warp: Warp) -> Self {
        Self::new(hitbox, ObjectEffect::Warp(warp), None)
    }
    /// A tile-coordinate warp object (8px tiles), mirroring the old
    /// `Warp::new_tile`: trigger tile at `(tx1, ty1)`, destination `(tx2, ty2)`.
    pub fn warp_tile(tx1: i16, ty1: i16, map: Option<&str>, tx2: i16, ty2: i16) -> Self {
        Self::warp(
            Hitbox::new(tx1 * 8, ty1 * 8, 8, 8),
            Warp::new(map, Vec2::new(tx2 * 8, ty2 * 8)),
        )
    }
    /// An interaction object showing the dialogue registered under `key`.
    pub fn dialogue(hitbox: Hitbox, key: &str) -> Self {
        Self::new(
            hitbox,
            ObjectEffect::Interact(Interaction::Dialogue(key.to_string())),
            None,
        )
    }
    /// An interaction object running a one-off [`InteractFn`].
    pub fn func(hitbox: Hitbox, func: InteractFn) -> Self {
        Self::new(
            hitbox,
            ObjectEffect::Interact(Interaction::Func(func)),
            None,
        )
    }
    /// Attach an animated sprite drawn at the object's location.
    pub fn with_sprite(mut self, frames: Vec<AnimFrame>) -> Self {
        self.sprite = Some(frames);
        self
    }
    /// Override the trigger axis (touch / press / either / on-enter), replacing
    /// the effect-kind default the constructor picked.
    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }
    /// Set this object's flag [`Gate`] (the `if` / `unless` conditions and the
    /// `sets` one-shot latch). The default gate allows the object always.
    pub fn with_gate(mut self, gate: Gate) -> Self {
        self.gate = gate;
        self
    }
    /// Set the warp's pre-warp narration dialogue key (warp objects only): when
    /// the warp fires it shows that dialogue first and only teleports once the
    /// box closes. No-op on non-warp objects.
    pub fn with_narration(self, key: &str) -> Self {
        self.map_warp(|w| w.with_narration(key))
    }
    /// Run `f` over this object's inner [`Warp`] effect, if it is one — lets the
    /// legacy builders keep their old fluent `Warp` setters as a single chain off
    /// the tile-warp constructor. No-op on non-warp objects.
    fn map_warp(mut self, f: impl FnOnce(Warp) -> Warp) -> Self {
        if let ObjectEffect::Warp(warp) = self.effect {
            self.effect = ObjectEffect::Warp(f(warp));
        }
        self
    }
    /// Set the warp's destination-flip axis (warp objects only).
    pub fn with_warp_flip(self, flip: Axis) -> Self {
        self.map_warp(|w| w.with_flip(flip))
    }
    /// Set the warp's [`WarpMode`] (warp objects only).
    pub fn with_warp_mode(self, mode: WarpMode) -> Self {
        self.map_warp(|w| w.with_mode(mode))
    }
    /// Set the warp's trigger sound (warp objects only).
    pub fn with_warp_sound(self, sound: SfxData) -> Self {
        self.map_warp(|w| w.with_sound(sound))
    }
}

/// The player-preference half of a warp's firing decision: whether the player
/// must press to use the door, or it opens on touch. It modulates **only the
/// warp's touch path**, and only when the player has opted into manual doors
/// (`SaveData::manual_doors`):
/// - [`Auto`](Self::Auto) — always opens on touch (and on press), regardless of
///   the manual-doors setting;
/// - [`Interact`](Self::Interact) — opens on touch *unless* `manual_doors` is
///   set, in which case only the press path remains.
///
/// The press path is never suppressed by the mode, and the mode never affects
/// interactions — only warps. Orthogonal to the object's [`Trigger`] (authored
/// geometry) and to warp narration. Parsed from `.tmj`, edited in the map
/// editor, and serialised back.
#[derive(Clone, Debug)]
pub enum WarpMode {
    /// Automatically used when touched.
    Auto,
    /// Requires the player to manually interact with the door
    /// if the "Automatic doors" setting is disabled.
    Interact,
}

/// The effect of a warp [`MapObject`]: where the player lands and how. The
/// trigger hitbox now lives on the owning [`MapObject`], not here.
#[derive(Clone, Debug)]
pub struct Warp {
    /// Destination map name (`None` = same map). Resolved via [`map_by_name`]
    /// against the loaded [`MapStore`]; an unresolvable name (e.g. a stale
    /// numeric `to_map` from an old `.tmj`) is a no-op (the warp keeps the
    /// current map, logged).
    pub map: Option<String>,
    pub to: Vec2,
    pub flip: Axis,
    pub mode: WarpMode,
    pub sound: Option<SfxData>,
    /// Optional pre-warp narration: a dialogue-registry key. When set, firing the
    /// warp shows that dialogue instead of teleporting immediately; the teleport
    /// is deferred until the box closes (see the walk loop's `pending_warp`).
    /// `None` (the default) warps land instantly, exactly as before. Orthogonal
    /// to [`WarpMode`] and to the object's [`Trigger`].
    pub narration: Option<String>,
}

impl Warp {
    pub fn new(map: Option<&str>, to: Vec2) -> Self {
        let to = Vec2::new(to.x, to.y);
        Self {
            map: map.map(str::to_string),
            to,
            flip: Axis::None,
            mode: WarpMode::Interact,
            sound: None,
            narration: None,
        }
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
    /// Set the pre-warp narration dialogue key (empty key clears it).
    pub fn with_narration(self, key: &str) -> Self {
        Self {
            narration: (!key.is_empty()).then(|| key.to_string()),
            ..self
        }
    }
    pub fn target(&self) -> Vec2 {
        Vec2::new(self.to.x, self.to.y)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Whether `point` collides with `layer` at this map position, read from the
/// layer's own bitmap [`Collider`]s. Every map is modern now: a tile layer's
/// colliders are derived from its tile art ([`Collider::from_sprite`], see
/// [`collision_tile_layer`]) and a `collision` image layer's from its painted
/// mask, so both kinds answer purely from their collider grid.
///
/// The grid is indexed (and sampled) relative to the layer's pixel `offset`, so
/// a mask placed at a non-tile-aligned offset still lines up.
pub fn layer_collides(point: Vec2, layer: &LayerInfo) -> bool {
    let layer_hitbox = layer.hitbox();
    if !layer_hitbox.touches_point(point) {
        return false;
    }
    // Pixel within the layer (≥ 0 inside the hitbox), the offset-relative
    // coordinate both the collider cell index and the in-cell sample derive from.
    let local = Vec2::new(point.x - layer_hitbox.x, point.y - layer_hitbox.y);
    let map_point = Vec2::new(local.x / 8 + layer.origin.x, local.y / 8 + layer.origin.y);
    collider_at(layer, map_point, local.x as usize, local.y as usize)
}

/// Sample `layer`'s bitmap collider grid: the cell at tile coordinate
/// `map_point` (row-major, `x % size.x + y * size.x`), probed at in-cell pixel
/// (`px`, `py`) (both taken mod 8 by [`Collider::get`]). `false` when the cell
/// is out of range or the layer has no colliders.
fn collider_at(layer: &LayerInfo, map_point: Vec2, px: usize, py: usize) -> bool {
    // Out of range (including negative coords left of / above the layer) → no
    // collider, matching the old behaviour where such indices wrapped to a huge
    // usize and missed the `Vec`.
    if layer.size.x <= 0 || map_point.x < 0 || map_point.y < 0 {
        return false;
    }
    // Compute the row-major index in usize: `map_point.y * size.x` overflows i16
    // on a layer more than ~180 tiles wide (a 256×256 map trips it), panicking in
    // debug and wrapping in release.
    let cols = layer.size.x as usize;
    let index = (map_point.x as usize % cols) + (map_point.y as usize) * cols;
    layer
        .colliders
        .get(index)
        .map(|collider| collider.get(px, py))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::tiled::{ObjectLayer, TileLayer};
    use egg_platform::test_console::TestConsole;

    /// [`Gate::allows`] reads the same save flags dialogue does: an empty gate
    /// always allows; `if` requires its flag set; `unless` requires its flag
    /// clear; the two compose (both must hold). `sets` is a fire-time side effect,
    /// not part of the allow decision.
    #[test]
    fn gate_allows_composes_if_and_unless() {
        let mut save = SaveData::default();
        assert!(Gate::default().allows(&save), "empty gate always allows");

        let if_key = Gate {
            if_flag: Some("has_key".into()),
            ..Gate::default()
        };
        assert!(!if_key.allows(&save), "if-flag unset ⇒ blocked");
        save.set_flag("has_key", true);
        assert!(if_key.allows(&save), "if-flag set ⇒ allowed");

        let unless_open = Gate {
            unless_flag: Some("door_open".into()),
            ..Gate::default()
        };
        assert!(unless_open.allows(&save), "unless-flag clear ⇒ allowed");
        save.set_flag("door_open", true);
        assert!(!unless_open.allows(&save), "unless-flag set ⇒ blocked");

        // Both conditions compose: needs has_key set AND door_open clear.
        let both = Gate {
            if_flag: Some("has_key".into()),
            unless_flag: Some("door_open".into()),
            sets: Some("door_open".into()),
        };
        assert!(!both.allows(&save), "door_open set blocks despite has_key");
        save.set_flag("door_open", false);
        assert!(both.allows(&save), "has_key set and door_open clear ⇒ allowed");
    }

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
                    ..Default::default()
                }),
                TiledMapLayer::ObjectLayer(ObjectLayer {
                    name: "Object Layer 1".to_string(),
                    objects: Vec::new(),
                }),
            ],
            tilesets: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Resolution is store-only now: a name with no store entry — including a
    /// former legacy-builder name like "town" — returns `None` (there is no
    /// hardcoded fallback table any more), as does an unknown name.
    #[test]
    fn map_by_name_unknown_name_is_none() {
        let console = TestConsole::new();
        let store = MapStore::default();
        assert!(
            map_by_name(&console.indexed_sprites, "town", &store).is_none(),
            "a legacy name with no store entry no longer resolves"
        );
        assert!(map_by_name(&console.indexed_sprites, "no_such_map", &store).is_none());
    }

    /// Numeric strings (old saves / numeric `to_map` properties) are not map
    /// names and resolve to nothing — the numeric-id shim is gone.
    #[test]
    fn map_by_name_numeric_string_is_none() {
        let console = TestConsole::new();
        let store = MapStore::default();
        assert!(map_by_name(&console.indexed_sprites, "4", &store).is_none());
    }

    /// Modern names build their MapInfo from the map's own layers: layer 0
    /// becomes the invisible collision layer with one collider per tile.
    #[test]
    fn map_by_name_builds_modern_map() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        store.insert("lab", synthetic_modern_map());
        assert!(store.is_modern("lab"));
        let lab =
            map_by_name(&console.indexed_sprites, "lab", &store).expect("lab is in the store");
        assert_eq!(lab.source, "lab");
        assert_eq!(lab.layers.len(), 1, "collision layer only");
        assert!(!lab.layers[0].visible);
        assert_eq!(lab.layers[0].colliders.len(), 16);
        assert!(lab.fg_layers.is_empty());
        assert!(lab.sprite_layers.is_empty());
        assert!(lab.sprite_components.is_empty());
    }

    // --- Sprite-plane layers --------------------------------------------------

    /// A tile layer carrying `data`, an optional `plane` property and a vertical
    /// pixel offset — the fixture the sprite-plane tests build maps from.
    fn plane_tile_layer(
        name: &str,
        w: usize,
        h: usize,
        data: Vec<usize>,
        plane: Option<Plane>,
        offsety: f64,
    ) -> TiledMapLayer {
        let mut properties = Vec::new();
        if let Some(p) = plane {
            properties.push(crate::data::tiled::Property::string("plane", p.name()));
        }
        TiledMapLayer::TileLayer(TileLayer {
            width: w,
            height: h,
            data,
            name: name.to_string(),
            offsety,
            properties,
            ..Default::default()
        })
    }

    /// A `w`×`h` tile-data grid with the listed cells set to a non-zero tile id.
    fn occupied_grid(w: usize, h: usize, cells: &[(usize, usize)]) -> Vec<usize> {
        let mut data = vec![0usize; w * h];
        for &(x, y) in cells {
            data[y * w + x] = 5;
        }
        data
    }

    /// Build + resolve a modern map from `layers` (an object layer is appended so
    /// it counts as modern), returning its [`MapInfo`].
    fn info_from_layers(layers: Vec<TiledMapLayer>, w: usize, h: usize) -> MapInfo {
        let console = TestConsole::new();
        let mut all = layers;
        all.push(TiledMapLayer::ObjectLayer(ObjectLayer {
            name: "obj".to_string(),
            objects: Vec::new(),
        }));
        let map = TiledMap {
            width: w,
            height: h,
            layers: all,
            tilesets: Vec::new(),
            properties: Vec::new(),
        };
        let mut store = MapStore::default();
        store.insert("m", map);
        map_by_name(&console.indexed_sprites, "m", &store).expect("m resolves")
    }

    /// The `plane` property routes a tile layer to the sprite / fg / bg list and
    /// wins over the `fg` name-prefix; a bare `fg`-named layer still falls back to
    /// fg without a property.
    #[test]
    fn sprite_plane_classification() {
        let info = info_from_layers(
            vec![
                plane_tile_layer("collision", 2, 2, vec![0; 4], None, 0.0),
                plane_tile_layer("props", 2, 2, vec![0; 4], Some(Plane::Sprite), 0.0),
                plane_tile_layer("roof", 2, 2, vec![0; 4], Some(Plane::Fg), 0.0),
                plane_tile_layer("fgWall", 2, 2, vec![0; 4], None, 0.0),
                plane_tile_layer("fgThing", 2, 2, vec![0; 4], Some(Plane::Bg), 0.0),
            ],
            2,
            2,
        );
        // Sprite plane: only the `plane=sprite` layer (source 1).
        let sprite: Vec<usize> = info.sprite_layers.iter().map(|l| l.source_layer).collect();
        assert_eq!(sprite, vec![1]);
        // Fg: the `plane=fg` layer (2) and the bare `fg`-named layer (3, fallback).
        let fg: Vec<usize> = info.fg_layers.iter().map(|l| l.source_layer).collect();
        assert_eq!(fg, vec![2, 3]);
        // Bg: collision (0) plus fgThing, whose `plane=bg` countermands its `fg`
        // name (4) — the property wins over the prefix.
        let bg: Vec<usize> = info.layers.iter().map(|l| l.source_layer).collect();
        assert_eq!(bg, vec![0, 4]);
    }

    /// One sprite layer with two disjoint blobs (an L-shape + a separate column)
    /// flood-fills into two components with the right cell counts; each baseline
    /// is its lowest row's bottom edge, the layer's `offsety` included.
    #[test]
    fn flood_fill_splits_blobs_and_computes_baseline() {
        // L-shape: (0,1),(1,1),(0,2) — lowest row 2. Column: (3,3),(3,4) — row 4.
        let cells = [(0, 1), (1, 1), (0, 2), (3, 3), (3, 4)];
        let info = info_from_layers(
            vec![
                plane_tile_layer("collision", 5, 5, vec![0; 25], None, 0.0),
                plane_tile_layer(
                    "props",
                    5,
                    5,
                    occupied_grid(5, 5, &cells),
                    Some(Plane::Sprite),
                    5.0,
                ),
            ],
            5,
            5,
        );
        assert_eq!(info.sprite_components.len(), 2, "two disjoint blobs");
        let mut counts: Vec<usize> = info
            .sprite_components
            .iter()
            .map(|c| c.sprite.cells.len())
            .collect();
        counts.sort();
        assert_eq!(counts, vec![2, 3]);
        let l_blob = info
            .sprite_components
            .iter()
            .find(|c| c.sprite.cells.len() == 3)
            .unwrap();
        let column = info
            .sprite_components
            .iter()
            .find(|c| c.sprite.cells.len() == 2)
            .unwrap();
        // Baseline = offsety(5) + (lowest_row + 1) * 8.
        assert_eq!(l_blob.baseline, 5 + (2 + 1) * 8);
        assert_eq!(column.baseline, 5 + (4 + 1) * 8);
        // Cell world positions include the layer offset: the column's top cell is
        // grid (3,3) → (24, 5 + 24). (`iter_at(origin)` is the world-space view;
        // the origin itself is the blob's bounding-box top-left.)
        assert_eq!(column.origin, Vec2::new(24, 29));
        assert!(
            column
                .sprite
                .iter_at(column.origin)
                .any(|(pos, _)| pos == Vec2::new(24, 29))
        );
        // Draw metadata rides onto the component from the layer.
        assert_eq!(l_blob.transparent, Some(0));
    }

    /// The same cells bridged into one 4-connected shape flood-fill into a single
    /// component.
    #[test]
    fn flood_fill_merges_touching_cells() {
        // A connected shape: (1,1)-(0,1)-(0,2)-(0,3)-(0,4).
        let cells = [(1, 1), (0, 1), (0, 2), (0, 3), (0, 4)];
        let info = info_from_layers(
            vec![
                plane_tile_layer("collision", 5, 5, vec![0; 25], None, 0.0),
                plane_tile_layer(
                    "props",
                    5,
                    5,
                    occupied_grid(5, 5, &cells),
                    Some(Plane::Sprite),
                    0.0,
                ),
            ],
            5,
            5,
        );
        assert_eq!(info.sprite_components.len(), 1, "touching cells merge");
        assert_eq!(info.sprite_components[0].sprite.cells.len(), 5);
        assert_eq!(info.sprite_components[0].baseline, (4 + 1) * 8);
    }

    /// Two sprite layers each holding a cell at the *same* grid position stay two
    /// components — each layer flood-fills on its own, so overlapping props on
    /// separate layers never merge (the authoring escape hatch).
    #[test]
    fn flood_fill_never_merges_across_sprite_layers() {
        let info = info_from_layers(
            vec![
                plane_tile_layer("collision", 3, 3, vec![0; 9], None, 0.0),
                plane_tile_layer(
                    "a",
                    3,
                    3,
                    occupied_grid(3, 3, &[(1, 1)]),
                    Some(Plane::Sprite),
                    0.0,
                ),
                plane_tile_layer(
                    "b",
                    3,
                    3,
                    occupied_grid(3, 3, &[(1, 1)]),
                    Some(Plane::Sprite),
                    0.0,
                ),
            ],
            3,
            3,
        );
        assert_eq!(info.sprite_layers.len(), 2);
        assert_eq!(
            info.sprite_components.len(),
            2,
            "overlapping cells on different layers don't merge"
        );
    }

    /// A `plane=sprite` layer survives a `to_tmj` → `from_json` round-trip: the
    /// re-parsed map still classifies it as a sprite plane with its component.
    #[test]
    fn plane_property_round_trips_through_tmj() {
        use crate::data::tiled::{Tileset, from_json};
        let console = TestConsole::new();
        let map = TiledMap {
            width: 2,
            height: 2,
            layers: vec![
                plane_tile_layer("collision", 2, 2, vec![0; 4], None, 0.0),
                plane_tile_layer(
                    "props",
                    2,
                    2,
                    occupied_grid(2, 2, &[(0, 0)]),
                    Some(Plane::Sprite),
                    0.0,
                ),
                TiledMapLayer::ObjectLayer(ObjectLayer {
                    name: "obj".to_string(),
                    objects: Vec::new(),
                }),
            ],
            tilesets: vec![Tileset {
                firstgid: 1,
                source: "tiles.tsj".to_string(),
            }],
            properties: Vec::new(),
        };
        let json = map.to_tmj(&map.parse_objects());
        let reparsed = from_json(json.as_bytes()).unwrap();
        let mut store = MapStore::default();
        store.insert("m", reparsed);
        let info = map_by_name(&console.indexed_sprites, "m", &store).unwrap();
        assert_eq!(info.sprite_layers.len(), 1, "sprite plane survives save/load");
        assert_eq!(info.sprite_components.len(), 1);
        assert_eq!(info.sprite_components[0].sprite.cells.len(), 1);
    }

    /// The y-sort tie rule: replaying `draw_world`'s keyed list (components pushed
    /// before entities, then a stable sort) an entity draws in front of a
    /// component when its feet are below the baseline *or exactly on it*, and
    /// behind when its feet are above.
    #[test]
    fn sprite_component_sorts_against_entity_feet() {
        let component = SpriteComponent {
            source_layer: 0,
            origin: Vec2::new(0, 0),
            sprite: MetaSprite::default(),
            baseline: 100,
            transparent: Some(0),
            palette_rotate: 0,
        };
        let cam_y = 0;
        let order = |feet: i32| -> Vec<&'static str> {
            let mut list: Vec<(i32, &'static str)> = Vec::new();
            // Component first (matches `draw_world`), then the entity.
            list.push((component.sort_key(cam_y), "component"));
            let entity = DrawParams::new(0, 0, feet - 8, SpriteOptions::default(), None, 0);
            assert_eq!(entity.bottom(), feet, "1×1 sprite bottom = y + 8");
            list.push((entity.bottom(), "entity"));
            list.sort_by_key(|(k, _)| *k); // stable, like the draw loop
            list.into_iter().map(|(_, name)| name).collect()
        };
        assert_eq!(order(120), vec!["component", "entity"], "feet below → in front");
        assert_eq!(order(80), vec!["entity", "component"], "feet above → behind");
        assert_eq!(
            order(100),
            vec!["component", "entity"],
            "feet on the baseline → entity in front (tie)"
        );
    }

    /// Hiding a sprite layer (the editor's eye toggle flips `LayerInfo.visible`
    /// without a reload) drops its components from the live draw list, while a
    /// visible layer's components stay — the filter is applied at draw time.
    #[test]
    fn hidden_sprite_layer_excluded_from_draw() {
        let mut info = info_from_layers(
            vec![
                plane_tile_layer("collision", 3, 3, vec![0; 9], None, 0.0),
                plane_tile_layer(
                    "a",
                    3,
                    3,
                    occupied_grid(3, 3, &[(0, 0)]),
                    Some(Plane::Sprite),
                    0.0,
                ),
                plane_tile_layer(
                    "b",
                    3,
                    3,
                    occupied_grid(3, 3, &[(2, 2)]),
                    Some(Plane::Sprite),
                    0.0,
                ),
            ],
            3,
            3,
        );
        // Both layers visible → both components draw.
        assert_eq!(info.sprite_components.len(), 2);
        assert_eq!(info.visible_sprite_components().count(), 2);
        // Hide layer "a" (source_layer 1), exactly as the eye toggle does.
        for layer in info.sprite_layers.iter_mut() {
            if layer.source_layer == 1 {
                layer.visible = false;
            }
        }
        // Only layer "b"'s component (source_layer 2) remains in the draw list;
        // the components themselves are untouched (no reload needed).
        let drawn: Vec<usize> = info
            .visible_sprite_components()
            .map(|c| c.source_layer)
            .collect();
        assert_eq!(drawn, vec![2]);
        assert_eq!(info.sprite_components.len(), 2, "derive is untouched");
    }

    /// The constructors set the trigger from the effect kind: warps default to
    /// `Any` (walk-in or press), interactions (dialogue / func / sprite-only
    /// None) to `Press`. This is what preserves pre-trigger behaviour.
    #[test]
    fn constructors_set_effect_kind_default_trigger() {
        let hb = Hitbox::new(0, 0, 8, 8);
        assert_eq!(
            MapObject::warp(hb, Warp::new(None, Vec2::new(0, 0))).trigger,
            Trigger::Any
        );
        assert_eq!(MapObject::warp_tile(0, 0, None, 1, 1).trigger, Trigger::Any);
        assert_eq!(MapObject::dialogue(hb, "k").trigger, Trigger::Press);
        assert_eq!(
            MapObject::func(hb, InteractFn::ToggleDog).trigger,
            Trigger::Press
        );
        // A bare Interact::None (sprite-only objects) is press-default too.
        assert_eq!(
            MapObject::new(hb, ObjectEffect::Interact(Interaction::None), None).trigger,
            Trigger::Press
        );
        // `with_trigger` overrides it.
        assert_eq!(
            MapObject::dialogue(hb, "k")
                .with_trigger(Trigger::Touch)
                .trigger,
            Trigger::Touch
        );
    }

    /// `default_for` is the single source of truth both the constructors and the
    /// `.tmj` "serialise only non-default" rule lean on.
    #[test]
    fn trigger_default_for_matches_constructors() {
        assert_eq!(
            Trigger::default_for(&ObjectEffect::Warp(Warp::new(None, Vec2::new(0, 0)))),
            Trigger::Any
        );
        assert_eq!(
            Trigger::default_for(&ObjectEffect::Interact(Interaction::None)),
            Trigger::Press
        );
    }

    /// `warps_overlapping` / `warp_landing_conflict` flag a landing whose player
    /// hitbox would sit on a warp's trigger (an instant re-warp), ignore
    /// non-warp objects, and read clear when the landing misses every warp.
    #[test]
    fn warp_landing_conflict_detects_overlap() {
        // A 7x5 player box (the modern flush player hitbox) and a map with one
        // warp trigger at (40,40)+16x16 and a dialogue object at (0,0).
        let player = Hitbox::new(0, 0, 7, 5);
        let map = MapInfo {
            objects: vec![
                MapObject::warp(Hitbox::new(40, 40, 16, 16), Warp::new(Some("elsewhere"), Vec2::new(0, 0))),
                MapObject::dialogue(Hitbox::new(40, 40, 16, 16), "chat"),
            ],
            ..Default::default()
        };

        // Landing whose player box (44..51 , 44..49) lands inside the warp.
        assert_eq!(map.warp_landing_conflict(Vec2::new(44, 44), player), Some(0));
        // Just-touching the warp's top-left edge still counts (inclusive bounds).
        assert_eq!(map.warps_overlapping(player.offset(Vec2::new(40, 40))).next(), Some(0));
        // Clear landing far from the warp.
        assert_eq!(map.warp_landing_conflict(Vec2::new(0, 0), player), None);
        // The dialogue object (index 1) is never reported, even when overlapped.
        assert_eq!(
            map.warps_overlapping(Hitbox::new(40, 40, 16, 16)).collect::<Vec<_>>(),
            vec![0],
            "only warp objects count, not the co-located dialogue"
        );
    }

    /// The interaction firing rule is the truth table over `(trigger, touched,
    /// was_inside, probed)`: touch is *edge-triggered* (fires only on entering),
    /// press is level-triggered, and `Any` is their union.
    #[test]
    fn interaction_firing_truth_table() {
        // Press-only: ignores touch entirely, fires iff probed.
        assert!(!Trigger::Press.interaction_fires(true, false, false));
        assert!(Trigger::Press.interaction_fires(false, false, true));
        assert!(Trigger::Press.interaction_fires(true, true, true));

        // Touch-only edge: fires on *entering* (touched && !was_inside), not while
        // standing in it (touched && was_inside), and never on a press alone.
        assert!(Trigger::Touch.interaction_fires(true, false, false));
        assert!(!Trigger::Touch.interaction_fires(true, true, false));
        assert!(!Trigger::Touch.interaction_fires(false, false, true));

        // Any: union — entering fires, a press fires, but standing still (no
        // press) does not re-fire.
        assert!(Trigger::Any.interaction_fires(true, false, false));
        assert!(Trigger::Any.interaction_fires(false, false, true));
        assert!(!Trigger::Any.interaction_fires(true, true, false));
    }

    /// The warp firing rule truth table over `(trigger, touched, probed, mode,
    /// manual_doors)`: touch is level-triggered and suppressed only when the
    /// player opted into manual doors *and* the mode is `Interact`; the press
    /// path is never suppressed; `Auto` warps always keep their touch path.
    #[test]
    fn warp_firing_truth_table() {
        use WarpMode::{Auto, Interact};
        // manual_doors == false (the default): behaves as before — touch or press
        // fires regardless of mode.
        assert!(Trigger::Any.warp_fires(true, false, &Interact, false));
        assert!(Trigger::Any.warp_fires(true, false, &Auto, false));
        assert!(Trigger::Any.warp_fires(false, true, &Interact, false));

        // manual_doors == true: an Interact-mode warp's touch path is suppressed,
        // but its press path still fires.
        assert!(!Trigger::Any.warp_fires(true, false, &Interact, true));
        assert!(Trigger::Any.warp_fires(false, true, &Interact, true));
        // Auto-mode warps keep their touch path even with manual doors on.
        assert!(Trigger::Any.warp_fires(true, false, &Auto, true));

        // A Touch-only warp with manual doors + Interact mode can't fire at all
        // (its only path is suppressed and it has no press path).
        assert!(!Trigger::Touch.warp_fires(true, true, &Interact, true));
        // A Press-only warp ignores touch entirely (mode/manual_doors irrelevant).
        assert!(!Trigger::Press.warp_fires(true, false, &Auto, false));
        assert!(Trigger::Press.warp_fires(false, true, &Interact, true));
    }

    // --- Image layers ---------------------------------------------------------

    use crate::data::tiled::ImageLayer;
    use egg_render::image::{Rgba, RgbaImage};

    /// An [`ImageLayer`] with `pixels` already attached, at the given offset and
    /// optionally flagged as a collision mask (by name) — the fixture the
    /// image-layer `MapInfo` tests build maps from.
    fn image_layer(name: &str, pixels: RgbaImage, offsetx: f64, offsety: f64) -> ImageLayer {
        ImageLayer {
            name: name.to_string(),
            image: "m.png".to_string(),
            offsetx,
            offsety,
            visible: true,
            opacity: 1.0,
            properties: Vec::new(),
            pixels: Some(pixels),
        }
    }

    /// An RGBA image with a single fully-opaque pixel at (`x`, `y`) on an
    /// otherwise transparent field — the smallest probe for collider derivation.
    fn one_solid_pixel(w: u32, h: u32, x: u32, y: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        img.set_pixel(x, y, Rgba::new(255, 255, 255, 255));
        img
    }

    /// `painted_colliders` turns alpha ≥ [`PAINTED_SOLID_ALPHA`] into solid cells
    /// and alpha below it into passable ones, including values that straddle the
    /// 128 threshold — the antialiasing tolerance band.
    #[test]
    fn painted_colliders_threshold_straddles_128() {
        // A single 8×8 cell with four pixels at telling alphas.
        let mut img = RgbaImage::new(8, 8);
        img.set_pixel(0, 0, Rgba::new(0, 0, 0, 127)); // just below → passable
        img.set_pixel(1, 0, Rgba::new(0, 0, 0, 128)); // exactly at → solid
        img.set_pixel(2, 0, Rgba::new(0, 0, 0, 255)); // opaque → solid
        img.set_pixel(3, 0, Rgba::new(0, 0, 0, 0)); // transparent → passable
        let colliders = painted_colliders(&img, 1, 1);
        assert_eq!(colliders.len(), 1);
        let c = &colliders[0];
        assert!(!c.get(0, 0), "alpha 127 is below the threshold");
        assert!(c.get(1, 0), "alpha 128 meets the threshold");
        assert!(c.get(2, 0), "alpha 255 is solid");
        assert!(!c.get(3, 0), "alpha 0 is transparent");
    }

    /// Collider derivation tiles the image into a row-major 8×8 grid: a solid
    /// pixel in the second cell across lands in collider index 1, at the right
    /// in-cell coordinate. Sizing rounds up so a partial last cell still exists.
    #[test]
    fn painted_colliders_cell_alignment() {
        // 16×8 image (2×1 cells); one solid pixel at (9, 2) → cell (1, 0),
        // in-cell (1, 2).
        let img = one_solid_pixel(16, 8, 9, 2);
        let colliders = painted_colliders(&img, 2, 1);
        assert_eq!(colliders.len(), 2);
        assert!(!colliders[0].get(1, 2), "the first cell is empty");
        assert!(
            colliders[1].get(1, 2),
            "the solid pixel lands in cell index 1"
        );

        // A 9×9 image rounds up to a 2×2 grid (the stray 9th row/col gets cells).
        let big = one_solid_pixel(9, 9, 8, 8);
        let grid = painted_colliders(&big, 2, 2);
        assert_eq!(grid.len(), 4);
        assert!(
            grid[3].get(0, 0),
            "the stray pixel at (8,8) is cell (1,1) in-cell (0,0)"
        );
    }

    /// A collision-marked image layer builds an **invisible**, collider-bearing
    /// [`LayerInfo`] of [`LayerKind::Image`], sized to the image and placed at its
    /// pixel offset — the seam where "collision masks are never drawn" is the
    /// `visible: false` flag the draw loop honours.
    #[test]
    fn collision_image_layer_is_invisible_with_colliders() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        // 16×8 mask with one solid pixel, marked collision by name, at offset (8, 0).
        let mask = image_layer("collision", one_solid_pixel(16, 8, 0, 0), 8.0, 0.0);
        store.insert(
            "painted",
            TiledMap {
                width: 2,
                height: 1,
                layers: vec![
                    TiledMapLayer::ImageLayer(mask),
                    TiledMapLayer::ObjectLayer(ObjectLayer {
                        name: "obj".to_string(),
                        objects: Vec::new(),
                    }),
                ],
                tilesets: Vec::new(),
                properties: Vec::new(),
            },
        );
        let info = map_by_name(&console.indexed_sprites, "painted", &store).unwrap();
        // The mask is the only (bg) layer; no fg layers; no tile collision layer.
        assert_eq!(info.layers.len(), 1);
        let layer = &info.layers[0];
        assert_eq!(layer.kind, LayerKind::Image);
        assert!(!layer.visible, "a collision mask is never drawn");
        assert_eq!(layer.offset, Vec2::new(8, 0));
        assert_eq!(layer.size, Vec2::new(2, 1)); // 16×8 px → 2×1 tiles
        assert_eq!(layer.colliders.len(), 2);
        assert!(layer.colliders[0].get(0, 0), "the painted pixel is solid");
    }

    /// A *plain* (non-collision) image layer is drawn: visible, image-kind, no
    /// colliders, split by the `fg` prefix like a tile layer. A propertyless,
    /// plainly-named layer goes to the bg list; an `fg`-named one goes to fg.
    #[test]
    fn plain_image_layers_draw_and_split_fg() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        let bg = image_layer("background", RgbaImage::new(8, 8), 0.0, 0.0);
        let fg = image_layer("fg_overlay", RgbaImage::new(8, 8), 0.0, 0.0);
        store.insert(
            "art",
            TiledMap {
                width: 1,
                height: 1,
                layers: vec![
                    TiledMapLayer::ImageLayer(bg),
                    TiledMapLayer::ImageLayer(fg),
                    TiledMapLayer::ObjectLayer(ObjectLayer {
                        name: "obj".to_string(),
                        objects: Vec::new(),
                    }),
                ],
                tilesets: Vec::new(),
                properties: Vec::new(),
            },
        );
        let info = map_by_name(&console.indexed_sprites, "art", &store).unwrap();
        assert_eq!(info.layers.len(), 1, "the bg image is a bg layer");
        assert_eq!(
            info.fg_layers.len(),
            1,
            "the fg-prefixed image is a fg layer"
        );
        assert!(info.layers[0].visible);
        assert_eq!(info.layers[0].kind, LayerKind::Image);
        assert!(
            info.layers[0].colliders.is_empty(),
            "a drawn layer has no colliders"
        );
        assert!(info.fg_layers[0].visible);
    }

    /// A pure-painted map — *no tile layers at all* — builds a valid `MapInfo`
    /// whose layers size from the map's declared `width`/`height` fallback (there
    /// is no tile layer 0 to read), and whose collision comes only from the
    /// painted mask.
    #[test]
    fn no_tile_layer_map_dimensions_and_collision() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        // One visible bg image + one collision mask + an object layer. No tiles.
        let mask = image_layer("collision", one_solid_pixel(8, 8, 0, 0), 0.0, 0.0);
        store.insert(
            "pure",
            TiledMap {
                width: 1,
                height: 1,
                layers: vec![
                    TiledMapLayer::ImageLayer(image_layer("bg", RgbaImage::new(8, 8), 0.0, 0.0)),
                    TiledMapLayer::ImageLayer(mask),
                    TiledMapLayer::ObjectLayer(ObjectLayer {
                        name: "obj".to_string(),
                        objects: Vec::new(),
                    }),
                ],
                tilesets: Vec::new(),
                properties: Vec::new(),
            },
        );
        let info = map_by_name(&console.indexed_sprites, "pure", &store).unwrap();
        // bg image + collision mask both in the bg list, no tile collision layer.
        assert_eq!(info.layers.len(), 2);
        assert!(info.layers.iter().all(|l| l.kind == LayerKind::Image));
        // The collision mask blocks the painted pixel via the bitmap path.
        let mask_layer = info.layers.iter().find(|l| !l.visible).unwrap();
        assert!(
            layer_collides(Vec2::new(0, 0), mask_layer),
            "the painted pixel at (0,0) collides"
        );
        // A point well outside the mask doesn't.
        assert!(!layer_collides(Vec2::new(7, 0), mask_layer));
    }

    /// A collision mask placed at a non-tile-aligned offset still collides at the
    /// right *world* pixel: the painted pixel at image (0,0) of a mask offset by
    /// (−36, −16) blocks world (−36, −16), not world (0,0).
    #[test]
    fn painted_collision_respects_nonaligned_offset() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        let mask = image_layer("collision", one_solid_pixel(8, 8, 0, 0), -36.0, -16.0);
        store.insert(
            "offset",
            TiledMap {
                width: 1,
                height: 1,
                layers: vec![
                    TiledMapLayer::ImageLayer(mask),
                    TiledMapLayer::ObjectLayer(ObjectLayer {
                        name: "obj".to_string(),
                        objects: Vec::new(),
                    }),
                ],
                tilesets: Vec::new(),
                properties: Vec::new(),
            },
        );
        let info = map_by_name(&console.indexed_sprites, "offset", &store).unwrap();
        let layer = &info.layers[0];
        assert_eq!(layer.offset, Vec2::new(-36, -16));
        // The solid pixel is at the mask's top-left → world (−36, −16).
        assert!(layer_collides(Vec2::new(-36, -16), layer));
        // World (0, 0) is 36px right / 16px down into transparent mask area.
        assert!(!layer_collides(Vec2::new(0, 0), layer));
    }

    /// A collision image layer whose pixels never arrived derives **empty**
    /// (size 0×0, no colliders) rather than panicking — the missing-PNG path.
    #[test]
    fn collision_image_layer_without_pixels_is_empty() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        let mask = ImageLayer {
            name: "collision".to_string(),
            image: "missing.png".to_string(),
            offsetx: 0.0,
            offsety: 0.0,
            visible: true,
            opacity: 1.0,
            properties: Vec::new(),
            pixels: None, // never attached
        };
        store.insert(
            "broken",
            TiledMap {
                width: 1,
                height: 1,
                layers: vec![
                    TiledMapLayer::ImageLayer(mask),
                    TiledMapLayer::ObjectLayer(ObjectLayer {
                        name: "obj".to_string(),
                        objects: Vec::new(),
                    }),
                ],
                tilesets: Vec::new(),
                properties: Vec::new(),
            },
        );
        let info = map_by_name(&console.indexed_sprites, "broken", &store).unwrap();
        let layer = &info.layers[0];
        assert_eq!(layer.size, Vec2::new(0, 0));
        assert!(layer.colliders.is_empty());
        // Collision is a clean no-op (the layer hitbox is empty).
        assert!(!layer_collides(Vec2::new(0, 0), layer));
    }

    /// The real bedroom1 builds a `MapInfo` without panicking now that image
    /// layers parse: its tile collision layer stays invisible, and its painted
    /// walls — drawn art, not a collision mask — join the bg list as an image
    /// layer (whether it *draws* is the file's `visible`, live authoring state
    /// the user toggles in Tiled, so it isn't asserted). (Its office-style
    /// collision tile ids sit high in the real sheet, past the blank test
    /// sheet's end, so [`Collider::from_sprite`]'s bounds guard derives them
    /// empty here; the real sheet covers them in-game. house_stairwell's *parse*
    /// is covered in `tmj.rs`, and `is_modern` over an image-only map is checked
    /// below.)
    #[test]
    fn real_bedroom1_builds_map_info_with_image_layer() {
        let console = TestConsole::new();
        let mut store = MapStore::default();
        let bytes = std::fs::read("../../assets/maps/bedroom1.tmj").unwrap();
        store.insert("bedroom1", crate::data::tiled::from_json(&bytes).unwrap());
        let bedroom = map_by_name(&console.indexed_sprites, "bedroom1", &store).unwrap();
        assert!(
            !bedroom.layers[0].visible,
            "tile layer 0 is the collision layer"
        );
        assert!(
            bedroom.layers.iter().any(|l| l.kind == LayerKind::Image),
            "the painted walls are an image bg layer"
        );
    }

    /// A map with an image layer but **no object layer** still counts as modern,
    /// so a pure-painted map resolves through [`map_by_name`] — `house_stairwell`'s
    /// `.tmj` (which carries a tracing-mask image layer) is the live example.
    /// (Stored under a non-legacy key here so the probe is purely about
    /// [`MapStore::is_modern`], independent of the modern-first resolution order
    /// in [`map_by_name`].)
    #[test]
    fn image_only_map_is_modern() {
        let mut store = MapStore::default();
        let bytes = std::fs::read("../../assets/maps/house_stairwell.tmj").unwrap();
        store.insert("painted_only", crate::data::tiled::from_json(&bytes).unwrap());
        assert!(
            store.is_modern("painted_only"),
            "an image layer alone marks a map modern (no object layer needed)"
        );
    }
}
