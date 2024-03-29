/// A 1-byte Pmem slot. When set, it will be saved to the player's storage device and persist across runs.
#[derive(Copy, Clone)]
pub struct PmemU8(usize);
impl PmemU8 {
    pub const fn new(i: usize) -> Self {
        assert!(i < 1024);
        Self(i)
    }
    pub fn index(&self) -> usize {
        self.0
    }
}

/// 1 bit from a Pmem slot.
#[derive(Copy, Clone)]
pub struct PmemBit {
    index: usize,
    bit: u8,
}
impl PmemBit {
    pub const fn new(index: usize, bit: u8) -> Self {
        assert!(index < 1024);
        assert!(bit.is_power_of_two());
        Self { index, bit }
    }
}
// Mocking interface
impl PmemBit {
    pub fn get_byte_with(&self, memory: &[u8]) -> u8 {
        memory[self.index]
    }
    pub fn is_true_with(&self, memory: &[u8]) -> bool {
        (memory[self.index] & self.bit) == self.bit
    }
    pub fn set_true_with(&self, memory: &mut [u8]) {
        let value = memory[self.index] | self.bit;
        memory[self.index] = value;
    }
    pub fn set_false_with(&self, memory: &mut [u8]) {
        let value = memory[self.index] & (self.bit ^ 255);
        memory[self.index] = value;
    }
    pub fn toggle_with(&self, memory: &mut [u8]) {
        let value = memory[self.index] ^ self.bit;
        memory[self.index] = value;
    }
}

pub const INTRO_ANIM_SEEN: PmemBit = PmemBit::new(0, 0b0000_0001);
pub const SMALL_TEXT_ON: PmemBit = PmemBit::new(0, 0b0000_0010);
pub const INSTRUCTIONS_READ: PmemBit = PmemBit::new(0, 0b0000_0100);
pub const MANUAL_DOORS: PmemBit = PmemBit::new(0, 0b0000_1000);

pub const HOUSE_STAIRWELL_WINDOW_INTERACTED: PmemBit = PmemBit::new(1, 0b0000_0001);
pub const DOG_FED: PmemBit = PmemBit::new(1, 0b0000_0010);
pub const LIVING_ROOM_SEEN: PmemBit = PmemBit::new(1, 0b0000_0100);

pub const EGG_COUNT_LO: PmemU8 = PmemU8::new(2);
pub const EGG_COUNT_HI: PmemU8 = PmemU8::new(3);
pub const EGG_FLAGS: PmemU8 = PmemU8::new(4);
pub const TOWN_FLAGS: PmemU8 = PmemU8::new(5);

pub const SUPERMARKET_THIEF: PmemBit = PmemBit::new(6, 0b0000_0001);
pub const SUPERMARKET_KEY_ACCESS: PmemBit = PmemBit::new(6, 0b0000_0010);
pub const SUPERMARKET_BACKROOM: PmemBit = PmemBit::new(6, 0b0000_0100);

pub const HOSPITAL_FLAGS: PmemU8 = PmemU8::new(7);

pub const WILDERNESS_EGG_FOUND: PmemBit = PmemBit::new(8, 0b0000_0001);

pub const FACTORY_FLAGS: PmemU8 = PmemU8::new(9);
pub const EGG_POP_COUNT: PmemU8 = PmemU8::new(10);

pub const IS_NIGHT: PmemBit = PmemBit::new(11, 0b0000_0001);

pub const SHELL_KEY: PmemBit = PmemBit::new(15, 0b0000_0001);
pub const SHELL_CURIOSITY: PmemBit = PmemBit::new(15, 0b0000_0010);
pub const SHELL_MATRYOSHKA: PmemBit = PmemBit::new(15, 0b0000_0100);
pub const SHELL_MONSTER: PmemBit = PmemBit::new(15, 0b0000_1000);

/// Inventory slots hold a u8 ItemID. There's no way I'll use ALL 255 items......
/// TODO: Convert between item and id.
pub const INVENTORY_SLOTS: [PmemU8; 8] = [
    PmemU8::new(16),
    PmemU8::new(17),
    PmemU8::new(18),
    PmemU8::new(19),
    PmemU8::new(20),
    PmemU8::new(21),
    PmemU8::new(22),
    PmemU8::new(23),
];
pub const CURRENT_MAP: PmemU8 = PmemU8::new(24);
pub const PLAYER_X: [PmemU8; 2] = [PmemU8::new(25), PmemU8::new(26)];
pub const PLAYER_Y: [PmemU8; 2] = [PmemU8::new(27), PmemU8::new(28)];
