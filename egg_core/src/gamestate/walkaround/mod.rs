use crate::Ctx;
use crate::animation::Animation;
use crate::data::map_data::{MapIndex, legacy_index};
use crate::data::sound;
use crate::debug::DebugInfo;
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, MapInfo, MapStore, map_by_name};
use crate::particles::{Particle, ParticleDraw, ParticleList};
use crate::player::{Companion, CompanionList, CompanionTrail, MoveMode, Shell};
use crate::position::{Collider, Vec2};
use crate::system::PrintOptions;
use crate::system::drawing::image::IndexedImage;
use crate::system::{ConsoleApi, ConsoleHelper, DrawParams, ScanCode, dpad_delta, just_pressed, pressed};
use crate::{camera::Camera, dialogue::Dialogue, gamestate::GameMode};
use log::info;

use self::creatures::Creature;
use self::cutscene::Cutscene;

use super::mapeditor::MapViewer;
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
            current_map: MapInfo::default(),
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
            self.camera = Camera::from_map_size(
                map1.size,
                map1.offset,
                system.width() as i16,
                system.height() as i16,
            );
        }
        self.bg_colour = map_set.bg_colour;
        system.music(map_set.music_track.as_ref());

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
    /// Load a map by name through [`map_by_name`] (legacy table, numeric
    /// fallback, then modern maps from `maps`). Unknown names log and leave
    /// the current map in place — the old bank-indexed loader panicked on a
    /// bad index, which a typo'd warp or stale save shouldn't do.
    /// `indexed_sprites` is the sheet `map_by_name` derives modern colliders
    /// from (it lives on `DrawState`); `system` covers the camera/audio setup.
    pub fn load_map_by_name(
        &mut self,
        system: &mut impl ConsoleApi,
        indexed_sprites: &IndexedImage,
        maps: &MapStore,
        name: &str,
    ) {
        let Some(map_info) = map_by_name(indexed_sprites, name, maps) else {
            info!("load_map_by_name: unknown map {name:?}");
            return;
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
    /// Function that does everything. No anti-pattern here. Returns an optional
    /// dialogue-registry key for the caller to resolve and display.
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
                    Some("dog_relinquished")
                } else {
                    self.companion_list.add(Companion::Dog);
                    system.play_sound(sound::EQUIP_OBTAINED);
                    Some("dog_obtained")
                }
            }
            InteractFn::StairwellWindow => {
                system.memory().house_stairwell_window_interacted = true;
                Some("house_stairwell_window")
            }
            InteractFn::StairwellPainting => {
                if system.memory().house_stairwell_window_interacted {
                    Some("house_stairwell_painting_after")
                } else {
                    Some("house_stairwell_painting_init")
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
        // Taken out of `self` while it runs so it can borrow the walkaround
        // mutably; put back only while it's still playing.
        if let Some(mut cutscene) = self.cutscene.take() {
            match cutscene.next_stage(self) {
                cutscene::CutsceneState::Playing => {
                    cutscene.advance(system, self);
                    self.cutscene = Some(cutscene);
                    true
                }
                cutscene::CutsceneState::Finished => false,
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

    fn save(&self, new_map: &str, system: &mut impl ConsoleApi) {
        let pos = self.player_ref().pos;
        let save = system.memory();
        save.save_count += 1;
        save.current_map_name = Some(new_map.to_string());
        // Legacy maps also refresh the numeric id, so a save written here
        // still loads in old binaries. Modern (named-only) maps leave it
        // untouched — there's no number that means them.
        if let Some(index) = legacy_index(new_map) {
            save.current_map = index.0 as u8;
        }
        save.player_x = pos.x;
        save.player_y = pos.y;
    }

    pub fn load_pmem(
        &mut self,
        system: &mut impl ConsoleApi,
        indexed_sprites: &IndexedImage,
        maps: &MapStore,
    ) {
        let save = system.memory().clone();
        // Pre-rename saves only carry the numeric id; translate it to a name
        // and resolve everything through the one name-based loader.
        let name = save
            .current_map_name
            .unwrap_or_else(|| MapIndex(save.current_map.into()).name().to_string());
        self.load_map_by_name(system, indexed_sprites, maps, &name);
        self.player().pos.x = save.player_x;
        self.player().pos.y = save.player_y;
    }

    /// Starts a fresh game and saves over the default zeroed
    /// player position and map name.
    pub fn new_game(
        &mut self,
        system: &mut impl ConsoleApi,
        indexed_sprites: &IndexedImage,
        maps: &MapStore,
    ) {
        // Rebuild the live walkaround to its fresh construction state. "Erase
        // data" only zeroes `SaveData`; the existing `WalkaroundState` (player
        // entity, companions, dialogue…) is never rebuilt, so without this the
        // player keeps the position/shell they had before the reset and the
        // seed `save()` below would persist that stale position.
        *self = Self::new();
        self.load_map_by_name(system, indexed_sprites, maps, "bedroom");
        self.save("bedroom", system);
    }
}

impl WalkaroundState {
    pub fn step<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        inventory_ui: &mut InventoryUi,
    ) -> Option<GameMode> {
        self.map_animations
            .iter_mut()
            .for_each(|anim| anim.advance());

        self.particles.step();
        self.creatures.iter_mut().for_each(|x| x.step(ctx.rng));

        if self.play_cutscene(ctx.system) {
            return None;
        }

        // When the map editor is open it takes over all input and freezes the
        // sim, so painting/typing can't move the player or trip warps/reloads.
        if self.map_viewer.focused {
            self.map_viewer
                .step_map_viewer(ctx.system, &mut self.current_map, ctx.maps, self.camera.pos);
            return None;
        }

        if ctx.system.keyp(ScanCode::Digit5) {
            self.load_pmem(ctx.system, &ctx.draw.indexed_sprites, ctx.maps);
        }
        if ctx.system.keyp(ScanCode::Digit6) {
            ctx.draw.set_palette(&crate::system::SWEETIE_16);
        }
        if ctx.system.keyp(ScanCode::Digit7) {
            ctx.draw.set_palette(&crate::system::NIGHT_16);
        }
        if ctx.system.keyp(ScanCode::Digit8) {
            ctx.draw.set_palette(&crate::system::B_W);
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;

        let pad = ctx.system.controller();
        if self.dialogue.current_text.is_none() && self.dialogue.next_text.is_empty() {
            (dx, dy) = dpad_delta(&pad, pressed);
            if just_pressed(pad.b) {
                inventory_ui.open(ctx.system);
                return Some(GameMode::Inventory);
            }
        } else {
            if self.dialogue.characters == 0 {
                ctx.system.play_sound(sound::INTERACT);
            }
            self.dialogue.tick(ctx.system, 1);
            if pressed(pad.a) {
                self.dialogue.tick(ctx.system, 2);
            }
            if just_pressed(pad.b) {
                self.dialogue.skip(ctx.system);
            }
        }
        if just_pressed(pad.a) && self.dialogue.is_line_done() {
            interact = true;
            if self.dialogue.next_text(ctx.system, false) {
                interact = false;
            } else if self.dialogue.current_text.is_some() {
                interact = false;
                self.dialogue.close();
            }
            info!("Attempting interact...");
        }
        if just_pressed(pad.x) {
            return Some(GameMode::MainMenu(super::menu::MenuState::debug_options(ctx.system)));
        }
        if ctx.system.any_btnpr() {
            self.player().flip_controls = Axis::None
        }
        let noclip = if ctx.system.key(ScanCode::Ctrl) && ctx.system.key(ScanCode::Shift) {
            dy *= 3;
            dx *= 4;
            true
        } else {
            false
        };

        let tiles = ctx.maps.get(&self.current_map.source);
        for shell in self.entities.iter_mut() {
            match shell.move_mode {
                MoveMode::Player => {
                    let (dx, dy) = shell.walk(
                        ctx.system,
                        &ctx.draw.sprite_flags,
                        dx,
                        dy,
                        noclip,
                        &self.current_map,
                        tiles,
                    );
                    shell.apply_motion(dx, dy, Some(&mut self.companion_trail));
                }
                MoveMode::Wander => {
                    let (dx, dy) = if ctx.rng.rand_u8() < 25 {
                        (
                            (ctx.rng.rand_u8() % 3) as i16 - 1,
                            (ctx.rng.rand_u8() % 3) as i16 - 1,
                        )
                    } else {
                        (shell.dir.0.into(), shell.dir.1.into())
                    };
                    let (dx, dy) = shell.walk(
                        ctx.system,
                        &ctx.draw.sprite_flags,
                        dx,
                        dy,
                        false,
                        &self.current_map,
                        tiles,
                    );
                    shell.apply_motion::<8>(dx, dy, None);
                }
            }
        }

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
                    ctx.system.play_sound(sound.clone());
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
                self.save(&new_map, ctx.system);
                self.load_map_by_name(ctx.system, &ctx.draw.indexed_sprites, ctx.maps, &new_map);
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
                        Interaction::Dialogue(key) => {
                            let convo = ctx.system.get_dialogue(key);
                            self.dialogue.set_messages(ctx.system, &convo);
                        }
                        Interaction::Func(x) => {
                            if let Some(key) = self.execute_interact_fn(x, ctx.system) {
                                let convo = ctx.system.get_dialogue(key);
                                self.dialogue.set_messages(ctx.system, &convo);
                            };
                        }
                        Interaction::None => {}
                    }
                    break;
                }
            }
        }

        self.camera.center_on(
            self.player_ref().pos.x + 4,
            self.player_ref().pos.y + 8,
            ctx.system.width() as i16,
            ctx.system.height() as i16,
        );
        None
    }
    pub fn draw<S: ConsoleApi>(&self, ctx: &mut Ctx<S>, debug_info: &DebugInfo) {
        // Draw the live world from the player-following camera, with this
        // walkaround's own map editor overlay, then composite into the console's
        // canonical output surface. The world build leaves its result in
        // `ctx.draw`, so the final composite is a separate step that takes the
        // output (avoiding a borrow conflict with the console).
        self.draw_world(ctx, self.camera.pos, &self.map_viewer, debug_info);
        WalkaroundState::composite_into(ctx.draw, ctx.system.output_image());
    }

    /// Render the walkaround world from an arbitrary `camera_pos` into
    /// `ctx.draw`, using `editor` for the map-editor overlay (so an extra view
    /// can drive its own free camera + editor without touching the live
    /// `self.camera`/`self.map_viewer`). Tile data comes from `ctx.maps`; the
    /// shared console is read for assets only. The finished frame is left in
    /// `ctx.draw.rgba(BG)` — call [`composite_into`](Self::composite_into)
    /// to blit it onto a surface.
    ///
    /// Engine-agnostic: it only touches `ctx.draw` (the layer canvases) and
    /// reads `ctx.system` for assets, with no knowledge of windows or the host.
    pub fn draw_world<S: ConsoleApi>(
        &self,
        ctx: &mut Ctx<S>,
        camera_pos: Vec2,
        editor: &MapViewer,
        debug_info: &DebugInfo,
    ) {
        use crate::drawstate::LayerId::*;

        let cam_x = i32::from(camera_pos.x);
        let cam_y = i32::from(camera_pos.y);

        let bg_colour = ctx.draw.colour(self.bg_colour);
        ctx.draw.rgba(BG).fill(bg_colour);

        // BG map layers
        if let Some(map) = ctx.maps.get(&self.current_map.source) {
            self.current_map
                .draw_bg_indexed(ctx.draw, BG, map, camera_pos, false);
        }

        // Particles
        self.particles.draw_indexed(ctx.draw, BG, -cam_x, -cam_y);

        // Collect sprites for drawing
        let mut sprites: Vec<DrawParams> = Vec::new();

        sprites.push(self.player_ref().draw_params(camera_pos));

        for (anim, hitbox) in self.map_animations.iter().zip(
            self.current_map
                .interactables
                .iter()
                .filter(|x| x.sprite.is_some())
                .map(|x| x.hitbox),
        ) {
            sprites.push(DrawParams::new(
                anim.current_frame().spr_id.into(),
                anim.current_frame().pos.x as i32 + hitbox.x as i32 - cam_x,
                anim.current_frame().pos.y as i32 + hitbox.y as i32 - cam_y,
                anim.current_frame().options.clone(),
                anim.current_frame().outline_colour,
                anim.current_frame().palette_rotate,
            ));
        }

        sprites.extend(self.creatures.iter().map(|x| x.draw_params(camera_pos)));

        sprites.extend(self.entities.iter().map(|x| x.draw_params(camera_pos)));

        for (i, companion) in self.companion_list.companions.iter().enumerate() {
            if let Some(companion) = companion {
                let (position, direction) = if i == 0 {
                    self.companion_trail.oldest()
                } else {
                    self.companion_trail.mid()
                };
                let walktime = self.companion_trail.walktime();
                // The companion sprite helper bounds against a camera; build a
                // throwaway camera at `camera_pos` so an extra view's free
                // camera offsets them correctly too.
                let cam = Camera::new(camera_pos, self.camera.bounds.clone());
                let params = companion.spr_params(position, direction, walktime, &cam);
                sprites.push(params);
            }
        }

        // Sort sprites in order of Y index
        sprites.sort_by_key(|sprite| sprite.bottom());

        // Draw sprites
        for options in sprites {
            options.draw_to(ctx.draw, BG);
        }

        // FG map layers (drawn on top of sprites)
        if let Some(map) = ctx.maps.get(&self.current_map.source) {
            self.current_map
                .draw_fg_indexed(ctx.draw, BG, map, camera_pos, false);
        }

        if let Some(string) = self.dialogue.current_text.clone() {
            self.dialogue
                .draw_dialogue_box(ctx.draw, BG, ctx.system, &string, true);
        }
        if debug_info.map_info {
            for warp in self.current_map.warps.iter() {
                warp.hitbox()
                    .offset_xy(-camera_pos.x, -camera_pos.y)
                    .draw(ctx.draw, BG, 12);
            }
            self.player_ref()
                .hitbox()
                .offset_xy(-camera_pos.x, -camera_pos.y)
                .draw(ctx.draw, BG, 12);
            for item in self.current_map.interactables.iter() {
                item.hitbox
                    .offset_xy(-camera_pos.x, -camera_pos.y)
                    .draw(ctx.draw, BG, 14);
            }
        }
        if debug_info.player_info {
            let c11 = ctx.draw.colour(11);
            let opts = PrintOptions {
                small_text: true,
                color: 11,
                ..Default::default()
            };
            ctx.system.print_to(
                ctx.draw.rgba(BG),
                &format!("Player: {:#?}", self.player_ref()),
                0,
                0,
                c11,
                opts.clone(),
            );
            ctx.system.print_to(
                ctx.draw.rgba(BG),
                &format!("Camera: {camera_pos:#?}"),
                74,
                0,
                c11,
                opts,
            );
        }
        editor.draw_at(ctx.draw, ctx.system, &self.current_map, camera_pos);
    }

    /// Composite the finished walkaround frame (left in `draw_state.rgba(BG)` by
    /// [`draw_world`](Self::draw_world)) onto `output`. Kept separate from the
    /// world build so the caller chooses the destination surface — the main
    /// window uses `system.output_image()`, an extra view its own framebuffer.
    pub fn composite_into(
        draw_state: &mut crate::drawstate::DrawState,
        output: &mut crate::system::drawing::image::RgbaImage,
    ) {
        use crate::drawstate::LayerId::*;
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::drawing::image::RgbaImage;

        output.blit::<RgbaImage>(
            0,
            0,
            &draw_state.rgba_canvas[BG as usize],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
    }
}
