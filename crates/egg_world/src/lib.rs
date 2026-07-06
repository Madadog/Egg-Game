//! The game domain: the on-disk data formats and their codecs ([`data`] — Tiled
//! maps, the dialogue/cutscene scripts, save data, and the item/creature/sound
//! registries), the persistent world simulation ([`world`] — loaded maps,
//! player/companion/shell behaviour, interaction verbs, camera, animation and
//! particles), the mutable per-frame draw + palette record ([`draw_state`]), and
//! the game's RNG ([`rand`]). Parsing stays fenced inside [`data`]; the simulation
//! and draw record sit on top of it. Depends on [`egg_render`] for the drawing
//! primitives and [`egg_platform`] for the console surface and screen consts; the
//! `GameMode` screens, UI toolkit and editor that step and draw this all stay up
//! in `egg_core`.

pub mod data;
pub mod draw_state;
pub mod rand;
pub mod world;
