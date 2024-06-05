use bevy::{
    asset::{AssetLoader, LoadContext, LoadedAsset},
    prelude::{AddAsset, Plugin},
    reflect::{TypePath, TypeUuid},
    utils::BoxedFuture,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TiledLayer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TypeUuid, TypePath)]
#[uuid = "37d8348c-47cc-4a1a-a3e9-d4e19fdc39b3"]
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
        app.add_asset::<TiledMap>()
            .init_asset_loader::<TiledMapLoader>();
    }
}

#[derive(Default)]
pub struct TiledMapLoader;

impl AssetLoader for TiledMapLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, Result<(), bevy::asset::Error>> {
        Box::pin(async move {
            let map: TiledMap = serde_json::from_slice(bytes).unwrap();
            load_context.set_default_asset(LoadedAsset::new(map));
            Ok(())
        })
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
        let json = std::fs::read_to_string("assets/map/bank1.json").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, 240);
        assert_eq!(map.height, 136);
    }
}
