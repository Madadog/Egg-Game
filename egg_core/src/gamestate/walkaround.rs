use std::fmt::format;
use std::sync::atomic::{Ordering, AtomicU8};

use crate::animation::Animation;
use crate::data::map_data::{
    MapIndex, BEDROOM, DEFAULT_MAP_SET, SUPERMARKET, TEST_PEN, WILDERNESS,
};
use crate::data::{dialogue_data::*, save, sound};
use crate::gamestate::inventory::INVENTORY;
use crate::gamestate::Game;
use tic80_api::helpers::input_manager::{any_btnpr, mem_btn, mem_btnp};
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, WarpMode};
use crate::particles::{Particle, ParticleDraw, ParticleList};
use crate::player::{Companion, CompanionList, CompanionTrail, Player};
use crate::position::Vec2;
use crate::{camera::Camera, dialogue::Dialogue, gamestate::GameState, map::MapSet};
use tic80_api::{core::*, helpers::*};

use self::creatures::Creature;
use self::cutscene::Cutscene;
mod creatures;
mod cutscene;

pub struct WalkaroundState<'a> {
    player: Player,
    companion_trail: CompanionTrail<16>,
    companion_list: CompanionList,
    map_animations: Vec<Animation<'a>>,
    creatures: Vec<Creature>,
    camera: Camera,
    current_map: MapSet<'a>,
    dialogue: Dialogue,
    particles: ParticleList,
    cutscene: Option<Cutscene>,
    bg_colour: AtomicU8,
    sync_helper: SyncHelper,
}
impl<'a> WalkaroundState<'a> {
    pub const fn new() -> Self {
        Self {
            player: Player::const_default(),
            companion_trail: CompanionTrail::new(),
            companion_list: CompanionList::new(),
            map_animations: Vec::new(),
            creatures: Vec::new(),
            camera: Camera::const_default(),
            current_map: DEFAULT_MAP_SET,
            dialogue: Dialogue::const_default(),
            particles: ParticleList::new(),
            cutscene: None,
        }
    }
    pub fn load_map(&mut self, map_set: MapSet<'a>, bg_colour: AtomicU8, sync_helper: &mut SyncHelper) {
        let map1 = &map_set.maps.first().expect("Tried to load an empty map...");
        if let Some(bounds) = &map_set.camera_bounds {
            self.camera.bounds = bounds.clone();
        } else {
            let map_size = map1.size();
            let map_offset = map1.offset();
            self.camera = Camera::from_map_size(map_size, map_offset);
        }
        bg_colour.store(map_set.bg_colour, Ordering::SeqCst);
        if let Some(track) = map_set.music_track {
            music(track as i32, MusicOptions::default());
        };
        if map_set.bank != sync_helper.last_bank() {
            let x = sync_helper.sync(1 | 4 | 8 | 16 | 64 | 128, map_set.bank);
            if x.is_err() {
                let bank = map_set.bank;
                tic80_api::trace_tic80!(
                    format!("COULD NOT SYNC TO BANK {bank} THIS IS A BUG BTW"),
                    12
                );
            }
        }

        self.map_animations = map_set
            .interactables
            .iter()
            .flat_map(|x| x.sprite)
            .map(|frames| Animation {
                frames,
                ..Animation::const_default()
            })
            .collect();

        self.current_map = map_set;

        self.map_animations.shrink_to_fit();

        self.creatures.clear();
        self.particles.clear();
    }
    pub fn cam_x(&self) -> i32 {
        self.camera.pos.x.into()
    }
    pub fn cam_y(&self) -> i32 {
        self.camera.pos.y.into()
    }
    pub fn cam_state(&mut self) -> &mut crate::camera::CameraBounds {
        &mut self.camera.bounds
    }
    pub fn execute_interact_fn(&mut self, interact: &InteractFn) -> Option<&'static str> {
        match interact {
            InteractFn::ToggleDog => {
                self.companion_trail.fill(self.player.pos, self.player.dir);
                if self.companion_list.has(Companion::Dog) {
                    self.companion_list.remove(Companion::Dog);
                    sound::ALERT_DOWN.play();
                    Some(DOG_RELINQUISHED)
                } else {
                    self.companion_list.add(Companion::Dog);
                    sound::EQUIP_OBTAINED.play();
                    Some(DOG_OBTAINED)
                }
            }
            InteractFn::StairwellWindow => {
                save::HOUSE_STAIRWELL_WINDOW_INTERACTED.set_true();
                Some(HOUSE_STAIRWELL_WINDOW)
            }
            InteractFn::StairwellPainting => {
                if save::HOUSE_STAIRWELL_WINDOW_INTERACTED.is_true() {
                    Some(HOUSE_STAIRWELL_PAINTING_AFTER)
                } else {
                    Some(HOUSE_STAIRWELL_PAINTING_INIT)
                }
            }
            InteractFn::Note(note) => {
                sound::PIANO.with_note(*note).play();
                None
            }
            InteractFn::Piano(origin) => {
                let mut note = (self.player.pos.x + 4 - origin.x) / 8;
                let x = origin.x + note * 8;
                let y = if self.player.pos.y - origin.y < 2 {
                    note += 5;
                    origin.y + 1
                } else {
                    origin.y + 17
                };
                self.particles.add(Particle::new(
                    ParticleDraw::Rect(7, 15, 3),
                    10,
                    Vec2::new(x, y),
                ));
                let (x, y) = (origin.x + note * 6 - 2, origin.y - 7);
                self.particles.add(
                    Particle::new(
                        ParticleDraw::Rect(6, 7, note as u8 % 12 + 1),
                        60,
                        Vec2::new(x, y),
                    )
                    .with_velocity(Vec2::new(0, -1)),
                );
                sound::PIANO.with_note(note as i32).play();
                None
            }
            InteractFn::AddCreatures(x) => {
                self.creatures.extend(
                    (0..=*x).map(|_| Creature::const_default().with_offset(self.player.pos)),
                );
                None
            }
            InteractFn::Pet(vec, flip) => {
                self.cutscene = Some(Cutscene::pet_dog(*vec, self.player.pos, *flip));
                None
            }
            _ => Some(HOUSE_BACKYARD_DOGHOUSE),
        }
    }

    fn play_cutscene(&mut self) -> bool {
        if self.cutscene.is_some() {
            let mut intermediate = self
                .cutscene
                .clone()
                .unwrap_or_else(|| std::process::abort());
            match intermediate.next_stage(&self) {
                cutscene::CutsceneState::Playing => {
                    intermediate.advance(self);
                    self.cutscene = Some(intermediate);
                    true
                }
                cutscene::CutsceneState::Finished => {
                    self.cutscene = None;
                    false
                }
            }
        } else {
            false
        }
    }

    fn save(&self, new_map: &MapIndex) {
        save::CURRENT_MAP.set(new_map.0 as u8);
        let x = self.player.pos.x.to_le_bytes();
        save::PLAYER_X[0].set(x[0]);
        save::PLAYER_X[1].set(x[1]);
        let y = self.player.pos.y.to_le_bytes();
        save::PLAYER_Y[0].set(y[0]);
        save::PLAYER_Y[1].set(y[1]);
    }

    pub fn load_pmem(&mut self, bg_colour: AtomicU8, sync_helper: &mut SyncHelper) {
        self.load_map(MapIndex(save::CURRENT_MAP.get().into()).map(), bg_colour, sync_helper);
        self.player.pos.x = i16::from_le_bytes([save::PLAYER_X[0].get(), save::PLAYER_X[1].get()]);
        self.player.pos.y = i16::from_le_bytes([save::PLAYER_Y[0].get(), save::PLAYER_Y[1].get()]);
    }
}

