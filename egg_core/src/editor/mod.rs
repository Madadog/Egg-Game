//! Dev tooling, hosted per extra window by the frontend's multi-window views:
//! the in-game [`map`] editor (the `MapViewer` + its dock UI) and the raw
//! [`text`] editor for the `.eggtext`/`.eggscene` script files. Top of the
//! dependency stack — nothing in the engine depends on it; the host wires these
//! into editor windows.

pub mod map;
pub mod text;
