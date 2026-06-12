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

use crate::animation::AnimFrame;
use crate::data::sound::{self, SfxData};
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, LayerInfo, MapObject, ObjectEffect, Trigger, Warp, WarpMode};
use crate::position::{Hitbox, Vec2};
use crate::system::SpriteOptions;
use crate::system::drawing::image::RgbaImage;
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

/// Parse a Tiled tileset file (`.tsj`) into a [`TilesetFile`]. The host reads
/// the bytes (its asset pipeline) and calls this, mirroring [`from_json`] for
/// maps — the byte-level loading stays host-side, the codec stays here.
pub fn tileset_from_json(bytes: &[u8]) -> Result<TilesetFile, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// A Tiled tileset file (`.tsj`), parsed for exactly what the engine needs from
/// it: its geometry (`tilecount`, `columns`) and the per-tile custom properties
/// that carry our gameplay data — today only the collision `flags` int. Every
/// other tileset field (image path, margins, version…) is ignored on load, so
/// Tiled stays free to add or reorder them. This is the data form of what used
/// to be the hardcoded blob in [`crate::data::sprite_flags`]; see
/// [`flag_table`](Self::flag_table).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TilesetFile {
    /// Number of tiles in the sheet (sheet width × height in tiles).
    pub tilecount: usize,
    /// Tiles per row — the sheet's width in tiles (32 for the egg sheet). This
    /// is the stride the flag table is indexed by, matching the `x + y * 32`
    /// layout the [`crate::map::layer_collides_flags`] reader expects.
    pub columns: usize,
    /// Only the tiles that carry custom properties. Tiled omits the rest, so an
    /// absent tile id means "no properties" (flags 0).
    #[serde(default)]
    pub tiles: Vec<TilesetTile>,
}
impl TilesetFile {
    /// Build the flat per-tile collision-flag table the runtime consults, sized
    /// `tilecount` and indexed by **plain sheet position** (`id`, i.e. column +
    /// row × `columns`) — exactly the index [`crate::map::layer_collides_flags`]
    /// derives from `tiles.get(0, x, y)` and the index the output of the legacy
    /// `parse_sprite_flags` lands at. Each tile's `flags` int property (absent =
    /// 0) becomes `table[id]`. No byte-swap and no 16-wide/32-wide split: that
    /// TIC-80 quirk is now baked into the exported `.tsj` data once and for all,
    /// so the ids here are honest sheet positions and the lookup is a direct
    /// index. The 256-offset `shift_sprite_flags` the reader applies is a
    /// *read-side* window into this same table, not part of its construction.
    pub fn flag_table(&self) -> Vec<u8> {
        let mut flags = vec![0u8; self.tilecount];
        for tile in &self.tiles {
            if let Some(slot) = flags.get_mut(tile.id) {
                *slot = tile.flags();
            }
        }
        flags
    }
}

/// One tile's per-tile data in a [`TilesetFile`]: its sheet id and the custom
/// properties Tiled stored on it. Only tiles with properties are present.
#[derive(Clone, Debug, Deserialize)]
pub struct TilesetTile {
    /// Sheet-local tile id (column + row × `columns`).
    pub id: usize,
    #[serde(default)]
    pub properties: Vec<TileProperty>,
}
impl TilesetTile {
    /// This tile's collision `flags` (the `flags` int property), clamped into a
    /// `u8` to match the runtime table; absent or out-of-range = 0.
    fn flags(&self) -> u8 {
        self.properties
            .iter()
            .find(|p| p.name == "flags")
            .and_then(|p| u8::try_from(p.value).ok())
            .unwrap_or(0)
    }
}

/// A Tiled integer custom property (`{ name, type: "int", value }`) as stored on
/// a tileset tile. Distinct from the object layer's string [`ObjectProperties`]:
/// Tiled serialises an `int` property's `value` as a JSON number, so this reads
/// it as one (and the only tile property we consume today, `flags`, is an int).
#[derive(Clone, Debug, Deserialize)]
pub struct TileProperty {
    pub name: String,
    pub value: i64,
}

