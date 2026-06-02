use crate::animation::Animation;
use crate::data::map_data::{DEFAULT_MAP_SET, MapIndex};
use crate::data::{dialogue_data::*, sound};
use crate::debug::DebugInfo;
use crate::gamestate::Game;
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, LayerInfo, MapInfo};
use crate::particles::{Particle, ParticleDraw, ParticleList};
use crate::player::{Companion, CompanionList, CompanionTrail, MoveMode, Shell};
use crate::position::{Collider, Vec2};
use crate::system::PrintOptions;
use crate::system::{ConsoleApi, ConsoleHelper, DrawParams, ScanCode, just_pressed, pressed};
use crate::{camera::Camera, dialogue::Dialogue, gamestate::GameMode};
use log::info;

use self::creatures::Creature;
use self::cutscene::Cutscene;

use super::debug::MapViewer;
use super::inventory::InventoryUi;

mod creatures;
mod cutscene;

#[derive(Clone, Debug)]
pub struct WalkaroundState {
    pub entities: Vec<Shell>,
    pub companion_trail: CompanionTrail<16>,
    pub companion_list: CompanionList,
    pub map_animations: Vec<Animation>,
    pub creatures: Vec<Creature>,
    pub camera: Camera,
    pub current_map: MapInfo,
    pub map_viewer: MapViewer,
    pub dialogue: Dialogue,
    pub particles: ParticleList,
    pub cutscene: Option<Cutscene>,
    pub bg_colour: u8,
    pub default_map_colliders: Vec<Collider>,
}
impl Default for WalkaroundState {
    fn default() -> Self {
        Self::new()
    }
}

impl WalkaroundState {
    pub fn new() -> Self {
        Self {
            entities: vec![Shell {
                move_mode: MoveMode::Player,
                ..Default::default()
            }],
            companion_trail: CompanionTrail::new(),
            companion_list: CompanionList::new(),
            map_animations: Vec::new(),
            creatures: Vec::new(),
            camera: Camera::default(),
            current_map: DEFAULT_MAP_SET.into(),
            map_viewer: MapViewer::default(),
            dialogue: Dialogue::default(),
            particles: ParticleList::new(),
            cutscene: None,
            bg_colour: 0,
            default_map_colliders: Vec::new(),
        }
    }

    /// Access the player entity
    /// Mostly so we don't rely implicitly on "player index is 0" since it'll probably change later
    pub fn player(&mut self) -> &mut Shell {
        &mut self.entities[0]
    }

    /// Access the player entity immutably
    pub fn player_ref(&self) -> &Shell {
        &self.entities[0]
    }

