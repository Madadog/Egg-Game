use crate::tic80::PERSISTENT_RAM;

pub struct PmemSlot(usize);
impl PmemSlot {
    pub const fn new(i: usize) -> Self {
        assert!(i < 1024);
        Self(i)
    }
    /// Get whole inner value as u8
    pub fn get(&self) -> u8 {
        unsafe { (*PERSISTENT_RAM)[self.0] }
    }
    /// Set whole inner value with u8
    pub fn set(&self, val: u8) {
        unsafe { (*PERSISTENT_RAM)[self.0] = val }
    }
    /// Set binary flags to 1. To set to 0, use `clear_flags()`.
    pub fn set_flags(&self, flags: u8) {
        unsafe { (*PERSISTENT_RAM)[self.0] |= flags }
    }
    /// Clear binary flags by setting them to 0.
    pub fn clear_flags(&self, flags: u8) {
        let flags = flags^255; // invert flags
        unsafe { (*PERSISTENT_RAM)[self.0] &= flags }
    }
    /// XORs binary flags.
    pub fn toggle_flags(&self, flags: u8) {
        unsafe { (*PERSISTENT_RAM)[self.0] ^= flags }
    }
    pub fn contains(&self, flag: u8) -> bool {
        self.get() & flag == flag
    }
}

/// * b0: Has intro anim been seen.
/// * b1: Font size setting.
/// * b2-7 reserved.
pub const MENU_DATA: PmemSlot = PmemSlot::new(0);
/// * b0: Window1 interacted.
/// * b1: Dog fed.
/// * b2: Living room entered.
/// * b3-7 reserved.
pub const HOUSE_FLAGS: PmemSlot = PmemSlot::new(1);
pub const EGG_COUNT: PmemSlot = PmemSlot::new(2);
pub const EGG_COUNT2: PmemSlot = PmemSlot::new(3);
pub const EGG_FLAGS: PmemSlot = PmemSlot::new(4);
pub const TOWN_FLAGS: PmemSlot = PmemSlot::new(5);
/// * b0: Thievery thwarted. 
/// * b1: Key access.
pub const SUPERMARKET_FLAGS: PmemSlot = PmemSlot::new(6);
pub const HOSPITAL_FLAGS: PmemSlot = PmemSlot::new(7);
pub const WILDERNESS_FLAGS: PmemSlot = PmemSlot::new(8);
pub const FACTORY_FLAGS: PmemSlot = PmemSlot::new(9);
pub const EGG_POP_COUNT: PmemSlot = PmemSlot::new(10);
/// * b0: Key
/// * b1: Curiosity
/// * b2: Egg^2
/// * b3: Monster
/// * b4-7 reserved.
pub const INVENTORY_FLAGS: PmemSlot = PmemSlot::new(15);
/// Inventory slots hold a u8 ItemID. There's no way I'll use ALL 255 items......
/// TODO: Convert between item and id.
pub const INVENTORY_SLOTS: [PmemSlot; 8] = [
    PmemSlot::new(16),
    PmemSlot::new(17),
    PmemSlot::new(18),
    PmemSlot::new(19),
    PmemSlot::new(20),
    PmemSlot::new(21),
    PmemSlot::new(22),
    PmemSlot::new(23),
];
/// TODO: Convert between map and id...
pub const CURRENT_MAP: PmemSlot = PmemSlot::new(24);
pub const PLAYER_X: [PmemSlot; 2] = [PmemSlot::new(25), PmemSlot::new(26)];
pub const PLAYER_Y: [PmemSlot; 2] = [PmemSlot::new(27), PmemSlot::new(28)];