//! Bevy asset wrapper for a language script file (`script/<lang>.eggtext` or
//! `.json`). Parses it into [`egg_core`]'s [`ScriptFile`] through the async
//! asset pipeline (so it works on web too), mirroring [`crate::tiled`]. The
//! `.eggtext` form goes through [`egg_core::data::eggtext`]'s DSL parser; a
//! `.json` form is deserialized directly. Either way the loaded file is
//! installed into the console's text registry by the asset-load loop.

use bevy::asset::AssetLoader;
use bevy::asset::io::Reader;
use bevy::prelude::*;
use egg_core::data::script::ScriptFile;
use std::io::{Error, ErrorKind};

#[derive(Clone, Asset, TypePath)]
pub struct ScriptAsset(pub ScriptFile);

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ScriptAsset>()
            .init_asset_loader::<ScriptLoader>();
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
