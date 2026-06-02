use bevy::{
    asset::{AssetApp, AssetLoader, LoadContext, io::Reader},
    prelude::{Asset, Plugin, TypePath},
};
use egg_core::{map::LayerInfo, position::Vec2};
use serde::{Deserialize, Serialize};

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
    pub properties: Vec<ObjectProperties>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectProperties {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Asset, TypePath)]
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
}

pub struct TiledMapPlugin;

impl Plugin for TiledMapPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_asset::<TiledMap>()
            .init_asset_loader::<TiledMapLoader>();
    }
}

#[derive(Default, TypePath)]
pub struct TiledMapLoader;

impl AssetLoader for TiledMapLoader {
    type Asset = TiledMap;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let mut map: TiledMap = serde_json::from_slice(&bytes)?;
        map.flatten_gids();
        Ok(map)
    }

    fn extensions(&self) -> &[&str] {
        &["tmj"]
    }
}

// Tests for map serialization/deserialization:
#[cfg(test)]
mod tests {
    use crate::tiled::TiledMap;

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
        let json = std::fs::read_to_string("assets/maps/bank1.tmj").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, 240);
        assert_eq!(map.height, 136);
    }
}
