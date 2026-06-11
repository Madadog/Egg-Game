/// Debug-overlay toggles, flipped by host hotkeys and read during draw.
#[derive(Default)]
pub struct DebugInfo {
    pub player_info: bool,
    pub map_info: bool,
    pub memory_info: bool,
    pub memory_index: usize,
}