/// Parse the game asset manifest (`assets/game.manifest`) into a [`GameManifest`].
/// JSON content, but a bespoke extension so it doesn't collide with the script
/// loader (which owns `.json`); the host reads the bytes and calls this, just
/// like [`from_json`] for maps.
pub fn manifest_from_json(bytes: &[u8]) -> Result<GameManifest, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// The game's asset manifest: the data-driven list of what to load at boot,
/// replacing a hardcoded set of map paths in the host. Each entry is a **base
/// name** (file stem), not a path — the host expands `maps/<name>.tmj` and
/// `maps/<name>.tsj`, and stores each loaded map in the [`crate::map::MapStore`]
/// under that same stem (which is also the name the in-game editor saves back
/// to). Shaped to grow: new asset categories become new fields, and both lists
/// default to empty so a partial manifest still parses.
///
/// Serialised as JSON in `assets/game.manifest`:
/// ```json
/// { "maps": ["bank1", "office", ...], "tilesets": ["tiles"] }
/// ```
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GameManifest {
    /// Map file stems to load (`maps/<name>.tmj`). The order is the load order;
    /// the names become the [`crate::map::MapStore`] keys.
    #[serde(default)]
    pub maps: Vec<String>,
    /// Tileset file stems to load (`maps/<name>.tsj`) for their per-tile data
    /// (today: the collision flag table). Usually just `"tiles"`.
    #[serde(default)]
    pub tilesets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TileLayer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<usize>,
    pub name: String,
}
impl TileLayer {
    pub fn get(&self, x: usize, y: usize) -> Option<usize> {
        self.data.get(y.checked_mul(self.width)? + x).copied()
    }
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut usize> {
        self.data.get_mut(y.checked_mul(self.width)? + x)
    }
    /// Subtract each tile's tileset `firstgid` so tile ids become sheet-local.
    pub fn flatten_gids(&mut self, tilesets: &[Tileset]) {
        for tile in self.data.iter_mut() {
            let max_gid = tilesets
                .iter()
                .map(|ts| ts.firstgid)
                .filter(|&gid| *tile >= gid)
                .max()
                .unwrap_or(0);
            *tile -= max_gid;
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
            size: Vec2::new(
                other.width.try_into().unwrap(),
                other.height.try_into().unwrap(),
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
/// all** (see [`crate::map::modern_map_info`] for that story end-to-end). Two
/// roles, chosen by the layer's name/properties (see
/// [`is_collision`](Self::is_collision)):
/// - a **visible** image layer draws into the world like a tile layer, obeying
///   the same conventions — file layer order for stacking, the `fg` name prefix
///   to sit above sprites, `visible: false` to never draw;
/// - a **collision** image layer is data, never drawn, its alpha sliced into the
///   per-tile bitmap [`Collider`](crate::position::Collider)s the walk loop
///   already consults (solid where alpha ≥
///   [`PAINTED_SOLID_ALPHA`](crate::map::PAINTED_SOLID_ALPHA)).
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
/// [`ObjectProperties`] and the tileset's int [`TileProperty`]: a `bool`
/// property's `value` is a JSON boolean, and the only one the engine reads is
/// the collision-mask marker.
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tileset {
    pub firstgid: usize,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TiledObject {
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
    /// 2. **func** — a `func` property names an [`InteractFn`](crate::interact::InteractFn)
    ///    ([`to_func`](Self::to_func));
    /// 3. **dialogue** — a non-empty `description` (the registry key)
    ///    ([`to_interactable`](Self::to_interactable));
    /// 4. **sprite-only** — just a `sprite` tile id: an [`Interaction::None`]
    ///    object that only draws an animation (e.g. the living-room TV);
    /// 5. otherwise `None` (also for degenerate zero-size objects, via
    ///    [`hitbox`](Self::hitbox)) — the object is skipped.
    fn to_object(&self) -> Option<MapObject> {
        let hitbox = self.hitbox()?;
        let object = if let Some(warp) = self.to_warp() {
            MapObject::warp(hitbox, warp)
        } else if let Some(func) = self.to_func() {
            self.attach_sprite(MapObject::func(hitbox, func))
        } else if let Some(object) = self.to_interactable() {
            object
        } else {
            self.to_sprite_only(hitbox)?
        };
        Some(self.apply_trigger(object))
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
    /// naming a known [`InteractFn`](crate::interact::InteractFn), reading any
    /// scalar properties that name needs (`pitch`, `count`) and taking
    /// positional data from the hitbox. The inverse of the `func` serialisation
    /// in [`interaction_to_object`].
    fn to_func(&self) -> Option<InteractFn> {
        let name = self.prop("func").filter(|s| !s.is_empty())?;
        InteractFn::from_name(
            name,
            self.prop_int("pitch"),
            self.prop_int("count"),
            self.hitbox()?,
        )
    }
    /// A pure sprite object: a `sprite` tile id with no warp/func/`description`,
    /// kept as an [`Interaction::None`] so legacy animation-only objects (the
    /// living-room TV) survive a map round-trip. No `sprite` ⇒ `None` (skip).
    fn to_sprite_only(&self, hitbox: Hitbox) -> Option<MapObject> {
        self.prop_int::<u16>("sprite")?;
        let object = MapObject::new(hitbox, ObjectEffect::Interact(Interaction::None), None);
        Some(self.attach_sprite(object))
    }
    /// Attach this object's `sprite` tile id (if any) as a one-frame animation.
    fn attach_sprite(&self, object: MapObject) -> MapObject {
        match self.prop_int::<u16>("sprite") {
            Some(id) => object.with_sprite(vec![AnimFrame::new(
                Vec2::splat(0),
                id,
                30,
                SpriteOptions::transparent_zero(),
            )]),
            None => object,
        }
    }
    /// Build a warp effect if this object is one (`type == "warp"`, or it carries
    /// warp properties): `to_map` (a map name, taken verbatim — numeric values
    /// from old files resolve through `map_by_name`'s fallback; absent = same
    /// map), `to_x`/`to_y` (destination pixels, default = the object's own
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
        _ => return None,
    })
}

/// The `trigger` property name for a [`Trigger`]. Always `Some` (every variant
/// has a spelling); the *caller* decides whether to emit it, serialising the
/// property only when the trigger differs from the effect-kind default so files
/// with no authored trigger round-trip byte-stable. Inverse of [`parse_trigger`].
fn trigger_name(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Touch => "touch",
        Trigger::Press => "press",
        Trigger::Any => "any",
    }
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
/// Tiled spelling — an unnamed func ([`InteractFn::Pet`](crate::interact::InteractFn::Pet)),
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
        properties.push(prop_str("trigger", trigger_name(object.trigger)));
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
/// carrying the optional `sprite` tile id. Dialogue → `description`; a named
/// `func` → `func` + its scalar props (`pitch`/`count`; piano/none need none);
/// a sprite-carrying [`Interaction::None`] → just its `sprite`. The cases with
/// no spelling (unnamed func, sprite-less `None`) → `None`.
fn interaction_to_object(
    hitbox: Hitbox,
    interaction: &Interaction,
    sprite: Option<&[AnimFrame]>,
    id: usize,
) -> Option<Value> {
    let sprite_id = sprite.and_then(|f| f.first()).map(|frame| frame.spr_id);
    let mut properties = match interaction {
        Interaction::Dialogue(key) => vec![prop_str("description", key)],
        Interaction::Func(func) => func_properties(func)?,
        // A pure animation object only round-trips if it actually has a sprite;
        // a sprite-less `None` is nothing Tiled can represent.
        Interaction::None => {
            sprite_id?;
            Vec::new()
        }
    };
    if let Some(id) = sprite_id {
        properties.push(prop_str("sprite", &id.to_string()));
    }
    Some(json!({
        "id": id, "name": "", "type": "", "rotation": 0, "visible": true,
        "x": hitbox.x, "y": hitbox.y, "width": hitbox.w, "height": hitbox.h,
        "properties": properties,
    }))
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
        InteractFn::ToggleDog | InteractFn::Piano(_) | InteractFn::Pet(..) => {}
    }
    Some(properties)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TiledMap {
    pub width: usize,
    pub height: usize,
    pub layers: Vec<TiledMapLayer>,
    pub tilesets: Vec<Tileset>,
}
impl TiledMap {
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
                    layers.push(json!({
                        "type": "tilelayer", "id": id, "name": tile_layer.name,
                        "width": tile_layer.width, "height": tile_layer.height,
                        "x": 0, "y": 0, "opacity": 1, "visible": true,
                        "data": data,
                    }));
                }
                TiledMapLayer::ObjectLayer(object_layer) => {
                    let mut json_objects = Vec::new();
                    for object in objects {
                        if let Some(value) = object_to_tmj(object, json_objects.len() + 1) {
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
        let map = json!({
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
        serde_json::to_string_pretty(&map).unwrap_or_default()
    }
}

// Tests for map serialization/deserialization:
#[cfg(test)]
mod tests {
    use super::{
        GameManifest, TiledMap, TiledMapLayer, from_json, manifest_from_json, tileset_from_json,
    };
    use crate::interact::{InteractFn, Interaction};
    use crate::map::{MapObject, ObjectEffect, Trigger, WarpMode};
    use crate::system::drawing::image::RgbaImage;

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

    /// A small inline `.tsj` snippet parses and its `flag_table` indexes by
    /// plain sheet id: tile 1 → flags 1, tile 35 → flags 8, every absent tile
    /// (including a clamped out-of-range value) → 0.
    #[test]
    fn tileset_parses_inline_snippet() {
        let json = r#"{
            "columns": 32, "tilecount": 64, "type": "tileset",
            "tiles": [
                { "id": 1, "properties": [{ "name": "flags", "type": "int", "value": 1 }] },
                { "id": 35, "properties": [{ "name": "flags", "type": "int", "value": 8 }] },
                { "id": 40, "properties": [{ "name": "other", "type": "int", "value": 9 }] }
            ]
        }"#;
        let tileset = tileset_from_json(json.as_bytes()).unwrap();
        assert_eq!(tileset.tilecount, 64);
        assert_eq!(tileset.columns, 32);
        let table = tileset.flag_table();
        assert_eq!(table.len(), 64, "table is sized by tilecount");
        assert_eq!(table[1], 1);
        assert_eq!(table[35], 8);
        assert_eq!(table[40], 0, "a non-`flags` property contributes nothing");
        assert_eq!(table[0], 0, "absent tiles are 0");
    }

    /// The real `assets/maps/tiles.tsj` parses and is the full 2048-tile sheet
    /// with exactly the tiles that carry nonzero flags.
    #[test]
    fn tileset_parses_real_tiles_tsj() {
        let bytes = std::fs::read("../assets/maps/tiles.tsj").unwrap();
        let tileset = tileset_from_json(&bytes).unwrap();
        assert_eq!(tileset.tilecount, 2048);
        assert_eq!(tileset.columns, 32);
        let table = tileset.flag_table();
        assert_eq!(table.len(), 2048);
        // Spot-check a couple of known entries (see the exported tsj).
        assert_eq!(table[1], 1);
        assert_eq!(table[490], 10);
        // Exactly the nonzero tiles the export carries.
        assert_eq!(table.iter().filter(|&&f| f != 0).count(), 149);
    }

    /// The manifest parses and lists the maps/tilesets to load.
    #[test]
    fn manifest_parses() {
        let json = r#"{
            "maps": ["bank1", "office"],
            "tilesets": ["tiles"]
        }"#;
        let manifest: GameManifest = manifest_from_json(json.as_bytes()).unwrap();
        assert_eq!(manifest.maps, vec!["bank1", "office"]);
        assert_eq!(manifest.tilesets, vec!["tiles"]);
    }

    /// The real `assets/game.manifest` parses and names every shipping map plus
    /// the tileset, and deliberately excludes the backup map.
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
        assert_eq!(manifest.tilesets, vec!["tiles"]);
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
        };
        let json = serde_json::to_string(&map).unwrap();
        println!("{}", json);
        let map2: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, map2.width);
        assert_eq!(map.height, map2.height);
    }
    #[test]
    fn test_map_deserialization() {
        let json = std::fs::read_to_string("../assets/maps/bank1.tmj").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, 240);
        assert_eq!(map.height, 136);
    }

    #[test]
    fn parses_office_interactables() {
        let json = std::fs::read_to_string("../assets/maps/office.tmj").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        let objects = map.parse_objects();
        // office.tmj's object layer is 7 dialogue interactions, no warps.
        assert_eq!(objects.len(), 7);
        assert!(
            objects
                .iter()
                .all(|o| matches!(o.effect, ObjectEffect::Interact(_)))
        );
        // The first object is the desk front; its hitbox matches the Tiled object.
        let desk = &objects[0];
        assert_eq!((desk.hitbox.x, desk.hitbox.y), (89, 65));
        assert!(matches!(
            &desk.effect,
            ObjectEffect::Interact(Interaction::Dialogue(k)) if k == "office_desk_front"
        ));
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
        // The numeric id is kept verbatim — resolution to a legacy map happens
        // in `map_by_name`, not here.
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
            assert!(matches!(
                (&a.effect, &b.effect),
                (
                    ObjectEffect::Interact(Interaction::Dialogue(x)),
                    ObjectEffect::Interact(Interaction::Dialogue(y)),
                ) if x == y
            ));
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
    /// the name intact (names are the canonical map identity; numbers are only
    /// a legacy fallback).
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
        assert!(image.visible);
        // Pixels are runtime-only: never filled by the parser.
        assert!(image.pixels.is_none());
        // "walls" is painted *art*, not a collision mask — collision stays on
        // the tile layer.
        assert!(!image.is_collision());
        // The image layer is enumerated for the host to load.
        assert_eq!(map.image_layer_paths(), vec!["images/bedroom1_walls.png"]);
        // The room's one object is a touch warp to house_stairwell.
        let objects = map.parse_objects();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].trigger, Trigger::Touch);
        match &objects[0].effect {
            ObjectEffect::Warp(warp) => {
                assert_eq!(warp.map.as_deref(), Some("house_stairwell"));
            }
            other => panic!("expected a warp, got {other:?}"),
        }
    }

    /// The real `assets/maps/house_stairwell.tmj` parses too: two tile layers
    /// and one image layer at its (positive) offset.
    #[test]
    fn parses_house_stairwell_image_layer() {
        let bytes = std::fs::read("../assets/maps/house_stairwell.tmj").unwrap();
        let map = from_json(&bytes).unwrap();
        assert_eq!(map.layers.len(), 3);
        let image = only_image_layer(&map);
        assert_eq!(image.name, "Image Layer 1");
        assert_eq!(image.image, "images/house_stairwell_mask.png");
        assert_eq!((image.offsetx, image.offsety), (74.0, 33.0));
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
}
