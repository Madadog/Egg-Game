//! Bevy asset wrapper for a language script file (`script/<lang>.json`). Parses
//! the JSON into [`egg_core`]'s [`ScriptFile`] through the async asset pipeline
//! (so it works on web too), mirroring [`crate::tiled`]. The loaded file is
//! installed into the console's text registry by the asset-load loop.

use bevy::asset::AssetLoader;
use bevy::asset::io::Reader;
use bevy::prelude::*;
use egg_core::data::script::ScriptFile;

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
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut bevy::asset::LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let file: ScriptFile = serde_json::from_slice(&bytes)?;
        Ok(ScriptAsset(file))
    }

    fn extensions(&self) -> &[&str] {
        &["json"]
    }
}
