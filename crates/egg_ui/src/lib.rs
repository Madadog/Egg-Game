//! Reusable UI toolkit: the immediate-mode flexbox [`layout`] over Taffy, the
//! shared line-editing [`text_field`], the [`dialogue`] box widget that plays a
//! conversation, and the [`portrait`] renderer it draws speakers with. Sits
//! above the stateless [`egg_render`] primitives and the [`egg_platform`] input
//! surface (and reads the game's [`egg_world`] draw record + text data); nothing
//! in the persistent world depends on it.

pub mod dialogue;
pub mod layout;
pub mod portrait;
pub mod text_field;
