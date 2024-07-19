use bevy::{
    asset::{io::{file::FileAssetReader, AssetReader, Reader}, Asset, AssetApp, AssetLoader, AsyncReadExt, LoadContext, LoadedAsset},
    prelude::Plugin,
    reflect::TypePath,
    utils::BoxedFuture,
};
use egg_core::{map::LayerInfo, packed::PackedI16};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TiledLayer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<usize>,
    pub name: String,
}
impl From<TiledLayer> for LayerInfo {
    fn from(other: TiledLayer) -> Self {
        Self {
            origin: PackedI16::from_i16(0, 0),
            size: PackedI16::from_i16(
                other.width.try_into().unwrap(),
                other.height.try_into().unwrap(),
            ),
            offset: PackedI16::from_i16(0, 0),
            ..Self::DEFAULT_MAP
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Asset, TypePath)]
pub struct TiledMap {
    pub width: usize,
    pub height: usize,
    pub layers: Vec<TiledLayer>,
}
impl TiledMap {
    pub fn get(&self, layer: usize, x: usize, y: usize) -> Option<usize> {
        self.layers.get(layer).and_then(|layer| {
            layer
                .data
                .get(
                    y.checked_mul(layer.width).unwrap_or_else(|| {
                        println!("layer.width: {}, y: {}", layer.width, y);
                        1
                    }) + x,
                )
                .cloned()
        })
    }
    pub fn set(&mut self, layer: usize, x: usize, y: usize, value: usize) {
        if let Some(tile) = self.layers.get_mut(layer).and_then(|layer| {
            layer.data.get_mut(
                y.checked_mul(layer.width).unwrap_or_else(|| {
                    println!("layer.width: {}, y: {}", layer.width, y);
                    1
                }) + x,
            )
        }) {
            *tile = value;
        };
    }
}

pub struct TiledMapPlugin;

impl Plugin for TiledMapPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_asset::<TiledMap>()
            .init_asset_loader::<TiledMapLoader>();
    }
}

#[derive(Default)]
pub struct TiledMapLoader;

impl AssetLoader for TiledMapLoader {
    type Asset = TiledMap;
    type Settings = ();
    type Error = std::io::Error;
    async fn load<'a>(
        &'a self,
        reader: &'a mut Reader<'_>,
        _settings: &'a (),
        _load_context: &'a mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let map: TiledMap = serde_json::from_slice(&bytes)?; 
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
