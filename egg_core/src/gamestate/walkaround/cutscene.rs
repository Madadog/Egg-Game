use std::collections::HashMap;

use crate::{
    data::sound,
    player::Shell,
    position::Vec2,
    system::{ConsoleApi, ConsoleHelper},
};

use super::WalkaroundState;

#[derive(Clone, Debug)]
pub struct Cutscene {
    stages: Vec<Vec<CutsceneItem>>,
    _entities: HashMap<String, usize>,
    index: usize,
}

impl Cutscene {
    pub fn new(stages: Vec<Vec<CutsceneItem>>, entities: HashMap<String, usize>) -> Self {
        Self {
            stages,
            _entities: entities,
            index: 0,
        }
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
        let hashmap = HashMap::new();
        Self::new(vec, hashmap)
    }
    /// An out-of-range stage counts as done; [`next_stage`](Self::next_stage)
    /// checks [`is_cutscene_done`](Self::is_cutscene_done) before this.
    pub fn is_stage_done(&self, walkaround: &WalkaroundState) -> bool {
        self.stages
            .get(self.index)
            .is_none_or(|stage| stage.iter().all(|x| x.is_done(walkaround)))
    }
    pub fn is_cutscene_done(&self, _walkaround: &WalkaroundState) -> bool {
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
        if let Some(x) = self.stages.get_mut(self.index) {
            x.iter_mut().for_each(|x| {
                x.advance(system, walkaround);
            });
        };
    }
}

pub enum CutsceneState {
    Playing,
    Finished,
}

#[derive(Clone, Debug)]
pub enum CutsceneItem {
    WalkPlayer(Vec2),
    WalkEntity(Vec2, usize),
    FacePlayer((i8, i8)),
    MovePlayer(Vec2),
    PetDog(u8),
}
impl CutsceneItem {
    pub fn is_done(&self, walkaround: &WalkaroundState) -> bool {
        match self {
            CutsceneItem::WalkPlayer(pos) => walkaround.player_ref().pos == *pos,
            CutsceneItem::WalkEntity(pos, i) => {
                walkaround.entities.get(*i).map(|x| x.pos == *pos).unwrap()
            }
            CutsceneItem::MovePlayer(pos) => walkaround.player_ref().pos == *pos,
            CutsceneItem::FacePlayer(_) => true,
            CutsceneItem::PetDog(x) => *x > 90,
        }
    }
    pub fn advance(&mut self, system: &mut impl ConsoleApi, walkaround: &mut WalkaroundState) {
        match self {
            CutsceneItem::WalkPlayer(vec2) => {
                let Vec2 { x, y } = walkaround.player().pos.towards(vec2);
                let (dx, dy) = walkaround.player().apply_walk_direction(x, y);
                let mut trail = walkaround.companion_trail.clone();

                walkaround.player().apply_motion(dx, dy, Some(&mut trail));

                if self.is_done(walkaround) {
                    walkaround.player().apply_motion(0, 0, Some(&mut trail));
                }

                walkaround.companion_trail = trail;
            }
            CutsceneItem::WalkEntity(vec2, i) => {
                let shell = if let Some(entity) = walkaround.entities.get_mut(*i) {
                    entity
                } else {
                    walkaround.entities.push(Shell::ellie());
                    *i = walkaround.entities.len() - 1;
                    walkaround.entities.last_mut().unwrap()
                };
                let Vec2 { x, y } = shell.pos.towards(vec2);
                let (dx, dy) = shell.apply_walk_direction(x, y);

                shell.apply_motion(dx, dy, Some(&mut walkaround.companion_trail));

                if shell.pos == *vec2 {
                    shell.apply_motion(0, 0, Some(&mut walkaround.companion_trail));
                }
            }
            CutsceneItem::MovePlayer(pos) => {
                let Vec2 { x, y } = walkaround.player().pos.towards(pos);
                let (dx, dy) = walkaround.player().apply_walk_direction(x, y);
                walkaround.player().pos = walkaround.player().pos + Vec2::new(dx, dy);
                walkaround.player().animate_walk();
                if self.is_done(walkaround) {
                    walkaround.player().animate_stop();
                }
            }
            CutsceneItem::PetDog(x) => {
                walkaround.player().pet_timer = Some(*x);
                if *x % 20 == 0 {
                    system.play_sound(sound::POP);
                }
                *x += 1;
                if self.is_done(walkaround) {
                    walkaround.player().pet_timer = None;
                }
            }
            CutsceneItem::FacePlayer(dir) => walkaround.player().dir = *dir,
        }
    }
}
