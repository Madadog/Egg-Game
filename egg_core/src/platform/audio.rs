//! Host audio options. The note/octave a sound effect plays at — paired with a
//! sound id by [`SfxData`](crate::data::sound::SfxData) and handed to the host
//! through [`ConsoleApi::sfx`](super::ConsoleApi::sfx).

#[derive(Debug, Clone)]
pub struct SfxOptions {
    pub note: i32,
    pub octave: i32,
}
impl Default for SfxOptions {
    fn default() -> Self {
        Self {
            note: -1,
            octave: -1,
        }
    }
}