    /// Loads a map from given data
    pub fn load_map(&mut self, system: &mut impl ConsoleApi, map_set: impl Into<MapInfo>) {
        let map_set = map_set.into();
        let map1 = &map_set
            .layers
            .first()
            .expect("Tried to load an empty map...");
        if let Some(bounds) = &map_set.camera_bounds {
            self.camera.bounds = bounds.clone();
        } else {
            self.camera = Camera::from_map_size(map1.size, map1.offset);
        }
        self.bg_colour = map_set.bg_colour;
        system.music(map_set.music_track.as_ref());
        if map_set.bank != system.bank().clone().into() {
            *system.bank() = map_set.bank.try_into().unwrap();
        }

        self.map_animations = map_set
            .interactables
            .iter()
            .flat_map(|x| x.clone().sprite)
            .map(|frames| Animation {
                frames,
                ..Animation::default()
            })
            .collect();

        self.current_map = map_set;

        self.map_animations.shrink_to_fit();

        self.creatures.clear();
        self.particles.clear();
    }
    /// Load a map from a tic80 bank. Legacy code.
    pub fn load_map_bank(&mut self, system: &mut impl ConsoleApi, bank: usize) {
        let mut game_map = system.maps()[bank].clone();
        for layer in game_map.layers.iter() {
            info!("{}", layer.name);
        }
        let mut collision_layer = game_map
            .layers
            .first()
            .map(|layer| {
                info!("collision layer: {}", layer.name);
                LayerInfo {
                    origin: Vec2::new(0, 0),
                    size: Vec2::new(
                        layer.width().try_into().unwrap(),
                        layer.height().try_into().unwrap(),
                    ),
                    offset: Vec2::new(0, 0),
                    source_layer: 0,
                    transparent: Some(0),
                    visible: false,
                    ..LayerInfo::DEFAULT_LAYER
                }
            })
            .unwrap();
        game_map.layers.remove(0);
        let mut colliders = Vec::new();
        for j in 0..collision_layer.size.y {
            for i in 0..collision_layer.size.x {
                let tile = system.map_get(bank, 0, i.into(), j.into());
                colliders.push(Collider::from_sprite(system, tile));
            }
        }
        for layer in game_map.layers.iter() {
            info!("{}", layer.name);
        }
        let fg: Vec<LayerInfo> = game_map
            .layers
            .iter_mut()
            .enumerate()
            .filter(|(_, layer)| {
                let condition = layer.name.to_lowercase().starts_with("fg");
                info!("{} starts with \"FG\"? {}", layer.name, condition);
                condition
            })
            .map(|(i, layer)| LayerInfo {
                origin: Vec2::new(0, 0),
                size: Vec2::new(
                    layer.width().try_into().unwrap(),
                    layer.height().try_into().unwrap(),
                ),
                offset: Vec2::new(0, 0),
                source_layer: i + 1,
                transparent: Some(0),
                ..LayerInfo::DEFAULT_LAYER
            })
            .collect();
        collision_layer.colliders = colliders;
        let layers: Vec<LayerInfo> = [collision_layer]
            .into_iter()
            .chain(
                game_map
                    .layers
                    .into_iter()
                    .enumerate()
                    .filter(|(_, layer)| !layer.name.to_lowercase().starts_with("fg"))
                    .map(|(i, layer)| LayerInfo {
                        origin: Vec2::new(0, 0),
                        size: Vec2::new(
                            layer.width().try_into().unwrap(),
                            layer.height().try_into().unwrap(),
                        ),
                        offset: Vec2::new(0, 0),
                        source_layer: i + 1,
                        transparent: Some(0),
                        ..LayerInfo::DEFAULT_LAYER
                    }),
            )
            .collect();
        let map_info = MapInfo {
            layers,
            fg_layers: fg,
            bank,
            ..Default::default()
        };

        self.load_map(system, map_info);
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
    /// Function that does everything. No anti-pattern here.
    pub fn execute_interact_fn(
        &mut self,
        interact: &InteractFn,
        system: &mut impl ConsoleApi,
    ) -> Option<&'static str> {
        match interact {
            InteractFn::ToggleDog => {
                self.companion_trail
                    .fill(self.player_ref().pos, self.player_ref().dir);
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
                system.memory().house_stairwell_window_interacted = true;
                Some(HOUSE_STAIRWELL_WINDOW)
            }
            InteractFn::StairwellPainting => {
                if system.memory().house_stairwell_window_interacted {
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
                let mut note = (self.player().pos.x + 4 - origin.x) / 8;
                let x = origin.x + note * 8;
                let y = if self.player().pos.y - origin.y < 2 {
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
                let pos = self.player_ref().pos;
                self.creatures
                    .extend((0..=*x).map(|_| Creature::default().with_offset(pos)));
                None
            }
            InteractFn::Pet(vec, flip) => {
                self.cutscene = Some(Cutscene::pet_dog(*vec, self.player().pos, *flip));
                None
            }
        }
    }

    /// Plays a cued cutscene until finished, then removes it from the cue.
    fn play_cutscene(&mut self, system: &mut impl ConsoleApi) -> bool {
        if self.cutscene.is_some() {
            let mut intermediate = self
                .cutscene
                .clone()
                .unwrap_or_else(|| std::process::abort());
            match intermediate.next_stage(self) {
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

    /// Adds a shell and returns its index
    pub fn spawn_shell(&mut self, shell: Shell) -> usize {
        self.entities.push(shell);
        self.entities.len() - 1
    }

    fn save(&self, new_map: &MapIndex, system: &mut impl ConsoleApi) {
        let pos = self.player_ref().pos;
        let save = system.memory();
        save.save_count += 1;
        save.current_map = new_map.0 as u8;
        save.player_x = pos.x;
        save.player_y = pos.y;
    }

    pub fn load_pmem(&mut self, system: &mut impl ConsoleApi) {
        let save = *system.memory();
        self.load_map(system, MapIndex(save.current_map.into()).map());
        self.player().pos.x = save.player_x;
        self.player().pos.y = save.player_y;
    }

    /// Starts a fresh game and saves over the default zeroed
    /// player position and map_index.
    pub fn new_game(&mut self, system: &mut impl ConsoleApi) {
        // Rebuild the live walkaround to its fresh construction state. "Erase
        // data" only zeroes `SaveData`; the existing `WalkaroundState` (player
        // entity, companions, dialogue…) is never rebuilt, so without this the
        // player keeps the position/shell they had before the reset and the
        // seed `save()` below would persist that stale position.
        *self = Self::new();
        self.load_map(system, MapIndex::BEDROOM.map());
        self.save(&MapIndex::BEDROOM, system);
    }
}

impl<T: ConsoleApi>
    Game<
        (&mut crate::drawstate::DrawState, &mut T, &mut InventoryUi),
        (&mut crate::drawstate::DrawState, &mut T, &DebugInfo),
    > for WalkaroundState
{
    fn step(
        &mut self,
        (draw_state, system, inventory_ui): (
            &mut crate::drawstate::DrawState,
            &mut T,
            &mut InventoryUi,
        ),
    ) -> Option<GameMode> {
        self.map_animations
            .iter_mut()
            .for_each(|anim| anim.advance());

        self.particles.step();
        self.creatures.iter_mut().for_each(|x| x.step(system));

        if self.play_cutscene(system) {
            return None;
        }

        if system.keyp(ScanCode::Digit5) {
            self.load_pmem(system);
        }
        if system.keyp(ScanCode::Digit6) {
            draw_state.set_palette(&crate::system::SWEETIE_16);
        }
        if system.keyp(ScanCode::Digit7) {
            draw_state.set_palette(&crate::system::NIGHT_16);
        }
        if system.keyp(ScanCode::Digit8) {
            draw_state.set_palette(&crate::system::B_W);
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;

        let pad = system.controller();
        if self.map_viewer.focused {
            self.map_viewer
                .step_map_viewer(system, &mut self.current_map);
        } else if self.dialogue.current_text.is_none() && self.dialogue.next_text.is_empty() {
            if pressed(pad.up) {
                dy -= 1;
            }
            if pressed(pad.down) {
                dy += 1;
            }
            if pressed(pad.left) {
                dx -= 1;
            }
            if pressed(pad.right) {
                dx += 1;
            }
            if just_pressed(pad.b) {
                inventory_ui.open(system);
                return Some(GameMode::Inventory);
            }
        } else {
            if self.dialogue.characters == 0 {
                system.play_sound(sound::INTERACT);
            }
            self.dialogue.tick(system, 1);
            if pressed(pad.a) {
                self.dialogue.tick(system, 2);
            }
            if just_pressed(pad.b) {
                self.dialogue.skip(system);
            }
        }
        if just_pressed(pad.a) && self.dialogue.is_line_done() {
            interact = true;
            if self.dialogue.next_text(system, false) {
                interact = false;
            } else if self.dialogue.current_text.is_some() {
                interact = false;
                self.dialogue.close();
            }
            info!("Attempting interact...");
        }
        if just_pressed(pad.x) {
            return Some(GameMode::MainMenu(super::menu::MenuState::debug_options()));
        }
        if system.any_btnpr() {
            self.player().flip_controls = Axis::None
        }
        let noclip = if system.key(ScanCode::Ctrl) && system.key(ScanCode::Shift) {
            dy *= 3;
            dx *= 4;
            true
        } else {
            false
        };

        for shell in self.entities.iter_mut() {
            match shell.move_mode {
                MoveMode::Player => {
                    let (dx, dy) = shell.walk(system, dx, dy, noclip, &self.current_map);
                    shell.apply_motion(dx, dy, Some(&mut self.companion_trail));
                }
                MoveMode::Wander => {
                    let (dx, dy) = if system.rng().rand_u8() < 25 {
                        (
                            (system.rng().rand_u8() % 3) as i16 - 1,
                            (system.rng().rand_u8() % 3) as i16 - 1,
                        )
                    } else {
                        (shell.dir.0.into(), shell.dir.1.into())
                    };
                    let (dx, dy) = shell.walk(system, dx, dy, false, &self.current_map);
                    shell.apply_motion::<8>(dx, dy, None);
                }
            }
        }

        {};

        // Set after player.dir has updated
        let interact_hitbox = self
            .player()
            .hitbox()
            .offset_xy(self.player().dir.0.into(), self.player().dir.1.into());

        let mut warp_target = None;
        for warp in self.current_map.warps.iter() {
            if self.player_ref().hitbox().touches(warp.hitbox())
                || (interact && interact_hitbox.touches(warp.hitbox()))
            {
                if let Some(sound) = &warp.sound {
                    system.play_sound(sound.clone());
                }
                warp_target = Some(warp.clone());
                break;
            }
        }
        if let Some(target) = warp_target {
            self.player().pos = target.target();
            self.player().flip_controls = target.flip;
            self.companion_trail
                .fill(self.player_ref().pos, self.player_ref().dir);
            if let Some(new_map) = target.map {
                self.save(&new_map, system);
                self.load_map(system, new_map.map());
            }
        } else if interact {
            for item in self.current_map.interactables.iter().cloned().chain(
                self.companion_list
                    .interact(&self.companion_trail)
                    .iter()
                    .cloned(),
            ) {
                if interact_hitbox.touches(item.hitbox) {
                    match &item.interaction {
                        Interaction::Text(x) => {
                            self.dialogue.add_text(system, x.clone());
                        }
                        Interaction::Dialogue(x) => {
                            self.dialogue.set_dialogue(system, x);
                        }
                        Interaction::Conversation(x) => {
                            self.dialogue.set_messages(system, x);
                        }
                        Interaction::Func(x) => {
                            if let Some(dialogue) = self.execute_interact_fn(x, system) {
                                let dialogue = dialogue.to_string();
                                self.dialogue.add_text(system, dialogue);
                            };
                        }
                        _x => {}
                    }
                    break;
                }
            }
        }

        self.camera
            .center_on(self.player_ref().pos.x + 4, self.player_ref().pos.y + 8);
        None
    }
    fn draw(
        &self,
        (draw_state, system, debug_info): (&mut crate::drawstate::DrawState, &mut T, &DebugInfo),
    ) {
        use crate::drawstate::LayerId::*;
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::image::RgbaImage;

        let bg = BG as usize;
        let bg_colour = draw_state.colour(self.bg_colour);
        draw_state.rgba(BG).fill(bg_colour);

        // BG map layers
        if let Some(map) = system.maps().get(self.current_map.bank) {
            self.current_map
                .draw_bg_indexed(draw_state, BG, map, self.camera.pos, false);
        }

        // Particles
        self.particles
            .draw_indexed(draw_state, BG, -self.cam_x(), -self.cam_y());

        // Collect sprites for drawing
        let mut sprites: Vec<DrawParams> = Vec::new();

        sprites.push(self.player_ref().draw_params(self.camera.pos));

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
                .map(|x| x.draw_params(self.camera.pos).into()),
        );

        sprites.extend(self.entities.iter().map(|x| x.draw_params(self.camera.pos)));

        for (i, companion) in self.companion_list.companions.iter().enumerate() {
            if let Some(companion) = companion {
                let (position, direction) = if i == 0 {
                    self.companion_trail.oldest()
                } else {
                    self.companion_trail.mid()
                };
                let walktime = self.companion_trail.walktime();
                let params = companion
                    .spr_params(position, direction, walktime, &self.camera)
                    .into();
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
            options.draw_to(draw_state, BG);
        }

        // FG map layers (drawn on top of sprites)
        if let Some(map) = system.maps().get(self.current_map.bank) {
            self.current_map
                .draw_fg_indexed(draw_state, BG, map, self.camera.pos, false);
        }

        if let Some(string) = self.dialogue.current_text.clone() {
            self.dialogue
                .draw_dialogue_box(draw_state, BG, system, &string, true);
        }
        if debug_info.map_info() {
            for warp in self.current_map.warps.iter() {
                warp.hitbox()
                    .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
                    .draw(draw_state, BG, 12);
            }
            self.player_ref()
                .hitbox()
                .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
                .draw(draw_state, BG, 12);
            for item in self.current_map.interactables.iter() {
                item.hitbox
                    .offset_xy(-self.camera.pos.x, -self.camera.pos.y)
                    .draw(draw_state, BG, 14);
            }
        }
        if debug_info.player_info() {
            let c11 = draw_state.colour(11);
            let opts = PrintOptions {
                small_text: true,
                color: 11,
                ..Default::default()
            };
            system.print_to(
                draw_state.rgba(BG),
                &format!("Player: {:#?}\0", self.player_ref()),
                0,
                0,
                c11,
                opts.clone(),
            );
            system.print_to(
                draw_state.rgba(BG),
                &format!("Camera: {:#?}\0", self.camera),
                74,
                0,
                c11,
                opts,
            );
        }
        self.map_viewer.draw_map_viewer(draw_state, system, self);

        // Composite all migrated draw output to output_image (once, at the
        // end of the draw fn).
        let output = system.output_image();
        output.blit::<RgbaImage>(
            0,
            0,
            &draw_state.rgba_canvas[bg],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
    }
}
