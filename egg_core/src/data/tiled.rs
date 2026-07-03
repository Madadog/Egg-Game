//! Codec for Tiled's JSON map format (`.tmj`): parsing, the object-layer ↔
//! runtime [`MapObject`] mapping, and re-serialisation for the in-game editor.
//! Lives in `egg_core` so every host shares one map model; hosts only wrap
//! [`TiledMap`] for their own asset pipelines.
//!
//! Each Tiled object becomes one [`MapObject`]: a `type == "warp"` (or warp
//! properties) object an [`ObjectEffect::Warp`], a `description`-carrying object
//! an [`ObjectEffect::Interact`] dialogue. Parsing preserves file order so a
//! hand-mixed object layer keeps its interleaving across a save round-trip;
//! serialisation walks the one objects list in order.

use crate::world::animation::AnimFrame;
use crate::data::sound::{self, SfxData};
use crate::world::interact::{InteractFn, Interaction};
use crate::world::map::{
    Axis, Gate, LayerInfo, MapObject, ObjectEffect, Plane, Trigger, Warp, WarpMode,
};
use crate::geometry::{Hitbox, Vec2};
use crate::render::SpriteOptions;
use crate::render::Rotate;
use crate::render::image::RgbaImage;
use crate::render::Flip;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Parse Tiled JSON into a [`TiledMap`] with sheet-local (flattened) tile ids.
/// The single entry point hosts share, so every loaded map lands in the same
/// id space the engine draws and edits in.
pub fn from_json(bytes: &[u8]) -> Result<TiledMap, serde_json::Error> {
    let mut map: TiledMap = serde_json::from_slice(bytes)?;
    map.flatten_gids();
    Ok(map)
}

/// Parse the game asset manifest (`assets/game.manifest`) into a [`GameManifest`].
/// JSON content, but a bespoke extension so it doesn't collide with the script
/// loader (which owns `.json`); the host reads the bytes and calls this, just
/// like [`from_json`] for maps.
pub fn manifest_from_json(bytes: &[u8]) -> Result<GameManifest, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Serialise a [`GameManifest`] back to its `assets/game.manifest` JSON, for the
/// in-editor map CRUD (which appends/removes map stems as maps are created and
/// deleted). Pretty-printed to stay human-diffable like the hand-authored file.
pub fn manifest_to_json(manifest: &GameManifest) -> String {
    serde_json::to_string_pretty(manifest).unwrap_or_else(|_| "{\"maps\":[]}".to_string())
}

/// The game's asset manifest: the data-driven list of what to load at boot,
/// replacing a hardcoded set of map paths in the host. Each entry is a **base
/// name** (file stem), not a path — the host expands `maps/<name>.tmj`, and
/// stores each loaded map in the [`crate::world::map::MapStore`] under that same stem
/// (which is also the name the in-game editor saves back to). Shaped to grow:
/// new asset categories become new fields, and the list defaults to empty so a
/// partial manifest still parses.
///
/// Serialised as JSON in `assets/game.manifest`:
/// ```json
/// { "maps": ["office", "town", ...] }
/// ```
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GameManifest {
    /// Map file stems to load (`maps/<name>.tmj`). The order is the load order;
    /// the names become the [`crate::world::map::MapStore`] keys.
    #[serde(default)]
    pub maps: Vec<String>,
}

/// A typed Tiled custom property (`{ name, type, value }`) read for round-trip
/// fidelity. Unlike the kind-specific [`ObjectProperties`] (string) and
/// [`ImageLayerProperty`] (bool), this keeps the raw `value` as a JSON
/// [`Value`] and carries the Tiled `type` tag, so any property — the int-valued
/// `palette_rotate` on a tile layer, the int `bg_colour` or string
/// `camera_stick` at map level — parses and re-serialises unchanged. The engine
/// only consumes a handful by name; the rest survive an in-game save verbatim.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Property {
    pub name: String,
    #[serde(rename = "type", default = "default_property_type")]
    pub r#type: String,
    pub value: Value,
}
impl Property {
    /// This property's integer value, if it is one (Tiled `int` properties carry
    /// a JSON number).
    pub fn as_int(&self) -> Option<i64> {
        self.value.as_i64()
    }
    /// This property's string value, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        self.value.as_str()
    }
    /// This property's floating-point value, if it is numeric (Tiled `float`).
    pub fn as_float(&self) -> Option<f64> {
        self.value.as_f64()
    }
    /// An `int` property.
    pub fn int(name: &str, value: i64) -> Self {
        Self {
            name: name.to_string(),
            r#type: "int".to_string(),
            value: Value::from(value),
        }
    }
    /// A `string` property.
    pub fn string(name: &str, value: &str) -> Self {
        Self {
            name: name.to_string(),
            r#type: "string".to_string(),
            value: Value::from(value),
        }
    }
}

/// Look up `name` in a property list and read it as an integer.
fn property_int(properties: &[Property], name: &str) -> Option<i64> {
    properties
        .iter()
        .find(|p| p.name == name)
        .and_then(Property::as_int)
}

/// Look up `name` in a property list and read it as a string.
fn property_str<'a>(properties: &'a [Property], name: &str) -> Option<&'a str> {
    properties
        .iter()
        .find(|p| p.name == name)
        .and_then(Property::as_str)
}

/// Look up `name` in a property list and read it as a float.
fn property_float(properties: &[Property], name: &str) -> Option<f64> {
    properties
        .iter()
        .find(|p| p.name == name)
        .and_then(Property::as_float)
}

