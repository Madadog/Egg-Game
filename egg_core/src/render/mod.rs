//! Stateless rendering primitives: pixel containers ([`image`]), the
//! format-agnostic raster core ([`canvas`] — the [`Canvas`] trait, pixel access,
//! `blit`, immediate-mode primitives, and the discrete [`Transform`] applied
//! during blits), bitmap text ([`font`]), the TIC-80 sheet/palette layer
//! ([`sheet`]), and the per-draw option structs ([`options`]). Knows nothing
//! about sprite sheets-as-game-data, maps, or the live palette state.

pub mod canvas;
pub mod font;
pub mod image;
pub mod options;
pub mod sheet;

pub use canvas::*;
pub use font::*;
pub use options::*;
