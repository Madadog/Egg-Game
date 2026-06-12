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
use crate::interact::Interaction;
use crate::map::{Axis, LayerInfo, MapObject, ObjectEffect, Warp, WarpMode};
use crate::position::{Hitbox, Vec2};
use crate::system::SpriteOptions;
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum TiledMapLayer {
    #[serde(rename = "tilelayer")]
    TileLayer(TileLayer),
    #[serde(rename = "objectgroup")]
    ObjectLayer(ObjectLayer),
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
    /// Resolve this object into a runtime [`MapObject`], discriminating the same
    /// way the two old paths did: try the warp interpretation first
    /// ([`to_warp`](Self::to_warp)), then the dialogue interactable
    /// ([`to_interactable`](Self::to_interactable)). `None` for an object that's
    /// neither (and for degenerate zero-size objects, via [`hitbox`](Self::hitbox)).
    fn to_object(&self) -> Option<MapObject> {
        let hitbox = self.hitbox()?;
        if let Some(warp) = self.to_warp() {
            Some(MapObject::warp(hitbox, warp))
        } else {
            self.to_interactable()
        }
    }
    /// Build a warp effect if this object is one (`type == "warp"`, or it carries
    /// warp properties): `to_map` (a map name, taken verbatim — numeric values
    /// from old files resolve through `map_by_name`'s fallback; absent = same
    /// map), `to_x`/`to_y` (destination pixels, default = the object's own
    /// position), `flip`, `mode` (`auto`/`interact`), `sound`. The trigger
    /// hitbox lives on the owning [`MapObject`], so it isn't built here.
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
            self.prop("to_x").and_then(|s| s.parse().ok()).unwrap_or(from.x),
            self.prop("to_y").and_then(|s| s.parse().ok()).unwrap_or(from.y),
        );
        let mut warp = Warp::new(map, to);
        if let Some(flip) = self.prop("flip") {
            warp = warp.with_flip(parse_axis(flip));
        }
        if self.prop("mode").is_some_and(|m| m.eq_ignore_ascii_case("auto")) {
            warp = warp.with_mode(WarpMode::Auto);
        }
        if let Some(sound) = self.prop("sound").and_then(parse_sound) {
            warp = warp.with_sound(sound);
        }
        Some(warp)
    }
    /// Build a dialogue interaction object if this object carries a `description`
    /// (the dialogue-registry key). Optional `sprite` property = a tile id drawn
    /// at the object.
    fn to_interactable(&self) -> Option<MapObject> {
        let key = self.prop("description").filter(|s| !s.is_empty())?;
        let mut object = MapObject::dialogue(self.hitbox()?, key);
        if let Some(id) = self.prop("sprite").and_then(|s| s.parse::<u16>().ok()) {
            object = object.with_sprite(vec![AnimFrame::new(
                Vec2::splat(0),
                id,
                30,
                SpriteOptions::transparent_zero(),
            )]);
        }
        Some(object)
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
/// owning object's `hitbox` for the placed rectangle. A warp serialises with
/// `type: "warp"` + warp properties; a dialogue interaction with `description` +
/// optional `sprite`. Interactions that aren't a dialogue key (functions / none)
/// can't be expressed as Tiled objects → `None` (the caller counts them dropped).
fn object_to_tmj(object: &MapObject, id: usize) -> Option<Value> {
    match &object.effect {
        ObjectEffect::Warp(warp) => Some(warp_to_object(object.hitbox, warp, id)),
        ObjectEffect::Interact(interaction) => {
            interaction_to_object(object.hitbox, interaction, object.sprite.as_deref(), id)
        }
    }
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
    json!({
        "id": id, "name": "", "type": "warp", "rotation": 0, "visible": true,
        "x": hitbox.x, "y": hitbox.y,
        "width": hitbox.w, "height": hitbox.h,
        "properties": properties,
    })
}

/// Serialise a dialogue interaction as a Tiled object (`description` + optional
/// `sprite` tile id), placed at `hitbox`. Non-dialogue interactions don't
/// round-trip → `None`.
fn interaction_to_object(
    hitbox: Hitbox,
    interaction: &Interaction,
    sprite: Option<&[AnimFrame]>,
    id: usize,
) -> Option<Value> {
    let Interaction::Dialogue(key) = interaction else {
        return None;
    };
    let mut properties = vec![prop_str("description", key)];
    if let Some(frame) = sprite.and_then(|f| f.first()) {
        properties.push(prop_str("sprite", &frame.spr_id.to_string()));
    }
    Some(json!({
        "id": id, "name": "", "type": "", "rotation": 0, "visible": true,
        "x": hitbox.x, "y": hitbox.y, "width": hitbox.w, "height": hitbox.h,
        "properties": properties,
    }))
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
    /// pretty-printed JSON. Objects whose effect is an interaction that isn't a
    /// dialogue key (function / none) can't be expressed as Tiled objects and
    /// are dropped (with a warning).
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
            }
        }
        if dropped > 0 {
            log::warn!(
                "{dropped} object(s) with non-dialogue interactions could not be serialised and were dropped"
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
    use super::{TiledMap, TiledMapLayer, from_json};
    use crate::interact::Interaction;
    use crate::map::{MapObject, ObjectEffect, WarpMode};

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
        assert!(objects.iter().all(|o| matches!(o.effect, ObjectEffect::Interact(_))));
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
}
