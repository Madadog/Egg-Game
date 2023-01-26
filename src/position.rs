#[derive(Debug, Clone, Copy)]
pub struct Vec2 {
    pub x: i16,
    pub y: i16,
}
impl Vec2 {
    pub const fn new(x: i16, y: i16) -> Self {
        Vec2 {x, y}
    }
    pub fn draw(&self, colour: u8) {
        crate::pix(self.x.into(), self.y.into(), colour);
    }
}
impl std::ops::Add for Vec2 {
    type Output = Vec2;
    
    fn add(self, rhs: Self) -> Self::Output {
        Vec2::new(self.x+rhs.x, self.y+rhs.y)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Hitbox {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}
impl Hitbox {
    pub const fn new(x: i16, y: i16, w: i16, h: i16) -> Self {
        assert!(w.is_positive() && h.is_positive());
        Hitbox {x, y, w, h}
    }
    pub fn ex(&self) -> i16 {self.x + self.w}
    pub fn ey(&self) -> i16 {self.y + self.h}
    pub fn area(&self) -> i16 {self.w * self.h}
    pub fn x_intersects(&self, other: Hitbox) -> bool {
        self.x <= other.ex() &&
        self.ex() >= other.x
    }
    pub fn y_intersects(&self, other: Hitbox) -> bool {
        self.y <= other.ey() &&
        self.ey() >= other.y
    }
    pub fn xy_intersects(&self, other: Hitbox) -> bool {
        self.x_intersects(other) ||
        self.y_intersects(other)
    }
    pub fn x_intersects_point(&self, point: Vec2) -> bool {
        self.x <= point.x &&
        self.ex() >= point.x
    }
    pub fn y_intersects_point(&self, point: Vec2) -> bool {
        self.y <= point.y &&
        self.ey() >= point.y
    }
    pub fn touches_point(&self, other: Vec2) -> bool {
        self.x_intersects_point(other) &&
        self.y_intersects_point(other)
    }
    pub fn touches(&self, other: Hitbox) -> bool {
        self.x_intersects(other) &&
        self.y_intersects(other)
    }
    pub fn offset(&self, delta: Vec2) -> Self {
        Self { x: self.x + delta.x, y: self.y + delta.y, .. *self }
    }
    pub fn draw(&self, colour: u8) {
        crate::rect(self.x.into(), self.y.into(), self.w.into(), self.h.into(), colour);
    }
}
