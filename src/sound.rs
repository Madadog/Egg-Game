use crate::tic80_core::{sfx, SfxOptions};

pub struct SfxData {
    id: i32,
    options: SfxOptions,
}
impl SfxData {
    pub const fn new(id: i32, options: SfxOptions) -> Self {
        Self { id, options }
    }
    pub fn play(self) {
        //todo: check if channel is occupied and return bool
        sfx(self.id, self.options);
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
    note: -1,
    octave: -1,
    duration: -1,
    channel: 0,
    volume_left: 15,
    volume_right: 15,
    speed: 0,
};

pub const EQUIP_OBTAINED: SfxData = SfxData::new(
    33,
    SfxOptions {
        note: 0,
        octave: 5,
        speed: -2,
        duration: 80,
        ..DEFAULT_SFX
    },
);

pub const DENY: SfxData = SfxData::new(
    34,
    SfxOptions {
        note: 0,
        octave: 4,
        speed: 1,
        channel: 3,
        volume_left: 7,
        volume_right: 7,
        duration: 50,
        ..DEFAULT_SFX
    },
);

pub const ALERT_DOWN: SfxData = SfxData::new(
    36,
    SfxOptions {
        note: 0,
        octave: 5,
        speed: 0,
        duration: 15,
        ..DEFAULT_SFX
    },
);

pub const PIANO: SfxData = SfxData::new(
    32,
    SfxOptions {
        note: 0,
        octave: 5,
        duration: 60,
        ..DEFAULT_SFX
    },
);

pub const INTERACT: SfxData = SfxData::new(
    39,
    SfxOptions {
        note: 4,
        octave: 5,
        speed: 2,
        channel: 3,
        volume_left: 7,
        volume_right: 7,
        duration: 5,
        ..DEFAULT_SFX
    },
);

pub const CLICK: SfxData = SfxData::new(
    41,
    SfxOptions {
        note: 0,
        octave: 6,
        channel: 3,
        volume_left: 7,
        volume_right: 7,
        duration: 50,
        ..DEFAULT_SFX
    },
);

pub const DOOR: SfxData = SfxData::new(
    42,
    SfxOptions {
        note: 0,
        octave: 4,
        channel: 3,
        volume_left: 3,
        volume_right: 3,
        duration: 20,
        ..DEFAULT_SFX
    },
);