impl<'a> Game for WalkaroundState<'a> {
    fn step(&mut self) -> Option<GameState> {
        self.map_animations
            .iter_mut()
            .for_each(|anim| anim.advance());

        self.particles.step();
        self.creatures.iter_mut().for_each(|x| x.step());

        if self.play_cutscene() {
            return None;
        }

        if keyp(28, -1, -1) {
            self.load_map(SUPERMARKET, bg_colour, sync_helper);
        }
        if keyp(29, -1, -1) {
            self.load_map(WILDERNESS);
        }
        if keyp(30, -1, -1) {
            self.load_map(TEST_PEN);
        }
        if keyp(31, -1, -1) {
            self.load_map(BEDROOM);
        }
        if keyp(32, -1, -1) {
            self.load_pmem();
        }
        if keyp(33, -1, -1) {
            set_palette(crate::tic80_helpers::SWEETIE_16);
        }
        if keyp(34, -1, -1) {
            set_palette(crate::tic80_helpers::NIGHT_16);
        }
        if keyp(35, -1, -1) {
            set_palette(crate::tic80_helpers::B_W);
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;
        if matches!(self.dialogue.current_text, None) && self.dialogue.next_text.is_empty() {
            if mem_btn(0) {
                dy -= 1;
            }
            if mem_btn(1) {
                dy += 1;
            }
            if mem_btn(2) {
                dx -= 1;
            }
            if mem_btn(3) {
                dx += 1;
            }
            if mem_btnp(5) {
                if let Ok(mut inventory) = INVENTORY.write() {
                    inventory.open()
                }
                return Some(GameState::Inventory);
            }
        } else {
            if self.dialogue.characters == 0 {
                sound::INTERACT.play();
            }
            self.dialogue.tick(1);
            if mem_btn(4) {
                self.dialogue.tick(2);
            }
            if mem_btnp(5) {
                self.dialogue.skip();
            }
        }
        if mem_btnp(4) && self.dialogue.is_line_done() {
            interact = true;
            if self.dialogue.next_text(false) {
                interact = false;
            } else if matches!(self.dialogue.current_text, Some(_)) {
                interact = false;
                self.dialogue.close();
            }
            trace!("Attempting interact...", 11);
        }
        if mem_btnp(6) {
            return Some(GameState::MainMenu(super::menu::MenuState::debug_options()));
        }
        if any_btnpr() {
            self.player.flip_controls = Axis::None
        }
        let noclip = if key(63) && key(64) {
            dy *= 3;
            dx *= 4;
            true
        } else {
            false
        };

        let (dx, dy) = self.player.walk(dx, dy, noclip, &self.current_map);
        self.player.apply_motion(dx, dy, &mut self.companion_trail);

        // Set after player.dir has updated
        let interact_hitbox = self
            .player
            .hitbox()
            .offset_xy(self.player.dir.0.into(), self.player.dir.1.into());

        let mut warp_target = None;
        for warp in self.current_map.warps.iter() {
            if self.player.hitbox().touches(warp.hitbox())
                || (interact && interact_hitbox.touches(warp.hitbox()))
            {
                match warp.mode {
                    WarpMode::Interact => {
                        sound::DOOR.play();
                    }
                    _ => {}
                };
                warp_target = Some(warp.clone());
                break;
            }
        }
        if let Some(target) = warp_target {
            self.player.pos = target.target();
            self.player.flip_controls = target.flip;
            self.companion_trail.fill(self.player.pos, self.player.dir);
            if let Some(new_map) = target.map {
                self.save(&new_map);
                self.load_map(new_map.map());
            }
        } else if interact {
            for item in self
                .current_map
                .interactables
                .iter()
                .chain(self.companion_list.interact(&self.companion_trail).iter())
            {
                if interact_hitbox.touches(item.hitbox) {
                    match &item.interaction {
                        Interaction::Text(x) => {
                            self.dialogue.add_text(x);
                        }
                        Interaction::Dialogue(x) => {
                            self.dialogue.set_dialogue(x);
                        }
                        Interaction::EnumText(x) => {
                            self.dialogue.set_enum_text(x);
                        }
                        Interaction::Func(x) => {
                            if let Some(dialogue) = self.execute_interact_fn(x) {
                                self.dialogue.add_text(dialogue);
                            };
                        }
                        x => {}
                    }
                    break;
                }
            }
        }

        self.camera
            .center_on(self.player.pos.x + 4, self.player.pos.y + 8);
        None
    }
    fn draw(&self) {
        // Draw BG
        palette_map_reset();
        cls(BG_COLOUR.load(Ordering::SeqCst));
        self.current_map.draw_bg(self.camera.pos);

        self.particles.draw(-self.cam_x(), -self.cam_y());
        blit_segment(4);
        // Collect sprites for drawing
        let mut sprites: Vec<DrawParams> = Vec::new();

        sprites.push(self.player.draw_params(self.camera.pos));

        for (anim, hitbox) in self.map_animations.iter().zip(
            self.current_map
                .interactables
                .iter()
                .filter(|x| x.sprite.is_some())
                .map(|x| x.hitbox),
        ) {
            sprites.push(DrawParams::new(
                anim.current_frame().spr_id.into(),
                anim.current_frame().pos.x as i32 + hitbox.x as i32 - self.cam_x(),
                anim.current_frame().pos.y as i32 + hitbox.y as i32 - self.cam_y(),
                anim.current_frame().options.clone(),
                anim.current_frame().outline_colour,
                anim.current_frame().palette_rotate,
            ));
        }

        sprites.extend(
            self.creatures
                .iter()
                .map(|x| x.draw_params(self.camera.pos)),
        );

        for (i, companion) in self.companion_list.companions.iter().enumerate() {
            if let Some(companion) = companion {
                let (position, direction) = if i == 0 {
                    self.companion_trail.oldest()
                } else {
                    self.companion_trail.mid()
                };
                let walktime = self.companion_trail.walktime();
                let params = companion.spr_params(position, direction, walktime, &self.camera);
                sprites.push(params);
            }
        }

        // Sort sprites in order of Y index
        sprites.sort_by(|a, b| a.bottom().partial_cmp(&b.bottom()).unwrap_or_else(|| std::process::abort()));

        // Draw sprites
        for options in sprites {
            options.draw();
        }

        // Draw FG
        palette_map_reset();
        self.current_map.draw_fg(self.camera.pos);

        if let Some(string) = &self.dialogue.current_text {
            self.dialogue.draw_dialogue_box(string, true);
        }
        if DEBUG_INFO.map_info() {
            for warp in self.current_map.warps.iter() {
                warp.hitbox()
                    .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
                    .draw(12);
            }
            self.player
                .hitbox()
                .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
                .draw(12);
            for item in self.current_map.interactables.iter() {
                item.hitbox
                    .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
                    .draw(14);
            }
        }
        if DEBUG_INFO.player_info() {
            print_raw(
                &format!("Player: {:#?}\0", self.player),
                0,
                0,
                PrintOptions {
                    small_text: true,
                    color: 11,
                    ..Default::default()
                }
            );
            print_raw(
                &format!("Camera: {:#?}\0", self.camera),
                74,
                0,
                PrintOptions {
                    small_text: true,
                    color: 11,
                    ..Default::default()
                }
            );
        }
    }
}
