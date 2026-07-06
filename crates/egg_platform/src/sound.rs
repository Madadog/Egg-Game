//! Host-facing sound value types named by the [`ConsoleApi`](crate::ConsoleApi)
//! surface: the resolved sound-effect record it plays ([`SfxData`]) and the
//! music track it streams ([`music::MusicTrack`]). These sit at the platform
//! layer because the trait signatures name them; the sound *registry* that
//! builds `SfxData`s from `data.toml` (`SfxDef`, `Sounds`, the named accessors)
//! stays in `egg_core::data::sound` and re-exports these.

use crate::SfxOptions;

/// A resolved sound effect: the file-stem id the host plays and the note/octave
/// it plays at. Built from an `SfxDef` via the `Sounds` store (in
/// `egg_core::data::sound`).
#[derive(Debug, Clone)]
pub struct SfxData {
    pub id: String,
    pub options: SfxOptions,
}
impl SfxData {
    pub fn new(id: impl Into<String>, options: SfxOptions) -> Self {
        Self {
            id: id.into(),
            options,
        }
    }
    pub fn with_note(self, note: i32) -> Self {
        Self {
            options: SfxOptions {
                note,
                ..self.options
            },
            ..self
        }
    }
}

pub mod music {
    /// A music track, identified by name — its file stem under `assets/music/`,
    /// which the host loads as `music/<id>.ogg`. The set of real tracks is
    /// discovered from that directory at runtime (see
    /// [`ConsoleApi::music_tracks`](crate::ConsoleApi::music_tracks)); a
    /// map (via its `music` property) or the title sequence refers to one by name.
    #[derive(Debug, Clone)]
    pub struct MusicTrack {
        pub id: String,
        pub speed: f32,
    }
    impl MusicTrack {
        /// A track named by its file stem — from a map's `music` property, a
        /// filename in the music directory, or an engine-fixed reference like the
        /// title theme. The only way to construct a track: there is no hardcoded
        /// track set anymore.
        pub fn named(name: impl Into<String>) -> Self {
            Self {
                id: name.into(),
                speed: 1.0,
            }
        }

        /// The same track at a given playback-rate multiplier (1.0 = normal).
        pub fn with_speed(self, speed: f32) -> Self {
            Self { speed, ..self }
        }
    }
}
