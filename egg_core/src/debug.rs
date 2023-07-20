use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct DebugInfo {
    pub player_info: AtomicBool,
    pub map_info: AtomicBool,
    pub memory_info: AtomicBool,
    pub memory_index: AtomicUsize,
}
impl DebugInfo {
    pub const fn const_default() -> Self {
        DebugInfo {
            player_info: AtomicBool::new(false),
            map_info: AtomicBool::new(true),
            memory_info: AtomicBool::new(false),
            memory_index: AtomicUsize::new(0),
        }
    }
    pub fn player_info(&self) -> bool {
        self.player_info.load(Ordering::SeqCst)
    }
    pub fn map_info(&self) -> bool {
        self.map_info.load(Ordering::SeqCst)
    }
    pub fn memory_info(&self) -> bool {
        self.memory_info.load(Ordering::SeqCst)
    }
    pub fn memory_index(&self) -> usize {
        self.memory_index.load(Ordering::SeqCst)
    }
    pub fn set_player_info(&self, new: bool) {
        self.player_info.store(new, Ordering::SeqCst);
    }
    pub fn set_map_info(&self, new: bool) {
        self.map_info.store(new, Ordering::SeqCst);
    }
    pub fn set_memory_info(&self, new: bool) {
        self.memory_info.store(new, Ordering::SeqCst);
    }
    pub fn set_memory_index(&self, new: usize) {
        self.memory_index.store(new, Ordering::SeqCst);
    }
}