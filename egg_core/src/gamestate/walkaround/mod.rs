use std::collections::BTreeMap;

use crate::Ctx;
use crate::data::save::SaveData;
use crate::data::scene::CutsceneDef;
use crate::data::sound;
use crate::debug::DebugInfo;
use crate::editor::map::MapViewer;
use crate::geometry::{Collider, Hitbox, Vec2};
use crate::platform::{ConsoleApi, ConsoleHelper, ScanCode, dpad_delta, just_pressed, pressed};
use crate::render::{DrawParams, PrintOptions, print_to_with_font};
use crate::ui::dialogue::Dialogue;
use crate::world::animation::Animation;
use crate::world::camera::Camera;
use crate::world::interact::{InteractFn, Interaction};
use crate::world::map::{Axis, MapInfo, ObjectEffect, map_by_name};
use crate::world::particles::{Particle, ParticleDraw, ParticleList};
use crate::world::player::{EntityId, MoveMode, PresetId, Shell};
use crate::gamestate::GameMode;
use log::info;

use self::cutscene::Cutscene;
use self::inventory::{Inventory, InventoryUi, InventoryUiState};
use crate::data::eggdata::GameItems;

mod cutscene;
pub mod inventory;

/// The *location* of a shell in the entity tree (a top-level entity, or a
/// companion of one) — the borrow-free result of
/// [`resolve_path`](WalkaroundState::resolve_path). Holding a path rather than a
/// `&mut Shell` lets a caller take the mutable shell *and* another `Walkaround`
/// field (e.g. `current_map`) at once, by indexing the disjoint fields directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityPath {
    /// `entities[i]`.
    Entity(usize),
    /// `entities[i].companions[j]`.
    Companion(usize, usize),
}

impl EntityPath {
    /// Borrow the shell this path points at.
    pub fn shell_ref(self, w: &WalkaroundState) -> &Shell {
        match self {
            EntityPath::Entity(i) => &w.entities[i],
            EntityPath::Companion(i, j) => &w.entities[i].companions[j],
        }
    }
    /// Mutably borrow the shell this path points at. (For collision-aware moves,
    /// index the fields directly instead, so `current_map` can be borrowed too.)
    pub fn shell_mut(self, w: &mut WalkaroundState) -> &mut Shell {
        match self {
            EntityPath::Entity(i) => &mut w.entities[i],
            EntityPath::Companion(i, j) => &mut w.entities[i].companions[j],
        }
    }
}

#[derive(Clone, Debug)]
pub struct WalkaroundState {
    pub entities: Vec<Shell>,
    /// Non-player entities parked by map name while the player is on another
    /// map. The player (`entities[0]`) travels across maps; `entities[1..]`
    /// belong to `current_map` and are swapped through here on every change in
    /// [`load_map`](Self::load_map), so creatures stay on the map that spawned
    /// them instead of bleeding into the next one. In-memory only for now — a
    /// later pass persists it (see the per-map save design).
    map_entities: BTreeMap<String, Vec<Shell>>,
    pub map_animations: Vec<Animation>,
    pub camera: Camera,
    pub current_map: MapInfo,
    pub map_viewer: MapViewer,
    pub dialogue: Dialogue,
    /// The player's bag and its on-screen view. Lives here (rather than on
    /// [`EggState`](crate::EggState)) because the inventory is part of the
    /// walkaround: the world opens it, the inventory mode dispatches into it, and
    /// it round-trips through the save with the rest of the walkaround's state.
    pub inventory_ui: InventoryUi,
    pub particles: ParticleList,
    /// The cutscene **stack**: the top is the active cutscene; a `load` step
    /// pushes a sub-cutscene (popped on finish), so map changes happen at
    /// cutscene boundaries with fresh requisition. Empty = normal gameplay.
    pub cutscene: Vec<Cutscene>,
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
    pending_warp: Option<crate::world::map::Warp>,
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
            map_entities: BTreeMap::new(),
            map_animations: Vec::new(),
            camera: Camera::default(),
            current_map: MapInfo::default(),
            map_viewer: MapViewer::primary(),
            dialogue: Dialogue::default(),
            inventory_ui: InventoryUi::new(),
            particles: ParticleList::new(),
            cutscene: Vec::new(),
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

    /// Every shell in the live world — top-level entities **and** their nested
    /// companions (depth-1) — in a stable order (each leader immediately followed
    /// by its companions). The single seam the update loop, draw, save and
    /// [`resolve`](Self::resolve) walk, so "every entity" never silently means
    /// "top-level only".
    pub fn all_shells(&self) -> impl Iterator<Item = &Shell> {
        self.entities
            .iter()
            .flat_map(|leader| leader.companions.iter().chain(std::iter::once(leader)))
    }

    /// Resolve an [`EntityId`] against the live entity tree: the player is
    /// `entities[0]`, a companion is `entities[0].companions[slot]`, and an
    /// [`EntityId::Id`] is the first shell whose [`Shell::id`] matches. `None`
    /// when nothing matches (an absent creature / out-of-range slot) — callers
    /// (cutscene chains) log and skip.
    pub fn resolve(&self, id: &EntityId) -> Option<&Shell> {
        match id {
            EntityId::Player => self.entities.first(),
            EntityId::PlayerCompanion(slot) => {
                self.entities.first().and_then(|p| p.companions.get(*slot))
            }
            EntityId::Id(name) => self
                .all_shells()
                .find(|s| s.id.as_deref() == Some(name.as_str())),
        }
    }

    /// After a cutscene that suspended companions (cutscene-driven actors don't
    /// drag their companions), re-seat each leader's trail so its companions stay
    /// where they are — resuming the trail toward the leader — instead of snapping
    /// to a stale breadcrumb when normal following resumes. The first companion's
    /// position seeds the trail tail (exact for the single-companion dog case).
    fn reseat_companion_trails(&mut self) {
        for leader in self.entities.iter_mut() {
            // Seed the tail with the companion's *own* facing, not the leader's,
            // so resuming the follow doesn't turn it — it keeps the direction it
            // held through the scene until it actually walks again.
            let Some((tail, dir)) = leader.companions.first().map(|c| (c.pos, c.dir)) else {
                continue;
            };
            leader.trail.fill_toward(tail, leader.pos, dir);
        }
    }

    /// Resolve an [`EntityId`] to a [`EntityPath`] — its *location* in the entity
    /// tree, not a borrow — so a caller can take a `&mut Shell` to it alongside
    /// another field (e.g. `current_map` for collision), which a `&mut Shell`
    /// from a resolver method would forbid. `None` like [`resolve`](Self::resolve).
    pub fn resolve_path(&self, id: &EntityId) -> Option<EntityPath> {
        match id {
            EntityId::Player => (!self.entities.is_empty()).then_some(EntityPath::Entity(0)),
            EntityId::PlayerCompanion(slot) => self
                .entities
                .first()
                .filter(|p| *slot < p.companions.len())
                .map(|_| EntityPath::Companion(0, *slot)),
            EntityId::Id(name) => {
                for (i, leader) in self.entities.iter().enumerate() {
                    if leader.id.as_deref() == Some(name.as_str()) {
                        return Some(EntityPath::Entity(i));
                    }
                    for (j, comp) in leader.companions.iter().enumerate() {
                        if comp.id.as_deref() == Some(name.as_str()) {
                            return Some(EntityPath::Companion(i, j));
                        }
                    }
                }
                None
            }
        }
    }

