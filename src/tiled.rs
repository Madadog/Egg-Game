//! Bevy-side plumbing for the engine's data files: Tiled maps (`.tmj`), Tiled
//! tilesets (`.tsj`, for their per-tile collision flags) and the game asset
//! manifest (`.manifest`). The codecs themselves ([`TiledMap`], [`TilesetFile`]
//! and [`GameManifest`]) live in `egg_core::data::tmj`; this module only wraps
//! them for the asset system, since Bevy's derives can't live on the
//! engine-agnostic types. Each loader does the byte-level read and hands the
//! bytes to the shared engine codec.

use bevy::{
    asset::{AssetApp, AssetLoader, LoadContext, io::Reader},
    prelude::{Asset, Plugin, TypePath},
};
use egg_core::data::tmj::{self, GameManifest, TiledMap, TilesetFile};

/// Asset wrapper around the engine's [`TiledMap`].
#[derive(Asset, TypePath)]
pub struct TiledMapAsset(pub TiledMap);

/// Asset wrapper around the engine's [`TilesetFile`] (a parsed `.tsj`). Carries
/// the per-tile collision-flag table the engine installs into `DrawState`.
#[derive(Asset, TypePath)]
pub struct TilesetAsset(pub TilesetFile);

/// Asset wrapper around the engine's [`GameManifest`] (the parsed
/// `game.manifest`): the data-driven list of maps and tilesets to load.
#[derive(Asset, TypePath)]
pub struct ManifestAsset(pub GameManifest);

pub struct TiledMapPlugin;

impl Plugin for TiledMapPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_asset::<TiledMapAsset>()
            .init_asset_loader::<TiledMapLoader>()
            .init_asset::<TilesetAsset>()
            .init_asset_loader::<TilesetLoader>()
            .init_asset::<ManifestAsset>()
            .init_asset_loader::<ManifestLoader>();
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
        let map = tmj::from_json(&bytes)?;
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

/// Loader for Tiled tileset files (`.tsj`), following [`TiledMapLoader`]'s
/// pattern: read the bytes, hand them to the engine's [`tmj::tileset_from_json`].
#[derive(Default, TypePath)]
pub struct TilesetLoader;

impl AssetLoader for TilesetLoader {
    type Asset = TilesetAsset;
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
        Ok(TilesetAsset(tmj::tileset_from_json(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["tsj"]
    }
}

/// Loader for the game asset manifest (`.manifest`). A bespoke extension so it
/// doesn't collide with the script loader (which owns `.json`), even though the
/// content is JSON.
#[derive(Default, TypePath)]
pub struct ManifestLoader;

impl AssetLoader for ManifestLoader {
    type Asset = ManifestAsset;
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
        Ok(ManifestAsset(tmj::manifest_from_json(&bytes)?))
    }

    fn extensions(&self) -> &[&str] {
        &["manifest"]
    }
}
