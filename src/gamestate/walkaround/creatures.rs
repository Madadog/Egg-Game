use crate::position::{Hitbox, Vec2};

pub struct Creature {
    pub hitbox: Hitbox,
    pub state: CreatureState,
    pub sprite: i16,
}
impl Creature {
    pub const fn const_default() -> Self {
        Self {
            hitbox: Hitbox::new(0, 0, 8, 8),
            state: CreatureState::Idle,
            sprite: 688,
        }
    }
    pub fn with_offset(self, delta: Vec2) -> Self {
        Self {
            hitbox: self.hitbox.offset(delta),
            ..self
        }
    }
}

pub enum CreatureState {
    Idle,
    Walking(u8),
}
