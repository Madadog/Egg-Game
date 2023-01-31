use crate::{Hitbox, SpriteOptions};
use crate::animation::*;

#[derive(Debug)]
pub enum Interaction<'a> {
    Text(&'a str),
}

pub struct Interactable<'a> {
    pub hitbox: Hitbox,
    pub interaction: Interaction<'a>,
    pub sprite: Option<Animation<'a>>,
}

impl<'a> Interactable<'a> {
    pub fn new(hitbox: Hitbox, interaction: Interaction<'a>, sprite: Option<Animation<'a>>) -> Self { Self { hitbox, interaction, sprite } }
}
