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
    pub fn ex(&self) -> i16 {self.x + self.w - 1}
    pub fn ey(&self) -> i16 {self.y + self.h - 1}
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
    pub fn offset_xy(&self, x: i16, y: i16) -> Self {
        Self { x: self.x + x, y: self.y + y, .. *self }
    }
    pub fn offset(&self, delta: Vec2) -> Self {
        self.offset_xy(delta.x, delta.y)
    }
    /// Returns corner points in the order `[Top Left, Top Right, Bottom Left, Bottom Right]`
    pub fn corners(&self) -> [Vec2; 4] {
        [
            Vec2::new(self.x, self.y),
            Vec2::new(self.ex(), self.y),
            Vec2::new(self.x, self.ey()),
            Vec2::new(self.ex(), self.ey()),
        ]
    }
    pub fn top_corners(&self) -> [Vec2; 2] {
        [
            Vec2::new(self.x, self.y),
            Vec2::new(self.ex(), self.y),
        ]
    }
    pub fn bottom_corners(&self) -> [Vec2; 2] {
        [
            Vec2::new(self.x, self.ey()),
            Vec2::new(self.ex(), self.ey()),
        ]
    }
    pub fn left_corners(&self) -> [Vec2; 2] {
        [
            Vec2::new(self.x, self.y),
            Vec2::new(self.x, self.ey()),
        ]
    }
    pub fn right_corners(&self) -> [Vec2; 2] {
        [
            Vec2::new(self.ex(), self.y),
            Vec2::new(self.ex(), self.ey()),
        ]
    }
    pub fn dx_corners(&self, dx: i16) -> Option<[Vec2; 2]> {
        if dx != 0 {
            if dx.is_positive() {
                Some(self.offset_xy(dx, 0).right_corners())
            } else {
                Some(self.offset_xy(dx, 0).left_corners())
            }
        } else {
            None
        }
    }
    pub fn dy_corners(&self, dy: i16) -> Option<[Vec2; 2]> {
        if dy != 0 {
            if dy.is_positive() {
                Some(self.offset_xy(0, dy).bottom_corners())
            } else {
                Some(self.offset_xy(0, dy).top_corners())
            }
        } else {
            None
        }
    }
    pub fn dd_corner(&self, d: Vec2) -> Option<Vec2> {
        if d.x != 0 && d.y != 0 {
            let offset = self.offset(d);
            if d.y.is_positive() {
                if d.x.is_positive() {
                    Some(offset.corners()[1])
                } else {
                    Some(offset.corners()[0])
                }
            } else {
                if d.x.is_positive() {
                    Some(offset.corners()[3])
                } else {
                    Some(offset.corners()[2])
                }
            }
        } else {
            None
        }
    }
    pub fn draw(&self, colour: u8) {
        crate::rectb(self.x.into(), self.y.into(), self.w.into(), self.h.into(), colour);
    }
}
