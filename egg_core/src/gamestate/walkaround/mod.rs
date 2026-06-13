use crate::Ctx;
use crate::animation::Animation;
use crate::data::save::SaveData;
use crate::data::sound;
use crate::debug::DebugInfo;
use crate::interact::{InteractFn, Interaction};
use crate::map::{Axis, MapInfo, ObjectEffect, map_by_name};
use crate::particles::{Particle, ParticleDraw, ParticleList};
use crate::player::{Companion, CompanionList, CompanionTrail, MoveMode, Shell};
use crate::position::{Collider, Vec2};
use crate::system::PrintOptions;
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
    /// Per-object "player was inside this hitbox last frame" latch, one slot per
    /// [`MapInfo::objects`] entry (rebuilt each frame). Drives the *edge-trigger*
    /// for touch-fired interactions: a step-on dialogue fires when the player
    /// enters the hitbox, not every frame they stand in it. Rebuilt/cleared in
    /// [`load_map`](Self::load_map). Warps don't consult it (teleport exits the
    /// hitbox), so it tracks interaction objects' edges only.
    inside_objects: Vec<bool>,
    /// A warp whose narration is currently playing: it has fired and shown its
    /// dialogue, but the teleport is deferred until the box closes. While this is
    /// `Some` the whole object scan/apply is skipped, so the player standing in
    /// the warp's hitbox with the box open can't re-fire it. Cleared on apply and
    /// defensively in [`load_map`](Self::load_map).
    pending_warp: Option<crate::map::Warp>,
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
            inside_objects: Vec::new(),
            pending_warp: None,
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
        // The camera frames the first bg layer's area. Tile-first maps keep their
        // historical sizing (layer 0 is the collision tile layer at offset 0); a
        // pure-painted map sizes from its first image layer instead. A layer with
        // no positive size (e.g. a collision mask whose pixels never loaded) is
        // skipped so `from_map_size`'s positive-size assert can't trip; if nothing
        // sizable remains we keep the existing camera rather than panicking.
        if let Some(bounds) = &map_set.camera_bounds {
            self.camera.bounds = bounds.clone();
        } else if let Some(map1) = map_set
            .layers
            .iter()
            .find(|l| l.size.x.is_positive() && l.size.y.is_positive())
        {
            self.camera = Camera::from_map_size(
                map1.size,
                map1.offset,
                system.width() as i16,
                system.height() as i16,
            );
        }
        self.bg_colour = map_set.bg_colour;
        system.music(map_set.music_track.as_ref());

        // One animation per object that carries a sprite, in object order — the
        // same order `draw_world` zips them back against the objects' hitboxes.
        self.map_animations = map_set
            .objects
            .iter()
            .filter_map(|object| object.sprite.clone())
            .map(|frames| Animation {
                frames,
                ..Animation::default()
            })
            .collect();

        // Reset the per-object edge latch to the new map's object count, all
        // "outside" — so a touch object the player happens to spawn inside still
        // fires once on the first frame (entering counts from "was outside").
        self.inside_objects.clear();
        self.inside_objects.resize(map_set.objects.len(), false);
        // Defensive: a debug map switch mid-narration must not carry a pending
        // teleport onto the new map.
        self.pending_warp = None;

        self.current_map = map_set;

        self.map_animations.shrink_to_fit();

        self.creatures.clear();
        self.particles.clear();
    }
    /// Load a map by name through [`map_by_name`] (the loaded `maps` store).
    /// Unknown names log and leave the current map in place — a typo'd warp or
    /// a stale save name shouldn't take the player anywhere. Reads the sprite
    /// sheet (`ctx.draw`, for modern colliders), the loaded maps, and the
    /// console (camera/audio setup) straight off `ctx`.
    pub fn load_map_by_name<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, name: &str) {
        let Some(map_info) = map_by_name(&ctx.draw.indexed_sprites, name, ctx.maps) else {
            info!("load_map_by_name: unknown map {name:?}");
            return;
        };
        self.load_map(ctx.system, map_info);
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
    /// dialogue-registry key for the caller to resolve and display, alongside
    /// the console it plays sounds through. State-driven conditionals (the old
    /// stairwell window/painting pair) no longer live here — they are dialogue
    /// objects whose `#set`/`#if` directives drive the named save flags during
    /// playback (see [`crate::data::eggtext`]), so this stays pure behaviour and
    /// needs no `save`.
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

    /// Record the player's position and the map they're on into `save` (the
    /// persistent progress on [`EggState`]). The engine flushes it to storage
    /// at the end of the frame — this just updates the in-memory copy. Saves
    /// carry the map *name* only now (every map is named).
    fn save(&self, new_map: &str, save: &mut SaveData) {
        let pos = self.player_ref().pos;
        save.save_count += 1;
        save.current_map_name = Some(new_map.to_string());
        save.player_x = pos.x;
        save.player_y = pos.y;
    }

    pub fn load_pmem<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>) {
        let save = ctx.save.clone();
        // A save with no map name (a pre-name save, whose only map field was the
        // numeric `current_map` we no longer read) falls back to the bedroom —
        // where [`new_game`](Self::new_game) starts, so a save with a lost
        // location resumes at the game's beginning rather than nowhere.
        let name = save
            .current_map_name
            .unwrap_or_else(|| "bedroom".to_string());
        self.load_map_by_name(ctx, &name);
        self.player().pos.x = save.player_x;
        self.player().pos.y = save.player_y;
    }

    /// Starts a fresh game and saves over the default zeroed
    /// player position and map name.
    pub fn new_game<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>) {
        // Rebuild the live walkaround to its fresh construction state. "Erase
        // data" only zeroes `SaveData`; the existing `WalkaroundState` (player
        // entity, companions, dialogue…) is never rebuilt, so without this the
        // player keeps the position/shell they had before the reset and the
        // seed `save()` below would persist that stale position.
        *self = Self::new();
        self.load_map_by_name(ctx, "bedroom");
        self.save("bedroom", ctx.save);
    }
}

