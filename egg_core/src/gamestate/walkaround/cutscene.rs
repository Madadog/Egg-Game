use crate::{data::sound, position::Vec2, system::{ConsoleApi, ConsoleHelper}};

use super::WalkaroundState;

#[derive(Clone)]
pub struct Cutscene {
    stages: Vec<Vec<CutsceneItem>>,
    index: usize,
}

impl Cutscene {
    pub fn new(stages: Vec<Vec<CutsceneItem>>) -> Self {
        Self { stages, index: 0 }
    }
    pub fn pet_dog(dog_position: Vec2, initial_position: Vec2, flip: Option<bool>) -> Cutscene {
        let (position, dir) = if let Some(flip) = flip {
            if flip {
                (dog_position + Vec2::new(8, 0), (-1, 1))
            } else {
                (dog_position + Vec2::new(-8, 0), (1, 1))
            }
        } else {
            (dog_position, (0, 0))
        };
        let mut vec = vec![
            vec![CutsceneItem::MovePlayer(position)],
            vec![CutsceneItem::PetDog(0)],
            vec![CutsceneItem::MovePlayer(initial_position)],
        ];
        if flip.is_some() {
            vec.insert(1, vec![CutsceneItem::FacePlayer(dir)]);
        }
        Self::new(vec)
    }
    pub fn is_stage_done(&self, walkaround: &WalkaroundState) -> bool {
        self.stages
            .get(self.index)
            .unwrap_or_else(|| std::process::abort())
            .iter()
            .all(|x| x.is_done(walkaround))
    }
    pub fn is_cutscene_done(&self, walkaround: &WalkaroundState) -> bool {
        self.stages.get(self.index).is_none()
    }
    pub fn next_stage(&mut self, walkaround: &WalkaroundState) -> CutsceneState {
        if self.is_cutscene_done(walkaround) {
            return CutsceneState::Finished;
        } else if self.is_stage_done(walkaround) {
            self.index += 1;
        }
        CutsceneState::Playing
    }
    pub fn advance(&mut self, system: &mut impl ConsoleApi, walkaround: &mut WalkaroundState) {
        self.stages.get_mut(self.index).and_then(|x| {
            x.iter_mut().for_each(|x| {
                x.advance(system, walkaround);
            });
            Some(())
        });
    }
}

pub enum CutsceneState {
    Playing,
    Finished,
}

#[derive(Clone)]
pub enum CutsceneItem {
    WalkPlayer(Vec2),
    FacePlayer((i8, i8)),
    MovePlayer(Vec2),
    PetDog(u8),
}
impl CutsceneItem {
    pub fn is_done(&self, walkaround: &WalkaroundState) -> bool {
        match self {
            CutsceneItem::WalkPlayer(pos) => walkaround.player.pos == *pos,
            CutsceneItem::MovePlayer(pos) => walkaround.player.pos == *pos,
            CutsceneItem::FacePlayer(_) => true,
            CutsceneItem::PetDog(x) => *x > 90,
        }
    }
    pub fn advance(&mut self, system: &mut impl ConsoleApi, walkaround: &mut WalkaroundState) {
        match self {
            CutsceneItem::WalkPlayer(pos) => {
                let Vec2 { x, y } = walkaround.player.pos.towards(pos);
                let (dx, dy) = walkaround.player.apply_walk_direction(x, y);

                walkaround
                    .player
                    .apply_motion(dx, dy, &mut walkaround.companion_trail);

                if self.is_done(walkaround) {
                    walkaround
                        .player
                        .apply_motion(0, 0, &mut walkaround.companion_trail);
                }
            }
            CutsceneItem::MovePlayer(pos) => {
                let Vec2 { x, y } = walkaround.player.pos.towards(pos);
                let (dx, dy) = walkaround.player.apply_walk_direction(x, y);
                walkaround.player.pos = walkaround.player.pos + Vec2::new(dx, dy);
                walkaround.player.animate_walk();
                if self.is_done(walkaround) {
                    walkaround.player.animate_stop();
                }
            }
            CutsceneItem::PetDog(x) => {
                walkaround.player.pet_timer = Some(*x);
                if *x % 20 == 0 {
                    system.play_sound(sound::POP);
                }
                *x += 1;
                if self.is_done(walkaround) {
                    walkaround.player.pet_timer = None;
                }
            }
            CutsceneItem::FacePlayer(dir) => walkaround.player.dir = *dir,
        }
    }
}
