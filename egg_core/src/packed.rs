#[derive(Debug, Clone)]
pub struct PackedI16(u32);
impl PackedI16 {
    pub const fn to_i16(&self) -> (i16, i16) {
        to_i16(self.0)
    }
    pub const fn from_i16(x: i16, y: i16) -> Self {
        Self(from_i16(x, y))
    }
    pub const fn x(&self) -> i16 {
        self.to_i16().0
    }
    pub const fn y(&self) -> i16 {
        self.to_i16().1
    }
    pub fn test() {
        let x = PackedI16::from_i16(-0x7FFA, -0x4ABC);
        assert_eq!(x.to_i16(), (0x7FFF, 0x4ABC));
    }
}

#[derive(Debug, Clone)]
pub struct PackedU8(u32);
impl PackedU8 {
    pub const fn to_u8(&self) -> (u8, u8, u8, u8) {
        to_u8(self.0)
    }
    pub const fn from_u8(i: (u8, u8, u8, u8)) -> Self {
        Self(from_u8(i))
    }
    pub fn test() {
        let x = PackedU8::from_u8((0xDE, 0xAD, 0xBE, 0xEF));
        assert_eq!(x.to_u8(), (0xDE, 0xAD, 0xBE, 0xEF));
    }
}

const fn to_i16(x: u32) -> (i16, i16) {
    ((x >> 16) as i16, (x & 0xFFFF) as i16)
}
const fn from_i16(x: i16, y: i16) -> u32 {
    y as u32 | (x as u32) << 16
}
const fn to_u8(x: u32) -> (u8, u8, u8, u8) {
    (
        (x >> 24) as u8,
        ((x >> 16) & 0xFF) as u8,
        ((x >> 8) & 0xFF) as u8,
        (x & 0xFF) as u8,
    )
}
const fn from_u8(i: (u8, u8, u8, u8)) -> u32 {
    (i.0 as u32) << 24 | (i.1 as u32) << 16 | (i.2 as u32) << 8 | i.3 as u32
}
