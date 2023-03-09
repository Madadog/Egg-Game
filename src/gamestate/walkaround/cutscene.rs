use crate::position::Vec2;

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
    pub fn pet_dog(position: Vec2, initial_position: Vec2) -> Cutscene {
        Self::new(vec![
            vec![CutsceneItem::MovePlayer(position)],
            vec![CutsceneItem::PetDog(0)],
            vec![CutsceneItem::MovePlayer(initial_position)],
        ])
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
    pub fn advance(&mut self, walkaround: &mut WalkaroundState) {
        self.stages.get_mut(self.index).and_then(|x| {
            x.iter_mut().for_each(|x| {
                x.advance(walkaround);
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
    MovePlayer(Vec2),
    PetDog(u8),
}
impl CutsceneItem {
    pub fn is_done(&self, walkaround: &WalkaroundState) -> bool {
        match self {
            CutsceneItem::WalkPlayer(pos) => walkaround.player.pos == *pos,
            CutsceneItem::MovePlayer(pos) => walkaround.player.pos == *pos,
            CutsceneItem::PetDog(x) => *x > 60,
        }
    }
    pub fn advance(&mut self, walkaround: &mut WalkaroundState) {
        match self {
            CutsceneItem::WalkPlayer(pos) => {
                let Vec2 { x, y } = walkaround.player.pos.towards(pos);
                let (dx, dy) = walkaround.player.walk(x, y, true, &walkaround.current_map);

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
                let (dx, dy) = walkaround.player.walk(x, y, true, &walkaround.current_map);
                walkaround.player.pos = walkaround.player.pos + Vec2::new(dx, dy);
            }
            CutsceneItem::PetDog(x) => {
                walkaround.player.pet_timer = Some(*x);
                *x += 1;
                if self.is_done(walkaround) {
                    walkaround.player.pet_timer = None;
                }
            }
        }
    }
}