/// Serialise a property list to Tiled's `[{ name, type, value }, …]` array, for
/// the tile-layer and map-level `properties` echoes in [`TiledMap::to_tmj`].
fn properties_to_json(properties: &[Property]) -> Value {
    Value::Array(
        properties
            .iter()
            .map(|p| json!({ "name": p.name, "type": p.r#type, "value": p.value }))
            .collect(),
    )
}

/// Serde default for an absent property `type` tag (Tiled defaults untyped
/// properties to `string`).
fn default_property_type() -> String {
    "string".to_string()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TileLayer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<usize>,
    pub name: String,
    /// Horizontal pixel placement (Tiled stores it as a JSON number; can be
    /// negative). Absent ⇒ 0. Mirrors [`ImageLayer::offsetx`] so an offset tile
    /// layer (e.g. a bed nudged a few pixels) round-trips instead of snapping to
    /// the grid on save.
    #[serde(default)]
    pub offsetx: f64,
    /// Vertical pixel placement. Absent ⇒ 0.
    #[serde(default)]
    pub offsety: f64,
    /// Custom properties — read only for the int `palette_rotate` (a per-layer
    /// palette rotation). Round-tripped verbatim.
    #[serde(default)]
    pub properties: Vec<Property>,
}
impl TileLayer {
    pub fn get(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.width {
            return None; // guard the row wraparound past the right edge
        }
        self.data.get(y.checked_mul(self.width)? + x).copied()
    }
    /// This layer's `palette_rotate` property (the per-layer palette rotation
    /// fed into [`LayerInfo::palette_rotate`](crate::world::map::LayerInfo::palette_rotate)),
    /// or 0 if absent.
    pub fn palette_rotate(&self) -> u8 {
        property_int(&self.properties, "palette_rotate")
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(0)
    }
    /// Set the per-layer `palette_rotate`. `0` drops the property (Tiled omits a
    /// default), so a plain layer round-trips without an empty property list.
    pub fn set_palette_rotate(&mut self, rotate: u8) {
        self.properties.retain(|p| p.name != "palette_rotate");
        if rotate != 0 {
            self.properties.push(Property {
                name: "palette_rotate".to_string(),
                r#type: "int".to_string(),
                value: Value::from(rotate),
            });
        }
    }
    /// This layer's draw [`Plane`]: the `plane` property if present, else the
    /// name-based fallback ([`Plane::from_name`]). Read at load to route the
    /// layer into the bg / sprite / fg lists.
    pub fn plane(&self) -> Plane {
        property_str(&self.properties, "plane")
            .and_then(Plane::from_property)
            .unwrap_or_else(|| Plane::from_name(&self.name))
    }
    /// Set the layer's draw [`Plane`] via the `plane` property (the editor's
    /// three-way cycle). The property is dropped when it would only restate the
    /// name-based fallback ([`Plane::from_name`]) — so a plain bg layer stays
    /// propertyless, while a `sprite`/`fg` choice (or a bg that must countermand
    /// an `fg`-prefixed name) is written explicitly. Never renames the layer.
    pub fn set_plane(&mut self, plane: Plane) {
        self.properties.retain(|p| p.name != "plane");
        if plane != Plane::from_name(&self.name) {
            self.properties.push(Property::string("plane", plane.name()));
        }
    }
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut usize> {
        if x >= self.width {
            return None; // guard the row wraparound past the right edge
        }
        self.data.get_mut(y.checked_mul(self.width)? + x)
    }
    /// Subtract each tile's tileset `firstgid` so tile ids become sheet-local.
    pub fn flatten_gids(&mut self, tilesets: &[Tileset]) {
        for tile in self.data.iter_mut() {
            // Strip Tiled's flip/rotate flag bits (the top 3 bits of the GID)
            // before resolving the tileset. A flagged GID is otherwise a huge
            // number that resolves to an empty (walkable) collider, draws blank,
            // and overflows the `(index / 32) * 2048` sheet math on wasm32 (where
            // usize is 32-bit).
            let gid = *tile & 0x1FFF_FFFF;
            let firstgid = tilesets
                .iter()
                .map(|ts| ts.firstgid)
                .filter(|&fg| gid >= fg)
                .max()
                .unwrap_or(0);
            *tile = gid - firstgid;
        }
    }
    pub fn into_layer_info(self, source_layer: usize) -> LayerInfo {
        LayerInfo {
            source_layer,
            ..self.into()
        }
    }
}
impl From<TileLayer> for LayerInfo {
    fn from(other: TileLayer) -> Self {
        Self {
            origin: Vec2::new(0, 0),
            // `unwrap_or(0)`: a layer wider/taller than i16::MAX is malformed —
            // degrade to an empty size rather than panicking on the conversion.
            size: Vec2::new(
                other.width.try_into().unwrap_or(0),
                other.height.try_into().unwrap_or(0),
            ),
            offset: Vec2::new(0, 0),
            ..Self::DEFAULT_LAYER
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectLayer {
    pub name: String,
    pub objects: Vec<TiledObject>,
}

/// A Tiled **image layer**: a single bitmap (a PNG path relative to the map
/// file) placed at a pixel offset, the engine's gateway to *painted maps*.
///
/// Where tile layers paint from the shared sheet, an image layer carries one
/// flat picture — so a complete modern map can be just a painted background
/// image + a painted collision mask + an object layer, with **no tile art at
/// all** (see [`crate::world::map::modern_map_info`] for that story end-to-end). Two
/// roles, chosen by the layer's name/properties (see
/// [`is_collision`](Self::is_collision)):
/// - a **visible** image layer draws into the world like a tile layer, obeying
///   the same conventions — file layer order for stacking, the `fg` name prefix
///   to sit above sprites, `visible: false` to never draw;
/// - a **collision** image layer is data, never drawn, its alpha sliced into the
///   per-tile bitmap [`Collider`](crate::geometry::Collider)s the walk loop
///   already consults (solid where alpha ≥
///   [`PAINTED_SOLID_ALPHA`](crate::world::map::PAINTED_SOLID_ALPHA)).
///
/// Rendering is deliberately minimal and matches the engine's tile blit:
/// **binary transparency** (a source pixel with `alpha == 0` is skipped, every
/// other pixel is opaque — `opacity` is parsed for round-trip fidelity but *not*
/// honoured while drawing), and **no scaling, no repeat, no alpha blending**
/// (all out of scope). The picture is blit 1:1 at its offset minus the camera.
///
/// The bitmap itself is **runtime-only** ([`pixels`](Self::pixels)): the codec
/// parses the path, and the host decodes the PNG and attaches the pixels after
/// load (see [`TiledMap::attach_image`]). A layer whose pixels never arrived
/// (missing/failed PNG) simply doesn't draw and contributes no collision.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ImageLayer {
    pub name: String,
    /// PNG path as authored in Tiled, **relative to the map file** (e.g.
    /// `images/bedroom1_mask.png`). The host resolves it under `maps/` to load
    /// the image; round-tripped verbatim so a save never rewrites it.
    pub image: String,
    /// Horizontal placement in pixels (Tiled stores it as a JSON number, hence
    /// `f64`; can be negative). Absent in Tiled ⇒ 0.
    #[serde(default)]
    pub offsetx: f64,
    /// Vertical placement in pixels. Absent in Tiled ⇒ 0.
    #[serde(default)]
    pub offsety: f64,
    /// Drawn unless explicitly hidden in Tiled. Absent ⇒ visible (Tiled's own
    /// default). A collision layer ignores this — it's never drawn regardless.
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Tiled layer opacity (0.0–1.0). Parsed only so a save round-trips it
    /// faithfully; the renderer uses binary transparency and ignores it.
    #[serde(default = "default_one")]
    pub opacity: f64,
    /// Custom properties — read only for the `collision` bool that (with the
    /// name prefix) marks a mask layer. Round-tripped verbatim.
    #[serde(default)]
    pub properties: Vec<ImageLayerProperty>,
    /// The decoded image, attached by the host after it reads the PNG; the codec
    /// never fills this (`#[serde(skip)]` ⇒ always `None` on parse, never
    /// serialised). `None` until attached, or if the PNG was missing/failed.
    #[serde(skip)]
    pub pixels: Option<RgbaImage>,
}
impl ImageLayer {
    /// Whether this image layer is a **collision mask** rather than a drawn
    /// picture: true when it carries a `collision: true` bool property, OR its
    /// name starts with `collision` (case-insensitive). The name rule matches
    /// the user's existing Tiled layer-naming; the property is the explicit
    /// opt-in for layers named anything else.
    pub fn is_collision(&self) -> bool {
        self.name.to_ascii_lowercase().starts_with("collision")
            || self
                .properties
                .iter()
                .any(|p| p.name == "collision" && p.value)
    }
}

/// A Tiled boolean custom property (`{ name, type: "bool", value }`) as carried
/// on an image layer. Distinct from the object layer's string
/// [`ObjectProperties`]: a `bool` property's `value` is a JSON boolean, and the
/// only one the engine reads is the collision-mask marker.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ImageLayerProperty {
    pub name: String,
    pub value: bool,
}

/// Serde default for an absent `visible` field (Tiled omits it when true).
fn default_true() -> bool {
    true
}

/// Serde default for an absent `opacity` field (Tiled omits it when 1.0).
fn default_one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum TiledMapLayer {
    #[serde(rename = "tilelayer")]
    TileLayer(TileLayer),
    #[serde(rename = "objectgroup")]
    ObjectLayer(ObjectLayer),
    #[serde(rename = "imagelayer")]
    ImageLayer(ImageLayer),
}
impl TiledMapLayer {
    /// This layer's name (all three kinds carry one).
    pub fn name(&self) -> &str {
        match self {
            TiledMapLayer::TileLayer(l) => &l.name,
            TiledMapLayer::ObjectLayer(l) => &l.name,
            TiledMapLayer::ImageLayer(l) => &l.name,
        }
    }
    /// Rename this layer (the `fg`-prefix convention means a rename can also flip
    /// it between the bg/fg draw lists on the next derive).
    pub fn set_name(&mut self, name: &str) {
        match self {
            TiledMapLayer::TileLayer(l) => l.name = name.to_string(),
            TiledMapLayer::ObjectLayer(l) => l.name = name.to_string(),
            TiledMapLayer::ImageLayer(l) => l.name = name.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tileset {
    pub firstgid: usize,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TiledObject {
    /// Tiled's per-object id: unique within the map and **stable across edits**
    /// — Tiled assigns it once and preserves it through moves, retiles and
    /// property tweaks; only delete-and-recreate yields a fresh one. The engine
    /// carries it onto the runtime [`MapObject`](crate::world::map::MapObject) so a
    /// removable object has a durable handle to record under in the save's
    /// `taken` set, rather than a positional index that shifts when a sibling is
    /// added or removed. Absent in a hand-written or pre-id map ⇒ `0` ("no stable
    /// id"); the writer then assigns a fresh one on the next save.
    #[serde(default)]
    pub id: usize,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    /// Tiled object "Type"/class. Used to mark warps (`type == "warp"`).
    #[serde(rename = "type", default)]
    pub class: String,
    #[serde(default)]
    pub properties: Vec<ObjectProperties>,
}
impl TiledObject {
    /// Value of the custom property `name`, if present.
    fn prop(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_str())
    }
    /// The object's pixel rectangle as a [`Hitbox`], or `None` if degenerate
    /// (Tiled occasionally emits zero-size point/text objects).
    fn hitbox(&self) -> Option<Hitbox> {
        let (w, h) = (self.width as i16, self.height as i16);
        (w > 0 && h > 0).then(|| Hitbox::new(self.x as i16, self.y as i16, w, h))
    }
    /// An integer-valued custom property, if present and parseable.
    fn prop_int<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.prop(name).and_then(|s| s.parse().ok())
    }
    /// Resolve this object into a runtime [`MapObject`] by a fixed property
    /// precedence, documented so the inverse [`object_to_tmj`] mirrors it:
    ///
    /// 1. **warp** — `type == "warp"` or any warp property ([`to_warp`](Self::to_warp));
    /// 2. **func** — a `func` property names an [`InteractFn`](crate::world::interact::InteractFn)
    ///    ([`to_func`](Self::to_func));
    /// 3. **cutscene** — a non-empty `cutscene` property names a cutscene-registry
    ///    entry ([`to_cutscene`](Self::to_cutscene));
    /// 4. **dialogue** — a non-empty `description` (the registry key)
    ///    ([`to_interactable`](Self::to_interactable));
    /// 5. **sprite-only** — just a `sprite` tile id: an [`Interaction::None`]
    ///    object that only draws an animation (e.g. the living-room TV);
    /// 6. otherwise `None` (also for degenerate zero-size objects, via
    ///    [`hitbox`](Self::hitbox)) — the object is skipped.
    fn to_object(&self) -> Option<MapObject> {
        let hitbox = self.hitbox()?;
        let object = if let Some(warp) = self.to_warp() {
            MapObject::warp(hitbox, warp)
        } else if let Some(func) = self.to_func() {
            self.attach_sprite(MapObject::func(hitbox, func))
        } else if let Some(object) = self.to_cutscene() {
            object
        } else if let Some(object) = self.to_interactable() {
            object
        } else {
            self.to_sprite_only(hitbox)?
        };
        // Carry the stable Tiled id onto the runtime object (0 ⇒ "no id"), so a
        // removable object keeps a durable handle for the save's `taken` set,
        // and the `removable` marker that opts it into that consume-on-interact
        // behaviour.
        let object = self
            .apply_trigger(object)
            .with_id((self.id != 0).then_some(self.id))
            .with_removable(self.is_removable())
            .with_gate(self.gate());
        Some(object)
    }
    /// Read this object's flag [`Gate`] from its `if` / `unless` / `sets`
    /// properties (each naming a story flag; empty/absent ⇒ that condition is
    /// unset). Parsed for any object kind, so a warp, dialogue, cutscene or func
    /// can all be flag-gated. Inverse of the gate emission in [`object_to_tmj`]
    /// (which uses [`Gate::properties`]).
    fn gate(&self) -> Gate {
        let flag = |name| self.prop(name).filter(|s| !s.is_empty()).map(str::to_string);
        Gate {
            if_flag: flag("if"),
            unless_flag: flag("unless"),
            sets: flag("sets"),
        }
    }
    /// Whether a `removable` property marks this object as a consume-on-interact
    /// pickup (see [`MapObject::removable`](crate::world::map::MapObject::removable)).
    /// Authored as the string `"true"` — consistent with the other string
    /// properties — so any other/absent value reads as `false`. Inverse of the
    /// `removable` emission in [`object_to_tmj`].
    fn is_removable(&self) -> bool {
        self.prop("removable") == Some("true")
    }
    /// Override the object's trigger from an optional `trigger` property
    /// (`"touch"`/`"press"`/`"any"`, case-insensitive), parsed for any object
    /// kind. An absent or unrecognised value leaves the effect-kind default the
    /// constructor picked (so an unknown value is silently ignored — the door
    /// still works on its default trigger rather than breaking the map). Inverse
    /// of the trigger half of [`object_to_tmj`].
    fn apply_trigger(&self, object: MapObject) -> MapObject {
        match self.prop("trigger").map(parse_trigger) {
            Some(Some(trigger)) => object.with_trigger(trigger),
            _ => object,
        }
    }
    /// Build a function interaction if this object carries a `func` property
    /// naming a known [`InteractFn`](crate::world::interact::InteractFn), reading any
    /// scalar properties that name needs (`pitch`, `count`) and taking
    /// positional data from the hitbox. The inverse of the `func` serialisation
    /// in [`interaction_to_object`].
    fn to_func(&self) -> Option<InteractFn> {
        let name = self.prop("func").filter(|s| !s.is_empty())?;
        InteractFn::from_name(
            name,
            self.prop_int("pitch"),
            self.prop_int("count"),
            self.prop("item"),
            self.hitbox()?,
        )
    }
    /// A pure sprite object: an animated sprite (`anim` or a `sprite` tile id)
    /// with no warp/func/`description`, kept as an [`Interaction::None`] so legacy
    /// animation-only objects (the living-room TV) survive a map round-trip.
    /// Neither sprite property ⇒ `None` (skip).
    fn to_sprite_only(&self, hitbox: Hitbox) -> Option<MapObject> {
        self.sprite_frames()?;
        let object = MapObject::new(hitbox, ObjectEffect::Interact(Interaction::None), None);
        Some(self.attach_sprite(object))
    }
    /// Attach this object's sprite (if any) as an animation. Prefers a full
    /// `anim` property (the JSON serialisation of a `Vec<AnimFrame>` — multi-frame
    /// with per-frame offsets/durations/palette-rotation/outline and multi-tile
    /// [`SpriteOptions`]) over the plain single-tile `sprite` id, so the richer
    /// legacy sprites round-trip; falls back to the one-frame `sprite` for the
    /// common case (and for maps authored before `anim` existed).
    fn attach_sprite(&self, object: MapObject) -> MapObject {
        match self.sprite_frames() {
            Some(frames) => object.with_sprite(frames),
            None => object,
        }
    }
    /// This object's sprite as animation frames, from `anim` (preferred) or the
    /// single-tile `sprite` id, or `None` if it carries neither. A malformed
    /// `anim` value is ignored (falls through to `sprite`).
    fn sprite_frames(&self) -> Option<Vec<AnimFrame>> {
        if let Some(frames) = self
            .prop("anim")
            .and_then(|s| serde_json::from_str::<Vec<AnimFrame>>(s).ok())
            .filter(|frames| !frames.is_empty())
        {
            return Some(frames);
        }
        let id = self.prop_int::<u16>("sprite")?;
        Some(vec![AnimFrame::new(
            Vec2::splat(0),
            id,
            30,
            SpriteOptions::transparent_zero(),
        )])
    }
    /// Build a warp effect if this object is one (`type == "warp"`, or it carries
    /// warp properties): `to_map` (a map name, taken verbatim and resolved
    /// against the map store when the warp fires; absent = same map),
    /// `to_x`/`to_y` (destination pixels, default = the object's own
    /// position), `flip`, `mode` (`auto`/`interact`), `sound`, and `narration`
    /// (a pre-warp dialogue key; absent/empty = none). The trigger hitbox lives
    /// on the owning [`MapObject`], and the `trigger` axis is applied there too
    /// (see [`apply_trigger`](Self::apply_trigger)), so neither is built here.
    fn to_warp(&self) -> Option<Warp> {
        let is_warp = self.class.eq_ignore_ascii_case("warp")
            || self.prop("to_map").is_some()
            || self.prop("to_x").is_some();
        if !is_warp {
            return None;
        }
        let from = self.hitbox()?;
        let map = self.prop("to_map");
        let to = Vec2::new(
            self.prop("to_x")
                .and_then(|s| s.parse().ok())
                .unwrap_or(from.x),
            self.prop("to_y")
                .and_then(|s| s.parse().ok())
                .unwrap_or(from.y),
        );
        let mut warp = Warp::new(map, to);
        if let Some(flip) = self.prop("flip") {
            warp = warp.with_flip(parse_axis(flip));
        }
        if self
            .prop("mode")
            .is_some_and(|m| m.eq_ignore_ascii_case("auto"))
        {
            warp = warp.with_mode(WarpMode::Auto);
        }
        if let Some(sound) = self.prop("sound").and_then(parse_sound) {
            warp = warp.with_sound(sound);
        }
        if let Some(key) = self.prop("narration").filter(|s| !s.is_empty()) {
            warp = warp.with_narration(key);
        }
        Some(warp)
    }
    /// Build a cutscene interaction object if this object carries a non-empty
    /// `cutscene` property (the cutscene-registry name; see
    /// [`crate::data::scene`]). The name is taken verbatim and resolved
    /// against the loaded registry when the object fires (like a warp's
    /// `to_map`). Optional `sprite` round-trips like the dialogue object's.
    /// Inverse of the `cutscene` serialisation in [`interaction_to_object`].
    fn to_cutscene(&self) -> Option<MapObject> {
        let name = self.prop("cutscene").filter(|s| !s.is_empty())?;
        let object = MapObject::new(
            self.hitbox()?,
            ObjectEffect::Interact(Interaction::Cutscene(name.to_string())),
            None,
        );
        Some(self.attach_sprite(object))
    }
    /// Build a dialogue interaction object if this object carries a `description`
    /// (the dialogue-registry key). Optional `sprite` property = a tile id drawn
    /// at the object.
    fn to_interactable(&self) -> Option<MapObject> {
        let key = self.prop("description").filter(|s| !s.is_empty())?;
        let object = MapObject::dialogue(self.hitbox()?, key);
        Some(self.attach_sprite(object))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectProperties {
    pub name: String,
    pub value: String,
}

/// Parse a `flip` property into an [`Axis`].
fn parse_axis(s: &str) -> Axis {
    match s.to_ascii_lowercase().as_str() {
        "x" => Axis::X,
        "y" => Axis::Y,
        "both" => Axis::Both,
        _ => Axis::None,
    }
}

/// Parse a `trigger` property into a [`Trigger`]. `None` for an unrecognised
/// value, so the caller can fall back to the effect-kind default rather than
/// guess. Inverse of [`trigger_name`].
fn parse_trigger(s: &str) -> Option<Trigger> {
    Some(match s.to_ascii_lowercase().as_str() {
        "touch" => Trigger::Touch,
        "press" => Trigger::Press,
        "any" => Trigger::Any,
        "enter" => Trigger::Enter,
        _ => return None,
    })
}

/// Map a `sound` property name to a known sound effect.
fn parse_sound(s: &str) -> Option<SfxData> {
    Some(match s.to_ascii_lowercase().as_str() {
        "door" => sound::DOOR,
        "stairs_down" => sound::STAIRS_DOWN,
        "stairs_up" => sound::STAIRS_UP,
        _ => return None,
    })
}

/// A Tiled string custom-property `{ name, type: "string", value }`.
fn prop_str(name: &str, value: &str) -> Value {
    json!({ "name": name, "type": "string", "value": value })
}

/// Reverse of [`parse_axis`] (`Axis::None` has no property).
fn axis_name(axis: &Axis) -> Option<&'static str> {
    match axis {
        Axis::None => None,
        Axis::X => Some("x"),
        Axis::Y => Some("y"),
        Axis::Both => Some("both"),
    }
}

/// Reverse of [`parse_sound`].
fn sound_name(sfx: &SfxData) -> Option<&'static str> {
    Some(if sfx.id == sound::DOOR.id {
        "door"
    } else if sfx.id == sound::STAIRS_DOWN.id {
        "stairs_down"
    } else if sfx.id == sound::STAIRS_UP.id {
        "stairs_up"
    } else {
        return None;
    })
}

/// Serialise one [`MapObject`] to a Tiled object by its effect, reusing the
/// owning object's `hitbox` for the placed rectangle. The inverse of the
/// parse-precedence in [`TiledObject::to_object`]: a warp serialises with
/// `type: "warp"` + warp properties; a named `func` interaction with `func` +
/// its scalar props; a dialogue interaction with `description`; a bare
/// [`Interaction::None`] that carries a sprite as a `sprite`-only object — all
/// with the object's optional `sprite` tile id, plus a `trigger` property when
/// the object's trigger differs from its effect-kind default (so an unauthored
/// trigger emits nothing and the file stays byte-stable). The cases that have no
/// Tiled spelling — an unnamed func ([`InteractFn::Pet`](crate::world::interact::InteractFn::Pet)),
/// or a sprite-less [`Interaction::None`] — return `None` (the caller counts
/// them dropped).
fn object_to_tmj(object: &MapObject, id: usize) -> Option<Value> {
    let mut value = match &object.effect {
        ObjectEffect::Warp(warp) => warp_to_object(object.hitbox, warp, id),
        ObjectEffect::Interact(interaction) => {
            interaction_to_object(object.hitbox, interaction, object.sprite.as_deref(), id)?
        }
    };
    // Trigger lives on the object (either kind) and is serialised only when it
    // differs from the effect-kind default, so files with no authored trigger
    // round-trip byte-stable. Appended after the effect's own properties.
    if object.trigger != Trigger::default_for(&object.effect)
        && let Some(properties) = value.get_mut("properties").and_then(Value::as_array_mut)
    {
        properties.push(prop_str("trigger", object.trigger.name()));
    }
    // The `removable` marker round-trips the parse ([`TiledObject::is_removable`]),
    // emitted only when set so a normal object's file stays byte-stable.
    if object.removable
        && let Some(properties) = value.get_mut("properties").and_then(Value::as_array_mut)
    {
        properties.push(prop_str("removable", "true"));
    }
    // The flag gate (`if` / `unless` / `sets`) round-trips the parse
    // ([`TiledObject::gate`]); [`Gate::properties`] emits only the set fields, so
    // an ungated object emits nothing and its file stays byte-stable.
    if let Some(properties) = value.get_mut("properties").and_then(Value::as_array_mut) {
        for (name, flag) in object.gate.properties() {
            properties.push(prop_str(name, flag));
        }
    }
    Some(value)
}

/// Serialise a warp effect as a Tiled object (`type: "warp"` + warp properties),
/// placed at `hitbox`.
fn warp_to_object(hitbox: Hitbox, warp: &Warp, id: usize) -> Value {
    let mut properties = Vec::new();
    if let Some(map) = &warp.map {
        properties.push(prop_str("to_map", map));
    }
    properties.push(prop_str("to_x", &warp.to.x.to_string()));
    properties.push(prop_str("to_y", &warp.to.y.to_string()));
    if let Some(flip) = axis_name(&warp.flip) {
        properties.push(prop_str("flip", flip));
    }
    if matches!(warp.mode, WarpMode::Auto) {
        properties.push(prop_str("mode", "auto"));
    }
    if let Some(name) = warp.sound.as_ref().and_then(sound_name) {
        properties.push(prop_str("sound", name));
    }
    if let Some(key) = &warp.narration {
        properties.push(prop_str("narration", key));
    }
    json!({
        "id": id, "name": "", "type": "warp", "rotation": 0, "visible": true,
        "x": hitbox.x, "y": hitbox.y,
        "width": hitbox.w, "height": hitbox.h,
        "properties": properties,
    })
}

/// Serialise an [`Interaction`] as a (non-warp) Tiled object placed at `hitbox`,
/// carrying its optional sprite. Dialogue → `description`; a named `func` →
/// `func` + its scalar props (`pitch`/`count`; piano/none need none); a cutscene
/// → `cutscene` (its registry name); a sprite-carrying [`Interaction::None`] →
/// just its sprite. The sprite is emitted as a plain `sprite` tile id when it is
/// a single default-options frame, and as a full `anim` (JSON `Vec<AnimFrame>`)
/// otherwise, so richer legacy sprites round-trip losslessly (see
/// [`sprite_property`]). The cases with no spelling (unnamed func, sprite-less
/// `None`) → `None`.
fn interaction_to_object(
    hitbox: Hitbox,
    interaction: &Interaction,
    sprite: Option<&[AnimFrame]>,
    id: usize,
) -> Option<Value> {
    let sprite_prop = sprite.and_then(sprite_property);
    let mut properties = match interaction {
        Interaction::Dialogue(key) => vec![prop_str("description", key)],
        Interaction::Func(func) => func_properties(func)?,
        Interaction::Cutscene(name) => vec![prop_str("cutscene", name)],
        // A pure animation object only round-trips if it actually has a sprite;
        // a sprite-less `None` is nothing Tiled can represent.
        Interaction::None => {
            sprite_prop.as_ref()?;
            Vec::new()
        }
    };
    if let Some(prop) = sprite_prop {
        properties.push(prop);
    }
    Some(json!({
        "id": id, "name": "", "type": "", "rotation": 0, "visible": true,
        "x": hitbox.x, "y": hitbox.y, "width": hitbox.w, "height": hitbox.h,
        "properties": properties,
    }))
}

/// The Tiled property carrying an object's sprite, or `None` for an empty frame
/// list. A sprite that is exactly **one default-options frame** — what the
/// `sprite`-property parse reconstructs ([`TiledObject::sprite_frames`]) —
/// serialises as the compact `sprite` tile id, keeping pre-`anim` maps stable;
/// any richer sprite (multiple frames, per-frame offsets/durations, palette
/// rotation, a non-default outline, or multi-tile [`SpriteOptions`]) serialises
/// as a full `anim` JSON array so nothing is lost.
fn sprite_property(frames: &[AnimFrame]) -> Option<Value> {
    let id = simple_sprite_id(frames)?;
    match id {
        Some(id) => Some(prop_str("sprite", &id.to_string())),
        None => Some(prop_str("anim", &serde_json::to_string(frames).ok()?)),
    }
}

/// Classify an object's sprite frames: `None` for an empty list (no sprite);
/// `Some(Some(id))` for a single frame identical to what `sprite`-property
/// parsing builds (so it can round-trip as the compact `sprite` id);
/// `Some(None)` for any richer sprite (so it must round-trip as `anim`).
fn simple_sprite_id(frames: &[AnimFrame]) -> Option<Option<u16>> {
    let [frame] = frames else {
        return (!frames.is_empty()).then_some(None);
    };
    let simple = frame.pos == Vec2::splat(0)
        && frame.duration == 30
        && frame.outline_colour == Some(1)
        && frame.palette_rotate == 0
        && is_transparent_zero(&frame.options);
    Some(simple.then_some(frame.spr_id))
}

/// Whether these sprite options are exactly [`SpriteOptions::transparent_zero`]
/// (the default a `sprite`-property frame carries): default everything but a
/// transparent index of 0.
fn is_transparent_zero(options: &SpriteOptions) -> bool {
    let SpriteOptions {
        id: 0,
        x_offset: 0,
        y_offset: 0,
        transparent: Some(0),
        scale: 1,
        flip: Flip::None,
        rotate: Rotate::None,
        w: 1,
        h: 1,
    } = options
    else {
        return false;
    };
    true
}

/// The Tiled properties an [`InteractFn`] serialises to: a `func` name plus the
/// scalar properties that name carries on parse (`pitch`, `count`). Positional
/// data (the piano origin) lives in the hitbox, so it needs no property.
/// `None` for an `InteractFn` with no name ([`InteractFn::Pet`]), which is the
/// signal to drop the object. Inverse of [`TiledObject::to_func`].
fn func_properties(func: &InteractFn) -> Option<Vec<Value>> {
    let name = func.name()?;
    let mut properties = vec![prop_str("func", name)];
    match func {
        InteractFn::Note(pitch) => properties.push(prop_str("pitch", &pitch.to_string())),
        InteractFn::AddCreatures(count) => properties.push(prop_str("count", &count.to_string())),
        InteractFn::GiveItem(key) => properties.push(prop_str("item", key)),
        InteractFn::ToggleDog | InteractFn::Piano(_) | InteractFn::Pet(..) => {}
    }
    Some(properties)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TiledMap {
    pub width: usize,
    pub height: usize,
    pub layers: Vec<TiledMapLayer>,
    pub tilesets: Vec<Tileset>,
    /// Map-level custom properties, read for the runtime metadata Tiled has no
    /// native field for: the int `bg_colour` (palette index behind the map) and
    /// the string `camera_stick` (`"x,y"`, a pinned camera position). Tiled's own
    /// cosmetic `backgroundcolor` is deliberately *not* consulted. Round-tripped
    /// verbatim, so a save preserves them. Absent ⇒ empty.
    #[serde(default)]
    pub properties: Vec<Property>,
}
impl TiledMap {
    /// This map's `bg_colour` property (palette index), if present.
    pub fn bg_colour(&self) -> Option<u8> {
        property_int(&self.properties, "bg_colour").and_then(|v| u8::try_from(v).ok())
    }
    /// This map's `camera_stick` property parsed as an `(x, y)` i16 pair (the
    /// pinned camera position), if present and well-formed (`"x,y"`).
    pub fn camera_stick(&self) -> Option<(i16, i16)> {
        let value = property_str(&self.properties, "camera_stick")?;
        let (x, y) = value.split_once(',')?;
        Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
    }
    pub fn get(&self, layer: usize, x: usize, y: usize) -> Option<usize> {
        self.layers.get(layer).and_then(|layer| match layer {
            TiledMapLayer::TileLayer(layer) => layer.get(x, y),
            _ => None,
        })
    }
    pub fn set(&mut self, layer: usize, x: usize, y: usize, value: usize) {
        if let Some(tile) = self.layers.get_mut(layer).and_then(|layer| match layer {
            TiledMapLayer::TileLayer(layer) => layer.get_mut(x, y),
            _ => None,
        }) {
            *tile = value;
        };
    }
    pub fn get_tile_source(&self, tile: usize) -> Option<Tileset> {
        self.tilesets
            .iter()
            .filter(|ts| tile >= ts.firstgid)
            .max_by_key(|ts| ts.firstgid)
            .cloned()
    }
    pub fn flatten_gids(&mut self) {
        for layer in self.layers.iter_mut() {
            if let TiledMapLayer::TileLayer(layer) = layer {
                layer.flatten_gids(&self.tilesets)
            }
        }
    }

    /// A blank `width`×`height` (tiles) **modern** map: an empty collision tile
    /// layer (layer 0), one empty drawable tile layer, and an empty object layer
    /// (which is what makes it [`MapStore::is_modern`](crate::world::map::MapStore::is_modern)).
    /// References the single standard `tiles.tsj` tileset — `to_tmj` re-gids with
    /// only the first tileset, so a one-tileset map round-trips cleanly. The
    /// editor's "new map" writes `to_tmj(&[])` of this to `maps/<name>.tmj`.
    pub fn blank_modern(width: usize, height: usize) -> TiledMap {
        let tile_layer = |name: &str| {
            TiledMapLayer::TileLayer(TileLayer {
                width,
                height,
                data: vec![0; width * height],
                name: name.to_string(),
                offsetx: 0.0,
                offsety: 0.0,
                properties: Vec::new(),
            })
        };
        TiledMap {
            width,
            height,
            layers: vec![
                tile_layer("collision"),
                tile_layer("Layer 1"),
                TiledMapLayer::ObjectLayer(ObjectLayer {
                    name: "objects".to_string(),
                    objects: Vec::new(),
                }),
            ],
            tilesets: vec![Tileset {
                firstgid: 1,
                source: "tiles.tsj".to_string(),
            }],
            properties: Vec::new(),
        }
    }
    /// The index of the collision tile layer — the first tile layer, which the
    /// editor never lets the user reorder or delete (its art derives the map's
    /// colliders). `None` for a pure-painted map with no tile layer.
    pub fn collision_layer(&self) -> Option<usize> {
        self.layers
            .iter()
            .position(|l| matches!(l, TiledMapLayer::TileLayer(_)))
    }

    /// Append an empty drawable tile layer (sized to the map's tile grid), placed
    /// just before the first object layer so tile layers stay contiguous. Returns
    /// the index it landed at (for the editor's undo). Used by the "add layer" tool.
    pub fn add_tile_layer(&mut self, name: &str) -> usize {
        let (w, h) = self
            .layers
            .iter()
            .find_map(|l| match l {
                TiledMapLayer::TileLayer(t) => Some((t.width, t.height)),
                _ => None,
            })
            .unwrap_or((self.width, self.height));
        let layer = TiledMapLayer::TileLayer(TileLayer {
            width: w,
            height: h,
            data: vec![0; w * h],
            name: name.to_string(),
            offsetx: 0.0,
            offsety: 0.0,
            properties: Vec::new(),
        });
        let pos = self
            .layers
            .iter()
            .position(|l| matches!(l, TiledMapLayer::ObjectLayer(_)))
            .unwrap_or(self.layers.len());
        self.layers.insert(pos, layer);
        pos
    }

    /// Insert `layer` at `idx` (clamped to the list end) — the raw inverse of a
    /// removal, used to replay layer undo/redo. Unprotected by design: it only
    /// restores a layer the editor previously took out.
    pub fn insert_layer(&mut self, idx: usize, layer: TiledMapLayer) {
        let idx = idx.min(self.layers.len());
        self.layers.insert(idx, layer);
    }

    /// Remove the layer at `idx` and return it, refusing to drop the collision
    /// (first tile) layer (returns `None`). The editor's "delete layer" tool and
    /// its undo recording both go through this so the protection is in one place.
    pub fn remove_layer_at(&mut self, idx: usize) -> Option<TiledMapLayer> {
        if idx < self.layers.len() && Some(idx) != self.collision_layer() {
            Some(self.layers.remove(idx))
        } else {
            None
        }
    }

    /// Back-compat shim: remove without returning the layer.
    pub fn remove_layer(&mut self, idx: usize) {
        self.remove_layer_at(idx);
    }

    /// Swap two layers by index (no-op if either is out of range) — the raw
    /// operation behind a move and its undo.
    pub fn swap_layers(&mut self, a: usize, b: usize) {
        if a < self.layers.len() && b < self.layers.len() {
            self.layers.swap(a, b);
        }
    }

    /// The layer at `idx`'s name, if it exists.
    pub fn layer_name(&self, idx: usize) -> Option<&str> {
        self.layers.get(idx).map(|l| l.name())
    }

    /// Rename the layer at `idx` (no-op if out of range).
    pub fn set_layer_name(&mut self, idx: usize, name: &str) {
        if let Some(l) = self.layers.get_mut(idx) {
            l.set_name(name);
        }
    }

    /// Set (replacing in place) a map-level custom property; the editor's Setup
    /// panel writes `bg_colour` / `camera_stick` through the typed wrappers below.
    fn set_property(&mut self, name: &str, ty: &str, value: Value) {
        if let Some(p) = self.properties.iter_mut().find(|p| p.name == name) {
            p.r#type = ty.to_string();
            p.value = value;
        } else {
            self.properties.push(Property {
                name: name.to_string(),
                r#type: ty.to_string(),
                value,
            });
        }
    }

    /// Drop a map-level property by name (no-op if absent).
    fn remove_property(&mut self, name: &str) {
        self.properties.retain(|p| p.name != name);
    }

    /// Set the map's background palette index (`bg_colour`).
    pub fn set_bg_colour(&mut self, colour: u8) {
        self.set_property("bg_colour", "int", Value::from(colour));
    }

    /// Pin the camera at `(x, y)` (`camera_stick`), or clear it (`None`) so the
    /// engine auto-frames from the map size.
    pub fn set_camera_stick(&mut self, point: Option<(i16, i16)>) {
        match point {
            Some((x, y)) => {
                self.set_property("camera_stick", "string", Value::from(format!("{x},{y}")))
            }
            None => self.remove_property("camera_stick"),
        }
    }

    /// This map's `music` property — a track *name* resolved against the known
    /// tracks at derive time (an unknown name no-ops, like a dangling warp).
    pub fn music(&self) -> Option<&str> {
        property_str(&self.properties, "music")
    }

    /// Set the map's music track by name, or clear it (`None`).
    pub fn set_music(&mut self, track: Option<&str>) {
        match track {
            Some(name) => self.set_property("music", "string", Value::from(name)),
            None => self.remove_property("music"),
        }
    }

    /// This map's `music_speed` property — the playback-rate multiplier for the
    /// track (1.0 = normal). Absent ⇒ 1.0.
    pub fn music_speed(&self) -> f32 {
        property_float(&self.properties, "music_speed").map_or(1.0, |v| v as f32)
    }

    /// Set the map's music playback speed. The default (1.0) drops the property
    /// so unchanged maps keep a clean `.tmj`.
    pub fn set_music_speed(&mut self, speed: f32) {
        if (speed - 1.0).abs() < f32::EPSILON {
            self.remove_property("music_speed");
        } else {
            // Store the f32's shortest round-tripping decimal rather than `speed as
            // f64` (which would bake float noise into the file, e.g. 0.1 ->
            // 0.10000000149011612), keeping the authored value readable in the `.tmj`.
            let clean = speed.to_string().parse::<f64>().unwrap_or(f64::from(speed));
            self.set_property("music_speed", "float", Value::from(clean));
        }
    }

    /// The tile layer at `idx`'s `(offsetx, offsety)` pixel offset, or `None` if
    /// it isn't a tile layer (image/object layers carry no editable tile offset
    /// in this editor).
    pub fn layer_offset(&self, idx: usize) -> Option<(f64, f64)> {
        match self.layers.get(idx) {
            Some(TiledMapLayer::TileLayer(t)) => Some((t.offsetx, t.offsety)),
            _ => None,
        }
    }
    pub fn set_layer_offset_x(&mut self, idx: usize, v: f64) {
        if let Some(TiledMapLayer::TileLayer(t)) = self.layers.get_mut(idx) {
            t.offsetx = v;
        }
    }
    pub fn set_layer_offset_y(&mut self, idx: usize, v: f64) {
        if let Some(TiledMapLayer::TileLayer(t)) = self.layers.get_mut(idx) {
            t.offsety = v;
        }
    }

    /// The tile layer at `idx`'s `palette_rotate` (0 if absent / not a tile layer).
    pub fn layer_palette_rotate(&self, idx: usize) -> u8 {
        match self.layers.get(idx) {
            Some(TiledMapLayer::TileLayer(t)) => t.palette_rotate(),
            _ => 0,
        }
    }
    pub fn set_layer_palette_rotate(&mut self, idx: usize, v: u8) {
        if let Some(TiledMapLayer::TileLayer(t)) = self.layers.get_mut(idx) {
            t.set_palette_rotate(v);
        }
    }

    /// The tile layer at `idx`'s draw [`Plane`] (its `plane` property or name
    /// fallback). [`Plane::Bg`] for an image/object layer or an out-of-range
    /// index — the editor only cycles the plane of tile layers.
    pub fn layer_plane(&self, idx: usize) -> Plane {
        match self.layers.get(idx) {
            Some(TiledMapLayer::TileLayer(t)) => t.plane(),
            _ => Plane::Bg,
        }
    }
    /// Set the tile layer at `idx`'s draw [`Plane`] via its `plane` property
    /// (no-op for a non-tile layer). See [`TileLayer::set_plane`].
    pub fn set_layer_plane(&mut self, idx: usize, plane: Plane) {
        if let Some(TiledMapLayer::TileLayer(t)) = self.layers.get_mut(idx) {
            t.set_plane(plane);
        }
    }

    /// Resize the map to `width`×`height` tiles, reflowing every tile layer's data
    /// anchored at the top-left: cells beyond the new bounds are dropped, new cells
    /// start empty. Image and object layers are untouched.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        for layer in self.layers.iter_mut() {
            if let TiledMapLayer::TileLayer(tl) = layer {
                let mut data = vec![0usize; width * height];
                for y in 0..height.min(tl.height) {
                    for x in 0..width.min(tl.width) {
                        data[y * width + x] = tl.data[y * tl.width + x];
                    }
                }
                tl.width = width;
                tl.height = height;
                tl.data = data;
            }
        }
    }

    /// Swap the layer at `idx` with its neighbour (`up` = earlier in draw order),
    /// never reordering the collision (first tile) layer. Returns the swapped
    /// `(idx, other)` pair (for undo), or `None` if nothing moved. Used by the
    /// editor's move-layer tools.
    pub fn move_layer(&mut self, idx: usize, up: bool) -> Option<(usize, usize)> {
        let n = self.layers.len();
        if idx >= n {
            return None;
        }
        let other = if up {
            idx.checked_sub(1)?
        } else {
            (idx + 1 < n).then_some(idx + 1)?
        };
        let collision = self.collision_layer();
        if Some(idx) == collision || Some(other) == collision {
            return None;
        }
        self.layers.swap(idx, other);
        Some((idx, other))
    }

    /// Move the layer at `from` to land at index `to` (remove + re-insert,
    /// sliding the layers between), the multi-step counterpart to
    /// [`move_layer`](Self::move_layer)'s single neighbour swap — used by the
    /// editor's drag-reorder. Never moves the collision (first tile) layer, and
    /// clamps `to` so the dragged layer can't slip across the collision layer
    /// (which must stay first among the tile layers); the clamp keeps
    /// [`collision_layer`](Self::collision_layer)'s index fixed. Returns the
    /// effective `(from, to)` actually applied (for undo), or `None` if out of
    /// range or nothing moved.
    pub fn reorder_layer(&mut self, from: usize, to: usize) -> Option<(usize, usize)> {
        let n = self.layers.len();
        if from >= n {
            return None;
        }
        let mut to = to.min(n - 1);
        if let Some(c) = self.collision_layer() {
            if from == c {
                return None;
            }
            // Stay on `from`'s side of the collision layer so it stays put.
            if from < c {
                to = to.min(c - 1);
            } else {
                to = to.max(c + 1);
            }
        }
        if from == to {
            return None;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to.min(self.layers.len()), layer);
        Some((from, to))
    }

    /// The `image` paths of every image layer, in file order — the list the
    /// host walks to know which PNGs to load for this map. Each path is as
    /// authored (relative to the map file); the host resolves it under `maps/`.
    pub fn image_layer_paths(&self) -> Vec<&str> {
        self.layers
            .iter()
            .filter_map(|layer| match layer {
                TiledMapLayer::ImageLayer(image) => Some(image.image.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Attach a decoded image to every image layer whose `image` path matches
    /// `path`, after the host reads the PNG. Keyed by path (not layer index) so
    /// the host loads each distinct image once and fans it out; a map referencing
    /// the same PNG from two layers gets it on both. The codec never fills
    /// [`ImageLayer::pixels`] itself — this is the one way runtime pixels arrive.
    pub fn attach_image(&mut self, path: &str, pixels: RgbaImage) {
        for layer in self.layers.iter_mut() {
            if let TiledMapLayer::ImageLayer(image) = layer
                && image.image == path
            {
                image.pixels = Some(pixels.clone());
            }
        }
    }

    /// Parse this map's object layers into one ordered list of runtime
    /// [`MapObject`]s, in file order. Warps are objects with `type == "warp"` or
    /// warp properties; interactions are objects carrying a `description`
    /// (dialogue key). See [`TiledObject::to_object`].
    pub fn parse_objects(&self) -> Vec<MapObject> {
        let mut objects = Vec::new();
        for layer in &self.layers {
            if let TiledMapLayer::ObjectLayer(group) = layer {
                for object in &group.objects {
                    if let Some(object) = object.to_object() {
                        objects.push(object);
                    }
                }
            }
        }
        objects
    }
    /// Re-serialise this map to Tiled JSON: `self` is both the structural
    /// template (dimensions, layer names, tilesets) and the live tile data
    /// (its tile layers hold flattened/sheet-local ids, which are re-gid'd on
    /// the way out), while the object layer is rebuilt from `objects` in vector
    /// order (so a hand-mixed layer keeps its interleaving). Returns
    /// pretty-printed JSON. Warps, named `func` interactions, dialogue keys and
    /// sprite-only [`Interaction::None`] objects all round-trip (see
    /// [`object_to_tmj`]); only the two cases with no Tiled spelling — an
    /// unnamed func ([`InteractFn::Pet`]) and a sprite-less `Interaction::None`
    /// — are dropped (with a warning).
    ///
    /// The flattened→gid inverse maps `0` to an empty cell, so a cell holding
    /// the tileset's very first tile (which flattened to `0` on load) is saved
    /// as empty — an unavoidable consequence of the lossy flatten and the same
    /// way the engine already treats those cells.
    pub fn to_tmj(&self, objects: &[MapObject]) -> String {
        // Single-tileset assumption: `flatten_gids` subtracted per-tile firstgids, but only the first is re-added.
        let firstgid = self.tilesets.first().map(|t| t.firstgid).unwrap_or(1);
        let mut dropped = 0usize;
        let mut layers = Vec::new();
        for (i, layer) in self.layers.iter().enumerate() {
            let id = i + 1;
            match layer {
                TiledMapLayer::TileLayer(tile_layer) => {
                    let data: Vec<usize> = tile_layer
                        .data
                        .iter()
                        .map(|&t| if t == 0 { 0 } else { t + firstgid })
                        .collect();
                    let mut layer = json!({
                        "type": "tilelayer", "id": id, "name": tile_layer.name,
                        "width": tile_layer.width, "height": tile_layer.height,
                        "x": 0, "y": 0, "opacity": 1, "visible": true,
                        "data": data,
                    });
                    // Tiled omits zero offsets and empty property lists; match
                    // that so a plain tile layer round-trips byte-stable while an
                    // offset/property-carrying one (a nudged decor layer, a
                    // `palette_rotate`d bg) keeps its data across an in-game save.
                    if tile_layer.offsetx != 0.0 {
                        layer["offsetx"] = json!(tile_layer.offsetx);
                    }
                    if tile_layer.offsety != 0.0 {
                        layer["offsety"] = json!(tile_layer.offsety);
                    }
                    if !tile_layer.properties.is_empty() {
                        layer["properties"] = properties_to_json(&tile_layer.properties);
                    }
                    layers.push(layer);
                }
                TiledMapLayer::ObjectLayer(object_layer) => {
                    // Preserve every object's stable id; any id-less object
                    // (runtime/editor-created, or from a pre-id map) gets a fresh
                    // id above every existing one, so survivors keep their ids
                    // when a sibling is added or removed (a positional counter
                    // would renumber them and break the save's `taken` keys).
                    let mut next_id = objects.iter().filter_map(|o| o.id).max().unwrap_or(0) + 1;
                    let mut json_objects = Vec::new();
                    for object in objects {
                        let id = object.id.unwrap_or_else(|| {
                            let id = next_id;
                            next_id += 1;
                            id
                        });
                        if let Some(value) = object_to_tmj(object, id) {
                            json_objects.push(value);
                        } else {
                            dropped += 1;
                        }
                    }
                    layers.push(json!({
                        "type": "objectgroup", "id": id, "name": object_layer.name,
                        "x": 0, "y": 0, "opacity": 1, "visible": true,
                        "draworder": "topdown", "objects": json_objects,
                    }));
                }
                // Image layers echo back faithfully (path/offsets/visible/
                // opacity/name/properties), in layer order with the correct id,
                // so an in-game save never destroys a painted background or a
                // collision mask. The runtime `pixels` are deliberately not
                // serialised — they're decoded from `image` on load.
                TiledMapLayer::ImageLayer(image_layer) => {
                    let properties: Vec<Value> = image_layer
                        .properties
                        .iter()
                        .map(|p| json!({ "name": p.name, "type": "bool", "value": p.value }))
                        .collect();
                    let mut layer = json!({
                        "type": "imagelayer", "id": id, "name": image_layer.name,
                        "image": image_layer.image,
                        "offsetx": image_layer.offsetx, "offsety": image_layer.offsety,
                        "x": 0, "y": 0, "opacity": image_layer.opacity,
                        "visible": image_layer.visible,
                    });
                    // Tiled only emits `properties` when non-empty; match that so
                    // a propertyless layer round-trips byte-stable.
                    if !properties.is_empty() {
                        layer["properties"] = Value::Array(properties);
                    }
                    layers.push(layer);
                }
            }
        }
        if dropped > 0 {
            log::warn!(
                "{dropped} object(s) had no Tiled spelling (unnamed func, or sprite-less Interaction::None) and were dropped"
            );
        }
        let mut map = json!({
            "type": "map", "version": "1.11", "tiledversion": "1.11.2",
            "orientation": "orthogonal", "renderorder": "right-down",
            "compressionlevel": -1, "infinite": false,
            "width": self.width, "height": self.height,
            "tilewidth": 8, "tileheight": 8,
            "nextlayerid": self.layers.len() + 1,
            "nextobjectid": objects.len() + 1,
            "tilesets": self
                .tilesets
                .iter()
                .map(|t| json!({ "firstgid": t.firstgid, "source": t.source }))
                .collect::<Vec<_>>(),
            "layers": layers,
        });
        // Map-level properties (`bg_colour`, `camera_stick`) echo only when
        // present, again matching Tiled's omit-when-empty convention.
        if !self.properties.is_empty() {
            map["properties"] = properties_to_json(&self.properties);
        }
        // Pretty-print the structure (layers / objects / tilesets / properties)
        // but keep each tile layer's big flat `data` array compact on one line —
        // a readable, reviewable diff without thousands of one-number lines.
        to_pretty_compact_arrays(&map)
    }
}

/// Serialize `value` as JSON that is pretty-printed (two-space indent, matching
/// [`manifest_to_json`]) **except** for arrays whose elements are all scalars
/// (numbers / strings / bools / null), which stay inline on one line. In a Tiled
/// map this keeps the structure browsable while the one huge per-layer tile
/// `data` array — the only large scalar array — stays compact.
fn to_pretty_compact_arrays(value: &Value) -> String {
    let mut out = String::new();
    write_pretty(&mut out, value, 0);
    out
}

/// Recursive worker for [`to_pretty_compact_arrays`]. Objects and arrays that
/// contain a nested object/array expand one entry per line; scalars, empty
/// containers, and all-scalar arrays fall through to serde's compact form.
fn write_pretty(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (i, (k, v)) in map.iter().enumerate() {
                indent(out, depth + 1);
                out.push_str(&Value::String(k.clone()).to_string());
                out.push_str(": ");
                write_pretty(out, v, depth + 1);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            indent(out, depth);
            out.push('}');
        }
        Value::Array(items) if items.iter().any(|v| !is_scalar(v)) => {
            out.push_str("[\n");
            for (i, v) in items.iter().enumerate() {
                indent(out, depth + 1);
                write_pretty(out, v, depth + 1);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            indent(out, depth);
            out.push(']');
        }
        // Scalar, empty object/array, or all-scalar array (e.g. tile `data`).
        other => out.push_str(&other.to_string()),
    }
}

/// Push `depth` levels of two-space indentation.
fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// Whether `v` is a JSON scalar (not an array or object) — an array of only
/// these is printed inline by [`write_pretty`].
fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

// Tests for map serialization/deserialization:
#[cfg(test)]
mod tests {
    use super::{GameManifest, TiledMap, TiledMapLayer, from_json, manifest_from_json};
    use crate::world::interact::{InteractFn, Interaction};
    use crate::world::map::{Gate, MapObject, ObjectEffect, Trigger, WarpMode};
    use crate::render::image::RgbaImage;

    /// The single image layer of a parsed map (panics if it has none) — the
    /// fixture the image-layer tests pull `name`/`image`/`offsets` from.
    fn only_image_layer(map: &TiledMap) -> &super::ImageLayer {
        map.layers
            .iter()
            .find_map(|l| match l {
                TiledMapLayer::ImageLayer(i) => Some(i),
                _ => None,
            })
            .expect("map has an image layer")
    }

    /// The manifest parses and lists the maps to load.
    #[test]
    fn manifest_parses() {
        let json = r#"{
            "maps": ["office", "town"]
        }"#;
        let manifest: GameManifest = manifest_from_json(json.as_bytes()).unwrap();
        assert_eq!(manifest.maps, vec!["office", "town"]);
    }

    /// The real `assets/game.manifest` parses and names every shipping map, and
    /// deliberately excludes the backup map.
    #[test]
    fn real_manifest_parses() {
        let bytes = std::fs::read("../assets/game.manifest").unwrap();
        let manifest = manifest_from_json(&bytes).unwrap();
        assert!(manifest.maps.contains(&"office".to_string()));
        assert!(manifest.maps.contains(&"house_stairwell".to_string()));
        assert!(
            !manifest.maps.iter().any(|m| m.contains("backup")),
            "the backup map is not shipped"
        );
    }

    /// The destination-map name of an object's warp effect, or `None` if it
    /// isn't a warp.
    fn warp_map(object: &MapObject) -> Option<&str> {
        match &object.effect {
            ObjectEffect::Warp(w) => w.map.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn test_map_serialization() {
        let map = TiledMap {
            width: 10,
            height: 10,
            layers: Vec::new(),
            tilesets: Vec::new(),
            properties: Vec::new(),
        };
        let json = serde_json::to_string(&map).unwrap();
        println!("{}", json);
        let map2: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, map2.width);
        assert_eq!(map.height, map2.height);
    }
    #[test]
    fn test_map_deserialization() {
        let json = std::fs::read_to_string("../assets/maps/office.tmj").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, 28);
        assert_eq!(map.height, 16);
    }

    #[test]
    fn parses_office_interactables() {
        let json = std::fs::read_to_string("../assets/maps/office.tmj").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        let objects = map.parse_objects();
        // office.tmj is a real, play-tested map (its objects get edited), so this
        // pins only the invariant: it parses into objects that include dialogue
        // interactions — not an exact count or per-index identity, which churn.
        // Precise object parsing is covered by the synthetic-map tests above.
        assert!(!objects.is_empty(), "office parses some objects");
        assert!(
            objects
                .iter()
                .any(|o| matches!(&o.effect, ObjectEffect::Interact(Interaction::Dialogue(_)))),
            "office has dialogue interactions"
        );
    }

    #[test]
    fn parses_warp_object() {
        // A synthetic object layer with one warp object, encoded the way the
        // editor serialises warps (string-valued custom properties).
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 16, "y": 24, "width": 8, "height": 8, "type": "warp",
                    "properties": [
                        {"name": "to_map", "type": "string", "value": "4"},
                        {"name": "to_x", "type": "string", "value": "120"},
                        {"name": "to_y", "type": "string", "value": "40"},
                        {"name": "mode", "type": "string", "value": "auto"},
                        {"name": "sound", "type": "string", "value": "door"}
                    ]
                }]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();
        assert_eq!(objects.len(), 1);
        let object = &objects[0];
        // The trigger hitbox now lives on the MapObject.
        assert_eq!((object.hitbox.x, object.hitbox.y), (16, 24));
        let ObjectEffect::Warp(warp) = &object.effect else {
            panic!("the parsed object is a warp");
        };
        assert_eq!((warp.to.x, warp.to.y), (120, 40));
        // The `to_map` string is kept verbatim — resolution against the map
        // store happens in `map_by_name`, not here (a stale numeric like this
        // simply won't resolve there).
        assert_eq!(warp.map.as_deref(), Some("4"));
        assert!(matches!(warp.mode, WarpMode::Auto));
        assert!(warp.sound.is_some());
    }

    #[test]
    fn tmj_round_trips_office_objects() {
        let json = std::fs::read_to_string("../assets/maps/office.tmj").unwrap();
        let map = from_json(json.as_bytes()).unwrap();
        let objects = map.parse_objects();
        // Re-serialise (the map's tile layers hold the live flattened data),
        // then reload + reparse.
        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), objects.len());
        for (a, b) in objects.iter().zip(&objects2) {
            assert_eq!(
                (a.hitbox.x, a.hitbox.y, a.hitbox.w, a.hitbox.h),
                (b.hitbox.x, b.hitbox.y, b.hitbox.w, b.hitbox.h)
            );
            // The effect round-trips: a dialogue keeps its key, a warp its
            // destination (office has both kinds now), and the kind never flips.
            match (&a.effect, &b.effect) {
                (
                    ObjectEffect::Interact(Interaction::Dialogue(x)),
                    ObjectEffect::Interact(Interaction::Dialogue(y)),
                ) => assert_eq!(x, y),
                (ObjectEffect::Warp(x), ObjectEffect::Warp(y)) => {
                    assert_eq!(x.map, y.map);
                    assert_eq!((x.to.x, x.to.y), (y.to.x, y.to.y));
                }
                (ObjectEffect::Interact(_), ObjectEffect::Interact(_)) => {}
                _ => panic!("object effect kind changed across the round-trip"),
            }
        }
        // Flattened tile data is stable across the gid round-trip.
        let tile_layers = |m: &TiledMap| -> Vec<Vec<usize>> {
            m.layers
                .iter()
                .filter_map(|l| match l {
                    TiledMapLayer::TileLayer(t) => Some(t.data.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(tile_layers(&map), tile_layers(&reloaded));
    }

    /// A hand-mixed object layer (warp, interaction, warp) keeps its
    /// interleaving across parse → to_tmj → parse — the single ordered objects
    /// list, not a group-by-kind split, is what survives.
    #[test]
    fn tmj_preserves_object_order() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [
                    {
                        "x": 0, "y": 0, "width": 8, "height": 8, "type": "warp",
                        "properties": [{"name": "to_map", "type": "string", "value": "a"}]
                    },
                    {
                        "x": 8, "y": 0, "width": 8, "height": 8, "type": "",
                        "properties": [{"name": "description", "type": "string", "value": "mid"}]
                    },
                    {
                        "x": 16, "y": 0, "width": 8, "height": 8, "type": "warp",
                        "properties": [{"name": "to_map", "type": "string", "value": "b"}]
                    }
                ]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();
        // Parsed in file order: warp("a"), dialogue("mid"), warp("b").
        let kinds: Vec<Option<&str>> = objects.iter().map(warp_map).collect();
        assert_eq!(kinds, vec![Some("a"), None, Some("b")]);
        // The interleaving survives a serialise → reparse cycle.
        let out = map.to_tmj(&objects);
        let reloaded: TiledMap = serde_json::from_str(&out).unwrap();
        let objects2 = reloaded.parse_objects();
        let kinds2: Vec<Option<&str>> = objects2.iter().map(warp_map).collect();
        assert_eq!(kinds2, kinds);
        assert!(matches!(
            &objects2[1].effect,
            ObjectEffect::Interact(Interaction::Dialogue(k)) if k == "mid"
        ));
    }

    /// Tiled object ids survive parse → [`MapObject`] → serialise and are *not*
    /// the positional index: dropping a sibling never renumbers the survivors,
    /// and an id-less (runtime-created) object is assigned a fresh id above every
    /// existing one rather than colliding — the stability the save's `taken`
    /// keys rely on.
    #[test]
    fn tmj_preserves_and_assigns_object_ids() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [
                    {"id": 5, "x": 0, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [{"name": "description", "type": "string", "value": "a"}]},
                    {"id": 8, "x": 8, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [{"name": "description", "type": "string", "value": "b"}]},
                    {"id": 3, "x": 16, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [{"name": "description", "type": "string", "value": "c"}]}
                ]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();
        // Each object carries its own (non-positional) id in file order.
        assert_eq!(
            objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            vec![Some(5), Some(8), Some(3)]
        );

        // Drop the middle object and re-serialise: the survivors keep their ids
        // (a positional counter would have renumbered them to 1, 2).
        let trimmed = vec![objects[0].clone(), objects[2].clone()];
        let reloaded: TiledMap = serde_json::from_str(&map.to_tmj(&trimmed)).unwrap();
        assert_eq!(
            reloaded
                .parse_objects()
                .iter()
                .map(|o| o.id)
                .collect::<Vec<_>>(),
            vec![Some(5), Some(3)]
        );

        // An id-less (runtime-created) object is assigned a fresh id above every
        // existing one — max(5, 8, 3) + 1 = 9 — never colliding with a sibling.
        let mut with_new = objects.clone();
        let mut extra = objects[0].clone();
        extra.id = None;
        with_new.push(extra);
        let reloaded2: TiledMap = serde_json::from_str(&map.to_tmj(&with_new)).unwrap();
        assert_eq!(
            reloaded2
                .parse_objects()
                .iter()
                .map(|o| o.id)
                .collect::<Vec<_>>(),
            vec![Some(5), Some(8), Some(3), Some(9)]
        );
    }

    /// The `removable` marker round-trips parse → [`MapObject`] → serialise: a
    /// flagged object parses `removable == true` and re-emits the property, while
    /// an unflagged sibling stays `false` and carries nothing.
    #[test]
    fn tmj_round_trips_removable_marker() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [
                    {"id": 1, "x": 0, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [
                        {"name": "description", "type": "string", "value": "key"},
                        {"name": "removable", "type": "string", "value": "true"}
                     ]},
                    {"id": 2, "x": 8, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [{"name": "description", "type": "string", "value": "sign"}]}
                ]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();
        assert_eq!(
            objects.iter().map(|o| o.removable).collect::<Vec<_>>(),
            vec![true, false]
        );
        // The marker survives a serialise → reparse cycle, still only on the
        // flagged object.
        let reloaded: TiledMap = serde_json::from_str(&map.to_tmj(&objects)).unwrap();
        assert_eq!(
            reloaded
                .parse_objects()
                .iter()
                .map(|o| o.removable)
                .collect::<Vec<_>>(),
            vec![true, false]
        );
    }

    /// Data-loss guard for the collect-then-save path: a removable pickup the
    /// player has collected stays in the live map object list (it's skipped at
    /// use-time, not removed — see
    /// [`WalkaroundState::take_object`](crate::gamestate::walkaround::WalkaroundState)),
    /// so serialising the map from the editor still writes it out. Here the whole
    /// object list (pickup id 5 + sign id 2) is handed to `to_tmj`, standing in
    /// for the editor saving a map whose id-5 pickup is already taken; both
    /// objects survive with their ids, so no authored data is dropped.
    #[test]
    fn to_tmj_keeps_collected_pickup() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [
                    {"id": 5, "x": 0, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [
                        {"name": "description", "type": "string", "value": "key"},
                        {"name": "removable", "type": "string", "value": "true"}
                     ]},
                    {"id": 2, "x": 8, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [{"name": "description", "type": "string", "value": "sign"}]}
                ]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();

        // The collected pickup is still in the list saved out (no load-time
        // filter); to_tmj writes both objects with their ids and the removable
        // marker on the pickup — the pickup is not lost.
        let reloaded: TiledMap = serde_json::from_str(&map.to_tmj(&objects)).unwrap();
        let out = reloaded.parse_objects();
        assert_eq!(
            out.iter().map(|o| o.id).collect::<Vec<_>>(),
            vec![Some(5), Some(2)],
            "the taken pickup is written out alongside the sign"
        );
        assert_eq!(
            out.iter().map(|o| o.removable).collect::<Vec<_>>(),
            vec![true, false],
            "the pickup keeps its removable marker"
        );
    }

    #[test]
    fn tmj_round_trips_warp() {
        let json = r#"{
            "width": 2, "height": 2,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 16, "y": 24, "width": 8, "height": 8, "type": "warp",
                    "properties": [
                        {"name": "to_map", "type": "string", "value": "4"},
                        {"name": "to_x", "type": "string", "value": "120"},
                        {"name": "to_y", "type": "string", "value": "40"},
                        {"name": "flip", "type": "string", "value": "y"},
                        {"name": "mode", "type": "string", "value": "auto"},
                        {"name": "sound", "type": "string", "value": "door"}
                    ]
                }]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();
        let out = map.to_tmj(&objects);
        let reloaded: TiledMap = serde_json::from_str(&out).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), 1);
        let (a, b) = (&objects[0], &objects2[0]);
        let (ObjectEffect::Warp(aw), ObjectEffect::Warp(bw)) = (&a.effect, &b.effect) else {
            panic!("both objects are warps");
        };
        assert_eq!((aw.to.x, aw.to.y), (bw.to.x, bw.to.y));
        assert_eq!(aw.map, bw.map);
        // The trigger hitbox round-trips through the owning MapObject.
        assert_eq!(
            (a.hitbox.x, a.hitbox.y, a.hitbox.w, a.hitbox.h),
            (b.hitbox.x, b.hitbox.y, b.hitbox.w, b.hitbox.h)
        );
        assert!(matches!(bw.mode, WarpMode::Auto));
        assert!(bw.sound.is_some());
        assert!(bw.flip.y());
    }

    /// A warp whose `to_map` is a map *name* survives serialise → reparse with
    /// the name intact (names are the only map identity now).
    #[test]
    fn tmj_round_trips_named_warp() {
        let json = r#"{
            "width": 2, "height": 2,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 8, "y": 8, "width": 8, "height": 8, "type": "warp",
                    "properties": [
                        {"name": "to_map", "type": "string", "value": "supermarket_hall"},
                        {"name": "to_x", "type": "string", "value": "72"},
                        {"name": "to_y", "type": "string", "value": "32"}
                    ]
                }]
            }]
        }"#;
        let map: TiledMap = serde_json::from_str(json).unwrap();
        let objects = map.parse_objects();
        assert_eq!(warp_map(&objects[0]), Some("supermarket_hall"));
        let out = map.to_tmj(&objects);
        let reloaded: TiledMap = serde_json::from_str(&out).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), 1);
        assert_eq!(warp_map(&objects2[0]), Some("supermarket_hall"));
        let ObjectEffect::Warp(warp) = &objects2[0].effect else {
            panic!("the round-tripped object is a warp");
        };
        assert_eq!((warp.to.x, warp.to.y), (72, 32));
    }

    /// A saved map pretty-prints its structure but keeps each tile layer's flat
    /// `data` array compact on one line — a readable, reviewable diff.
    #[test]
    fn to_tmj_is_pretty_with_compact_tile_data() {
        let map = TiledMap::blank_modern(4, 3);
        let out = map.to_tmj(&[]);
        // Structure is indented and multi-line.
        assert!(
            out.contains("\n  \"layers\""),
            "expected indented keys:\n{out}"
        );
        // The 4×3 tile data sits inline, not one number per line.
        assert!(
            out.contains("\"data\": [0,0,0,0,0,0,0,0,0,0,0,0]"),
            "expected a compact data array:\n{out}"
        );
        // And it still parses back to an equivalent map.
        let reloaded: TiledMap = serde_json::from_str(&out).unwrap();
        assert_eq!((reloaded.width, reloaded.height), (4, 3));
        assert_eq!(reloaded.layers.len(), map.layers.len());
    }

    /// The runtime `InteractFn` of an object's interaction effect, if it is one.
    fn func(object: &MapObject) -> Option<&InteractFn> {
        match &object.effect {
            ObjectEffect::Interact(Interaction::Func(f)) => Some(f),
            _ => None,
        }
    }

    /// A `func` object with no scalar properties (`toggle_dog`) and one with a
    /// scalar property (`note`/`pitch`) both round-trip through serialise →
    /// reparse, name and scalar intact.
    #[test]
    fn tmj_round_trips_func_objects() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [
                    {
                        "x": 8, "y": 8, "width": 8, "height": 8, "type": "",
                        "properties": [{"name": "func", "type": "string", "value": "toggle_dog"}]
                    },
                    {
                        "x": 16, "y": 24, "width": 8, "height": 8, "type": "",
                        "properties": [
                            {"name": "func", "type": "string", "value": "note"},
                            {"name": "pitch", "type": "string", "value": "7"}
                        ]
                    }
                ]
            }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        let objects = map.parse_objects();
        assert_eq!(objects.len(), 2);
        assert!(matches!(func(&objects[0]), Some(InteractFn::ToggleDog)));
        assert!(matches!(func(&objects[1]), Some(InteractFn::Note(7))));

        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), 2);
        assert!(matches!(func(&objects2[0]), Some(InteractFn::ToggleDog)));
        assert!(matches!(func(&objects2[1]), Some(InteractFn::Note(7))));
        // Positions are preserved (the piano's origin proves the hitbox is the
        // source of truth; toggle_dog's hitbox is checked here as a stand-in).
        assert_eq!((objects2[0].hitbox.x, objects2[0].hitbox.y), (8, 8));
        assert_eq!((objects2[1].hitbox.x, objects2[1].hitbox.y), (16, 24));
    }

    /// A `give_item` func round-trips its `item` string property (the granted
    /// item's registry key) through serialise → reparse, the key intact.
    #[test]
    fn tmj_round_trips_give_item_func() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 8, "y": 8, "width": 8, "height": 8, "type": "",
                    "properties": [
                        {"name": "func", "type": "string", "value": "give_item"},
                        {"name": "item", "type": "string", "value": "chegg"}
                    ]
                }]
            }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        let objects = map.parse_objects();
        assert_eq!(
            func(&objects[0]),
            Some(&InteractFn::GiveItem("chegg".to_string()))
        );

        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(
            func(&objects2[0]),
            Some(&InteractFn::GiveItem("chegg".to_string()))
        );
    }

    /// A `piano` func takes its origin from the hitbox (no property), so the
    /// round-trip must reconstruct the origin from the placed rectangle.
    #[test]
    fn tmj_round_trips_piano_origin_from_hitbox() {
        let json = r#"{
            "width": 8, "height": 8,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 32, "y": 8, "width": 40, "height": 24, "type": "",
                    "properties": [{"name": "func", "type": "string", "value": "piano"}]
                }]
            }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        let objects = map.parse_objects();
        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), 1);
        // The origin is the hitbox top-left, reconstructed identically.
        assert!(matches!(func(&objects2[0]), Some(InteractFn::Piano(o)) if (o.x, o.y) == (32, 8)));
    }

    /// A sprite-only object (a `sprite` tile id, no description/func/warp) parses
    /// to an `Interaction::None` carrying the sprite, and round-trips as such —
    /// what legacy animation-only objects (the living-room TV) rely on.
    #[test]
    fn tmj_round_trips_sprite_only_object() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 8, "y": 16, "width": 16, "height": 16, "type": "",
                    "properties": [{"name": "sprite", "type": "string", "value": "524"}]
                }]
            }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        let objects = map.parse_objects();
        assert_eq!(objects.len(), 1);
        assert!(matches!(
            objects[0].effect,
            ObjectEffect::Interact(Interaction::None)
        ));
        assert_eq!(objects[0].sprite.as_ref().unwrap()[0].spr_id, 524);

        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), 1);
        assert!(matches!(
            objects2[0].effect,
            ObjectEffect::Interact(Interaction::None)
        ));
        assert_eq!(objects2[0].sprite.as_ref().unwrap()[0].spr_id, 524);
        assert_eq!((objects2[0].hitbox.x, objects2[0].hitbox.y), (8, 16),);
    }

    /// A `cutscene` property parses to an [`Interaction::Cutscene`] carrying the
    /// registry name verbatim, and round-trips back to the same `cutscene`
    /// property (the trigger object the cutscene authoring path relies on).
    #[test]
    fn tmj_round_trips_cutscene_object() {
        let json = r#"{
            "width": 4, "height": 4,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [{
                    "x": 8, "y": 16, "width": 16, "height": 16, "type": "",
                    "properties": [{"name": "cutscene", "type": "string", "value": "pet_dog"}]
                }]
            }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        let objects = map.parse_objects();
        assert_eq!(objects.len(), 1);
        assert!(matches!(
            &objects[0].effect,
            ObjectEffect::Interact(Interaction::Cutscene(n)) if n == "pet_dog"
        ));

        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let objects2 = reloaded.parse_objects();
        assert_eq!(objects2.len(), 1);
        assert!(matches!(
            &objects2[0].effect,
            ObjectEffect::Interact(Interaction::Cutscene(n)) if n == "pet_dog"
        ));
        assert_eq!((objects2[0].hitbox.x, objects2[0].hitbox.y), (8, 16));
    }

    /// The pre-warp narration key of an object's warp effect, if it has one.
    fn warp_narration(object: &MapObject) -> Option<&str> {
        match &object.effect {
            ObjectEffect::Warp(w) => w.narration.as_deref(),
            _ => None,
        }
    }

    /// One object-layer map from a single object's `(type, [properties])` —
    /// scaffolding for the trigger/narration property tests.
    fn one_object_map(class: &str, properties: &str) -> TiledMap {
        let json = format!(
            r#"{{
                "width": 2, "height": 2,
                "tilesets": [{{"firstgid": 1, "source": "tiles.tsj"}}],
                "layers": [{{
                    "type": "objectgroup", "name": "Object Layer 1",
                    "objects": [{{
                        "x": 8, "y": 8, "width": 8, "height": 8, "type": "{class}",
                        "properties": [{properties}]
                    }}]
                }}]
            }}"#
        );
        from_json(json.as_bytes()).unwrap()
    }

    /// A `trigger` property parses on *any* object kind and overrides the
    /// effect-kind default; an absent or unrecognised value keeps the default.
    #[test]
    fn parses_trigger_property() {
        // Explicit trigger on a warp (whose default is Any) and an interaction
        // (whose default is Press).
        let warp = one_object_map(
            "warp",
            r#"{"name":"to_map","type":"string","value":"a"},
               {"name":"trigger","type":"string","value":"touch"}"#,
        );
        assert_eq!(warp.parse_objects()[0].trigger, Trigger::Touch);
        let dialogue = one_object_map(
            "",
            r#"{"name":"description","type":"string","value":"k"},
               {"name":"trigger","type":"string","value":"any"}"#,
        );
        assert_eq!(dialogue.parse_objects()[0].trigger, Trigger::Any);

        // Absent → effect-kind default (warp = Any, interaction = Press).
        let warp_def = one_object_map("warp", r#"{"name":"to_map","type":"string","value":"a"}"#);
        assert_eq!(warp_def.parse_objects()[0].trigger, Trigger::Any);
        let dialogue_def =
            one_object_map("", r#"{"name":"description","type":"string","value":"k"}"#);
        assert_eq!(dialogue_def.parse_objects()[0].trigger, Trigger::Press);

        // Unknown value → effect-kind default (silently ignored, door still works).
        let bogus = one_object_map(
            "warp",
            r#"{"name":"to_map","type":"string","value":"a"},
               {"name":"trigger","type":"string","value":"sideways"}"#,
        );
        assert_eq!(bogus.parse_objects()[0].trigger, Trigger::Any);
    }

    /// A *non-default* trigger serialises and round-trips, while a *default*
    /// trigger emits no `trigger` property at all — so existing files (which
    /// have none) stay byte-stable.
    #[test]
    fn tmj_round_trips_non_default_trigger_only() {
        // Non-default: a press-triggered warp (default is Any).
        let map = one_object_map(
            "warp",
            r#"{"name":"to_map","type":"string","value":"a"},
               {"name":"trigger","type":"string","value":"press"}"#,
        );
        let objects = map.parse_objects();
        assert_eq!(objects[0].trigger, Trigger::Press);
        let out = map.to_tmj(&objects);
        assert!(
            out.contains("\"trigger\""),
            "non-default trigger is serialised"
        );
        let reloaded = from_json(out.as_bytes()).unwrap();
        assert_eq!(reloaded.parse_objects()[0].trigger, Trigger::Press);

        // Default: a warp left at Any emits no `trigger` property.
        let def = one_object_map("warp", r#"{"name":"to_map","type":"string","value":"a"}"#);
        let def_objects = def.parse_objects();
        assert_eq!(def_objects[0].trigger, Trigger::Any);
        let def_out = def.to_tmj(&def_objects);
        assert!(
            !def_out.contains("\"trigger\""),
            "a default trigger must not be serialised (byte-stable round-trip)"
        );
        // And a default-trigger interaction likewise emits nothing.
        let di = one_object_map("", r#"{"name":"description","type":"string","value":"k"}"#);
        let di_objects = di.parse_objects();
        assert!(!di.to_tmj(&di_objects).contains("\"trigger\""));
    }

    /// A warp `narration` key round-trips (absent → `None`, present → the key),
    /// and an empty narration value is treated as absent.
    #[test]
    fn tmj_round_trips_warp_narration() {
        let map = one_object_map(
            "warp",
            r#"{"name":"to_map","type":"string","value":"a"},
               {"name":"narration","type":"string","value":"door_creaks"}"#,
        );
        let objects = map.parse_objects();
        assert_eq!(warp_narration(&objects[0]), Some("door_creaks"));
        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        assert_eq!(
            warp_narration(&reloaded.parse_objects()[0]),
            Some("door_creaks")
        );

        // Absent narration → None, and nothing serialised.
        let plain = one_object_map("warp", r#"{"name":"to_map","type":"string","value":"a"}"#);
        let plain_objects = plain.parse_objects();
        assert_eq!(warp_narration(&plain_objects[0]), None);
        assert!(!plain.to_tmj(&plain_objects).contains("narration"));

        // An empty value is treated as no narration.
        let empty = one_object_map(
            "warp",
            r#"{"name":"to_map","type":"string","value":"a"},
               {"name":"narration","type":"string","value":""}"#,
        );
        assert_eq!(warp_narration(&empty.parse_objects()[0]), None);
    }

    /// The flag gate (`if` / `unless` / `sets`) parses on any object kind and
    /// round-trips all three flag names; an ungated object carries the default
    /// gate and emits none of the properties (byte-stable, like a default trigger).
    #[test]
    fn tmj_round_trips_gate_conditions() {
        let map = one_object_map(
            "",
            r#"{"name":"description","type":"string","value":"k"},
               {"name":"if","type":"string","value":"has_key"},
               {"name":"unless","type":"string","value":"door_open"},
               {"name":"sets","type":"string","value":"door_open"}"#,
        );
        let objects = map.parse_objects();
        assert_eq!(objects[0].gate.if_flag.as_deref(), Some("has_key"));
        assert_eq!(objects[0].gate.unless_flag.as_deref(), Some("door_open"));
        assert_eq!(objects[0].gate.sets.as_deref(), Some("door_open"));

        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let gate = reloaded.parse_objects()[0].gate.clone();
        assert_eq!(gate.if_flag.as_deref(), Some("has_key"));
        assert_eq!(gate.unless_flag.as_deref(), Some("door_open"));
        assert_eq!(gate.sets.as_deref(), Some("door_open"));

        // An ungated object: default gate, and none of the gate properties are
        // serialised (so an existing file with no gate stays byte-stable).
        let plain = one_object_map("", r#"{"name":"description","type":"string","value":"k"}"#);
        let plain_objects = plain.parse_objects();
        assert_eq!(plain_objects[0].gate, Gate::default());
        let plain_out = plain.to_tmj(&plain_objects);
        assert!(!plain_out.contains("\"unless\""), "no unless emitted");
        assert!(!plain_out.contains("\"sets\""), "no sets emitted");
        assert_eq!(
            from_json(plain_out.as_bytes()).unwrap().parse_objects()[0].gate,
            Gate::default(),
            "ungated round-trip stays ungated"
        );

        // An empty gate value is treated as absent (that condition unset).
        let empty = one_object_map(
            "",
            r#"{"name":"description","type":"string","value":"k"},
               {"name":"if","type":"string","value":""}"#,
        );
        assert_eq!(empty.parse_objects()[0].gate.if_flag, None);
    }

    /// A `trigger: "enter"` (the map-enter hook) parses to [`Trigger::Enter`] and
    /// round-trips — it's never an effect-kind default, so it always serialises.
    #[test]
    fn tmj_round_trips_enter_trigger() {
        let map = one_object_map(
            "",
            r#"{"name":"cutscene","type":"string","value":"intro"},
               {"name":"trigger","type":"string","value":"enter"}"#,
        );
        let objects = map.parse_objects();
        assert_eq!(objects[0].trigger, Trigger::Enter);
        assert!(matches!(
            &objects[0].effect,
            ObjectEffect::Interact(Interaction::Cutscene(n)) if n == "intro"
        ));
        let out = map.to_tmj(&objects);
        let reloaded = from_json(out.as_bytes()).unwrap();
        assert_eq!(reloaded.parse_objects()[0].trigger, Trigger::Enter);
    }

    /// A tile read/write past the right edge (`x >= width`) returns `None`
    /// instead of wrapping into the next row — the paint-drag overflow bug.
    #[test]
    fn tile_layer_guards_the_x_edge() {
        let mut layer = super::TileLayer {
            width: 4,
            height: 3,
            data: vec![0usize; 12],
            name: String::new(),
            offsetx: 0.0,
            offsety: 0.0,
            properties: Vec::new(),
        };
        // The right-edge cell (3, 0) is in bounds.
        *layer.get_mut(3, 0).unwrap() = 7;
        assert_eq!(layer.get(3, 0), Some(7));
        // x == width (raw index 4 = cell (0, 1)) must not wrap.
        assert_eq!(layer.get(4, 0), None);
        assert!(layer.get_mut(4, 0).is_none());
        assert_eq!(layer.get(0, 1), Some(0), "the wrap target stayed untouched");
    }

    /// Editor layer ops: add inserts a drawable tile layer before the object
    /// layer; delete and move both refuse to touch the collision (first tile)
    /// layer that the colliders derive from.
    #[test]
    fn layer_ops_protect_the_collision_layer() {
        fn names(m: &TiledMap) -> Vec<&str> {
            m.layers
                .iter()
                .filter_map(|l| match l {
                    TiledMapLayer::TileLayer(t) => Some(t.name.as_str()),
                    TiledMapLayer::ObjectLayer(o) => Some(o.name.as_str()),
                    _ => None,
                })
                .collect()
        }

        let mut m = TiledMap::blank_modern(4, 4);
        // blank_modern = [collision (tile), "Layer 1" (tile), objects].
        assert_eq!(names(&m), vec!["collision", "Layer 1", "objects"]);
        assert_eq!(m.collision_layer(), Some(0));

        // Add: a new tile layer lands before the object layer.
        m.add_tile_layer("extra");
        assert_eq!(names(&m), vec!["collision", "Layer 1", "extra", "objects"]);

        // Delete: the collision layer (0) is protected; a drawable one goes.
        m.remove_layer(0);
        assert_eq!(m.layers.len(), 4, "collision delete refused");
        m.remove_layer(1);
        assert_eq!(names(&m), vec!["collision", "extra", "objects"]);

        // Move: neither the collision layer nor a swap into its slot is allowed
        // (both refused, so the order is unchanged).
        m.move_layer(0, false); // collision down — refused
        m.move_layer(1, true); // would swap with collision — refused
        assert_eq!(names(&m), vec!["collision", "extra", "objects"]);
        // A legal move swaps two non-collision layers.
        m.add_tile_layer("third");
        m.move_layer(2, true); // swap "third" up with "extra"
        assert_eq!(names(&m), vec!["collision", "third", "extra", "objects"]);
    }

    /// Drag-reorder (`reorder_layer`): a multi-slot move slides the layers
    /// between, the collision layer stays put, a move across it clamps, and the
    /// op cleanly inverts (the undo direction).
    #[test]
    fn reorder_layer_moves_and_protects_collision() {
        fn names(m: &TiledMap) -> Vec<&str> {
            m.layers
                .iter()
                .filter_map(|l| match l {
                    TiledMapLayer::TileLayer(t) => Some(t.name.as_str()),
                    TiledMapLayer::ObjectLayer(o) => Some(o.name.as_str()),
                    _ => None,
                })
                .collect()
        }

        let mut m = TiledMap::blank_modern(4, 4);
        m.add_tile_layer("a");
        m.add_tile_layer("b");
        // [collision, "Layer 1", a, b, objects], collision at 0.
        assert_eq!(names(&m), vec!["collision", "Layer 1", "a", "b", "objects"]);

        // Slide "b" (3) up to index 1: the layers between shift down.
        assert_eq!(m.reorder_layer(3, 1), Some((3, 1)));
        assert_eq!(names(&m), vec!["collision", "b", "Layer 1", "a", "objects"]);
        assert_eq!(m.collision_layer(), Some(0));

        // Invert it (the undo direction): "b" is now at 1, move it back to 3.
        assert_eq!(m.reorder_layer(1, 3), Some((1, 3)));
        assert_eq!(names(&m), vec!["collision", "Layer 1", "a", "b", "objects"]);

        // Moving the collision layer is refused outright.
        assert_eq!(m.reorder_layer(0, 2), None);
        // Dropping onto/above the collision slot clamps to just below it, so
        // collision stays first (here a no-op: "Layer 1" is already at 1).
        assert_eq!(m.reorder_layer(1, 0), None);
        assert_eq!(names(&m), vec!["collision", "Layer 1", "a", "b", "objects"]);
        // From a deeper layer the clamp still lands it just under collision.
        assert_eq!(m.reorder_layer(3, 0), Some((3, 1)));
        assert_eq!(names(&m), vec!["collision", "b", "Layer 1", "a", "objects"]);

        // Degenerate / out-of-range moves do nothing.
        assert_eq!(m.reorder_layer(2, 2), None);
        assert_eq!(m.reorder_layer(9, 1), None);
    }

    /// The Setup panel's data ops: bg_colour / camera_stick round-trip through the
    /// property list (replacing in place), and resize reflows tile layers.
    #[test]
    fn setup_properties_and_resize() {
        let mut m = TiledMap::blank_modern(4, 3);

        m.set_bg_colour(7);
        assert_eq!(m.bg_colour(), Some(7));
        m.set_camera_stick(Some((10, 20)));
        assert_eq!(m.camera_stick(), Some((10, 20)));
        m.set_camera_stick(None);
        assert_eq!(m.camera_stick(), None);
        // Re-setting replaces in place (no duplicate property).
        m.set_bg_colour(3);
        assert_eq!(m.bg_colour(), Some(3));
        assert_eq!(
            m.properties
                .iter()
                .filter(|p| p.name == "bg_colour")
                .count(),
            1
        );

        // Resize: top-left anchored, in-bounds cells preserved, new cells empty.
        m.set(1, 0, 0, 5);
        m.set(1, 3, 2, 9);
        m.resize(6, 5);
        assert_eq!((m.width, m.height), (6, 5));
        assert_eq!(m.get(1, 0, 0), Some(5));
        assert_eq!(m.get(1, 3, 2), Some(9));
        assert_eq!(m.get(1, 5, 4), Some(0));
        if let TiledMapLayer::TileLayer(tl) = &m.layers[1] {
            assert_eq!(tl.data.len(), 30);
        } else {
            panic!("layer 1 is a tile layer");
        }
        // Shrinking drops out-of-bounds cells.
        m.resize(2, 2);
        assert_eq!((m.width, m.height), (2, 2));
        assert_eq!(m.get(1, 0, 0), Some(5));
        assert_eq!(m.get(1, 1, 1), Some(0));
    }

    /// A map's `music` name round-trips and resolves to a track by name (unknown
    /// names resolve to `None`, like a dangling warp); a tile layer's offset and
    /// `palette_rotate` set/clear correctly (rotate 0 drops the property).
    #[test]
    fn music_and_layer_props_round_trip() {
        let mut m = TiledMap::blank_modern(2, 2);

        assert_eq!(m.music(), None);
        m.set_music(Some("supermarket"));
        let reloaded: TiledMap = serde_json::from_str(&m.to_tmj(&[])).unwrap();
        assert_eq!(reloaded.music(), Some("supermarket"));
        m.set_music(None);
        assert_eq!(m.music(), None);

        // Layer 1 offset + palette rotation.
        m.set_layer_offset_x(1, 3.0);
        m.set_layer_offset_y(1, -2.0);
        m.set_layer_palette_rotate(1, 5);
        let reloaded: TiledMap = serde_json::from_str(&m.to_tmj(&[])).unwrap();
        assert_eq!(reloaded.layer_offset(1), Some((3.0, -2.0)));
        assert_eq!(reloaded.layer_palette_rotate(1), 5);
        // Rotate 0 drops the property (no empty round-trip noise).
        m.set_layer_palette_rotate(1, 0);
        if let TiledMapLayer::TileLayer(t) = &m.layers[1] {
            assert!(t.properties.iter().all(|p| p.name != "palette_rotate"));
        }
    }

    /// The real `assets/maps/bedroom1.tmj` now parses (it has an image layer,
    /// which used to fail the whole parse) — and it's the first painted-art
    /// map: an office-style tile collision layer plus its wall art as an image
    /// layer, with a modern `warp`-typed object out to house_stairwell.
    #[test]
    fn parses_bedroom1_image_layer() {
        let bytes = std::fs::read("../assets/maps/bedroom1.tmj").unwrap();
        let map = from_json(&bytes).unwrap();
        // 4 tile layers, 1 image layer, 1 object layer = 6 layers.
        assert_eq!(map.layers.len(), 6);
        let image = only_image_layer(&map);
        assert_eq!(image.name, "walls");
        assert_eq!(image.image, "images/bedroom1_walls.png");
        assert_eq!((image.offsetx, image.offsety), (14.0, 15.0));
        // (`visible` deliberately unasserted — it's live authoring state the
        // user toggles in Tiled, not parse behaviour.)
        // Pixels are runtime-only: never filled by the parser.
        assert!(image.pixels.is_none());
        // "walls" is painted *art*, not a collision mask — collision stays on
        // the tile layer.
        assert!(!image.is_collision());
        // The image layer is enumerated for the host to load.
        assert_eq!(map.image_layer_paths(), vec!["images/bedroom1_walls.png"]);
        // The room keeps its touch warp to house_stairwell (alongside whatever
        // interactables have since been authored in the in-game editor — so we
        // find the warp rather than assert an exact object count).
        let objects = map.parse_objects();
        let warp = objects
            .iter()
            .find(|o| matches!(&o.effect, ObjectEffect::Warp(_)))
            .expect("the stairwell warp");
        assert_eq!(warp.trigger, Trigger::Touch);
        match &warp.effect {
            ObjectEffect::Warp(w) => assert_eq!(w.map.as_deref(), Some("house_stairwell")),
            _ => unreachable!(),
        }
    }

    /// The exported `assets/maps/house_stairwell.tmj` keeps the user's tracing
    /// mask: a single (now hidden, non-collision) image layer at its positive
    /// offset, appended after the exported collision/art/object layers.
    #[test]
    fn parses_house_stairwell_image_layer() {
        let bytes = std::fs::read("../assets/maps/house_stairwell.tmj").unwrap();
        let map = from_json(&bytes).unwrap();
        let image = only_image_layer(&map);
        assert_eq!(image.name, "Image Layer 1");
        assert_eq!(image.image, "images/house_stairwell_mask.png");
        assert_eq!((image.offsetx, image.offsety), (74.0, 33.0));
        assert!(!image.visible, "the tracing mask is preserved but hidden");
        assert!(!image.is_collision());
    }

    /// An image layer survives serialise → reparse with its path, offsets,
    /// visibility, opacity and name intact — so an in-game save never destroys a
    /// painted background or a collision mask. Checked on the real bedroom1 map
    /// (a tile + object + image layer mix) so layer *order* round-trips too.
    #[test]
    fn tmj_round_trips_image_layer() {
        let bytes = std::fs::read("../assets/maps/bedroom1.tmj").unwrap();
        let map = from_json(&bytes).unwrap();
        let out = map.to_tmj(&map.parse_objects());
        let reloaded = from_json(out.as_bytes()).unwrap();
        // Same layer count and the image layer still second (order preserved —
        // it sits *between* tile layers, not appended at the end).
        assert_eq!(reloaded.layers.len(), map.layers.len());
        assert!(matches!(reloaded.layers[1], TiledMapLayer::ImageLayer(_)));
        let before = only_image_layer(&map);
        let after = only_image_layer(&reloaded);
        assert_eq!(after.name, before.name);
        assert_eq!(after.image, before.image);
        assert_eq!(
            (after.offsetx, after.offsety),
            (before.offsetx, before.offsety)
        );
        assert_eq!(after.visible, before.visible);
        assert_eq!(after.opacity, before.opacity);
    }

    /// A small map with one image layer round-trips its `opacity`/`visible`
    /// values faithfully even when they aren't the Tiled defaults — proving
    /// they're carried, not assumed.
    #[test]
    fn tmj_round_trips_image_layer_nondefault_fields() {
        let json = r#"{
            "width": 4, "height": 4, "tilesets": [],
            "layers": [{
                "type": "imagelayer", "name": "bg", "id": 1,
                "image": "images/room.png",
                "offsetx": 12, "offsety": -8,
                "visible": false, "opacity": 0.5
            }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        let image = only_image_layer(&map);
        assert!(!image.visible);
        assert_eq!(image.opacity, 0.5);

        let out = map.to_tmj(&[]);
        let reloaded = from_json(out.as_bytes()).unwrap();
        let after = only_image_layer(&reloaded);
        assert!(!after.visible);
        assert_eq!(after.opacity, 0.5);
        assert_eq!((after.offsetx, after.offsety), (12.0, -8.0));
    }

    /// Collision marking: an image layer is a mask when its name starts with
    /// `collision` (case-insensitive) OR it carries a `collision: true` bool
    /// property; anything else is a drawn layer.
    #[test]
    fn image_layer_collision_marking() {
        // By name prefix (case-insensitive), no property needed.
        let by_name = r#"{
            "width": 2, "height": 2, "tilesets": [],
            "layers": [{ "type": "imagelayer", "name": "Collision Mask", "id": 1,
                         "image": "m.png", "offsetx": 0, "offsety": 0 }]
        }"#;
        assert!(only_image_layer(&from_json(by_name.as_bytes()).unwrap()).is_collision());

        // By explicit `collision: true` property on a layer named anything.
        let by_prop = r#"{
            "width": 2, "height": 2, "tilesets": [],
            "layers": [{ "type": "imagelayer", "name": "blockers", "id": 1,
                         "image": "m.png", "offsetx": 0, "offsety": 0,
                         "properties": [{ "name": "collision", "type": "bool", "value": true }] }]
        }"#;
        assert!(only_image_layer(&from_json(by_prop.as_bytes()).unwrap()).is_collision());

        // A `collision: false` property does not mark it.
        let prop_false = r#"{
            "width": 2, "height": 2, "tilesets": [],
            "layers": [{ "type": "imagelayer", "name": "art", "id": 1,
                         "image": "m.png", "offsetx": 0, "offsety": 0,
                         "properties": [{ "name": "collision", "type": "bool", "value": false }] }]
        }"#;
        assert!(!only_image_layer(&from_json(prop_false.as_bytes()).unwrap()).is_collision());

        // A plainly-named, propertyless layer is drawn, not collision.
        let plain = r#"{
            "width": 2, "height": 2, "tilesets": [],
            "layers": [{ "type": "imagelayer", "name": "background", "id": 1,
                         "image": "m.png", "offsetx": 0, "offsety": 0 }]
        }"#;
        assert!(!only_image_layer(&from_json(plain.as_bytes()).unwrap()).is_collision());
    }

    /// The collision `properties` round-trip through serialise → reparse, so a
    /// `collision: true` marker authored in-engine survives a save.
    #[test]
    fn tmj_round_trips_image_layer_collision_property() {
        let json = r#"{
            "width": 2, "height": 2, "tilesets": [],
            "layers": [{ "type": "imagelayer", "name": "blockers", "id": 1,
                         "image": "m.png", "offsetx": 0, "offsety": 0,
                         "properties": [{ "name": "collision", "type": "bool", "value": true }] }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        assert!(only_image_layer(&map).is_collision());
        let out = map.to_tmj(&[]);
        let reloaded = from_json(out.as_bytes()).unwrap();
        assert!(only_image_layer(&reloaded).is_collision());
    }

    /// `attach_image` fills the runtime pixels of every layer matching the path,
    /// keyed by the authored relative path (the codec never fills them itself).
    #[test]
    fn attach_image_by_path() {
        let json = r#"{
            "width": 2, "height": 2, "tilesets": [],
            "layers": [{ "type": "imagelayer", "name": "bg", "id": 1,
                         "image": "images/room.png", "offsetx": 0, "offsety": 0 }]
        }"#;
        let mut map = from_json(json.as_bytes()).unwrap();
        assert!(only_image_layer(&map).pixels.is_none());
        map.attach_image("images/room.png", RgbaImage::new(8, 8));
        assert!(only_image_layer(&map).pixels.is_some());
        // A path that matches nothing is a harmless no-op.
        map.attach_image("images/other.png", RgbaImage::new(8, 8));
        assert!(only_image_layer(&map).pixels.is_some());
    }

    /// The first tile layer of a parsed map (panics if it has none).
    fn first_tile_layer(map: &TiledMap) -> &super::TileLayer {
        map.layers
            .iter()
            .find_map(|l| match l {
                TiledMapLayer::TileLayer(t) => Some(t),
                _ => None,
            })
            .expect("map has a tile layer")
    }

    /// A tile layer's pixel offset (`offsetx`/`offsety`) parses and round-trips —
    /// previously dropped on save, which is the motivating fix (the user's
    /// bedroom1 bed layer sits at offset (−3, 3)). Checked on the real bedroom1.
    #[test]
    fn tmj_round_trips_tile_layer_offset() {
        let bytes = std::fs::read("../assets/maps/bedroom1.tmj").unwrap();
        let map = from_json(&bytes).unwrap();
        let bed = map
            .layers
            .iter()
            .find_map(|l| match l {
                TiledMapLayer::TileLayer(t) if t.name == "bed" => Some(t),
                _ => None,
            })
            .expect("bedroom1 has a `bed` tile layer");
        assert_eq!((bed.offsetx, bed.offsety), (-3.0, 3.0));
        let out = map.to_tmj(&map.parse_objects());
        let reloaded = from_json(out.as_bytes()).unwrap();
        let bed2 = reloaded
            .layers
            .iter()
            .find_map(|l| match l {
                TiledMapLayer::TileLayer(t) if t.name == "bed" => Some(t),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (bed2.offsetx, bed2.offsety),
            (-3.0, 3.0),
            "the bed layer's offset survives a save (no longer dropped)"
        );
        // A zero-offset layer emits no `offsetx`/`offsety` (Tiled-style omission).
        assert!(!out.contains("\"offsetx\": 0"));
    }

    /// A tile layer's int `palette_rotate` property parses (into the
    /// accessor) and round-trips through `to_tmj`; an absent one reads 0.
    #[test]
    fn tmj_round_trips_tile_layer_palette_rotate() {
        let json = r#"{
            "width": 1, "height": 1, "tilesets": [],
            "layers": [{ "type": "tilelayer", "name": "bg", "id": 1,
                         "width": 1, "height": 1, "data": [0],
                         "properties": [{ "name": "palette_rotate", "type": "int", "value": 1 }] }]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        assert_eq!(first_tile_layer(&map).palette_rotate(), 1);
        let out = map.to_tmj(&[]);
        let reloaded = from_json(out.as_bytes()).unwrap();
        assert_eq!(first_tile_layer(&reloaded).palette_rotate(), 1);

        // A layer with no properties reads palette_rotate 0 and emits none.
        let plain = r#"{
            "width": 1, "height": 1, "tilesets": [],
            "layers": [{ "type": "tilelayer", "name": "bg", "id": 1,
                         "width": 1, "height": 1, "data": [0] }]
        }"#;
        let pmap = from_json(plain.as_bytes()).unwrap();
        assert_eq!(first_tile_layer(&pmap).palette_rotate(), 0);
        assert!(!pmap.to_tmj(&[]).contains("palette_rotate"));
    }

    /// Map-level `bg_colour` / `camera_stick` properties parse (via the
    /// accessors) and round-trip; absent ones read `None`.
    #[test]
    fn tmj_round_trips_map_properties() {
        let json = r#"{
            "width": 1, "height": 1, "tilesets": [], "layers": [],
            "properties": [
                { "name": "bg_colour", "type": "int", "value": 3 },
                { "name": "camera_stick", "type": "string", "value": "-36,-64" }
            ]
        }"#;
        let map = from_json(json.as_bytes()).unwrap();
        assert_eq!(map.bg_colour(), Some(3));
        assert_eq!(map.camera_stick(), Some((-36, -64)));
        let out = map.to_tmj(&[]);
        let reloaded = from_json(out.as_bytes()).unwrap();
        assert_eq!(reloaded.bg_colour(), Some(3));
        assert_eq!(reloaded.camera_stick(), Some((-36, -64)));

        // Absent map properties read None and serialise nothing.
        let plain =
            from_json(r#"{ "width": 1, "height": 1, "tilesets": [], "layers": [] }"#.as_bytes())
                .unwrap();
        assert_eq!(plain.bg_colour(), None);
        assert_eq!(plain.camera_stick(), None);
        assert!(!plain.to_tmj(&[]).contains("properties"));
    }

    /// A rich object sprite (multi-frame, per-frame offset, palette rotation,
    /// multi-tile options) serialises as an `anim` property and round-trips
    /// frame-for-frame; a single default-options frame still serialises as the
    /// compact `sprite` id (so pre-`anim` maps stay byte-stable).
    #[test]
    fn tmj_round_trips_anim_sprite() {
        use crate::world::animation::AnimFrame;
        use crate::world::map::{MapObject, ObjectEffect};
        use crate::geometry::{Hitbox, Vec2};
        use crate::render::SpriteOptions;

        let frames = vec![
            AnimFrame::new(
                Vec2::new(0, 0),
                661,
                30,
                SpriteOptions {
                    w: 2,
                    h: 2,
                    ..SpriteOptions::transparent_zero()
                },
            )
            .with_palette_rotate(1),
            AnimFrame::new(
                Vec2::new(0, 1),
                661,
                30,
                SpriteOptions {
                    w: 2,
                    h: 2,
                    ..SpriteOptions::transparent_zero()
                },
            )
            .with_palette_rotate(1),
        ];
        let object = MapObject::dialogue(Hitbox::new(8, 8, 16, 16), "thing").with_sprite(frames);
        let map = TiledMap {
            width: 4,
            height: 4,
            layers: vec![TiledMapLayer::ObjectLayer(super::ObjectLayer {
                name: "objects".to_string(),
                objects: Vec::new(),
            })],
            tilesets: Vec::new(),
            properties: Vec::new(),
        };
        let out = map.to_tmj(std::slice::from_ref(&object));
        assert!(out.contains("\"anim\""), "a rich sprite emits `anim`");
        let reloaded = from_json(out.as_bytes()).unwrap();
        let parsed = reloaded.parse_objects();
        assert_eq!(parsed.len(), 1);
        let sprite = parsed[0].sprite.as_ref().expect("sprite round-tripped");
        assert_eq!(sprite.len(), 2);
        assert_eq!(sprite[0].spr_id, 661);
        assert_eq!(sprite[0].palette_rotate, 1);
        assert_eq!((sprite[0].options.w, sprite[0].options.h), (2, 2));
        assert_eq!(sprite[1].pos, Vec2::new(0, 1));
        assert!(matches!(
            parsed[0].effect,
            ObjectEffect::Interact(crate::world::interact::Interaction::Dialogue(ref k)) if k == "thing"
        ));

        // A single default-options frame stays the compact `sprite` id.
        let simple =
            MapObject::dialogue(Hitbox::new(8, 8, 8, 8), "egg").with_sprite(vec![AnimFrame::new(
                Vec2::splat(0),
                524,
                30,
                SpriteOptions::transparent_zero(),
            )]);
        let simple_out = map.to_tmj(std::slice::from_ref(&simple));
        assert!(
            simple_out.contains("\"sprite\""),
            "a simple sprite stays `sprite`"
        );
        assert!(!simple_out.contains("\"anim\""));
    }
}
