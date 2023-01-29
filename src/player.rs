use crate::{Vec2, Hitbox, Flip};

#[derive(Debug)]
pub struct Player {
    /// coords are (x, y)
    pub dir: (i8, i8),
    pub hp: u8,
    pub local_hitbox: Hitbox,
    pub pos: Vec2,
    pub walking: bool,
    pub walktime: u16,
}
impl Player {
    pub const fn const_default() -> Self {
        Self {
            pos: Vec2::new(96, 38),
            local_hitbox: Hitbox::new(0,10,7,5),
            hp: 3,
            dir: (0, 1),
            walktime: 0,
            walking: false,
        }
    }
    pub fn sprite_index(&self) -> (i32, Flip, i32) {
        let t = (((self.walktime+19) / 20) % 2) as i32;
        let anim = if self.walktime > 0 {t + 1} else {0};
        if self.dir.1 > 0 { return (768 + anim, Flip::None, t) } // Up
        if self.dir.1 < 0 { return (771 + anim, Flip::None, t) } // Down
        if self.dir.0 > 0 { return (832 + anim, Flip::None, t) } // Right
        return (832 + anim, Flip::Horizontal, t) // Left
    }
    pub fn hitbox(&self) -> Hitbox {
        self.local_hitbox.offset(self.pos)
    }
}
impl Default for Player {
    fn default() -> Self { Self::const_default() }
}
