//! The persistent simulation: the loaded maps ([`map`]), the player/companion/
//! shell behaviour ([`player`]), the scripting verbs an interaction runs
//! ([`interact`]), the [`camera`], and the [`animation`]/[`particles`] systems
//! that drive on-screen motion. Sits above the data formats and the UI toolkit;
//! the `GameMode` screens in `gamestate` step and draw it.

pub mod animation;
pub mod camera;
pub mod interact;
pub mod map;
pub mod particles;
pub mod player;
