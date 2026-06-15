//! Bevy asset wrappers for the script-domain text files. The language script
//! (`script/<lang>.eggtext` or `.json`) parses into [`egg_core`]'s [`ScriptFile`];
//! the cutscene file (`script/main.eggscene`) parses into its [`SceneFile`]
//! registry. Both go through the async asset pipeline (so they work on web too),
//! mirroring [`crate::tiled`], and are installed into the console by the
//! asset-load loop — dialogue via `Script::set_base`, cutscenes via
//! `EggState::set_scenes`.

use bevy::asset::AssetLoader;
use bevy::asset::io::Reader;
use bevy::prelude::*;
use egg_core::data::eggscene::SceneFile;
use egg_core::data::script::ScriptFile;
use std::io::{Error, ErrorKind};

#[derive(Clone, Asset, TypePath)]
pub struct ScriptAsset(pub ScriptFile);

/// The parsed cutscene registry (`script/main.eggscene`) — a separate asset
/// from [`ScriptAsset`] because it parses into a different type and is a single,
/// language-independent file (no per-language overlay).
#[derive(Clone, Asset, TypePath)]
pub struct SceneAsset(pub SceneFile);

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ScriptAsset>()
            .init_asset_loader::<ScriptLoader>()
            .init_asset::<SceneAsset>()
            .init_asset_loader::<SceneLoader>();
    }
}

#[derive(Default, TypePath)]
pub struct ScriptLoader;

impl AssetLoader for ScriptLoader {
    type Asset = ScriptAsset;
    type Settings = ();
    type Error = Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut bevy::asset::LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let is_eggtext = load_context
            .path()
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("eggtext"));
        let file: ScriptFile = if is_eggtext {
            let text = std::str::from_utf8(&bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
            egg_core::data::eggtext::parse(text)
                .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?
        } else {
            serde_json::from_slice(&bytes)?
        };
        Ok(ScriptAsset(file))
    }

    fn extensions(&self) -> &[&str] {
        &["eggtext", "json"]
    }
}

#[derive(Default, TypePath)]
pub struct SceneLoader;

impl AssetLoader for SceneLoader {
    type Asset = SceneAsset;
    type Settings = ();
    type Error = Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut bevy::asset::LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let text = std::str::from_utf8(&bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
        let file = egg_core::data::eggscene::parse(text)
            .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        Ok(SceneAsset(file))
    }

    fn extensions(&self) -> &[&str] {
        &["eggscene"]
    }
}