impl WalkaroundState {
    /// Fire one triggered [`Interaction`]: open its dialogue, run its function
    /// (then maybe open the dialogue that function returns), or do nothing. The
    /// single place both the map-object and companion interact paths resolve to,
    /// so they stay identical.
    fn fire_interaction<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, interaction: &Interaction) {
        match interaction {
            Interaction::Dialogue(key) => {
                let convo = ctx.get_dialogue(key);
                self.dialogue.set_messages(ctx.system, ctx.save, &convo);
            }
            Interaction::Func(x) => {
                if let Some(key) = self.execute_interact_fn(x, ctx.system) {
                    let convo = ctx.get_dialogue(key);
                    self.dialogue.set_messages(ctx.system, ctx.save, &convo);
                }
            }
            Interaction::None => {}
        }
    }

    /// Apply a warp's teleport: move the player, set the destination control-flip,
    /// refill the companion trail, and (for a cross-map warp) save + load the
    /// destination. Does **not** play the warp sound — that fires once at trigger
    /// time (see [`fire_warp`](Self::fire_warp)), so the narrated and un-narrated
    /// paths play it at the same moment and the deferred apply stays silent.
    fn apply_warp<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, warp: crate::map::Warp) {
        self.player().pos = warp.target();
        self.player().flip_controls = warp.flip;
        self.companion_trail
            .fill(self.player_ref().pos, self.player_ref().dir);
        if let Some(new_map) = warp.map {
            self.save(&new_map, ctx.save);
            self.load_map_by_name(ctx, &new_map);
        }
    }

    /// Trigger a warp that the object scan picked as the winner: play its sound
    /// (once, here, for both the immediate and narrated paths), then either show
    /// its narration and stash the payload in
    /// [`pending_warp`](Self::pending_warp) for the box-close apply, or teleport
    /// straight away. The narrated branch resolves the dialogue exactly as the
    /// interaction path does.
    fn fire_warp<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, warp: crate::map::Warp) {
        if let Some(sound) = &warp.sound {
            ctx.system.play_sound(sound.clone());
        }
        if let Some(key) = warp.narration.clone() {
            let convo = ctx.get_dialogue(&key);
            self.dialogue.set_messages(ctx.system, ctx.save, &convo);
            self.pending_warp = Some(warp);
        } else {
            self.apply_warp(ctx, warp);
        }
    }

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
            self.load_pmem(ctx);
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
            self.dialogue.tick(ctx.system, ctx.save, 1);
            if pressed(pad.a) {
                self.dialogue.tick(ctx.system, ctx.save, 2);
            }
            if just_pressed(pad.b) {
                self.dialogue.skip(ctx.system, ctx.save);
            }
        }
        if just_pressed(pad.a) && self.dialogue.is_line_done() {
            interact = true;
            if self.dialogue.next_text(ctx.system, ctx.save, false) {
                interact = false;
            } else if self.dialogue.current_text.is_some() {
                interact = false;
                self.dialogue.close();
            }
            info!("Attempting interact...");
        }
        if just_pressed(pad.x) {
            return Some(GameMode::MainMenu(super::menu::MenuState::debug_options(ctx.script)));
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

        // A narrated warp has fired and is showing its dialogue: the player is
        // standing in its hitbox with the box open, so the whole object pass is
        // skipped until the box has *fully* closed (no current line and an empty
        // queue) — otherwise the scan below would re-fire the same warp every
        // frame. When it closes, take the stashed payload and teleport (the sound
        // already played at fire time; this apply is silent). Skipping the dialogue
        // with B empties the queue the same way, so it warps then too.
        if self.pending_warp.is_some() {
            let box_closed =
                self.dialogue.current_text.is_none() && self.dialogue.next_text.is_empty();
            if box_closed && let Some(warp) = self.pending_warp.take() {
                self.apply_warp(ctx, warp);
            }
            self.camera.center_on(
                self.player_ref().pos.x + 4,
                self.player_ref().pos.y + 8,
                ctx.system.width() as i16,
                ctx.system.height() as i16,
            );
            return None;
        }

        // Two-phase object trigger. Phase 1 only *reads*: it finds the winning
        // warp and/or interaction by index, touching nothing (beyond the
        // per-object edge latch). Phase 2 acts on the winner — and a warp's
        // `load_map_by_name` replaces the very vec we scan, so the scan must
        // finish (and not borrow the vec) before we apply.
        //
        // The firing rule composes three axes (see [`crate::map::MapObject`]):
        // the object's authored [`Trigger`](crate::map::Trigger) decides the
        // touch vs. press paths; a warp's [`WarpMode`](crate::map::WarpMode) plus
        // the player's `manual_doors` preference can suppress a warp's touch path;
        // narration is orthogonal. Interactions' touch path is *edge-triggered*
        // (fires on entering the hitbox, via `inside_objects`) so a step-on
        // dialogue plays once; warps re-evaluate touch every frame because the
        // teleport exits the hitbox. Warp beats interaction.
        let player_hitbox = self.player_ref().hitbox();
        let manual_doors = ctx.save.manual_doors;
        // Keep the latch sized to the live object list (load_map syncs it, but an
        // editor session can change the count) before reading last frame's edges.
        self.inside_objects
            .resize(self.current_map.objects.len(), false);
        let mut warp_hit = None;
        let mut interact_hit = None;
        for (i, object) in self.current_map.objects.iter().enumerate() {
            let touched = player_hitbox.touches(object.hitbox);
            let probed = interact && interact_hitbox.touches(object.hitbox);
            let was_inside = self.inside_objects[i];
            // Update the edge latch for next frame regardless of what fires.
            self.inside_objects[i] = touched;
            match &object.effect {
                ObjectEffect::Warp(warp)
                    if warp_hit.is_none()
                        && object.trigger.warp_fires(touched, probed, &warp.mode, manual_doors) =>
                {
                    warp_hit = Some(i);
                }
                ObjectEffect::Interact(_)
                    if interact_hit.is_none()
                        && object.trigger.interaction_fires(touched, was_inside, probed) =>
                {
                    interact_hit = Some(i);
                }
                _ => {}
            }
        }

        if let Some(i) = warp_hit {
            let ObjectEffect::Warp(target) = &self.current_map.objects[i].effect else {
                unreachable!("warp_hit only records Warp effects");
            };
            let target = target.clone();
            // Plays the sound, then either narrates-then-defers or teleports now.
            self.fire_warp(ctx, target);
        } else if let Some(i) = interact_hit {
            // An interaction hit can now exist without a press (touch-triggered),
            // so it's gated on the hit, not on `interact`. Clone only the winning
            // interaction, then fire it exactly as before.
            let ObjectEffect::Interact(interaction) = self.current_map.objects[i].effect.clone()
            else {
                unreachable!("interact_hit only records Interact effects");
            };
            self.fire_interaction(ctx, &interaction);
        } else if interact {
            // No map object matched: fall back to the companions, checked against
            // the facing hitbox in order (today's chain ordering — companions
            // fire only when nothing on the map did, and stay press-only).
            for companion in self.companion_list.interact(&self.companion_trail) {
                if interact_hitbox.touches(companion.hitbox) {
                    if let ObjectEffect::Interact(interaction) = companion.effect {
                        self.fire_interaction(ctx, &interaction);
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
                .objects
                .iter()
                .filter(|object| object.sprite.is_some())
                .map(|object| object.hitbox),
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
                .draw_dialogue_box(ctx.draw, BG, ctx.system, ctx.save.small_text_on, &string, true);
        }
        if debug_info.map_info {
            // Warp hitboxes in colour 12, interaction hitboxes in colour 14;
            // the player hitbox shares the warps' colour.
            self.player_ref()
                .hitbox()
                .offset_xy(-camera_pos.x, -camera_pos.y)
                .draw(ctx.draw, BG, 12);
            for object in self.current_map.objects.iter() {
                let colour = match object.effect {
                    ObjectEffect::Warp(_) => 12,
                    ObjectEffect::Interact(_) => 14,
                };
                object
                    .hitbox
                    .offset_xy(-camera_pos.x, -camera_pos.y)
                    .draw(ctx.draw, BG, colour);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{LayerInfo, MapObject, Trigger, Warp};
    use crate::position::Hitbox;
    use crate::system::test_console::TestConsole;

    /// A minimal loadable map: one default layer (so `load_map` doesn't panic on
    /// an empty layer list) plus the given objects.
    fn map_with_objects(objects: Vec<MapObject>) -> MapInfo {
        MapInfo {
            layers: vec![LayerInfo::DEFAULT_LAYER],
            objects,
            ..MapInfo::default()
        }
    }

    /// The edge-trigger contract the walk loop relies on, exercised over a
    /// frame-by-frame touch sequence with the *real* predicate the loop calls
    /// ([`Trigger::interaction_fires`]) and the same latch update the loop does
    /// (`was_inside = latch; latch = touched`). A touch interaction fires once on
    /// entering, stays quiet while the player stands in it, and re-arms after the
    /// player leaves — without this a step-on dialogue would re-fire every frame.
    ///
    /// (A full walk `step` needs a live `Ctx` and simulated `just_pressed` button
    /// edges; the latch *computation* is the load-bearing part, so it's unit-
    /// tested here in isolation, per the brief.)
    #[test]
    fn touch_interaction_edge_fires_once_per_entry() {
        let trigger = Trigger::Touch;
        // `touched` per frame: outside, enter, stay, stay, leave, re-enter.
        let touches = [false, true, true, true, false, true];
        let mut latch = false; // the loop seeds `inside_objects` to false.
        let fired: Vec<bool> = touches
            .iter()
            .map(|&touched| {
                let was_inside = latch;
                latch = touched; // mirrors `self.inside_objects[i] = touched`.
                // No press in this scenario, so `probed` is always false.
                trigger.interaction_fires(touched, was_inside, false)
            })
            .collect();
        // Fires only on the two *entering* frames (1 and 5), not while standing.
        assert_eq!(fired, vec![false, true, false, false, false, true]);
    }

    /// `load_map` (re)sizes the edge latch to the new object count — all
    /// "outside" — and clears any pending narrated warp, so a debug map switch
    /// mid-narration can't teleport the player on the new map.
    #[test]
    fn load_map_resets_latch_and_pending_warp() {
        let mut console = TestConsole::new();
        let mut walk = WalkaroundState::new();

        // Pretend a narrated warp is mid-flight and the latch is stale.
        walk.pending_warp = Some(Warp::new(Some("somewhere"), Vec2::new(0, 0)));
        walk.inside_objects = vec![true; 9];

        let objects = vec![
            MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k"),
            MapObject::warp(Hitbox::new(8, 0, 8, 8), Warp::new(None, Vec2::new(0, 0))),
        ];
        walk.load_map(&mut console, map_with_objects(objects));

        assert_eq!(walk.inside_objects, vec![false, false], "latch sized + cleared");
        assert!(walk.pending_warp.is_none(), "pending warp dropped on map load");
    }
}
