use bevy::{
    asset::{io::Reader, AssetApp, AssetLoader, AsyncReadExt, LoadContext},
    prelude::{Asset, Plugin, TypePath},
};
use egg_core::{
    map::{LayerInfo, MapInfo},
    position::Vec2,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TiledLayer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<usize>,
    pub name: String,
}
impl TiledLayer {
    pub fn get(&self, x: usize, y: usize) -> Option<usize> {
        self.data
            .get(
                y.checked_mul(self.width).unwrap_or_else(|| {
                    println!("layer.width: {}, y: {}", self.width, y);
                    1
                }) + x,
            )
            .cloned()
    }
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut usize> {
        self.data.get_mut(
            y.checked_mul(self.width).unwrap_or_else(|| {
                println!("layer.width: {}, y: {}", self.width, y);
                1
            }) + x,
        )
    }
    pub fn flatten_gids(&mut self, tilesets: &[Tileset]) {
        let gids = {
            let mut gids: Vec<usize> = tilesets.iter().map(|x| x.firstgid).collect();
            gids.sort_unstable_by(|a, b| b.cmp(a));
            gids
        };
        // TODO: actually use the above sort...
        for tile in self.data.iter_mut() {
            let mut max_gid = 0;
            for gid in gids.iter() {
                if *tile >= *gid && *gid > max_gid {
                    max_gid = *gid;
                }
            }
            *tile = *tile - max_gid;
        }
    }
    pub fn into_layer_info(self, source_layer: usize) -> LayerInfo {
        LayerInfo {
            source_layer,
            ..self.into()
        }
    }
}
impl From<TiledLayer> for LayerInfo {
    fn from(other: TiledLayer) -> Self {
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
pub struct Tileset {
    pub firstgid: usize,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Asset, TypePath)]
pub struct TiledMap {
    pub width: usize,
    pub height: usize,
    pub layers: Vec<TiledLayer>,
    pub tilesets: Vec<Tileset>,
}
impl TiledMap {
    pub fn get(&self, layer: usize, x: usize, y: usize) -> Option<usize> {
        self.layers.get(layer).and_then(|layer| layer.get(x, y))
    }
    pub fn set(&mut self, layer: usize, x: usize, y: usize, value: usize) {
        if let Some(tile) = self
            .layers
            .get_mut(layer)
            .and_then(|layer| layer.get_mut(x, y))
        {
            *tile = value;
        };
    }
    pub fn get_tile_source(&self, tile: usize) -> Option<Tileset> {
        let mut source = None;
        for tileset in self.tilesets.iter() {
            if tile >= tileset.firstgid {
                source = Some(tileset.clone());
            }
        }
        source
    }
    pub fn flatten_gids(&mut self) {
        for layer in self.layers.iter_mut() {
            layer.flatten_gids(&self.tilesets);
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
