use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::animation::AnimFrame;
use crate::system::MapOptions;
use crate::{
    camera::CameraBounds,
    data::{
        sound::{SfxData, music::MusicTrack},
        tmj::{ImageLayer, TiledMap, TiledMapLayer},
    },
    interact::{InteractFn, Interaction},
    position::{Collider, Hitbox, Vec2},
    system::drawing::image::{IndexedImage, RgbaImage},
};

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
    /// Drop a map from the store (its `.tmj` on disk is left untouched — the
    /// editor removes the name from the manifest so it won't reload).
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
/// sheet that lives on [`crate::drawstate::DrawState`]).
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
            layer.width.try_into().unwrap(),
            layer.height.try_into().unwrap(),
        ),
        _ => (
            map.width.try_into().unwrap_or(0),
            map.height.try_into().unwrap_or(0),
        ),
    };

    let mut layers = Vec::new();
    let mut fg_layers = Vec::new();
    let mut seen_collision_tiles = false;
    for (i, layer) in map.layers.iter().enumerate() {
        match layer {
            // The first tile layer is the collision layer: invisible, colliders
            // from the sprite art. Later tile layers draw, bg/fg by `fg` prefix.
            TiledMapLayer::TileLayer(tile_layer) => {
                if !seen_collision_tiles {
                    seen_collision_tiles = true;
                    layers.push(collision_tile_layer(indexed_sprites, map, i, width, height));
                    continue;
                }
                let info = LayerInfo {
                    origin: Vec2::new(0, 0),
                    size: Vec2::new(
                        tile_layer.width.try_into().unwrap(),
                        tile_layer.height.try_into().unwrap(),
                    ),
                    offset: Vec2::new(tile_layer.offsetx as i16, tile_layer.offsety as i16),
                    source_layer: i,
                    transparent: Some(0),
                    palette_rotate: tile_layer.palette_rotate(),
                    ..LayerInfo::DEFAULT_LAYER
                };
                push_bg_or_fg(&mut layers, &mut fg_layers, info, &tile_layer.name);
            }
            // A collision image layer is invisible data; a plain one draws.
            TiledMapLayer::ImageLayer(image) => {
                if image.is_collision() {
                    layers.push(painted_collision_layer(image, i));
                } else {
                    push_bg_or_fg(
                        &mut layers,
                        &mut fg_layers,
                        image_draw_layer(image, i),
                        &image.name,
                    );
                }
            }
            TiledMapLayer::ObjectLayer(_) => {}
        }
    }

    let objects = map.parse_objects();
    MapInfo {
        layers,
        fg_layers,
        objects,
        bg_colour: map.bg_colour().unwrap_or(0),
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

/// Push a built [`LayerInfo`] into the bg or fg list by the Tiled layer-name
/// `fg` prefix (case-insensitive) — the one convention shared by tile and image
/// draw layers, so an `fg`-named painted overlay sits above sprites just like an
/// `fg` tile layer does.
fn push_bg_or_fg(
    layers: &mut Vec<LayerInfo>,
    fg_layers: &mut Vec<LayerInfo>,
    info: LayerInfo,
    name: &str,
) {
    if name.to_lowercase().starts_with("fg") {
        fg_layers.push(info);
    } else {
        layers.push(info);
    }
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

/// Metadata necessary to load a map into Walkaround.
#[derive(Clone, Debug, Default)]
pub struct MapInfo {
    pub layers: Vec<LayerInfo>,
    pub fg_layers: Vec<LayerInfo>,
    /// The map's triggerable objects (warps + interactions) in one ordered
    /// list — the walk loop scans them in vector order, so order is gameplay.
    pub objects: Vec<MapObject>,
    pub bg_colour: u8,
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
    /// A single bitmap blit from the map's [`ImageLayer`](crate::data::tmj::ImageLayer)
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
    // pub display_mode: BG, FG, Object
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
        draw_state: &mut crate::drawstate::DrawState,
        layer: crate::drawstate::LayerId,
        map: &TiledMap,
        offset: Vec2,
    ) {
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
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

/// How a [`MapObject`] is triggered — the *authored geometry* half of the firing
/// decision (the map author's intent for this object), independent of the effect
/// kind and of any player preference:
/// - [`Touch`](Self::Touch) — fires only when the player's body overlaps the
///   hitbox (a step-on trigger);
/// - [`Press`](Self::Press) — fires only when the player presses the interact
///   button while facing into the hitbox;
/// - [`Any`](Self::Any) — fires on either path.
///
/// Defaults preserve the historical effect-driven behaviour and are set by the
/// constructors, not by `Default`: warps default to [`Any`](Self::Any) (a door
/// you can walk into or press), interactions to [`Press`](Self::Press) (a sign
/// you must face and read). See [`MapObject`] for how this composes with the
/// effect kind, the warp [`WarpMode`], and warp narration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Trigger {
    /// Fires on body-touch only.
    Touch,
    /// Fires on a facing-direction interact press only.
    #[default]
    Press,
    /// Fires on either body-touch or a facing press.
    Any,
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
    /// ([`TiledObject::id`](crate::data::tmj::TiledObject::id)), carried through
    /// the parse so a removable object can be recorded durably in the save's
    /// `taken` set (a positional index would shift when a sibling is added or
    /// removed). `None` for a runtime/editor-created object that has no id yet;
    /// the map writer then assigns it a fresh one above every existing id on the
    /// next save, so survivors never renumber.
    pub id: Option<usize>,
    /// Whether interacting with this object *consumes* it: a pickup that vanishes
    /// once taken and stays gone. On interaction the engine records it (by
    /// [`id`](Self::id)) in the save's `taken` set and drops it from the live map;
    /// every later load of this map filters it back out (see
    /// [`load_map_by_name`](crate::gamestate::walkaround::WalkaroundState::load_map_by_name)).
    /// Authored as a `removable` object property; only meaningful for interaction
    /// objects (warps fire on touch and are never "taken").
    pub removable: bool,
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
    /// Override the trigger axis (touch / press / either), replacing the
    /// effect-kind default the constructor picked.
    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
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
    if layer.size.x <= 0 {
        return false;
    }
    layer
        .colliders
        .get((map_point.x % layer.size.x) as usize + (map_point.y * layer.size.x) as usize)
        .map(|collider| collider.get(px, py))
        .unwrap_or_default()
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

    use crate::data::tmj::ImageLayer;
    use crate::system::drawing::image::{Rgba, RgbaImage};

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
        let bytes = std::fs::read("../assets/maps/bedroom1.tmj").unwrap();
        store.insert("bedroom1", crate::data::tmj::from_json(&bytes).unwrap());
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
        let bytes = std::fs::read("../assets/maps/house_stairwell.tmj").unwrap();
        store.insert("painted_only", crate::data::tmj::from_json(&bytes).unwrap());
        assert!(
            store.is_modern("painted_only"),
            "an image layer alone marks a map modern (no object layer needed)"
        );
    }
}
