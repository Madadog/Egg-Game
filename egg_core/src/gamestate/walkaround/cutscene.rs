use std::collections::HashMap;

use crate::Ctx;
use crate::data::eggscene::{CutsceneDef, StepDef};
use crate::data::sound::music::MusicTrack;
use crate::data::sound::{self, SfxData};
use crate::player::Shell;
use crate::position::Vec2;
use crate::system::{ConsoleApi, ConsoleHelper, just_pressed, pressed};

use super::WalkaroundState;

/// A playable cutscene: stages run in sequence, the items within one stage in
/// parallel each frame ([`advance`](Self::advance)). A stage is finished when
/// every item [`is_done`](CutsceneItem::is_done); the cutscene as a whole when
/// the stage index runs past the end. Built from a parsed [`CutsceneDef`] via
/// [`from_def`](Self::from_def), or directly for the companion-internal
/// [`pet_dog`](Self::pet_dog) (whose targets come from the dog's live position).
#[derive(Clone, Debug)]
pub struct Cutscene {
    stages: Vec<Vec<CutsceneItem>>,
    /// Named spawned entities, by index into [`WalkaroundState::entities`].
    /// Reserved for `spawn`/`despawn` verbs (deferred); empty today.
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

    /// Build a playable cutscene from a parsed [`CutsceneDef`]. Resolves the
    /// steps that can resolve at build time (sounds via [`sound::by_name`], music
    /// via [`MusicTrack::named`]); a dialogue step keeps its key string, resolved
    /// at play time against the active [`Script`](crate::data::script::Script). An
    /// unknown sound name becomes an inert no-op item (it still advances) rather
    /// than a hard failure, mirroring how a dangling warp/music name no-ops.
    pub fn from_def(def: &CutsceneDef) -> Cutscene {
        let stages = def
            .iter()
            .map(|stage| stage.iter().map(CutsceneItem::from_step).collect())
            .collect();
        Self::new(stages, HashMap::new())
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
    pub fn advance<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, walkaround: &mut WalkaroundState) {
        if let Some(x) = self.stages.get_mut(self.index) {
            x.iter_mut().for_each(|x| {
                x.advance(ctx, walkaround);
            });
        };
    }

    /// Fast-forward the whole cutscene to its end *safely* — the skip/abort path
    /// (B button). Every remaining item is finalised in stage order so its lasting
    /// side effects still happen: end positions snap into place, `SetFlag`/`Sound`
    /// /`Music` fire, the dialogue box closes. After this the cutscene reads as
    /// done and is dropped, so a stuck stage (an unreachable walk target) can
    /// never soft-lock the player.
    pub fn skip<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, walkaround: &mut WalkaroundState) {
        for stage in &mut self.stages[self.index..] {
            for item in stage.iter_mut() {
                item.finish(ctx, walkaround);
            }
        }
        self.index = self.stages.len();
    }
}

pub enum CutsceneState {
    Playing,
    Finished,
}

/// One runtime cutscene step. Each variant advances toward its goal per frame
/// ([`advance`](Self::advance)) and reports completion ([`is_done`](Self::is_done));
/// the stage finishes when all of its items are done. Sounds/music are resolved
/// to concrete values at build time; a [`Dialogue`](Self::Dialogue) step keeps
/// its registry *key* and resolves it against the active script the first frame
/// it runs.
#[derive(Clone, Debug)]
pub enum CutsceneItem {
    /// Walk the player to a target, with a frame budget (counts down) so an
    /// unreachable point can't hang the stage — see [`WALK_BUDGET`].
    WalkPlayer {
        target: Vec2,
        budget: u32,
    },
    WalkEntity(Vec2, usize),
    FacePlayer((i8, i8)),
    MovePlayer(Vec2),
    PetDog(u8),
    /// Hold for `frames` more frames (counts down).
    Wait(u32),
    /// Open the named dialogue and wait for the box to close. The bool latches
    /// "has been opened" so it opens exactly once; once opened, `is_done` reads
    /// the closed box. (Resolved at play time — the key follows the language.)
    Dialogue {
        key: String,
        opened: bool,
    },
    /// Set a save flag (fires once, on the first advance).
    SetFlag(String, bool),
    /// Play a sound effect (fires once). `None` = an unknown name, an inert no-op.
    Sound(Option<SfxData>),
    /// Switch the music track, or stop it (fires once).
    Music(Option<MusicTrack>),
}

/// A frame budget for [`CutsceneItem::WalkPlayer`]: if the player can't reach the
/// target within this many frames (an unreachable point behind a wall), the item
/// reports done anyway so the stage can't hang. Generous — a long on-screen walk
/// is well under this.
const WALK_BUDGET: u32 = 600;

