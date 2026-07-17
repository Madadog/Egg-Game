//! Bevy-side plumbing for the engine's data files: Tiled maps (`.tmj`). The
//! codec itself ([`TiledMap`]) lives in `egg_core::data::tiled`; this module
//! only wraps it for the asset system, since Bevy's derives can't live on the
//! engine-agnostic type. The loader does the byte-level read and hands the
//! bytes to the shared engine codec.

use bevy::{
    asset::{AssetApp, AssetLoader, LoadContext, io::Reader},
    prelude::{Asset, Plugin, TypePath},
};
use egg_core::data::tiled::{self, TiledMap};

/// Asset wrapper around the engine's [`TiledMap`].
#[derive(Asset, TypePath)]
pub struct TiledMapAsset(pub TiledMap);

pub struct TiledMapPlugin;

impl Plugin for TiledMapPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_asset::<TiledMapAsset>()
            .init_asset_loader::<TiledMapLoader>();
    }
}

#[derive(Default, TypePath)]
pub struct TiledMapLoader;

impl AssetLoader for TiledMapLoader {
    type Asset = TiledMapAsset;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let map = tiled::from_json(&bytes)?;
        if map.tilesets.len() > 1 {
            bevy::log::warn!(
                "{} has {} tilesets: `to_tmj` re-adds only the first firstgid, so tile edits saved through the in-game editor will corrupt gids for multi-tileset maps",
                load_context.path(),
                map.tilesets.len()
            );
        }
        Ok(TiledMapAsset(map))
    }

    fn extensions(&self) -> &[&str] {
        &["tmj"]
    }
}
