use tic80_api::core::SfxOptions;

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
    pub fn with_volume(self, volume: i32) -> Self {
        Self {
            options: SfxOptions {
                volume_left: volume,
                volume_right: volume,
                ..self.options
            },
            ..self
        }
    }
}

pub const DEFAULT_SFX: SfxOptions = SfxOptions {
    note: 0,
    octave: 5,
    duration: -1,
    channel: 0,
    volume_left: 15,
    volume_right: 15,
    speed: 0,
};

pub const PIANO: SfxData = SfxData::new(
    "1_piano",
    SfxOptions {
        note: 0,
        octave: 4,
        duration: 60,
        ..DEFAULT_SFX
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
    "15_click_pup",
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