    /// Frame the camera and background from `map_set`: the camera bounds (an
    /// explicit `camera_stick`, else auto-sized from the first sizable layer) and
    /// the background palette colour. A layer with no positive size (e.g. a
    /// collision mask whose pixels never loaded) is skipped so `from_map_size`'s
    /// positive-size assert can't trip; if nothing sizable remains the existing
    /// camera is kept rather than panicking. Shared by [`load_map`](Self::load_map)
    /// and the in-editor re-derive so a Setup-panel edit (camera / bg / resize)
    /// applies live, not only after a full reload.
    fn apply_map_framing(&mut self, system: &mut impl ConsoleApi, map_set: &MapInfo) {
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
    }

    /// Loads a map from given data
    pub fn load_map(&mut self, system: &mut impl ConsoleApi, map_set: impl Into<MapInfo>) {
        let map_set = map_set.into();
        self.apply_map_framing(system, &map_set);
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

        // Swap per-map entities: park the leaving map's non-player entities
        // (`entities[1..]`) under its name and restore the entering map's, so
        // creatures stay on the map that spawned them. The player (`entities[0]`)
        // travels — its position is already set by the warp. Same-map reloads and
        // the unnamed initial map are skipped, so neither parks a stray entry.
        let old_source = self.current_map.source.clone();
        let new_source = map_set.source.clone();
        if old_source != new_source {
            let leaving = self.entities.drain(1..).collect::<Vec<_>>();
            if !old_source.is_empty() {
                self.map_entities.insert(old_source, leaving);
            }
            let arriving = self.map_entities.remove(&new_source).unwrap_or_default();
            self.entities.extend(arriving);
        }

        self.current_map = map_set;

        self.map_animations.shrink_to_fit();

        self.particles.clear();
    }
    /// Load a map by name through [`map_by_name`] (the loaded `maps` store).
    /// Unknown names log and leave the current map in place — a typo'd warp or
    /// a stale save name shouldn't take the player anywhere. Reads the sprite
    /// sheet (`ctx.draw`, for modern colliders), the loaded maps, and the
    /// console (camera/audio setup) straight off `ctx`.
    pub fn load_map_by_name<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, name: &str) {
        let Some(mut map_info) = map_by_name(&ctx.draw.indexed_sprites, name, ctx.maps) else {
            info!("load_map_by_name: unknown map {name:?}");
            return;
        };
        // Drop removable objects already consumed in this save, so a picked-up
        // item stays gone across reloads. Done here in the gameplay loader (which
        // has the save), not in `load_map`, so the editor's raw loads still show
        // every object regardless of save state. An id-less removable can't have
        // been recorded as taken, so it always survives the filter.
        map_info
            .objects
            .retain(|o| !o.removable || !o.id.is_some_and(|id| ctx.save.is_taken(name, id)));
        self.load_map(ctx.system, map_info);
    }
    pub fn cam_x(&self) -> i32 {
        self.camera.pos.x.into()
    }
    pub fn cam_y(&self) -> i32 {
        self.camera.pos.y.into()
    }
    /// Centre the camera on a map-pixel point framed as a player landing there
    /// (the same +4/-2 hitbox offset the follow-camera uses), clamped to the
    /// map's bounds. Used when the editor opens a warp's destination so the
    /// landing point is framed the way gameplay shows it on arrival.
    pub fn center_camera_on(&mut self, p: Vec2, w: i32, h: i32) {
        self.camera.center_on(p.x + 4, p.y - 2, w as i16, h as i16);
    }
    pub fn cam_state(&mut self) -> &mut crate::world::camera::CameraBounds {
        &mut self.camera.bounds
    }
    /// Function that does everything. No anti-pattern here. Returns an optional
    /// dialogue-registry key for the caller to resolve and display, alongside
    /// the console it plays sounds through. State-driven conditionals (the old
    /// stairwell window/painting pair) no longer live here — they are dialogue
    /// objects whose `#set`/`#if` directives drive the named save flags during
    /// playback (see [`crate::data::script::eggtext`]), so this stays pure behaviour and
    /// needs no `save`.
    pub fn execute_interact_fn(
        &mut self,
        interact: &InteractFn,
        system: &mut impl ConsoleApi,
        inventory: &mut Inventory,
        presets: &crate::data::eggdata::Presets,
    ) -> Option<&'static str> {
        match interact {
            InteractFn::ToggleDog => {
                let (ppos, pdir) = (self.player_ref().pos, self.player_ref().dir);
                // Seed the player's trail at its own position so a freshly
                // summoned dog snaps to it rather than the stale tail.
                self.player().trail.fill(ppos, pdir);
                let dog = PresetId::dog();
                if self.player_ref().companions.iter().any(|c| c.preset == dog) {
                    self.player().companions.retain(|c| c.preset != dog);
                    system.play_sound(sound::ALERT_DOWN);
                    Some("dog_relinquished")
                } else {
                    let slot = self.player_ref().companions.len();
                    let mut shell = presets.spawn(&dog).unwrap_or_default();
                    shell.move_mode = MoveMode::Companion { slot };
                    shell.interaction = Some(crate::world::player::pet_marker());
                    shell.pos = ppos;
                    self.player().companions.push(shell);
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
                self.entities
                    .extend((0..=*x).map(|_| Shell::egg(PresetId::critter()).with_pos(pos)));
                None
            }
            InteractFn::GiveItem(key) => {
                // Slot the item key into the inventory. A full inventory is a
                // no-op (no panic), so a player with no free slot simply gains
                // nothing; an unknown key still occupies a slot but draws no
                // sprite/name until the registry knows it.
                inventory.add(key.clone());
                None
            }
            InteractFn::Pet(..) => {
                // The pet *beat*: the player's petting animation + a sound. The
                // walk-up is the cutscene's job (a `beside` move); this is the
                // intrinsic effect its `interact` step fires. `pet_timer` counts
                // down in the walk loop, so it plays out after the scene ends.
                self.player().pet_timer = Some(90);
                system.play_sound(sound::POP);
                None
            }
        }
    }

    /// Plays a cued cutscene until finished, then removes it from the cue.
    /// Pressing B fast-forwards it: [`Cutscene::skip`](cutscene::Cutscene::skip)
    /// applies every remaining stage's end state + side effects safely, so a
    /// cutscene can always be cut short without soft-locking. Takes the whole
    /// [`Ctx`] (not just the console) because a `dialogue` step resolves its key
    /// against `ctx.script` and drives the box through `ctx.save`.
    fn play_cutscene<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>) -> bool {
        if self.cutscene.is_empty() {
            return false;
        }
        // Drive the top of the stack, held apart from `self` so it can borrow the
        // walkaround mutably. B fast-forwards; on an `interruptible` scene a
        // just-pressed movement direction cancels it instead.
        let mut top = self.cutscene.pop().expect("non-empty checked above");
        let pad = ctx.input.controller();
        let (idx, idy) = dpad_delta(&pad, just_pressed);
        let interrupted = top.is_interruptible() && (idx != 0 || idy != 0);
        let outcome = if interrupted {
            cutscene::Outcome::Cancelled
        } else {
            if just_pressed(pad.b) {
                top.skip(ctx, self);
            }
            top.step(ctx, self)
        };
        match outcome {
            cutscene::Outcome::Running => self.cutscene.push(top),
            // Reached the end, or cancelled (interrupt / blocked required move):
            // clean up its transient actors, and when the whole stack is done hand
            // companions back where they are (no snap to a stale breadcrumb).
            cutscene::Outcome::Finished | cutscene::Outcome::Cancelled => {
                top.cleanup(self);
                if self.cutscene.is_empty() {
                    self.reseat_companion_trails();
                }
            }
            cutscene::Outcome::Load(name) => {
                // The parent already advanced past its `load`; keep it below the
                // sub-cutscene, which drives until it finishes and pops.
                self.cutscene.push(top);
                match ctx.scenes.get_cutscene(&name).cloned() {
                    Some(def) => {
                        let sub = Cutscene::launch(&def, ctx, self);
                        self.cutscene.push(sub);
                    }
                    None => info!("cutscene load: unknown cutscene {name:?}"),
                }
            }
        }
        // Keep the camera on the player while a scene plays (the normal follow
        // update below is skipped during cutscenes) — so the player stays framed
        // as it walks and pets, with no snap when the scene ends. A per-scene
        // camera target is a future verb.
        self.camera.center_on(
            self.player_ref().pos.x + 4,
            self.player_ref().pos.y - 2,
            ctx.system.width() as i16,
            ctx.system.height() as i16,
        );
        true
    }

    /// Frame cap for the scrubber's re-simulation, so a scene that never
    /// terminates (e.g. a required move blocked forever) can't hang the editor.
    /// ~27 min at 60fps — far beyond any authored cutscene.
    pub(crate) const SCRUB_MAX_FRAMES: usize = 100_000;

    /// Re-simulate this world's cutscene stack to completion on a CLONE,
    /// returning the number of frames it runs (one per [`play_cutscene`] step).
    /// The clone leaves `self` untouched, so the scrubber can measure a scene's
    /// length and then seek into it freely. Capped at [`SCRUB_MAX_FRAMES`](Self::SCRUB_MAX_FRAMES)
    /// so a scene that never terminates can't hang the editor.
    pub(crate) fn measure_cutscene<S: ConsoleApi>(&self, ctx: &mut Ctx<S>) -> usize {
        let mut world = self.clone();
        let mut frames = 0;
        while world.play_cutscene(ctx) {
            frames += 1;
            if frames >= Self::SCRUB_MAX_FRAMES {
                break;
            }
        }
        frames
    }

    /// Re-simulate `frame` steps on a CLONE (clamped by the scene ending) and
    /// return the world at that frame — what the scrubber draws as its ghost.
    /// A pure re-sim from the post-launch snapshot, so any playhead position is
    /// reproducible; this is the determinism the scrubber relies on (the AI/RNG
    /// loop is skipped while a cutscene plays, so there's nothing nondeterministic
    /// to diverge).
    pub(crate) fn sim_cutscene_to<S: ConsoleApi>(&self, frame: usize, ctx: &mut Ctx<S>) -> Self {
        let mut world = self.clone();
        for _ in 0..frame.min(Self::SCRUB_MAX_FRAMES) {
            if !world.play_cutscene(ctx) {
                break;
            }
        }
        world
    }

    /// Launch `def` and arm it on this world's cutscene stack — the scrubber's
    /// snapshot setup. After this, [`play_cutscene`](Self::play_cutscene) (hence
    /// [`measure_cutscene`](Self::measure_cutscene)/[`sim_cutscene_to`](Self::sim_cutscene_to))
    /// drives the scene. Keeps [`Cutscene`] construction inside this module.
    pub(crate) fn arm_cutscene<S: ConsoleApi>(&mut self, def: &CutsceneDef, ctx: &mut Ctx<S>) {
        let cs = Cutscene::launch(def, ctx, self);
        self.cutscene.push(cs);
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
        save.save_count += 1;
        save.current_map_name = Some(new_map.to_string());
        // The player is a Shell like any other entity — persist it whole, so its
        // position and its nested companions (the dog) ride along. Its derived
        // sprites/trail/interaction are serde-skipped and rebuilt on load.
        save.player = Some(self.player_ref().clone());
        // Snapshot per-map creatures: the already-parked maps, plus the map the
        // player is on now — its live non-player `entities[1..]` under its own
        // name. Keyed by `current_map.source` (where those entities *are*), not
        // `new_map`: at a warp this runs before the destination loads, so the live
        // entities still belong to the current map. Sprites ride along in memory
        // but are skipped on serialise (rebuilt from each shell's preset on load).
        let mut parked = self.map_entities.clone();
        let here = &self.current_map.source;
        if !here.is_empty() {
            parked.insert(here.clone(), self.entities[1..].to_vec());
        }
        save.map_entities = parked;
    }

    pub fn load_pmem<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>) {
        let save = ctx.save.clone();
        // Drop any live non-player entities and current-map identity first, so a
        // mid-game reload (the debug hotkey) can't park stale creatures over the
        // restored set when the map loads below.
        self.entities.truncate(1);
        self.current_map = MapInfo::default();
        // Restore parked per-map creatures before the map loads, so the load_map
        // swap pulls the current map's creatures back into `entities`. Their
        // sprites were skipped in the save — rebuild each from its preset.
        self.map_entities = save.map_entities;
        for shells in self.map_entities.values_mut() {
            for shell in shells.iter_mut() {
                shell.reattach_sprites(ctx.presets);
            }
        }
        // A save with no map name (a pre-name save, whose only map field was the
        // numeric `current_map` we no longer read) falls back to the bedroom —
        // where [`new_game`](Self::new_game) starts, so a save with a lost
        // location resumes at the game's beginning rather than nowhere.
        let name = save
            .current_map_name
            .unwrap_or_else(|| "bedroom".to_string());
        self.load_map_by_name(ctx, &name);
        // Restore the whole player entity. A modern save carries it (position +
        // nested companions + state); an older save only had a position, so place
        // the freshly-built default player there instead. Either way rebuild the
        // derived sprites, re-derive the dog's serde-skipped pet interaction, and
        // seed the trail so companions regroup on the player next step.
        let (legacy_x, legacy_y) = (save.player_x, save.player_y);
        if let Some(mut player) = save.player {
            player.reattach_sprites(ctx.presets);
            let dog = PresetId::dog();
            for companion in &mut player.companions {
                if companion.preset == dog {
                    companion.interaction = Some(crate::world::player::pet_marker());
                }
            }
            self.entities[0] = player;
        } else {
            self.player().pos = Vec2::new(legacy_x, legacy_y);
        }
        let (ppos, pdir) = (self.player_ref().pos, self.player_ref().dir);
        self.player().trail.fill(ppos, pdir);
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
    fn fire_interaction<S: ConsoleApi>(
        &mut self,
        ctx: &mut Ctx<S>,
        interaction: &Interaction,
        inventory: &mut Inventory,
    ) {
        match interaction {
            Interaction::Dialogue(key) => {
                let convo = ctx.get_dialogue(key);
                self.dialogue.set_messages(ctx.system, ctx.font, ctx.save, &convo);
            }
            Interaction::Func(x) => {
                if let Some(key) = self.execute_interact_fn(x, ctx.system, inventory, ctx.presets) {
                    let convo = ctx.get_dialogue(key);
                    self.dialogue.set_messages(ctx.system, ctx.font, ctx.save, &convo);
                }
            }
            Interaction::Cutscene(name) => {
                // Look the name up in the registry, then launch it (requisition
                // its actors) and push it onto the stack. An unknown name logs and
                // does nothing (like a dangling warp target), so a typo can't
                // crash or soft-lock.
                match ctx.get_cutscene(name).cloned() {
                    Some(def) => {
                        let cutscene = Cutscene::launch(&def, ctx, self);
                        self.cutscene.push(cutscene);
                    }
                    None => info!("fire_interaction: unknown cutscene {name:?}"),
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
    fn apply_warp<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, warp: crate::world::map::Warp) {
        self.player().pos = warp.target();
        self.player().flip_controls = warp.flip;
        let (ppos, pdir) = (self.player_ref().pos, self.player_ref().dir);
        // Collapse the player's trail onto the landing so companions teleport in
        // beside it instead of streaming across the map from the old position,
        // then snap them there this frame (apply_warp runs after the entity loop,
        // so without this the dog would draw at its old position for one frame).
        self.player().trail.fill(ppos, pdir);
        self.player().update_companions();
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
    fn fire_warp<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, warp: crate::world::map::Warp) {
        if let Some(sound) = &warp.sound {
            ctx.system.play_sound(sound.clone());
        }
        if let Some(key) = warp.narration.clone() {
            let convo = ctx.get_dialogue(&key);
            self.dialogue.set_messages(ctx.system, ctx.font, ctx.save, &convo);
            self.pending_warp = Some(warp);
        } else {
            self.apply_warp(ctx, warp);
        }
    }

    /// Re-sync the cached per-object [`Animation`]s to `current_map.objects` so
    /// live edits from the map editor (retiled / added / removed frames) show
    /// in-world at once. Patches each animation's frames in place — keeping its
    /// playback cursor where the frames still fit — and only rebuilds when the
    /// sprited-object count changes (a frame add/remove that creates or drops a
    /// whole sprite). Called while any map editor is open: [`step`](Self::step)
    /// drives it for the primary editor, and the host drives it for an extra
    /// view's editor (which mutates the same shared map but never passes through
    /// `step`), so an extra "map preview" window reflects its own edits too.
    pub fn sync_map_animations(&mut self) {
        let live: Vec<_> = self
            .current_map
            .objects
            .iter()
            .filter_map(|o| o.sprite.clone())
            .collect();
        if live.len() != self.map_animations.len() {
            self.map_animations = live
                .into_iter()
                .map(|frames| Animation {
                    frames,
                    ..Animation::default()
                })
                .collect();
            return;
        }
        for (anim, frames) in self.map_animations.iter_mut().zip(live) {
            if anim.frames != frames {
                anim.frames = frames;
                if anim.index >= anim.frames.len() {
                    anim.index = 0;
                    anim.tick = 0;
                }
            }
        }
    }

    /// Consume a just-fired [`removable`](crate::world::map::MapObject::removable) object
    /// at index `i`: record it in the save's `taken` set by its stable
    /// [`id`](crate::world::map::MapObject::id) — so every later
    /// [`load_map_by_name`](Self::load_map_by_name) of this map filters it out —
    /// then drop it from the live map so it vanishes at once. A no-op for a
    /// non-removable object; an id-less removable still vanishes for the session
    /// but can't be persisted (it returns on the next load).
    fn take_object(&mut self, i: usize, save: &mut SaveData) {
        let object = &self.current_map.objects[i];
        if !object.removable {
            return;
        }
        if let Some(id) = object.id {
            save.mark_taken(&self.current_map.source, id);
        }
        self.remove_object(i);
    }

    /// Remove the object at index `i` from the live map, keeping the parallel
    /// per-object caches aligned: the edge latch [`inside_objects`](Self::inside_objects)
    /// (1:1 with objects) drops the same slot, and the sprite animations are
    /// resynced (a sprited object leaving changes their count — see
    /// [`sync_map_animations`](Self::sync_map_animations)).
    fn remove_object(&mut self, i: usize) {
        self.current_map.objects.remove(i);
        if i < self.inside_objects.len() {
            self.inside_objects.remove(i);
        }
        self.sync_map_animations();
    }

    /// Step the open bag overlay and translate its state into an optional mode
    /// transition. Drives the inventory's own input/step, then maps: `Close`
    /// resumes the world next frame (no transition — we're already in
    /// Walkaround, and the overlay guard simply stops firing once it reads
    /// closed); `Options` leaves to the shared options menu; any other state is
    /// "still browsing", drawn over the world by [`draw`](Self::draw). Relocated
    /// here from the old `GameMode::Inventory` dispatch arm — the bag is now an
    /// overlay the walkaround owns, not a top-level mode.
    fn step_inventory(&mut self, ctx: &mut Ctx<impl ConsoleApi>) -> Option<GameMode> {
        self.inventory_ui.step(ctx);
        match self.inventory_ui.state {
            InventoryUiState::Close => None,
            InventoryUiState::Options => Some(GameMode::InventoryOptions),
            _ => None,
        }
    }

    /// Rehydrate the bag's inventory from a save's persisted item keys. The save
    /// round-trip is encapsulated behind the walkaround (the bag lives here), so
    /// [`run`](crate::EggState::run) reaches it through this rather than the
    /// inventory's internals.
    pub fn load_inventory(&mut self, saved: &[Option<String>; 8], items: &GameItems) {
        self.inventory_ui.inventory.load_from_save(saved, items);
    }

    /// Snapshot the bag's inventory as the persistent `[Option<String>; 8]` a
    /// save stores. The inverse of [`load_inventory`](Self::load_inventory);
    /// like it, the save round-trip is encapsulated behind the walkaround.
    pub fn snapshot_inventory(&self) -> [Option<String>; 8] {
        self.inventory_ui.inventory.to_save()
    }

    pub fn step<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>) -> Option<GameMode> {
        // While the primary map editor is open, mirror live frame edits into the
        // cached animations before advancing them, so the in-world sprite updates
        // too. (An extra view's editor is synced by the host — see
        // `sync_map_animations` — since its edits never pass through here.)
        if self.map_viewer.focused {
            self.sync_map_animations();
        }
        self.map_animations
            .iter_mut()
            .for_each(|anim| anim.advance());

        self.particles.step();

        if self.play_cutscene(ctx) {
            return None;
        }

        // When the map editor is open it takes over all input and freezes the
        // sim, so painting/typing can't move the player or trip warps/reloads.
        if self.map_viewer.focused {
            let sheet = (
                ctx.draw.indexed_sprites.width() as usize / 8,
                ctx.draw.indexed_sprites.height() as usize / 8,
            );
            // Hand the editor the current cutscene names so its scene picker can
            // list them (it doesn't otherwise see the registry). Refreshed each
            // focused frame, so a just-recorded scene shows up.
            self.map_viewer.scene_names = ctx.scenes.names();
            self.map_viewer.step_map_viewer(
                ctx.system,
                ctx.input,
                &mut self.current_map,
                ctx.maps,
                self.camera.pos,
                sheet,
                ctx.script,
                ctx.save,
            );
            // The browser can't resolve a map itself (it lacks the sprite sheet),
            // so it parks the request here and we load it through the tested path.
            if let Some((name, focus)) = self.map_viewer.pending_open.take() {
                self.load_map_by_name(ctx, &name);
                // A warp "open" carries its landing point: frame it as gameplay
                // would when the player arrives there.
                if let Some(p) = focus {
                    self.center_camera_on(p, ctx.system.width(), ctx.system.height());
                }
            }
            // A layer or Setup edit changed the stored map: re-derive the runtime
            // layer lists and the scalar metadata (bg colour, camera framing), so
            // a colour / camera / resize edit applies live. Objects and the player
            // stay as they are.
            if self.map_viewer.pending_reload {
                self.map_viewer.pending_reload = false;
                if let Some(fresh) = map_by_name(
                    &ctx.draw.indexed_sprites,
                    &self.current_map.source,
                    ctx.maps,
                ) {
                    self.apply_map_framing(ctx.system, &fresh);
                    self.current_map.bg_colour = fresh.bg_colour;
                    self.current_map.camera_bounds = fresh.camera_bounds;
                    self.current_map.layers = fresh.layers;
                    self.current_map.fg_layers = fresh.fg_layers;
                }
            }
            return None;
        }

        // The bag is an overlay on the walkaround: while it's open, step it
        // instead of the world. A pausing overlay (the bag returns `true` from
        // `pauses`) early-returns here so the world sim is frozen — opening the
        // inventory stops the world the same way the map editor does. The
        // fall-through (a non-pausing overlay) is left structurally present but
        // unexercised: the bag is the only overlay today and it pauses, so a
        // non-pausing overlay would step here and then let the world step too.
        if self.inventory_ui.is_open() {
            let trans = self.step_inventory(ctx);
            if self.inventory_ui.pauses() {
                return trans;
            }
        }

        if ctx.input.keyp(ScanCode::Digit5) && ctx.input.key(ScanCode::Ctrl) {
            self.load_pmem(ctx);
        }
        if ctx.input.keyp(ScanCode::Digit6) {
            ctx.draw.set_palette(&crate::platform::SWEETIE_16);
        }
        if ctx.input.keyp(ScanCode::Digit7) {
            ctx.draw.set_palette(&crate::platform::NIGHT_16);
        }
        if ctx.input.keyp(ScanCode::Digit8) {
            ctx.draw.set_palette(&crate::platform::B_W);
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;

        let pad = ctx.input.controller();
        if self.dialogue.current_text.is_none() && self.dialogue.next_text.is_empty() {
            (dx, dy) = dpad_delta(&pad, pressed);
            if just_pressed(pad.b) {
                // Open the bag overlay in place. No mode change: the overlay
                // guard above (which `is_open` now sees as true) drives it from
                // the next frame, and `draw` composites it over the world.
                self.inventory_ui.open(ctx.system);
                return None;
            }
        } else {
            if self.dialogue.characters == 0 {
                ctx.system.play_sound(sound::INTERACT);
            }
            self.dialogue.tick(ctx.system, ctx.font, ctx.save, 1);
            if pressed(pad.a) {
                self.dialogue.tick(ctx.system, ctx.font, ctx.save, 2);
            }
            if just_pressed(pad.b) {
                self.dialogue.skip(ctx.system, ctx.font, ctx.save);
            }
            if ctx.input.keyp(ScanCode::Q) && ctx.input.key(ScanCode::Ctrl) {
                self.dialogue.close();
            }
        }
        if just_pressed(pad.a) && self.dialogue.is_line_done() {
            interact = true;
            if self.dialogue.next_text(ctx.system, ctx.font, ctx.save, false) {
                interact = false;
            } else if self.dialogue.current_text.is_some() {
                interact = false;
                self.dialogue.close();
            }
            info!("Attempting interact...");
        }
        if just_pressed(pad.x) {
            return Some(GameMode::DebugMenu);
        }
        if ctx.input.any_btnpr() {
            self.player().flip_controls = Axis::None
        }
        let noclip = if ctx.input.key(ScanCode::Ctrl) && ctx.input.key(ScanCode::Shift) {
            dy *= 3;
            dx *= 4;
            true
        } else {
            false
        };

        let tiles = ctx.maps.get(&self.current_map.source);
        // What a shell wants this step. We decide behind a `&mut move_mode` borrow
        // (so egg/amble timers can tick), then act once it's released — hatching
        // reassigns the whole `Shell`, which the live borrow would forbid.
        enum Act {
            Player,
            Drive(i16, i16),
            // The critter gait: move on the third tick like `Drive`, but animate
            // for the whole Walking state (`walking`) so the sprite cycles
            // smoothly rather than flickering on the idle ticks between moves.
            Amble { vx: i16, vy: i16, walking: bool },
            Hatch(PresetId),
        }
        for shell in self.entities.iter_mut() {
            let act = match &mut shell.move_mode {
                MoveMode::Player => Act::Player,
                MoveMode::Wander => {
                    let (vx, vy) = if ctx.rng.rand_u8() < 25 {
                        (
                            (ctx.rng.rand_u8() % 3) as i16 - 1,
                            (ctx.rng.rand_u8() % 3) as i16 - 1,
                        )
                    } else {
                        (shell.dir.0.into(), shell.dir.1.into())
                    };
                    Act::Drive(vx, vy)
                }
                MoveMode::Egg {
                    timer,
                    hatches_into,
                } => {
                    if timer.tick() {
                        Act::Hatch(hatches_into.clone())
                    } else {
                        Act::Drive(0, 0)
                    }
                }
                MoveMode::Amble(state) => {
                    let (vx, vy) = state.step(ctx.rng);
                    Act::Amble {
                        vx,
                        vy,
                        walking: state.is_walking(),
                    }
                }
                // Companions are driven by their leader's `update_companions`, not
                // self — a top-level companion (shouldn't happen) just idles.
                MoveMode::Companion { .. } => Act::Drive(0, 0),
            };
            match act {
                Act::Player => {
                    let (dx, dy) = shell.walk(ctx.system, dx, dy, noclip, &self.current_map, tiles);
                    shell.apply_motion(dx, dy);
                }
                Act::Drive(vx, vy) => {
                    // `walk` updates the shell's facing (incl. the sticky
                    // horizontal that keeps a vertical-only wanderer's mirror).
                    let (dx, dy) = shell.walk(ctx.system, vx, vy, false, &self.current_map, tiles);
                    shell.apply_motion(dx, dy);
                }
                Act::Amble { vx, vy, walking } => {
                    let (dx, dy) = shell.walk(ctx.system, vx, vy, false, &self.current_map, tiles);
                    shell.pos.x += dx;
                    shell.pos.y += dy;
                    // Animate by the Walking *state*, not this tick's motion, so
                    // the slow every-third-tick gait still cycles smoothly.
                    if walking {
                        shell.animate_walk();
                    } else {
                        shell.animate_stop();
                    }
                }
                Act::Hatch(preset) => {
                    let pos = shell.pos;
                    *shell = ctx
                        .presets
                        .spawn(&preset)
                        .unwrap_or_else(|| {
                            log::warn!("egg hatches into unknown preset `{preset}`; using default");
                            Shell::default()
                        })
                        .with_pos(pos);
                }
            }
            // Each leader, having moved (and pushed its breadcrumb), drags its
            // companions onto the trail — the dog follows the player here.
            shell.update_companions();
        }

        // The petting animation (set by the pet beat): it plays out over its
        // remaining frames after the cutscene ends. Player movement interrupts it
        // immediately, and a `pop` fires every 20 frames synced with the pose flip.
        if let Some(t) = self.player().pet_timer {
            if self.player_ref().walking {
                self.player().pet_timer = None;
            } else {
                let next = t.saturating_sub(1);
                if next > 0 && next.is_multiple_of(20) {
                    ctx.system.play_sound(sound::POP);
                }
                self.player().pet_timer = (next > 0).then_some(next);
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
                self.player_ref().pos.y - 2,
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
        // The firing rule composes three axes (see [`crate::world::map::MapObject`]):
        // the object's authored [`Trigger`](crate::world::map::Trigger) decides the
        // touch vs. press paths; a warp's [`WarpMode`](crate::world::map::WarpMode) plus
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
                        && object
                            .trigger
                            .warp_fires(touched, probed, &warp.mode, manual_doors) =>
                {
                    warp_hit = Some(i);
                }
                ObjectEffect::Interact(_)
                    if interact_hit.is_none()
                        && object
                            .trigger
                            .interaction_fires(touched, was_inside, probed) =>
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
            // The bag now lives on `self`, so lift it out for the duration of the
            // call (which also borrows `self` mutably) and put it straight back.
            let mut inventory = std::mem::take(&mut self.inventory_ui.inventory);
            self.fire_interaction(ctx, &interaction, &mut inventory);
            self.inventory_ui.inventory = inventory;
            // A removable object is consumed by the interaction: record it taken
            // (by stable id) and drop it from the live map so it vanishes now.
            self.take_object(i, ctx.save);
        } else if interact {
            // No map object matched: fall back to the player's companions. If the
            // facing hitbox is on a pettable companion (the dog), launch the pet
            // cutscene — it walks the player up via a `beside` move, then fires the
            // dog's intrinsic pet. Lowest precedence, press-only.
            let pettable = self.player_ref().companions.iter().any(|c| {
                if c.interaction.is_none() {
                    return false;
                }
                // Hit-test the companion's drawn *body* (its sprite footprint), not
                // its small feet hitbox, so petting isn't finicky. Centre a
                // sprite-sized box on the hitbox, rising from its feet.
                let (sprite, _) = c.sprite_options();
                let (sw, sh) = (sprite.w as i16 * 8, sprite.h as i16 * 8);
                let hb = c.hitbox();
                let body = Hitbox::new(hb.x + hb.w / 2 - sw / 2, hb.y + hb.h - sh, sw, sh);
                interact_hitbox.touches(body)
            });
            if pettable && let Some(def) = ctx.scenes.get_cutscene("pet_dog").cloned() {
                let cutscene = Cutscene::launch(&def, ctx, self);
                self.cutscene.push(cutscene);
            }
        }

        self.camera.center_on(
            self.player_ref().pos.x + 4,
            self.player_ref().pos.y - 2,
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
        // The bag overlay: drawn last, over the just-composited world, so it
        // reads as an inventory on top of the (frozen) world rather than its own
        // screen. `InventoryUi::draw` builds its panel on the FG layer and
        // composites BG+FG into the output itself, so the world stays visible
        // beneath it.
        if self.inventory_ui.is_open() {
            self.inventory_ui.draw(ctx);
        }
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
        use crate::draw_state::LayerId::*;

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

        // Every shell — leaders and their nested companions — draws through the
        // one Y-sorted list, so the dog sorts against the player and the map by
        // its feet line like any other entity (no separate companion pass).
        sprites.extend(self.all_shells().map(|s| s.draw_params(camera_pos)));

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
            self.dialogue.draw_dialogue_box(
                ctx.draw,
                BG,
                ctx.font,
                ctx.save.small_text_on,
                &string,
                true,
            );
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
            print_to_with_font(ctx.font, 
                ctx.draw.rgba(BG),
                &format!("Player: {:#?}", self.player_ref()),
                0,
                0,
                c11,
                opts.clone(),
            );
            print_to_with_font(ctx.font, 
                ctx.draw.rgba(BG),
                &format!("Camera: {camera_pos:#?}"),
                74,
                0,
                c11,
                opts,
            );
        }
        editor.draw_at(
            ctx.draw,
            ctx.input,
            ctx.font,
            &self.current_map,
            ctx.maps,
            camera_pos,
        );
    }

    /// Composite the finished walkaround frame (left in `draw_state.rgba(BG)` by
    /// [`draw_world`](Self::draw_world)) onto `output`. Kept separate from the
    /// world build so the caller chooses the destination surface — the main
    /// window uses `system.output_image()`, an extra view its own framebuffer.
    pub fn composite_into(
        draw_state: &mut crate::draw_state::DrawState,
        output: &mut crate::render::image::RgbaImage,
    ) {
        use crate::draw_state::LayerId::*;
        use crate::render::image::RgbaImage;
        use crate::render::{Canvas, EdgePolicy, Transform};

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
    use crate::world::map::{LayerInfo, MapObject, Trigger, Warp};
    use crate::geometry::Hitbox;
    use crate::platform::test_console::TestConsole;

    /// Spawn a built-in critter from the embedded data (the data is the only
    /// source of creatures now).
    fn critter() -> Shell {
        crate::data::eggdata::Presets::builtin()
            .spawn(&PresetId::critter())
            .unwrap()
    }

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

        assert_eq!(
            walk.inside_objects,
            vec![false, false],
            "latch sized + cleared"
        );
        assert!(
            walk.pending_warp.is_none(),
            "pending warp dropped on map load"
        );
    }

    /// `load_map` swaps non-player entities per map: the leaving map's creatures
    /// park under its name and the entering map's are restored, while the player
    /// (`entities[0]`) travels. Creatures spawned on one map no longer bleed into
    /// the next, and a same-map reload doesn't disturb the live list.
    #[test]
    fn load_map_swaps_per_map_entities() {
        let mut console = TestConsole::new();
        let mut walk = WalkaroundState::new();
        let named = |name: &str| MapInfo {
            source: name.to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            ..MapInfo::default()
        };
        // Identify creatures by position so a restore proves identity, not count.
        let creature_xs =
            |w: &WalkaroundState| w.entities[1..].iter().map(|s| s.pos.x).collect::<Vec<_>>();

        // On map "a": mark the player, then spawn two creatures at known spots.
        walk.load_map(&mut console, named("a"));
        walk.player().pos = Vec2::new(42, 7);
        walk.spawn_shell(critter().with_pos(Vec2::new(11, 0)));
        walk.spawn_shell(critter().with_pos(Vec2::new(22, 0)));
        assert_eq!(creature_xs(&walk), vec![11, 22]);

        // Warp to "b": a's creatures park; only the player travels.
        walk.load_map(&mut console, named("b"));
        assert_eq!(walk.entities.len(), 1, "first-visit b has no creatures");
        assert_eq!(walk.player().pos, Vec2::new(42, 7), "player carried across");

        // Spawn one creature on "b".
        walk.spawn_shell(critter().with_pos(Vec2::new(99, 0)));

        // Back to "a": its two creatures return in order; b's parks.
        walk.load_map(&mut console, named("a"));
        assert_eq!(creature_xs(&walk), vec![11, 22], "a's creatures restored");

        // Return to "b": its single creature is still parked and restored.
        walk.load_map(&mut console, named("b"));
        assert_eq!(creature_xs(&walk), vec![99], "b's creature restored");

        // A same-map reload leaves the live entities untouched (no spurious swap).
        walk.load_map(&mut console, named("b"));
        assert_eq!(creature_xs(&walk), vec![99], "same-map reload is a no-op");
    }

    /// `save()` snapshots per-map creatures into the save: the current map's live
    /// non-player entities under its name, plus the already-parked maps.
    #[test]
    fn save_gathers_per_map_entities() {
        let mut console = TestConsole::new();
        let mut walk = WalkaroundState::new();
        let mut save = SaveData::default();
        let town = MapInfo {
            source: "town".to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            ..MapInfo::default()
        };
        walk.load_map(&mut console, town);
        walk.spawn_shell(critter().with_pos(Vec2::new(5, 0)));
        walk.spawn_shell(critter().with_pos(Vec2::new(6, 0)));
        // A different map already has a parked creature.
        walk.map_entities.insert(
            "field".to_string(),
            vec![critter().with_pos(Vec2::new(7, 0))],
        );

        walk.save("town", &mut save);
        // The current map's live creatures are captured under its name...
        let town_xs: Vec<i16> = save.map_entities["town"].iter().map(|s| s.pos.x).collect();
        assert_eq!(town_xs, vec![5, 6]);
        // ...and the already-parked map rides along untouched.
        assert_eq!(save.map_entities["field"].len(), 1);
        assert_eq!(save.map_entities["field"][0].pos.x, 7);
    }

    /// The whole player entity survives a save → load round-trip, including
    /// through JSON: its position and its nested companion (the dog) come back,
    /// with the dog's skipped `sprites`/`interaction` rebuilt (pettable again).
    /// Guards against the "companions vanish on reload" gap.
    #[test]
    fn player_entity_round_trips_through_save_and_load() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.player().pos = Vec2::new(91, 73);

        // Summon the dog onto the player.
        with_ctx(&mut console, &mut parts, |ctx| {
            let mut inventory = Inventory {
                items: [const { None }; 8],
            };
            walk.execute_interact_fn(
                &InteractFn::ToggleDog,
                ctx.system,
                &mut inventory,
                ctx.presets,
            );
        });
        assert_eq!(walk.player_ref().companions.len(), 1, "dog summoned");

        // Save captures the whole player; a JSON round-trip proves the derived
        // sprites / interaction don't need to serialise.
        let mut save = SaveData::default();
        walk.save("bedroom", &mut save);
        let saved = save.player.as_ref().expect("player saved");
        assert_eq!(saved.pos, Vec2::new(91, 73), "player position saved");
        assert_eq!(saved.companions.len(), 1, "dog saved with the player");
        let json = serde_json::to_string(&save).expect("serialise save");
        parts.save = serde_json::from_str(&json).expect("deserialise save");

        // A fresh walkaround loads it back: position and a pettable dog.
        let mut reloaded = WalkaroundState::new();
        with_ctx(&mut console, &mut parts, |ctx| reloaded.load_pmem(ctx));
        assert_eq!(reloaded.player_ref().pos, Vec2::new(91, 73), "position restored");
        let dogs = &reloaded.player_ref().companions;
        assert_eq!(dogs.len(), 1, "dog restored on load");
        assert_eq!(dogs[0].preset, PresetId::dog());
        assert!(dogs[0].interaction.is_some(), "restored dog is pettable");
    }

    /// Interacting with a removable object consumes it: `take_object` records it
    /// in the save's `taken` set by stable id (so a later `load_map_by_name`
    /// filters it) and drops it from the live map at once, keeping the edge latch
    /// aligned. A non-removable object is left untouched.
    #[test]
    fn take_object_records_and_vanishes_removable() {
        let mut console = TestConsole::new();
        let mut walk = WalkaroundState::new();
        let mut save = SaveData::default();

        // A named map: a removable pickup (stable id 5) then a plain sign.
        let pickup = MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "key")
            .with_id(Some(5))
            .with_removable(true);
        let sign = MapObject::dialogue(Hitbox::new(8, 0, 8, 8), "sign");
        let map = MapInfo {
            source: "town".to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            objects: vec![pickup, sign],
            ..MapInfo::default()
        };
        walk.load_map(&mut console, map);
        assert_eq!(walk.inside_objects.len(), 2);

        // Consume the pickup: recorded taken by map#id, and gone from the map.
        walk.take_object(0, &mut save);
        assert!(save.is_taken("town", 5), "pickup recorded under map#id");
        assert_eq!(walk.current_map.objects.len(), 1, "pickup vanished now");
        assert_eq!(walk.inside_objects.len(), 1, "edge latch stays aligned");
        assert_eq!(
            walk.current_map.objects[0].hitbox.x, 8,
            "the plain sign is what remains"
        );

        // The remaining (non-removable) object is a no-op for take_object.
        walk.take_object(0, &mut save);
        assert_eq!(walk.current_map.objects.len(), 1, "non-removable untouched");
        assert_eq!(save.taken.len(), 1, "no spurious taken entry");
    }

    /// `sync_map_animations` mirrors live editor edits into the cached object
    /// animations: it patches frames in place (keeping the playback cursor) and
    /// rebuilds only when the set of sprited objects changes.
    #[test]
    fn sync_map_animations_reflects_live_edits() {
        use crate::world::animation::AnimFrame;
        let frame = |id: u16| AnimFrame {
            spr_id: id,
            ..AnimFrame::default()
        };

        let mut walk = WalkaroundState::new();
        walk.current_map = map_with_objects(vec![
            MapObject::dialogue(Hitbox::new(0, 0, 8, 8), "k").with_sprite(vec![frame(5)]),
            MapObject::dialogue(Hitbox::new(8, 0, 8, 8), "j"), // no sprite
        ]);

        // Initial sync: one animation, matching the single sprited object.
        walk.sync_map_animations();
        assert_eq!(walk.map_animations.len(), 1);
        assert_eq!(walk.map_animations[0].frames, vec![frame(5)]);

        // Advance the cursor, then a live retile: frames update in place, the
        // sprited-object count is unchanged so the cursor is preserved.
        walk.map_animations[0].tick = 1;
        walk.current_map.objects[0].sprite = Some(vec![frame(9)]);
        walk.sync_map_animations();
        assert_eq!(
            walk.map_animations.len(),
            1,
            "same count: patched, not rebuilt"
        );
        assert_eq!(
            walk.map_animations[0].frames,
            vec![frame(9)],
            "frames synced"
        );
        assert_eq!(walk.map_animations[0].tick, 1, "playback cursor preserved");

        // Giving the second object a sprite changes the count -> rebuild to two.
        walk.current_map.objects[1].sprite = Some(vec![frame(2)]);
        walk.sync_map_animations();
        assert_eq!(walk.map_animations.len(), 2, "count change rebuilds");
        assert_eq!(walk.map_animations[1].frames, vec![frame(2)]);
    }

    /// `execute_interact_fn(GiveItem)` slots the item key into the first free
    /// slot of the live inventory; an inventory with no free slot is a graceful
    /// no-op (no panic, no items lost).
    #[test]
    fn give_item_adds_to_inventory_and_handles_full() {
        use crate::world::interact::InteractFn;

        let mut console = TestConsole::new();
        let mut walk = WalkaroundState::new();

        // Start empty so the grant lands in slot 0 deterministically.
        let mut inventory = Inventory {
            items: [const { None }; 8],
        };
        let give = InteractFn::GiveItem("ff".to_string());
        let presets = crate::data::eggdata::Presets::builtin();
        assert!(
            walk.execute_interact_fn(&give, &mut console, &mut inventory, &presets)
                .is_none()
        );
        assert_eq!(
            inventory.get(0),
            Some("ff"),
            "item granted to first free slot"
        );

        // Fill the rest, then a further grant on a full inventory changes nothing.
        for slot in inventory.items.iter_mut() {
            *slot = Some("lm".to_string());
        }
        let before = inventory.to_save();
        walk.execute_interact_fn(
            &InteractFn::GiveItem("chegg".to_string()),
            &mut console,
            &mut inventory,
            &presets,
        );
        assert_eq!(
            inventory.to_save(),
            before,
            "full inventory: grant dropped, nothing lost"
        );
    }

    /// `EntityId` resolves the three addressing modes against the live tree: the
    /// player (`entities[0]`), a player companion by slot, and a map creature by
    /// its `id` — with misses (absent id / out-of-range slot) returning `None`.
    #[test]
    fn entity_id_resolves_player_companion_and_id() {
        let mut walk = WalkaroundState::new();
        let mut dog = Shell::default();
        dog.move_mode = MoveMode::Companion { slot: 0 };
        dog.preset = PresetId::dog();
        walk.player().companions.push(dog);
        let mut critter = Shell::default();
        critter.id = Some("critter_a".to_string());
        walk.entities.push(critter);

        assert_eq!(
            walk.resolve(&EntityId::Player).unwrap().move_mode,
            MoveMode::Player,
        );
        assert_eq!(
            walk.resolve(&EntityId::PlayerCompanion(0)).unwrap().preset,
            PresetId::dog(),
        );
        assert_eq!(
            walk.resolve(&EntityId::Id("critter_a".into()))
                .unwrap()
                .id
                .as_deref(),
            Some("critter_a"),
        );
        assert!(walk.resolve(&EntityId::Id("missing".into())).is_none());
        assert!(walk.resolve(&EntityId::PlayerCompanion(5)).is_none());
    }

    /// All the owned game-data a [`Ctx`] borrows, built from the embedded
    /// defaults so a `step` can run end-to-end in a unit test. Kept as one struct
    /// so a test owns the backing storage while `with_ctx` hands out the
    /// short-lived `Ctx` of borrows into it.
    struct CtxParts {
        draw: crate::draw_state::DrawState,
        /// This frame's input, threaded into the `Ctx` — a test injects presses
        /// here (button edges, held movement) instead of into the console.
        input: crate::platform::EggInput,
        maps: crate::world::map::MapStore,
        rng: crate::rand::Lcg64Xsh32,
        script: crate::data::script::Script,
        scenes: crate::data::scene::SceneFile,
        save: SaveData,
        items: GameItems,
        presets: crate::data::eggdata::Presets,
        font: crate::render::Font,
    }
    impl CtxParts {
        fn new() -> Self {
            Self {
                draw: crate::draw_state::DrawState::default(),
                input: crate::platform::EggInput::new(),
                maps: crate::world::map::MapStore::default(),
                rng: crate::rand::Lcg64Xsh32::default(),
                script: crate::data::script::Script::new(),
                scenes: crate::data::scene::SceneFile::default(),
                save: SaveData::default(),
                items: GameItems::default(),
                presets: crate::data::eggdata::Presets::builtin(),
                font: crate::render::Font::blank(),
            }
        }
    }

    /// Run `f` with a live [`Ctx`] split-borrowing `console` + `parts`, the same
    /// shape `EggState::step_mode` builds. Lets a test drive the real `step`
    /// (input, overlay, world sim) rather than only the isolated helpers.
    fn with_ctx<R>(
        console: &mut TestConsole,
        parts: &mut CtxParts,
        f: impl FnOnce(&mut Ctx<TestConsole>) -> R,
    ) -> R {
        let mut ctx = Ctx {
            draw: &mut parts.draw,
            system: console,
            input: &parts.input,
            maps: &mut parts.maps,
            rng: &mut parts.rng,
            script: &parts.script,
            scenes: &parts.scenes,
            save: &mut parts.save,
            items: &parts.items,
            presets: &parts.presets,
            font: &parts.font,
        };
        f(&mut ctx)
    }

    /// Set a one-frame rising edge on the primary controller's B button (down
    /// now, up last frame), so a single `step` sees `just_pressed(pad.b)` once.
    fn press_b(parts: &mut CtxParts) {
        parts.input.controllers[0].b = [true, false];
    }

    /// Pressing the bag button opens the inventory overlay in place: `step`
    /// requests no mode change (the bag is no longer a `GameMode`) and the
    /// overlay reads open afterwards. The next frame the overlay guard takes over.
    #[test]
    fn bag_button_opens_overlay_in_place() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map_with_objects(vec![]));
        // The bag starts closed for this test (new walkarounds open it on
        // PageSelect, so close it explicitly to prove the B-press is what opens).
        walk.inventory_ui.state = InventoryUiState::Close;
        assert!(!walk.inventory_ui.is_open(), "bag starts closed");

        press_b(&mut parts);
        let trans = with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx));

        assert_eq!(trans, None, "opening the bag is not a mode transition");
        assert!(walk.inventory_ui.is_open(), "the bag overlay is now open");
    }

    /// While the bag overlay is open it freezes the walkaround sim: a `step` with
    /// a movement input held neither moves the player nor advances a wandering
    /// creature — the overlay guard early-returns before the entity loop. (The
    /// bag pauses, so the world is skipped entirely that frame.)
    #[test]
    fn open_bag_freezes_the_world() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map_with_objects(vec![]));

        // Park the player at a known spot and add a creature that would wander.
        walk.player().pos = Vec2::new(40, 40);
        let mut creature = critter();
        creature.move_mode = MoveMode::Wander;
        creature.pos = Vec2::new(80, 40);
        walk.spawn_shell(creature);

        // Open the bag, then hold "right" and step: a pausing overlay is up, so
        // nothing in the world should move.
        walk.inventory_ui.open(&mut console);
        assert!(walk.inventory_ui.is_open());
        parts.input.controllers[0].right = [true, true];

        let player_before = walk.player_ref().pos;
        let creature_before = walk.entities[1].pos;
        let trans = with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx));

        assert_eq!(trans, None, "browsing the open bag drives no transition");
        assert_eq!(
            walk.player_ref().pos,
            player_before,
            "player frozen while the bag is open"
        );
        assert_eq!(
            walk.entities[1].pos, creature_before,
            "creatures frozen while the bag is open"
        );
    }

    /// Closing the bag (its state goes to `Close`) lets the world resume: with
    /// the overlay closed, the next `step` runs the world sim again, so a held
    /// movement input moves the player.
    #[test]
    fn closing_bag_resumes_the_world() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map_with_objects(vec![]));
        walk.player().pos = Vec2::new(40, 40);

        // Bag closed -> overlay guard is inert; a held "right" moves the player.
        walk.inventory_ui.state = InventoryUiState::Close;
        parts.input.controllers[0].right = [true, true];

        let before = walk.player_ref().pos;
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx));

        assert!(
            walk.player_ref().pos.x > before.x,
            "with the bag closed the world resumes and the player moves"
        );
    }

    /// `step_inventory` translates the overlay's `Options` state into the
    /// `InventoryOptions` mode transition — the one case where browsing the bag
    /// leaves the walkaround (to the shared options menu).
    #[test]
    fn step_inventory_requests_options_menu() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        // Land directly on the Options page; `step_inventory`'s own
        // `inventory_ui.step` then leaves it there (no input this frame), so the
        // state->mode mapping is what we read.
        walk.inventory_ui.state = InventoryUiState::Options;
        let trans = with_ctx(&mut console, &mut parts, |ctx| walk.step_inventory(ctx));

        assert_eq!(
            trans,
            Some(GameMode::InventoryOptions),
            "Options state asks to open the options menu"
        );
    }

    /// The bag -> Options -> back round trip lands back in the walkaround with
    /// the bag still open. The menu's "back to bag" handler sets the overlay
    /// state (PageSelect) and returns `Walkaround` — there is no `Inventory` mode
    /// anymore, so resuming the walkaround re-draws the still-open overlay.
    #[test]
    fn menu_back_to_bag_resumes_walkaround_with_bag_open() {
        use crate::gamestate::MenuState;

        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        // Simulate having left the open bag for its options menu: the bag is
        // open, and we drive the menu's back-to-bag entry (index 0 of the
        // inventory-options menu) exactly as `step_mode` would.
        walk.inventory_ui.open(&mut console);
        let mut menu = MenuState::inventory_options();
        let trans = with_ctx(&mut console, &mut parts, |ctx| {
            menu.click(Some(0), ctx, &mut walk)
        });

        assert_eq!(
            trans,
            Some(GameMode::Walkaround),
            "back-to-bag resumes the walkaround, not a defunct Inventory mode"
        );
        assert!(
            walk.inventory_ui.is_open(),
            "the bag overlay stays open after returning from its options menu"
        );
        assert!(
            matches!(
                walk.inventory_ui.state,
                InventoryUiState::PageSelect(2)
            ),
            "and reopens on its options page"
        );
    }
}
