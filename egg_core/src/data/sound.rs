use crate::system::SfxOptions;

#[derive(Debug, Clone)]
pub struct SfxData {
    pub id: &'static str,
    pub options: SfxOptions,
}
impl SfxData {
    pub const fn new(id: &'static str, options: SfxOptions) -> Self {
        Self { id, options }
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

pub const DEFAULT_SFX: SfxOptions = SfxOptions {
    note: 0,
    octave: 5,
};

/// Resolve a sound effect by its script name (lowercased identifier, e.g.
/// `"gain"`), for sounds embedded in dialogue.
pub fn by_name(name: &str) -> Option<SfxData> {
    Some(match name {
        "gain" => GAIN,
        "loss" => LOSS,
        "fanfare" => FANFARE,
        "equip_obtained" => EQUIP_OBTAINED,
        "alert_up" => ALERT_UP,
        "alert_down" => ALERT_DOWN,
        "deny" => DENY,
        "reject" => REJECT,
        "click" => CLICK,
        "door" => DOOR,
        "interact" => INTERACT,
        "save" => SAVE,
        _ => return None,
    })
}

pub const PIANO: SfxData = SfxData::new(
    "1_piano",
    SfxOptions {
        note: 0,
        octave: 4,
    },
);

pub const EQUIP_OBTAINED: SfxData = SfxData::new(
    "2_obtained",
    DEFAULT_SFX,
);

pub const DENY: SfxData = SfxData::new(
    "3_deny",
    DEFAULT_SFX,
);
pub const ALERT_UP: SfxData = SfxData::new(
    "4_alert_up",
    DEFAULT_SFX,
);
pub const ALERT_DOWN: SfxData = SfxData::new(
    "5_alert_down",
    DEFAULT_SFX,
);

pub const SAVE: SfxData = SfxData::new(
    "6_save",
    DEFAULT_SFX,
);

pub const REJECT: SfxData = SfxData::new(
    "7_reject",
    DEFAULT_SFX,
);

pub const ITEM_UP: SfxData = SfxData::new(
    "8_item_up",
    DEFAULT_SFX,
);

pub const ITEM_SWAP: SfxData = SfxData::new(
    "9_item_swap",
    DEFAULT_SFX,
);

pub const ITEM_DOWN: SfxData = SfxData::new(
    "10_item_down",
    DEFAULT_SFX,
);

pub const INTERACT: SfxData = SfxData::new(
    "11_interact",
    DEFAULT_SFX,
);

pub const CLICK: SfxData = SfxData::new(
    "12_bip",
    DEFAULT_SFX,
);

pub const DOOR: SfxData = SfxData::new(
    "13_door",
    DEFAULT_SFX,
);

pub const POP: SfxData = SfxData::new(
    "14_pop",
    DEFAULT_SFX,
);

pub const CLICK_POP: SfxData = SfxData::new(
    "15_click_pop",
    DEFAULT_SFX,
);

pub const FANFARE: SfxData = SfxData::new(
    "16_fanfare",
    DEFAULT_SFX,
);

pub const GAIN: SfxData = SfxData::new(
    "17_gain",
    DEFAULT_SFX,
);

pub const LOSS: SfxData = SfxData::new(
    "18_loss",
    DEFAULT_SFX,
);

pub const STAIRS_DOWN: SfxData = SfxData::new(
    "19_stairs_down",
    DEFAULT_SFX,
);

pub const STAIRS_UP: SfxData = SfxData::new(
    "20_stairs_up",
    DEFAULT_SFX,
);

pub const FOOTSTEP_PLAIN: SfxData = SfxData::new(
    "21_footstep_plain",
    DEFAULT_SFX,
);

pub mod music {
    /// A music track, identified by name — its file stem under `assets/music/`,
    /// which the host loads as `music/<id>.ogg`. The set of real tracks is
    /// discovered from that directory at runtime (see
    /// [`ConsoleApi::music_tracks`](crate::system::ConsoleApi::music_tracks)); a
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