impl CutsceneItem {
    /// Build a runtime item from a parsed [`StepDef`], resolving build-time names.
    fn from_step(step: &StepDef) -> CutsceneItem {
        match step {
            StepDef::Wait(frames) => CutsceneItem::Wait(*frames),
            StepDef::Dialogue(key) => CutsceneItem::Dialogue {
                key: key.clone(),
                opened: false,
            },
            StepDef::SetFlag(name, value) => CutsceneItem::SetFlag(name.clone(), *value),
            StepDef::Sound(name) => CutsceneItem::Sound(sound::by_name(name)),
            StepDef::Music(track) => CutsceneItem::Music(track.as_deref().map(MusicTrack::named)),
            StepDef::Walk(pos) => CutsceneItem::WalkPlayer {
                target: *pos,
                budget: WALK_BUDGET,
            },
            StepDef::Move(pos) => CutsceneItem::MovePlayer(*pos),
            StepDef::Face(dx, dy) => CutsceneItem::FacePlayer((*dx, *dy)),
        }
    }

    pub fn is_done(&self, walkaround: &WalkaroundState) -> bool {
        match self {
            // Reached the target, or the budget ran out (an unreachable point).
            CutsceneItem::WalkPlayer { target, budget } => {
                walkaround.player_ref().pos == *target || *budget == 0
            }
            CutsceneItem::WalkEntity(pos, i) => {
                walkaround.entities.get(*i).map(|x| x.pos == *pos).unwrap()
            }
            CutsceneItem::MovePlayer(pos) => walkaround.player_ref().pos == *pos,
            CutsceneItem::FacePlayer(_) => true,
            CutsceneItem::PetDog(x) => *x > 90,
            CutsceneItem::Wait(frames) => *frames == 0,
            // Done once the box has been opened *and* has fully closed (no current
            // line and an empty queue) — the pending-warp "wait until box closed"
            // rule. Before it has opened it is not done (the open is its work).
            CutsceneItem::Dialogue { opened, .. } => {
                *opened
                    && walkaround.dialogue.current_text.is_none()
                    && walkaround.dialogue.next_text.is_empty()
            }
            CutsceneItem::SetFlag(..) => true,
            CutsceneItem::Sound(_) => true,
            CutsceneItem::Music(_) => true,
        }
    }
    pub fn advance<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, walkaround: &mut WalkaroundState) {
        match self {
            CutsceneItem::WalkPlayer { target, budget } => {
                *budget = budget.saturating_sub(1);
                let Vec2 { x, y } = walkaround.player().pos.towards(target);
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
                    ctx.system.play_sound(sound::POP);
                }
                *x += 1;
                if self.is_done(walkaround) {
                    walkaround.player().pet_timer = None;
                }
            }
            CutsceneItem::FacePlayer(dir) => walkaround.player().face(*dir),
            CutsceneItem::Wait(frames) => {
                *frames = frames.saturating_sub(1);
            }
            CutsceneItem::Dialogue { key, opened } => {
                if !*opened {
                    // Resolve the key against the *active* script and open the box.
                    let convo = ctx.get_dialogue(key);
                    walkaround
                        .dialogue
                        .set_messages(ctx.system, ctx.save, &convo);
                    *opened = true;
                    return;
                }
                // The normal walk-loop dialogue input is short-circuited while a
                // cutscene runs (see `WalkaroundState::step`), so the box is driven
                // here: tick the typewriter, advance/finish a line on A, close the
                // box on the last A press, fast-skip on B — the same gestures the
                // walk loop applies.
                let pad = ctx.system.controller();
                walkaround.dialogue.tick(ctx.system, ctx.save, 1);
                if pressed(pad.a) {
                    walkaround.dialogue.tick(ctx.system, ctx.save, 2);
                }
                if just_pressed(pad.b) {
                    walkaround.dialogue.skip(ctx.system, ctx.save);
                }
                if just_pressed(pad.a)
                    && walkaround.dialogue.is_line_done()
                    && !walkaround.dialogue.next_text(ctx.system, ctx.save, false)
                    && walkaround.dialogue.current_text.is_some()
                {
                    walkaround.dialogue.close();
                }
            }
            CutsceneItem::SetFlag(name, value) => ctx.save.set_flag(name, *value),
            CutsceneItem::Sound(sfx) => {
                if let Some(sfx) = sfx {
                    ctx.system.play_sound(sfx.clone());
                }
            }
            CutsceneItem::Music(track) => ctx.system.music(track.as_ref()),
        }
    }

    /// Finalise this item for the skip path: apply its end state immediately and
    /// run any lasting side effect, leaving it [`is_done`]. Walks/moves snap to
    /// the target; a dialogue closes (opening first if it never ran, so a
    /// `#set` inside it still applies); flags/sounds/music fire if they hadn't.
    fn finish<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, walkaround: &mut WalkaroundState) {
        match self {
            CutsceneItem::WalkPlayer { target: pos, .. } | CutsceneItem::MovePlayer(pos) => {
                walkaround.player().pos = *pos;
                walkaround.player().animate_stop();
            }
            CutsceneItem::WalkEntity(pos, i) => {
                if let Some(entity) = walkaround.entities.get_mut(*i) {
                    entity.pos = *pos;
                }
            }
            CutsceneItem::FacePlayer(dir) => walkaround.player().face(*dir),
            CutsceneItem::PetDog(x) => {
                *x = 91;
                walkaround.player().pet_timer = None;
            }
            CutsceneItem::Wait(frames) => *frames = 0,
            CutsceneItem::Dialogue { key, opened } => {
                if !*opened {
                    // Never ran: resolve + queue it so its `#set` side effects
                    // still apply, then close immediately.
                    let convo = ctx.get_dialogue(key);
                    walkaround
                        .dialogue
                        .set_messages(ctx.system, ctx.save, &convo);
                    *opened = true;
                }
                walkaround.dialogue.close();
            }
            CutsceneItem::SetFlag(name, value) => ctx.save.set_flag(name, *value),
            CutsceneItem::Sound(sfx) => {
                if let Some(sfx) = sfx {
                    ctx.system.play_sound(sfx.clone());
                }
            }
            CutsceneItem::Music(track) => ctx.system.music(track.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::eggscene;
    use crate::data::save::SaveData;
    use crate::data::script::Script;
    use crate::drawstate::DrawState;
    use crate::map::MapStore;
    use crate::rand::Lcg64Xsh32;
    use crate::system::test_console::TestConsole;

    /// Everything a [`Ctx`] borrows, owned in one place so a test can hand out a
    /// fresh `Ctx` each frame (it borrows mutably, so it can't outlive a frame).
    struct Harness {
        system: TestConsole,
        draw: DrawState,
        maps: MapStore,
        rng: Lcg64Xsh32,
        script: Script,
        scenes: eggscene::SceneFile,
        save: SaveData,
        items: crate::gamestate::inventory::GameItems,
        walk: WalkaroundState,
    }
    impl Harness {
        fn new() -> Self {
            Self {
                system: TestConsole::new(),
                draw: DrawState::default(),
                maps: MapStore::default(),
                rng: Lcg64Xsh32::default(),
                script: Script::new(),
                scenes: eggscene::SceneFile::default(),
                save: SaveData::default(),
                items: crate::gamestate::inventory::GameItems::default(),
                walk: WalkaroundState::new(),
            }
        }
        /// Install a one-line dialogue under `key` so a `Dialogue` step resolves.
        fn with_dialogue(mut self, key: &str, line: &str) -> Self {
            let src = format!("#dialogue {key}\n    {line}");
            self.script
                .set_base(crate::data::eggtext::parse(&src).unwrap());
            self
        }
    }

    /// One `advance` of a lone item against the harness's walkaround.
    fn step(h: &mut Harness, item: &mut CutsceneItem) {
        // Split the borrow: the walkaround is held apart from the Ctx-borrowed
        // fields, exactly as `play_cutscene` does with `self`.
        let mut walk = std::mem::take(&mut h.walk);
        let mut ctx = Ctx {
            draw: &mut h.draw,
            system: &mut h.system,
            maps: &mut h.maps,
            rng: &mut h.rng,
            script: &h.script,
            scenes: &h.scenes,
            save: &mut h.save,
            items: &h.items,
        };
        item.advance(&mut ctx, &mut walk);
        h.walk = walk;
    }

    #[test]
    fn wait_counts_down_then_is_done() {
        let mut h = Harness::new();
        let mut item = CutsceneItem::Wait(2);
        assert!(!item.is_done(&h.walk));
        step(&mut h, &mut item);
        assert!(!item.is_done(&h.walk), "one frame left");
        step(&mut h, &mut item);
        assert!(item.is_done(&h.walk), "countdown reached zero");
        // A `Wait(0)` reads done before any advance.
        assert!(CutsceneItem::Wait(0).is_done(&h.walk));
    }

    #[test]
    fn set_flag_fires_on_advance_and_is_immediately_done() {
        let mut h = Harness::new();
        let mut item = CutsceneItem::SetFlag("seen".into(), true);
        // A `SetFlag` is structurally done the moment it exists (no frames to
        // wait), but its side effect lands on advance.
        assert!(item.is_done(&h.walk));
        assert!(!h.save.flag("seen"));
        step(&mut h, &mut item);
        assert!(h.save.flag("seen"), "flag written on advance");
    }

    #[test]
    fn sound_resolves_by_name_and_unknown_is_inert() {
        // A known name resolves to a real sfx; an unknown one becomes an inert
        // `None` item. Either way the item is done (nothing to wait on) and
        // advancing it doesn't panic.
        let mut known = CutsceneItem::from_step(&StepDef::Sound("gain".into()));
        assert!(matches!(known, CutsceneItem::Sound(Some(_))));
        let mut unknown = CutsceneItem::from_step(&StepDef::Sound("not_a_sound".into()));
        assert!(matches!(unknown, CutsceneItem::Sound(None)));

        let mut h = Harness::new();
        assert!(known.is_done(&h.walk) && unknown.is_done(&h.walk));
        step(&mut h, &mut known);
        step(&mut h, &mut unknown);
    }

    #[test]
    fn dialogue_opens_the_box_then_done_when_closed() {
        let mut h = Harness::new().with_dialogue("hi", "Hello.");
        let mut item = CutsceneItem::Dialogue {
            key: "hi".into(),
            opened: false,
        };
        // Not done before it opens (the open is its work).
        assert!(!item.is_done(&h.walk));
        // First advance opens the box; now it's not done (box is live).
        step(&mut h, &mut item);
        assert!(h.walk.dialogue.current_text.is_some(), "box opened");
        assert!(!item.is_done(&h.walk));
        // Press A to advance past the single line and close the box.
        h.system.controllers[0].a = [true, false];
        // The line must be done to register the A advance.
        h.walk.dialogue.finish_line();
        step(&mut h, &mut item);
        assert!(
            h.walk.dialogue.current_text.is_none() && h.walk.dialogue.next_text.is_empty(),
            "box closed",
        );
        assert!(item.is_done(&h.walk), "done once the box has closed");
    }

    #[test]
    fn skip_fast_forwards_side_effects_and_finishes() {
        // A scene whose remaining stages set a flag, move the player, and open a
        // dialogue: skip applies the end position, fires the flag, and closes the
        // box, all at once, leaving the cutscene done.
        let mut h = Harness::new().with_dialogue("hi", "Hello.");
        let def: eggscene::CutsceneDef = vec![
            vec![StepDef::Move(Vec2::new(40, 50))],
            vec![
                StepDef::SetFlag("done".into(), true),
                StepDef::Dialogue("hi".into()),
            ],
        ];
        let mut cutscene = Cutscene::from_def(&def);

        let mut walk = std::mem::take(&mut h.walk);
        {
            let mut ctx = Ctx {
                draw: &mut h.draw,
                system: &mut h.system,
                maps: &mut h.maps,
                rng: &mut h.rng,
                script: &h.script,
                scenes: &h.scenes,
                save: &mut h.save,
                items: &h.items,
            };
            cutscene.skip(&mut ctx, &mut walk);
        }
        h.walk = walk;

        assert!(matches!(
            cutscene.next_stage(&h.walk),
            CutsceneState::Finished
        ));
        assert_eq!(
            h.walk.player_ref().pos,
            Vec2::new(40, 50),
            "end position applied"
        );
        assert!(h.save.flag("done"), "flag side effect fired");
        assert!(
            h.walk.dialogue.current_text.is_none() && h.walk.dialogue.next_text.is_empty(),
            "dialogue closed",
        );
    }

    #[test]
    fn walk_player_budget_caps_an_unreachable_target() {
        // A target the player can't actually reach (no map/collision moves it) is
        // capped by the frame budget so the stage can't hang. Drive the item with
        // a tiny budget to keep the test fast.
        let mut h = Harness::new();
        let mut item = CutsceneItem::WalkPlayer {
            target: Vec2::new(9999, 9999),
            budget: 3,
        };
        for _ in 0..3 {
            assert!(!item.is_done(&h.walk));
            step(&mut h, &mut item);
        }
        assert!(
            item.is_done(&h.walk),
            "budget exhausted -> done despite not arriving"
        );
    }

    #[test]
    fn from_def_builds_a_playable_cutscene_from_the_shipped_pet_dog() {
        // The shipped `pet_dog` block builds without panicking and yields the same
        // stage shape it was authored with (proving the registry -> build path).
        let scenes = eggscene::parse(include_str!("../../../../assets/data/main.eggscene"))
            .expect("parse main.eggscene");
        let def = scenes.get_cutscene("pet_dog").expect("pet_dog defined");
        let cutscene = Cutscene::from_def(def);
        assert_eq!(cutscene.stages.len(), def.len());
        assert!(!cutscene.stages.is_empty());
    }
}
