//! Reusable UI toolkit: the immediate-mode flexbox [`layout`] over Taffy, the
//! shared line-editing [`text_field`], and the [`dialogue`] box widget that
//! plays a conversation. Sits above the stateless [`render`](crate::render)
//! primitives and the [`platform`](crate::platform) input surface; nothing in
//! the persistent world depends on it.

pub mod dialogue;
pub mod layout;
pub mod text_field;
