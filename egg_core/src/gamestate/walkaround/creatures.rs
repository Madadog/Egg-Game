use crate::{
    position::{Hitbox, Vec2}, system::{StaticDrawParams, ConsoleApi},
};

#[derive(Clone)]
pub struct Creature {
    pub hitbox: Hitbox,
    pub state: CreatureState,
    pub sprite: i16,
    pub flip_h: bool,
}
impl Creature {
    pub const fn default() -> Self {
        Self {
            hitbox: Hitbox::new(0, 0, 8, 8),
            state: CreatureState::Egg(Timer(255)),
            sprite: 688,
            flip_h: false,
        }
    }
    pub fn with_offset(self, delta: Vec2) -> Self {
        Self {
            hitbox: self.hitbox.offset(delta),
            ..self
        }
    }
    pub fn step(&mut self, system: &mut impl ConsoleApi) {
        match &mut self.state {
            CreatureState::Idle(timer) => {
                if timer.tick() {
                    self.state = CreatureState::Walking(
                        Timer(system.rng().rand_u8().min(80)),
                        Vec2::new((system.rng().rand_u8() % 3) as i16 - 1, (system.rng().rand_u8() % 3) as i16 - 1),
                    );
                }
            }
            CreatureState::Walking(timer, vec) => {
                if timer.tick() {
                    self.state = CreatureState::Idle(Timer(system.rng().rand_u8().min(80)));
                } else if timer.0 % 3 == 0 {
                    if vec.x != 0 {
                        self.flip_h = vec.x.is_negative()
                    }
                    self.hitbox = self.hitbox.offset(*vec);
                }
            }
            CreatureState::Egg(timer) => {
                if timer.tick() {
                    self.state = CreatureState::Idle(Timer(system.rng().rand_u8().min(80)));
                }
            }
        }
    }
    pub fn draw_params(&self, offset: Vec2) -> StaticDrawParams {
        let sprite: i32 = match &self.state {
            CreatureState::Idle(_) => self.sprite.into(),
            CreatureState::Walking(x, _) => i32::from(self.sprite) + i32::from(x.0 / 20) % 2,
            CreatureState::Egg(_) => i32::from(self.sprite) - 32 * 5 - 4,
        };
        let offset = offset * Vec2::new(-1, -1);
        let flip = match self.flip_h {
            true => tic80_api::core::Flip::Horizontal,
            false => tic80_api::core::Flip::None,
        };
        StaticDrawParams::new(
            sprite,
            self.hitbox.offset(offset).x.into(),
            self.hitbox.offset(offset).y.into(),
            tic80_api::core::StaticSpriteOptions {
                flip,
                ..tic80_api::core::StaticSpriteOptions::transparent_zero()
            },
            Some(1),
            1,
        )
    }
}

#[derive(Clone)]
pub struct Timer(pub u8);

impl Timer {
    pub fn tick_amt(&mut self, amount: u8) -> bool {
        self.0 = self.0.saturating_sub(amount);
        self.0 == 0
    }
    pub fn tick(&mut self) -> bool {
        self.tick_amt(1)
    }
}

#[derive(Clone)]
pub enum CreatureState {
    Idle(Timer),
    Walking(Timer, Vec2),
    Egg(Timer),
}

/*
struct GameData {next_map: Option<&'static crate::map::MapSet<'static>>}
impl GameData {
    fn load_next_map(&mut self) -> Option<&'static crate::map::MapSet<'static>> {
        self.next_map.take()
    }
}
struct GameMap(&'static crate::map::MapSet<'static>);
impl GameMap {
    fn interact(&self, game_data: &mut GameData) {
        game_data.next_map = None;
    }
}

struct Game {game_data: GameData, game_map: GameMap}



impl Game {
    fn run(&mut self) {
        if let Some(x) = self.game_data.load_next_map() {
            self.game_map = GameMap(x);
        }
        self.game_map.interact(&mut self.game_data)
    }
} */