//! Reusable UI toolkit: the immediate-mode flexbox [`layout`] over Taffy, the
//! shared line-editing [`text_field`], the [`dialogue`] box widget that plays a
//! conversation, and the [`portrait`] renderer it draws speakers with. Sits
//! above the stateless [`render`](crate::render) primitives and the
//! [`platform`](crate::platform) input surface; nothing in the persistent world
//! depends on it.

pub mod dialogue;
pub mod layout;
pub mod portrait;
pub mod text_field;
