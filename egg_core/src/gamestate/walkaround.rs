use crate::animation::Animation;
use crate::data::map_data::{
    MapIndex, BEDROOM, DEFAULT_MAP_SET, SUPERMARKET, TEST_PEN, WILDERNESS,
};
use crate::data::{dialogue_data::*, save, sound};
use crate::debug::DebugInfo;
use crate::gamestate::Game;
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, WarpMode};
use crate::particles::{Particle, ParticleDraw, ParticleList};
use crate::player::{Companion, CompanionList, CompanionTrail, Player};
use crate::position::Vec2;
use crate::system::{ConsoleApi, ConsoleHelper, DrawParams};
use crate::{camera::Camera, dialogue::Dialogue, gamestate::GameState, map::MapSet};
use log::{error, info};
use tic80_api::core::{MusicOptions, PrintOptions};
use tic80_api::helpers::SyncHelper;

use self::creatures::Creature;
use self::cutscene::Cutscene;

use super::inventory::InventoryUi;
// use super::{EggInput};
mod creatures;
mod cutscene;

#[derive(Clone)]
pub struct WalkaroundState<'a> {
    pub player: Player,
    pub companion_trail: CompanionTrail<16>,
    pub companion_list: CompanionList,
    pub map_animations: Vec<Animation<'a>>,
    pub creatures: Vec<Creature>,
    pub camera: Camera,
    pub current_map: MapSet<'a>,
    pub dialogue: Dialogue,
    pub particles: ParticleList,
    pub cutscene: Option<Cutscene>,
    pub bg_colour: u8,
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
            bg_colour: 0,
        }
    }
    pub fn load_map(&mut self, system: &mut impl ConsoleApi, map_set: MapSet<'a>) {
        let map1 = &map_set.maps.first().expect("Tried to load an empty map...");
        if let Some(bounds) = &map_set.camera_bounds {
            self.camera.bounds = bounds.clone();
        } else {
            let map_size = map1.size();
            let map_offset = map1.offset();
            self.camera = Camera::from_map_size(map_size, map_offset);
        }
        self.bg_colour = map_set.bg_colour;
        if let Some(track) = map_set.music_track {
            system.music(track as i32, MusicOptions::default());
        };
        if map_set.bank != system.sync_helper().last_bank() {
            let x = system.sync_helper().sync(1 | 4 | 8 | 16 | 64 | 128, map_set.bank);
            if x.is_err() {
                let bank = map_set.bank;
                error!("COULD NOT SYNC TO BANK {bank} THIS IS A BUG BTW",);
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
    pub fn execute_interact_fn(
        &mut self,
        interact: &InteractFn,
        system: &mut impl ConsoleApi,
    ) -> Option<&'static str> {
        match interact {
            InteractFn::ToggleDog => {
                self.companion_trail.fill(self.player.pos, self.player.dir);
                if self.companion_list.has(Companion::Dog) {
                    self.companion_list.remove(Companion::Dog);
                    system.play_sound(sound::ALERT_DOWN);
                    Some(DOG_RELINQUISHED)
                } else {
                    self.companion_list.add(Companion::Dog);
                    system.play_sound(sound::EQUIP_OBTAINED);
                    Some(DOG_OBTAINED)
                }
            }
            InteractFn::StairwellWindow => {
                system.memory().set(save::HOUSE_STAIRWELL_WINDOW_INTERACTED);
                Some(HOUSE_STAIRWELL_WINDOW)
            }
            InteractFn::StairwellPainting => {
                if system.memory().is(save::HOUSE_STAIRWELL_WINDOW_INTERACTED) {
                    Some(HOUSE_STAIRWELL_PAINTING_AFTER)
                } else {
                    Some(HOUSE_STAIRWELL_PAINTING_INIT)
                }
            }
            InteractFn::Note(note) => {
                system.play_sound(sound::PIANO.with_note(*note));
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
                system.play_sound(sound::PIANO.with_note(note as i32));
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

    fn play_cutscene(&mut self, system: &mut impl ConsoleApi) -> bool {
        if self.cutscene.is_some() {
            let mut intermediate = self
                .cutscene
                .clone()
                .unwrap_or_else(|| std::process::abort());
            match intermediate.next_stage(&self) {
                cutscene::CutsceneState::Playing => {
                    intermediate.advance(system, self);
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

    fn save(&self, new_map: &MapIndex, system: &mut impl ConsoleApi) {
        system.memory().set_byte(save::CURRENT_MAP, new_map.0 as u8);
        let x = self.player.pos.x.to_le_bytes();
        system.memory().set_byte(save::PLAYER_X[0], x[0]);
        system.memory().set_byte(save::PLAYER_X[1], x[1]);
        let y = self.player.pos.y.to_le_bytes();
        system.memory().set_byte(save::PLAYER_Y[0], y[0]);
        system.memory().set_byte(save::PLAYER_Y[1], y[1]);
    }

    pub fn load_pmem(&mut self, system: &mut impl ConsoleApi) {
        let current_map = system.memory().get_byte(save::CURRENT_MAP);
        self.load_map(system, MapIndex(current_map.into()).map());
        self.player.pos.x = i16::from_le_bytes([
            system.memory().get_byte(save::PLAYER_X[0]),
            system.memory().get_byte(save::PLAYER_X[1]),
        ]);
        self.player.pos.y = i16::from_le_bytes([
            system.memory().get_byte(save::PLAYER_Y[0]),
            system.memory().get_byte(save::PLAYER_Y[1]),
        ]);
    }
}

impl<'a, T: ConsoleApi>
    Game<
        (
            &mut T,
            &mut InventoryUi,
        ),
        (
            &mut T,
            &DebugInfo,
        ),
    > for WalkaroundState<'a>
{
    fn step(
        &mut self,
        (system, inventory_ui): (
            &mut T,
            &mut InventoryUi,
        ),
    ) -> Option<GameState> {
        self.map_animations
            .iter_mut()
            .for_each(|anim| anim.advance());

        self.particles.step();
        self.creatures.iter_mut().for_each(|x| x.step());

        if self.play_cutscene(system) {
            return None;
        }

        if system.keyp(28, -1, -1) {
            self.load_map(system, SUPERMARKET);
        }
        if system.keyp(29, -1, -1) {
            self.load_map(system, WILDERNESS);
        }
        if system.keyp(30, -1, -1) {
            self.load_map(system, TEST_PEN);
        }
        if system.keyp(31, -1, -1) {
            self.load_map(system, BEDROOM);
        }
        if system.keyp(32, -1, -1) {
            self.load_pmem(system);
        }
        if system.keyp(33, -1, -1) {
            system.set_palette(tic80_api::helpers::SWEETIE_16);
        }
        if system.keyp(34, -1, -1) {
            system.set_palette(tic80_api::helpers::NIGHT_16);
        }
        if system.keyp(35, -1, -1) {
            system.set_palette(tic80_api::helpers::B_W);
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;
        if matches!(self.dialogue.current_text, None) && self.dialogue.next_text.is_empty() {
            if system.mem_btn(0) {
                dy -= 1;
            }
            if system.mem_btn(1) {
                dy += 1;
            }
            if system.mem_btn(2) {
                dx -= 1;
            }
            if system.mem_btn(3) {
                dx += 1;
            }
            if system.mem_btnp(5) {
                inventory_ui.open(system);
                return Some(GameState::Inventory);
            }
        } else {
            if self.dialogue.characters == 0 {
                system.play_sound(sound::INTERACT);
            }
            self.dialogue.tick(system, 1);
            if system.mem_btn(4) {
                self.dialogue.tick(system, 2);
            }
            if system.mem_btnp(5) {
                self.dialogue.skip(system);
            }
        }
        if system.mem_btnp(4) && self.dialogue.is_line_done() {
            interact = true;
            if self.dialogue.next_text(system, false) {
                interact = false;
            } else if matches!(self.dialogue.current_text, Some(_)) {
                interact = false;
                self.dialogue.close();
            }
            info!("Attempting interact...");
        }
        if system.mem_btnp(6) {
            return Some(GameState::MainMenu(super::menu::MenuState::debug_options()));
        }
        if system.any_btnpr() {
            self.player.flip_controls = Axis::None
        }
        let noclip = if system.key(63) && system.key(64) {
            dy *= 3;
            dx *= 4;
            true
        } else {
            false
        };

        let (dx, dy) = self
            .player
            .walk(dx, dy, noclip, &self.current_map, system.get_sprite_flags());
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
                        system.play_sound(sound::DOOR);
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
                self.save(&new_map, system);
                self.load_map(system, new_map.map());
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
                            self.dialogue.add_text(system, x);
                        }
                        Interaction::Dialogue(x) => {
                            self.dialogue.set_dialogue(system, x);
                        }
                        Interaction::EnumText(x) => {
                            self.dialogue.set_enum_text(system, x);
                        }
                        Interaction::Func(x) => {
                            if let Some(dialogue) = self.execute_interact_fn(x, system) {
                                self.dialogue.add_text(system, dialogue);
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
    fn draw(&self, (system, debug_info): (&mut T, &DebugInfo)) {
        // Draw BG
        system.palette_map_reset();
        system.cls(self.bg_colour);
        self.current_map.draw_bg(system, self.camera.pos, false);

        self.particles.draw_tic80(system, -self.cam_x(), -self.cam_y());
        system.blit_segment(4);
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
        sprites.sort_by(|a, b| {
            a.bottom()
                .partial_cmp(&b.bottom())
                .unwrap_or_else(|| std::process::abort())
        });

        // Draw sprites
        for options in sprites {
            options.draw(system);
        }

        // Draw FG
        system.palette_map_reset();
        self.current_map.draw_fg(system, self.camera.pos, false);

        if let Some(string) = &self.dialogue.current_text {
            self.dialogue.draw_dialogue_box(system, string, true);
        }
        if debug_info.map_info() {
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
        if debug_info.player_info() {
            system.print_raw(
                &format!("Player: {:#?}\0", self.player),
                0,
                0,
                PrintOptions {
                    small_text: true,
                    color: 11,
                    ..Default::default()
                },
            );
            system.print_raw(
                &format!("Camera: {:#?}\0", self.camera),
                74,
                0,
                PrintOptions {
                    small_text: true,
                    color: 11,
                    ..Default::default()
                },
            );
        }
    }
}
