use std::collections::BTreeMap;

use crate::Ctx;
use crate::data::save::SaveData;
use crate::data::scene::CutsceneDef;
use crate::data::sound;
use crate::debug::DebugInfo;
use crate::draw_state::BgColour;
use crate::geometry::{Collider, Hitbox, Vec2};
use crate::platform::{ConsoleApi, ConsoleHelper, ScanCode, dpad_delta, just_pressed, pressed};
use crate::draw_state::DrawParams;
use crate::render::{PrintOptions, print_to_with_font};
use crate::ui::dialogue::Dialogue;
use crate::world::animation::Animation;
use crate::world::camera::{Camera, Shake};
use crate::world::interact::{InteractFn, Interaction};
use crate::world::map::{Axis, MapInfo, MapObject, ObjectEffect, Trigger, map_by_name};
use crate::world::particles::{Particle, ParticleDraw, ParticleList};
use crate::world::player::{EntityId, MoveMode, PresetId, Shell};
use crate::gamestate::GameMode;
use log::info;

use self::cutscene::Cutscene;
use self::inventory::{Inventory, InventoryUi, InventoryUiState};
use crate::data::eggdata::{GameItems, UseDef};

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
    /// them instead of bleeding into the next one. Persisted through the save
    /// ([`SaveData::map_entities`](crate::data::save::SaveData::map_entities)),
    /// written in [`save`](Self::save) and restored in [`load_pmem`](Self::load_pmem).
    map_entities: BTreeMap<String, Vec<Shell>>,
    pub map_animations: Vec<Animation>,
    pub camera: Camera,
    pub current_map: MapInfo,
    pub dialogue: Dialogue,
    /// A dialogue-`#shake` in flight: armed from
    /// [`Dialogue::pending_shake`], advanced and applied in
    /// [`center_with_shake`](Self::center_with_shake). Distinct from a
    /// cutscene's own `shake`-verb state, which lives on the scene (and pauses
    /// with a parent scene); the two compose additively if both run.
    shake: Option<Shake>,
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
    /// Monotonic counter minting the collision-proof ids of `spawn`ed cutscene
    /// actors (see [`mint_spawn_id`](Self::mint_spawn_id)). Walkaround-wide, not
    /// per-cutscene, so sibling scenes on the stack never mint the same id; it
    /// clones with the world, so the scrubber's re-sim stays deterministic.
    spawn_counter: u64,
    /// Art/animation override for the background colour. `None` (the norm) uses
    /// the map's own [`MapInfo::bg_colour`]; `Some` wins over it — the seam a
    /// cutscene or effect drives to re-colour the backdrop at runtime. Cleared
    /// on map load so a warp never carries a stale override.
    pub bg_colour: Option<BgColour>,
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
    /// Armed by [`load_map`](Self::load_map): the just-loaded map hasn't yet had
    /// its map-enter hook scanned. [`step`](Self::step) consumes it once — after
    /// the cutscene/editor/overlay guards — to launch the first `Enter`-triggered
    /// cutscene whose [`Gate`](crate::world::map::Gate) allows it (see
    /// [`launch_map_enter`](Self::launch_map_enter)). Because every entry path
    /// (warp, save-load, debug jump, initial spawn) funnels through `load_map`,
    /// this fires the hook exactly once per load however the map was entered.
    pending_enter_scan: bool,
    /// The day/night state currently painted into the palette, or `None` before
    /// the first paint. [`step`](Self::step) reconciles it against the
    /// [`IS_NIGHT_FLAG`](crate::data::save::IS_NIGHT_FLAG) save flag each frame
    /// (see [`sync_day_night_palette`](Self::sync_day_night_palette)) and repaints
    /// only on a change — so a dialogue/gate/cutscene flip of the flag swaps
    /// day↔night live, while a one-off debug palette (Digit8's B/W) is left alone.
    day_night_shown: Option<bool>,
}
impl Default for WalkaroundState {
    fn default() -> Self {
        Self::new()
    }
}

/// A precomputed re-simulation of an armed cutscene: its length, where each
/// authored beat begins, and a **snapshot ladder** — cloned worlds captured at a
/// fixed frame stride — so the scrubber can seek to any frame by replaying only
/// from the nearest snapshot at or below it, instead of from frame 0 every time
/// (which makes dragging quadratic in scene length).
///
/// Built once by [`WalkaroundState::replay_cutscene`] from the armed base world.
/// That snapshot never mutates for a scrubber session's lifetime (every seek
/// re-sims a clone), so the ladder can't serve a stale frame — a change to the
/// scene, its source text, or the base world reopens the scrubber, rebuilding
/// the ladder from scratch.
pub(crate) struct CutsceneReplay {
    /// Total frames the scene runs (one per
    /// [`play_cutscene`](WalkaroundState::play_cutscene) step).
    pub total: usize,
    /// Start frame of each authored beat, in play order: the first frame at which
    /// a new content step becomes the active frame-consuming beat. `beats[0]` is
    /// always `0` (the opening pose); instant steps (sound/flag/…) fold into the
    /// next visible beat and get no marker of their own.
    pub beats: Vec<usize>,
    /// Frames between consecutive keyframes.
    stride: usize,
    /// The snapshot ladder: `keyframes[i]` is the world at frame `i * stride`.
    /// `keyframes[0]` is the armed frame-0 snapshot; the last rung is the largest
    /// multiple of `stride` not past [`total`](Self::total). Never empty.
    keyframes: Vec<WalkaroundState>,
}

impl CutsceneReplay {
    /// The world at `frame`, replayed from the nearest keyframe at or below it —
    /// at most `stride - 1` steps forward, rather than `frame` steps from 0.
    /// Because the re-sim is deterministic this is identical to
    /// [`sim_cutscene_to`](WalkaroundState::sim_cutscene_to) for every frame, just
    /// cheaper; a `frame` past the scene end clamps to the last rung's tail, as
    /// the naive path does.
    pub(crate) fn seek<S: ConsoleApi>(&self, frame: usize, ctx: &mut Ctx<S>) -> WalkaroundState {
        let rung = (frame / self.stride).min(self.keyframes.len() - 1);
        self.keyframes[rung].sim_cutscene_to(frame - rung * self.stride, ctx)
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
            dialogue: Dialogue::default(),
            shake: None,
            inventory_ui: InventoryUi::new(),
            particles: ParticleList::new(),
            cutscene: Vec::new(),
            spawn_counter: 0,
            bg_colour: None,
            default_map_colliders: Vec::new(),
            inside_objects: Vec::new(),
            pending_warp: None,
            pending_enter_scan: false,
            day_night_shown: None,
        }
    }

    /// Mint a fresh, collision-proof id for a `spawn`ed cutscene actor: a reserved
    /// prefix + a walkaround-wide monotonic counter. The leading control char
    /// can't occur in an authored map-object name or eggscene token, and the
    /// counter is unique across the whole world, so a minted id never matches a
    /// pre-existing creature or another spawn — [`Cutscene::cleanup`] then removes
    /// only the exact shells a scene spawned, and the author name resolves to this
    /// id through the scene's actor table (not the ambiguous name itself).
    pub(crate) fn mint_spawn_id(&mut self) -> String {
        let id = format!("\u{1}cutscene-spawn:{}", self.spawn_counter);
        self.spawn_counter += 1;
        id
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

    /// The actors the map editor's path recorder can record: `(token, position)`
    /// for the player, each of its companions, and every named top-level creature
    /// on the current map. The `token` is exactly what a cutscene chain names
    /// (`player`, `companion N`, or a `Shell::id`), so the recorder can emit it
    /// verbatim; the position seeds the puppet so clicked waypoints land relative
    /// to where the actor really is. Companionless nameless creatures are skipped —
    /// a chain can't refer to them. Pushed into the editor each focused frame (it
    /// can't see this tree itself).
    pub fn recorder_actors(&self) -> Vec<(String, Vec2)> {
        let Some(player) = self.entities.first() else {
            return Vec::new();
        };
        let mut actors = vec![("player".to_string(), player.pos)];
        for (slot, companion) in player.companions.iter().enumerate() {
            actors.push((format!("companion {slot}"), companion.pos));
        }
        for entity in &self.entities[1..] {
            if let Some(id) = &entity.id {
                actors.push((id.clone(), entity.pos));
            }
        }
        actors
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
    /// explicit `camera_stick`, else auto-sized from the first sizable layer),
    /// and drop any runtime background override so the map's own colour shows
    /// (a warp can't carry a stale override; a Setup-panel bg edit isn't masked
    /// by one). A layer with no positive size (e.g. a
    /// collision mask whose pixels never loaded) is skipped so `from_map_size`'s
    /// positive-size assert can't trip; if nothing sizable remains the existing
    /// camera is kept rather than panicking. Shared by [`load_map`](Self::load_map)
    /// and the in-editor re-derive so a Setup-panel edit (camera / bg / resize)
    /// applies live, not only after a full reload.
    pub fn apply_map_framing(&mut self, system: &mut impl ConsoleApi, map_set: &MapInfo) {
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
        self.bg_colour = None;
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
        // Arm the map-enter hook scan for the next `step`: a freshly loaded map
        // gets one chance to launch its `Enter`-triggered cutscene. Set on every
        // load (warp, save-load, debug jump, initial spawn), so the hook composes
        // uniformly with each entry path; the gate + `sets` latch keep a one-shot
        // beat from replaying.
        self.pending_enter_scan = true;

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
    ///
    /// Removable pickups already consumed in this save are **not** filtered out
    /// here — they stay in the loaded map data (so the editor can still see and
    /// edit them, and a map save round-trips them). Taken pickups are instead
    /// skipped at use-time by [`object_taken`](Self::object_taken): the walk loop
    /// won't fire their interaction and the world draw won't draw their sprite.
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
    /// Centre the camera on a map-pixel point framed as a player landing there
    /// (the same +4/-2 hitbox offset the follow-camera uses), clamped to the
    /// map's bounds. Used when the editor opens a warp's destination so the
    /// landing point is framed the way gameplay shows it on arrival.
    pub fn center_camera_on(&mut self, p: Vec2, w: i32, h: i32) {
        self.camera.center_on(p.x + 4, p.y - 2, w as i16, h as i16);
    }
    /// Centre the camera on `(x, y)` — the per-frame choke point all three
    /// camera drivers route through (the cutscene focus, the warp-dialogue
    /// hold, and the normal player follow). A dialogue `#shake` banked on the
    /// box is armed here, and the running shake's offset rides on top of
    /// whatever focus was asked for. The shake also advances one frame per
    /// call — ticking here rather than in `step` keeps live play and the
    /// scrubber's re-sim (which drives `play_cutscene` directly, skipping
    /// `step`) on the same clock. Bounds still clamp, absorbing the jiggle at
    /// map edges.
    fn center_with_shake(&mut self, x: i16, y: i16, w: i16, h: i16) {
        if let Some((frames, amplitude)) = self.dialogue.pending_shake.take() {
            self.shake = Shake::begin(frames, amplitude);
        }
        let offset = self.shake.as_ref().map_or(Vec2::new(0, 0), Shake::offset);
        self.camera.center_on(x + offset.x, y + offset.y, w, h);
        Shake::tick(&mut self.shake);
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
                    system.play_sound(sound::alert_down());
                    Some("dog_relinquished")
                } else {
                    let slot = self.player_ref().companions.len();
                    let mut shell = presets.spawn(&dog).unwrap_or_default();
                    shell.move_mode = MoveMode::Companion { slot };
                    shell.interaction = Some(crate::world::player::pet_marker());
                    shell.pos = ppos;
                    self.player().companions.push(shell);
                    system.play_sound(sound::equip_obtained());
                    Some("dog_obtained")
                }
            }
            InteractFn::Note(note) => {
                system.play_sound(sound::piano().with_note(*note));
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
                system.play_sound(sound::piano().with_note(note as i32));
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
                system.play_sound(sound::pop());
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
        // Point the camera where the active scene wants it (the normal follow
        // update below is skipped during cutscenes, so this is the sole camera
        // driver while a scene plays). The default focus follows the player; a
        // `camera` step retargets to an actor or a fixed map point (glides ease
        // there inside `camera_focus`). Reading the focus off the top of the
        // stack each frame gives restore for free: a sub-scene's pop leaves the
        // parent's focus in effect, and the final pop (drained stack) lands back
        // on the player with no snap. A running `shake` jiggles whatever the
        // focus is — the player-follow default included. Bounds still clamp —
        // the centring routes through `Camera::center_on` — so a shake at a map
        // edge is absorbed rather than showing past the map.
        let focus = self
            .cutscene
            .last()
            .and_then(|cs| cs.camera_focus(self))
            .unwrap_or_else(|| {
                Vec2::new(self.player_ref().pos.x + 4, self.player_ref().pos.y - 2)
            });
        let shake = self
            .cutscene
            .last()
            .map_or(Vec2::new(0, 0), |cs| cs.shake_offset());
        self.center_with_shake(
            focus.x + shake.x,
            focus.y + shake.y,
            ctx.system.width() as i16,
            ctx.system.height() as i16,
        );
        true
    }

    /// Frame cap for the scrubber's re-simulation, so a scene that never
    /// terminates (e.g. a required move blocked forever) can't hang the editor.
    /// ~27 min at 60fps — far beyond any authored cutscene.
    pub(crate) const SCRUB_MAX_FRAMES: usize = 100_000;

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

    /// The base (bottom-of-stack) cutscene's active content step, or `None` once
    /// the stack has drained. The base scene is `cutscene[0]`: a `load` step
    /// pushes sub-cutscenes *above* it, so its own step index tracks the
    /// top-level authored beats throughout the scene — the cursor
    /// [`replay_cutscene`](Self::replay_cutscene) samples to place beat markers.
    fn base_beat(&self) -> Option<usize> {
        self.cutscene.first().map(Cutscene::active_step)
    }

    /// Re-simulate this world's armed cutscene stack to completion on a CLONE,
    /// capturing in one pass everything the scrubber needs: the total length, the
    /// start frame of each authored beat, and a snapshot ladder (a cloned world
    /// every `stride` frames) for cheap seeking. See [`CutsceneReplay`]. Leaves
    /// `self` untouched and caps at [`SCRUB_MAX_FRAMES`](Self::SCRUB_MAX_FRAMES);
    /// `stride` is clamped non-zero.
    pub(crate) fn replay_cutscene<S: ConsoleApi>(
        &self,
        stride: usize,
        ctx: &mut Ctx<S>,
    ) -> CutsceneReplay {
        let stride = stride.max(1);
        let mut world = self.clone();
        // Frame 0 is the armed snapshot: the first rung of the ladder and the
        // opening beat's start.
        let mut keyframes = vec![world.clone()];
        let mut beats = vec![0usize];
        let mut last_beat = world.base_beat();
        let mut frames = 0;
        while world.play_cutscene(ctx) {
            frames += 1;
            // A new content step became the active frame-consuming beat ⇒ a
            // boundary. The drained-stack `None` on the final cleanup frame is
            // the scene ending, not a new beat.
            let beat = world.base_beat();
            if beat.is_some() && beat != last_beat {
                beats.push(frames);
                last_beat = beat;
            }
            if frames.is_multiple_of(stride) {
                keyframes.push(world.clone());
            }
            if frames >= Self::SCRUB_MAX_FRAMES {
                break;
            }
        }
        CutsceneReplay {
            total: frames,
            beats,
            stride,
            keyframes,
        }
    }

    /// Launch `def` and arm it on this world's cutscene stack — the scrubber's
    /// snapshot setup. After this, [`play_cutscene`](Self::play_cutscene) (hence
    /// [`replay_cutscene`](Self::replay_cutscene)/[`sim_cutscene_to`](Self::sim_cutscene_to))
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
    /// [`id`](crate::world::map::MapObject::id). The object *stays* in the live
    /// map — so the editor can still show and edit it, and a map save round-trips
    /// it — but from now on [`object_taken`](Self::object_taken) skips it at
    /// use-time: the walk loop won't fire its interaction and the world draw won't
    /// draw its sprite. A no-op for a non-removable object; an id-less removable
    /// can't be persisted, so it re-appears on the next load.
    fn take_object(&mut self, i: usize, save: &mut SaveData) {
        let object = &self.current_map.objects[i];
        if !object.removable {
            return;
        }
        if let Some(id) = object.id {
            save.mark_taken(&self.current_map.source, id);
        }
    }

    /// Whether `object` (on the map named `source`) is a removable pickup already
    /// collected in `save`. Such an object is kept in the map data but skipped at
    /// use-time — the walk loop's object scan won't fire its interaction and
    /// [`draw_world`](Self::draw_world) won't draw its sprite. Only a removable
    /// object with a stable [`id`](crate::world::map::MapObject::id) can be taken;
    /// everything else (warps, plain interactions, id-less objects) reads `false`.
    fn object_taken(object: &MapObject, source: &str, save: &SaveData) -> bool {
        object.removable && object.id.is_some_and(|id| save.is_taken(source, id))
    }

    /// Apply a fired object's `sets` latch: set its [`Gate`](crate::world::map::Gate)'s
    /// `sets` flag (if any) in the save. Called at every firing site — a
    /// touch/press warp or interaction, and the map-enter hook — so the one-shot
    /// mechanism is uniform: an object gated `unless X` with `sets X` fires once,
    /// sets `X`, and its own gate then holds it off forever (persisted through the
    /// normal save flags). Idempotent; a no-op for an object with no `sets` flag.
    fn set_object_flag(object: &MapObject, save: &mut SaveData) {
        if let Some(flag) = &object.gate.sets {
            save.set_flag(flag, true);
        }
    }

    /// Launch a freshly-loaded map's one-shot map-enter cutscene, if it has one
    /// whose flag gate currently allows it. Scans this map's objects (first-wins,
    /// mirroring the walk loop's object scan) for an [`Trigger::Enter`] cutscene
    /// interaction whose [`Gate`](crate::world::map::Gate) passes against the live
    /// save, latches its `sets` flag, and pushes the launched cutscene onto the
    /// stack — [`play_cutscene`](Self::play_cutscene) drives it from the next
    /// frame. Returns whether it launched one. An unknown cutscene name logs and
    /// launches nothing, *without* latching the flag (so a fixed typo can still
    /// fire) — matching [`fire_interaction`](Self::fire_interaction). `Enter` on a
    /// non-cutscene effect is ignored here (and never fires in the touch/press
    /// scan either, since it allows neither), so a stray `Enter` warp is inert.
    fn launch_map_enter<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>) -> bool {
        let hit = self.current_map.objects.iter().position(|o| {
            o.trigger == Trigger::Enter
                && o.gate.allows(ctx.save)
                && matches!(&o.effect, ObjectEffect::Interact(Interaction::Cutscene(_)))
        });
        let Some(i) = hit else { return false };
        let ObjectEffect::Interact(Interaction::Cutscene(name)) =
            self.current_map.objects[i].effect.clone()
        else {
            unreachable!("position() only matched a cutscene interaction");
        };
        let Some(def) = ctx.get_cutscene(&name).cloned() else {
            info!("launch_map_enter: unknown cutscene {name:?}");
            return false;
        };
        Self::set_object_flag(&self.current_map.objects[i], ctx.save);
        let cutscene = Cutscene::launch(&def, ctx, self);
        self.cutscene.push(cutscene);
        true
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
        // A "Use" placed on the Use button this frame stages the item's authored
        // effect here; drain and fire it now. The bag has closed (the Use path
        // sets `Close`), so it plays in the world — the same `fire_interaction`
        // seam a map object uses, with the same borrow dance for the bag's
        // inventory (which `fire_interaction` also borrows).
        if let Some(def) = self.inventory_ui.pending_use.take() {
            let interaction = match def {
                UseDef::Dialogue(key) => Some(Interaction::Dialogue(key)),
                UseDef::Cutscene(name) => Some(Interaction::Cutscene(name)),
                UseDef::Func(name) => {
                    // Resolve the func name against the player's current hitbox
                    // (the piano's origin etc.); an unknown name logs and does
                    // nothing, like an unknown cutscene (garbage tolerance).
                    let hitbox = self.player_ref().hitbox();
                    match InteractFn::from_name(&name, None, None, None, hitbox) {
                        Some(f) => Some(Interaction::Func(f)),
                        None => {
                            info!("use effect: unknown func {name:?}");
                            None
                        }
                    }
                }
            };
            if let Some(interaction) = interaction {
                let mut inventory = std::mem::take(&mut self.inventory_ui.inventory);
                self.fire_interaction(ctx, &interaction, &mut inventory);
                self.inventory_ui.inventory = inventory;
            }
        }
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

    /// Reconcile the world palette with the day/night save flag
    /// ([`IS_NIGHT_FLAG`](crate::data::save::IS_NIGHT_FLAG)): paint
    /// [`NIGHT_16`](crate::platform::NIGHT_16) when it is set, else
    /// [`SWEETIE_16`](crate::platform::SWEETIE_16). Change-gated against
    /// [`day_night_shown`](Self::day_night_shown), so it repaints only when the
    /// flag actually flips — making a dialogue `#set is_night …`, an object gate,
    /// or a cutscene `set` step swap the world live, while leaving a one-off debug
    /// palette (Digit8's B/W) in place until day/night genuinely changes. Cheap
    /// enough to call every frame; the guard is what keeps it from stomping.
    fn sync_day_night_palette(&mut self, ctx: &mut Ctx<impl ConsoleApi>) {
        let night = ctx.save.flag(crate::data::save::IS_NIGHT_FLAG);
        if self.day_night_shown != Some(night) {
            self.day_night_shown = Some(night);
            ctx.draw.set_palette(if night {
                &crate::platform::NIGHT_16
            } else {
                &crate::platform::SWEETIE_16
            });
        }
    }

    /// Set the day/night state directly: record it in the
    /// [`IS_NIGHT_FLAG`](crate::data::save::IS_NIGHT_FLAG) save flag and repaint
    /// the world palette to match, at once. The immediate path used by the debug
    /// palette toggles (walkaround Digit6/Digit7, the debug menu's palette entries)
    /// so they still flip day↔night even from the B/W debug view; ordinary
    /// day/night changes go through the flag alone and are picked up next frame by
    /// [`sync_day_night_palette`](Self::sync_day_night_palette).
    pub fn set_day_night<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, night: bool) {
        ctx.save.set_flag(crate::data::save::IS_NIGHT_FLAG, night);
        self.day_night_shown = Some(night);
        ctx.draw.set_palette(if night {
            &crate::platform::NIGHT_16
        } else {
            &crate::platform::SWEETIE_16
        });
    }

    pub fn step<S: ConsoleApi>(&mut self, ctx: &mut Ctx<S>, editor_open: bool) -> Option<GameMode> {
        // While the primary map editor is open, mirror live frame edits into the
        // cached animations before advancing them, so the in-world sprite updates
        // too. (An extra view's editor is synced by the host — see
        // `sync_map_animations` — since its edits never pass through here.) The
        // primary `MapViewer` now lives on the host (`EggGame`); it hands us its
        // focus as `editor_open` so this sync still runs before the animations
        // advance, and the host steps the editor itself after this returns.
        if editor_open {
            self.sync_map_animations();
        }
        self.map_animations
            .iter_mut()
            .for_each(|anim| anim.advance());

        self.particles.step();

        // Keep the world's day/night palette in step with the save flag before
        // anything early-returns, so a `#set is_night …` fired from a running
        // cutscene or an open dialogue box repaints the world (next frame) too.
        self.sync_day_night_palette(ctx);

        if self.play_cutscene(ctx) {
            return None;
        }

        // When the map editor is open it takes over all input and freezes the
        // sim, so painting/typing can't move the player or trip warps/reloads.
        // Kept after `play_cutscene` so a running cutscene still wins the frame
        // over the editor. The editor step itself and every `pending_*` drain now
        // live on the host (which owns the primary `MapViewer`); here we only
        // freeze the world on the host-supplied `editor_open`.
        if editor_open {
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

        // A freshly loaded map opens its one-shot map-enter cutscene here, once,
        // before the player gets control. Placed after the cutscene guard (so it
        // never fires over a running scene — `play_cutscene` returns early while a
        // scene plays, deferring the scan until the stack drains) and after the
        // editor/overlay guards (so it doesn't fire mid-authoring). If it launches,
        // return so the scene takes over next frame instead of also stepping the
        // world this frame.
        if self.pending_enter_scan {
            self.pending_enter_scan = false;
            if self.launch_map_enter(ctx) {
                return None;
            }
        }

        if ctx.input.keyp(ScanCode::Digit5) && ctx.input.key(ScanCode::Ctrl) {
            self.load_pmem(ctx);
        }
        // Digit6/Digit7 toggle day/night through the save flag (so the change
        // persists and dialogue/gates see it); Digit8 is a one-off B/W debug view
        // the day/night sync deliberately leaves alone.
        if ctx.input.keyp(ScanCode::Digit6) {
            self.set_day_night(ctx, false);
        }
        if ctx.input.keyp(ScanCode::Digit7) {
            self.set_day_night(ctx, true);
        }
        if ctx.input.keyp(ScanCode::Digit8) {
            ctx.draw.set_palette(&crate::platform::B_W);
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;

        let pad = ctx.input.controller();
        // Captured before any handling: a choice open at frame start owns this
        // frame's A press, so the generic advance below must not also spend it.
        let choosing = self.dialogue.is_choosing();
        if !self.dialogue.is_active() {
            (dx, dy) = dpad_delta(&pad, pressed);
            if just_pressed(pad.b) {
                // Open the bag overlay in place. No mode change: the overlay
                // guard above (which `is_open` now sees as true) drives it from
                // the next frame, and `draw` composites it over the world.
                self.inventory_ui.open(ctx.system);
                return None;
            }
        } else if choosing {
            // Choice menu: up/down moves the highlight, A confirms (writing the
            // picked option's flags and resuming playback).
            let (_, ddy) = dpad_delta(&pad, just_pressed);
            if ddy != 0 {
                self.dialogue.move_choice(ddy as i32);
                ctx.system.play_sound(sound::interact());
            }
            if just_pressed(pad.a) {
                ctx.system.play_sound(sound::interact());
                // Confirm; if the choice was the last content and nothing more
                // opened, close the box.
                let opened = self.dialogue.confirm_choice(ctx.system, ctx.font, ctx.save);
                if !opened && !self.dialogue.is_choosing() && self.dialogue.current_text.is_some() {
                    self.dialogue.close();
                }
            }
        } else {
            if self.dialogue.characters == 0 {
                ctx.system.play_sound(sound::interact());
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
        if !choosing && just_pressed(pad.a) && self.dialogue.is_line_done() {
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
                    ctx.system.play_sound(sound::pop());
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
            // Also wait out an open choice menu: `is_active` covers a pending
            // `#choice` that has no current line or queued text.
            let box_closed = !self.dialogue.is_active();
            if box_closed && let Some(warp) = self.pending_warp.take() {
                self.apply_warp(ctx, warp);
            }
            self.center_with_shake(
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
                        && object.gate.allows(ctx.save)
                        && object
                            .trigger
                            .warp_fires(touched, probed, &warp.mode, manual_doors) =>
                {
                    warp_hit = Some(i);
                }
                // A removable pickup already collected in this save stays in the
                // map data (so the editor can still show it) but is skipped here —
                // its interaction never fires again. Warps are never "taken".
                ObjectEffect::Interact(_)
                    if interact_hit.is_none()
                        && object.gate.allows(ctx.save)
                        && !Self::object_taken(object, &self.current_map.source, ctx.save)
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
            // Latch the object's `sets` flag *before* firing: a warp's `fire_warp`
            // can load a new map (replacing the object vec), so read it while it's
            // still here.
            Self::set_object_flag(&self.current_map.objects[i], ctx.save);
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
            // Latch the object's `sets` flag when it fires (the one-shot side
            // effect), before running the interaction.
            Self::set_object_flag(&self.current_map.objects[i], ctx.save);
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

        self.center_with_shake(
            self.player_ref().pos.x + 4,
            self.player_ref().pos.y - 2,
            ctx.system.width() as i16,
            ctx.system.height() as i16,
        );
        None
    }
    pub fn draw<S: ConsoleApi>(&self, ctx: &mut Ctx<S>, debug_info: &DebugInfo) {
        // Draw the live world from the player-following camera, then composite
        // into the console's canonical output surface. The world build leaves its
        // result in `ctx.draw`, so the final composite is a separate step that
        // takes the output (avoiding a borrow conflict with the console).
        //
        // The primary map-editor overlay is NOT painted here: the host owns the
        // primary `MapViewer` and draws it after this returns (over the same
        // output), exactly as each extra F8 view paints its own editor after its
        // world. Keeping the editor out of here leaves world rendering with no
        // editor dependency (the scrubber ghost-draws a world with no editor at
        // all).
        self.draw_world(ctx, self.camera.pos, debug_info);
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
    /// `ctx.draw` (so an extra view can drive its own free camera without
    /// touching the live `self.camera`). Tile data comes from `ctx.maps`; the
    /// shared console is read for assets only. The finished frame is left in
    /// `ctx.draw.rgba(BG)` — call [`composite_into`](Self::composite_into)
    /// to blit it onto a surface.
    ///
    /// Draws the *world only* — no map-editor overlay. Each caller paints its
    /// own editor (or none: the scrubber's ghost frames) on top; keeping the
    /// editor out of here is a crate-extraction seam, so world rendering never
    /// depends on editor types.
    ///
    /// Engine-agnostic: it only touches `ctx.draw` (the layer canvases) and
    /// reads `ctx.system` for assets, with no knowledge of windows or the host.
    pub fn draw_world<S: ConsoleApi>(
        &self,
        ctx: &mut Ctx<S>,
        camera_pos: Vec2,
        debug_info: &DebugInfo,
    ) {
        use crate::draw_state::LayerId::*;

        let cam_x = i32::from(camera_pos.x);
        let cam_y = i32::from(camera_pos.y);

        let bg_colour = ctx
            .draw
            .resolve(self.bg_colour.unwrap_or(self.current_map.bg_colour));
        ctx.draw.rgba(BG).fill(bg_colour);

        // BG map layers
        if let Some(map) = ctx.maps.get(&self.current_map.source) {
            self.current_map
                .draw_bg_indexed(ctx.draw, BG, map, camera_pos, false);
        }

        // Particles
        self.particles.draw_indexed(ctx.draw, BG, -cam_x, -cam_y);

        // Collect sprites for drawing, each paired with its y-sort key. Entities
        // key on their feet (`DrawParams::bottom`); a sprite-plane component's
        // cells all key on the component's baseline.
        let mut sprites: Vec<(i32, DrawParams)> = Vec::new();

        // Sprite-plane components go in FIRST, so on a baseline tie (an entity's
        // feet exactly on the component's baseline) the stable sort below leaves
        // the later-pushed entity after the component — i.e. drawn in front.
        // Only components whose source layer is visible — the eye toggle flips
        // `visible` without a reload, so the filter is live here (like the flat
        // draw paths), not baked into the derive.
        for component in self.current_map.visible_sprite_components() {
            let key = component.sort_key(cam_y);
            sprites.extend(component.cell_params(cam_x, cam_y).map(|dp| (key, dp)));
        }

        let player = self.player_ref().draw_params(camera_pos);
        sprites.push((player.bottom(), player));

        // `map_animations` is 1:1 with the sprited objects in order, so zip the
        // two together and skip drawing any pickup already collected in this save
        // (`object_taken`) — the animation still advances in `step`, we just omit
        // its sprite here, so a taken pickup vanishes while staying in the data.
        for (anim, object) in self.map_animations.iter().zip(
            self.current_map
                .objects
                .iter()
                .filter(|object| object.sprite.is_some()),
        ) {
            if Self::object_taken(object, &self.current_map.source, ctx.save) {
                continue;
            }
            let hitbox = object.hitbox;
            let dp = DrawParams::new(
                anim.current_frame().spr_id.into(),
                anim.current_frame().pos.x as i32 + hitbox.x as i32 - cam_x,
                anim.current_frame().pos.y as i32 + hitbox.y as i32 - cam_y,
                anim.current_frame().options.clone(),
                anim.current_frame().outline_colour,
                anim.current_frame().palette_rotate,
            );
            sprites.push((dp.bottom(), dp));
        }

        // Every shell — leaders and their nested companions — draws through the
        // one Y-sorted list, so the dog sorts against the player and the map by
        // its feet line like any other entity (no separate companion pass).
        sprites.extend(self.all_shells().map(|s| {
            let dp = s.draw_params(camera_pos);
            (dp.bottom(), dp)
        }));

        // Stable sort by key: components pushed before entities keep entities in
        // front on a tie (see above).
        sprites.sort_by_key(|(key, _)| *key);

        // Draw sprites
        for (_, options) in sprites {
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
        // The choice menu (if any) stacks above the box; a no-op otherwise, and
        // it draws even without a prompt page (a prompt-less `#choice`).
        self.dialogue
            .draw_choice(ctx.draw, BG, ctx.font, ctx.save.small_text_on);
        if debug_info.map_info {
            // Warp hitboxes in colour 12, interaction hitboxes in colour 14;
            // the player hitbox shares the warps' colour.
            ctx.draw.stroke_hitbox(
                BG,
                self.player_ref()
                    .hitbox()
                    .offset_xy(-camera_pos.x, -camera_pos.y),
                12,
            );
            for object in self.current_map.objects.iter() {
                let colour = match object.effect {
                    ObjectEffect::Warp(_) => 12,
                    ObjectEffect::Interact(_) => 14,
                };
                ctx.draw.stroke_hitbox(
                    BG,
                    object.hitbox.offset_xy(-camera_pos.x, -camera_pos.y),
                    colour,
                );
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
    use crate::world::map::{Gate, LayerInfo, MapObject, Trigger, Warp};
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
    /// in the save's `taken` set by stable id, but the object *stays* in the live
    /// map (so the editor still shows it and a map save round-trips it). The taken
    /// state is what `object_taken` reads to skip it at use-time. A non-removable
    /// object is left untouched.
    #[test]
    fn take_object_records_but_keeps_object_in_map() {
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

        // Consume the pickup: recorded taken by map#id, but still present so the
        // editor and a map save can see it — it's `object_taken` that hides it.
        walk.take_object(0, &mut save);
        assert!(save.is_taken("town", 5), "pickup recorded under map#id");
        assert_eq!(walk.current_map.objects.len(), 2, "pickup stays in the map");
        assert_eq!(walk.inside_objects.len(), 2, "edge latch keeps every object");
        assert!(
            WalkaroundState::object_taken(&walk.current_map.objects[0], "town", &save),
            "the pickup now reads as taken (skipped at use-time)"
        );
        assert!(
            !WalkaroundState::object_taken(&walk.current_map.objects[1], "town", &save),
            "the plain sign is unaffected"
        );

        // The non-removable object is a no-op for take_object (no taken entry).
        walk.take_object(1, &mut save);
        assert_eq!(save.taken.len(), 1, "no spurious taken entry");
    }

    /// The full take → skip → un-take round trip the editor's test toggle drives,
    /// exercised through the real `step` so the two-phase object scan's use-time
    /// skip is what's under test. The pickup runs `AddCreatures(0)` (spawns one
    /// entity) so "did it fire?" is observable as a change in `entities.len()`
    /// without needing a populated script. A press-fired pickup: once recorded
    /// taken it's skipped (stays in the map, doesn't fire); un-taking its
    /// `<map>#<id>` key (what [`SaveData::toggle_taken`] does when the editor's
    /// drain runs) restores interactivity.
    #[test]
    fn taken_pickup_skips_then_untake_restores_interaction() {
        use crate::world::interact::InteractFn;

        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        // A press-fired removable pickup (id 5) that spawns a creature when it
        // fires. Its hitbox blankets the player so a facing A-press always probes
        // it, keeping the test about the taken skip, not hitbox geometry.
        let pickup = MapObject::func(Hitbox::new(0, 0, 200, 200), InteractFn::AddCreatures(0))
            .with_id(Some(5))
            .with_removable(true);
        let map = MapInfo {
            source: "town".to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            objects: vec![pickup],
            ..MapInfo::default()
        };
        walk.load_map(&mut console, map);
        walk.player().pos = Vec2::new(40, 40);
        walk.player().dir = (0, 1);
        // A fresh walkaround opens the bag overlay (PageSelect); close it so the
        // world sim — and the object scan — runs.
        walk.inventory_ui.state = InventoryUiState::Close;

        // Only the player to start; each firing of the pickup adds one creature.
        assert_eq!(walk.entities.len(), 1, "just the player at start");
        let press_a = |parts: &mut CtxParts| parts.input.controllers[0].a = [true, false];
        let release_a = |parts: &mut CtxParts| parts.input.controllers[0].a = [false, false];

        // A rising A-edge fires the pickup: one creature spawns and the pickup is
        // recorded taken. The object stays in the map (still one object).
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert!(parts.save.is_taken("town", 5), "pickup recorded taken");
        assert_eq!(walk.current_map.objects.len(), 1, "object kept in the map");
        assert_eq!(walk.entities.len(), 2, "the pickup fired once");

        // Press A again while taken: the pickup is skipped, so no creature spawns.
        release_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(
            walk.entities.len(),
            2,
            "a taken pickup's interaction no longer fires"
        );

        // Un-take it (the editor's toggle, drained into the save), then press A:
        // interactivity is restored and it fires again (another creature spawns).
        parts.save.toggle_taken(&SaveData::taken_key("town", 5));
        assert!(!parts.save.is_taken("town", 5), "un-taken");
        release_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(
            walk.entities.len(),
            3,
            "un-taking restores the pickup's interaction"
        );
    }

    /// An interaction's flag [`Gate`] decides *whether* it fires, orthogonally to
    /// its trigger. A press-fired `AddCreatures(0)` gated `if has_key` is blocked
    /// while that flag is clear (nothing spawns) and fires once it's set —
    /// exercised through the real `step`, so the object-scan gate check is what's
    /// under test. Observability is the same trick as the taken-pickup test: a
    /// spawn shows up in `entities.len()`.
    #[test]
    fn gated_interaction_blocks_then_allows() {
        use crate::world::interact::InteractFn;

        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        // A press-fired spawner whose hitbox blankets the player (so a facing
        // A-press always probes it), gated `if has_key`.
        let gated = MapObject::func(Hitbox::new(0, 0, 200, 200), InteractFn::AddCreatures(0))
            .with_gate(Gate {
                if_flag: Some("has_key".into()),
                ..Gate::default()
            });
        let map = MapInfo {
            source: "town".to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            objects: vec![gated],
            ..MapInfo::default()
        };
        walk.load_map(&mut console, map);
        walk.player().pos = Vec2::new(40, 40);
        walk.player().dir = (0, 1);
        walk.inventory_ui.state = InventoryUiState::Close;

        let press_a = |parts: &mut CtxParts| parts.input.controllers[0].a = [true, false];
        let release_a = |parts: &mut CtxParts| parts.input.controllers[0].a = [false, false];

        // Flag clear: the gate blocks the interaction — a press spawns nothing.
        assert_eq!(walk.entities.len(), 1, "just the player at start");
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(walk.entities.len(), 1, "gate blocks while has_key is clear");

        // Set the flag: the gate now allows it, and a fresh press fires it once.
        parts.save.set_flag("has_key", true);
        release_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(walk.entities.len(), 2, "gate allows once has_key is set");
    }

    /// A one-shot object (`unless done` + `sets done`) fires once, latches its
    /// flag, and its own gate then holds it off — and the flag persists across a
    /// JSON save round-trip so it stays blocked on a fresh load. The whole loop:
    /// fire → set → persist → gate-blocks. Fires an observable `AddCreatures(0)`.
    #[test]
    fn one_shot_object_fires_once_and_persists() {
        use crate::world::interact::InteractFn;

        let one_shot = || {
            MapObject::func(Hitbox::new(0, 0, 200, 200), InteractFn::AddCreatures(0)).with_gate(
                Gate {
                    unless_flag: Some("done".into()),
                    sets: Some("done".into()),
                    ..Gate::default()
                },
            )
        };
        let map = || MapInfo {
            source: "town".to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            objects: vec![one_shot()],
            ..MapInfo::default()
        };

        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map());
        walk.player().pos = Vec2::new(40, 40);
        walk.player().dir = (0, 1);
        walk.inventory_ui.state = InventoryUiState::Close;

        let press_a = |parts: &mut CtxParts| parts.input.controllers[0].a = [true, false];
        let release_a = |parts: &mut CtxParts| parts.input.controllers[0].a = [false, false];

        // First press: fires once, spawns a creature, and latches `done`.
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(walk.entities.len(), 2, "one-shot fires the first time");
        assert!(parts.save.flag("done"), "firing latched its `sets` flag");

        // Press again: its own `unless done` gate now blocks it (fired once).
        release_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(walk.entities.len(), 2, "the latch blocks a second firing");

        // The flag persists across a JSON save round-trip.
        let json = serde_json::to_string(&parts.save).expect("serialise save");
        parts.save = serde_json::from_str(&json).expect("deserialise save");
        assert!(parts.save.flag("done"), "one-shot flag survives save/load");

        // A fresh walkaround loads the same map with the persisted save: the gate
        // is still closed, so even a valid press doesn't re-fire it.
        let mut reloaded = WalkaroundState::new();
        reloaded.load_map(&mut console, map());
        reloaded.player().pos = Vec2::new(40, 40);
        reloaded.player().dir = (0, 1);
        reloaded.inventory_ui.state = InventoryUiState::Close;
        release_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| reloaded.step(ctx, false));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| reloaded.step(ctx, false));
        assert_eq!(
            reloaded.entities.len(),
            1,
            "the persisted one-shot flag keeps it blocked on reload"
        );
    }

    /// The map-enter hook: an [`Trigger::Enter`] cutscene object launches its
    /// scene when the map *loads* (not on any player contact), once, subject to
    /// the flag gate. A one-shot enter beat (`unless seen` + `sets seen`) fires on
    /// the first load and latches `seen`; a re-load then finds the gate closed and
    /// launches nothing — the beat plays exactly once. It composes with every
    /// entry path because they all funnel through `load_map` (which arms the scan).
    #[test]
    fn map_enter_cutscene_fires_once_when_gated() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        // Register a trivial cutscene so the enter hook has something to launch.
        parts.scenes.cutscenes.insert(
            "intro".to_string(),
            crate::data::scene::CutsceneDef::default(),
        );

        let enter = MapObject::new(
            Hitbox::new(0, 0, 8, 8),
            ObjectEffect::Interact(Interaction::Cutscene("intro".to_string())),
            None,
        )
        .with_trigger(Trigger::Enter)
        .with_gate(Gate {
            unless_flag: Some("seen".into()),
            sets: Some("seen".into()),
            ..Gate::default()
        });
        let map = || MapInfo {
            source: "town".to_string(),
            layers: vec![LayerInfo::DEFAULT_LAYER],
            objects: vec![enter.clone()],
            ..MapInfo::default()
        };
        walk.load_map(&mut console, map());
        walk.inventory_ui.state = InventoryUiState::Close;

        // First step after load: the enter scan launches the scene and latches
        // `seen` (before the player gets control).
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(walk.cutscene.len(), 1, "map-enter hook launched the cutscene");
        assert!(parts.save.flag("seen"), "enter hook latched its one-shot flag");

        // Simulate the scene finishing (drain the stack), then re-enter the map:
        // the gate is now closed, so nothing relaunches — the beat played once.
        walk.cutscene.clear();
        walk.load_map(&mut console, map());
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(walk.cutscene.len(), 0, "gated enter hook does not replay");
    }

    /// `load_map_by_name` no longer filters out already-taken removable objects:
    /// a collected pickup stays in the loaded `current_map` (skipped at use-time
    /// by `object_taken`, and written back out by the editor's `to_tmj`). Guards
    /// the load-time filter from creeping back — which would make a taken pickup
    /// invisible/uneditable in the map editor and drop it on the next map save.
    #[test]
    fn load_map_by_name_keeps_taken_object() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        // A modern map (object layer) with one removable pickup, id 5.
        let json = r#"{
            "width": 2, "height": 2,
            "tilesets": [{"firstgid": 1, "source": "tiles.tsj"}],
            "layers": [{
                "type": "objectgroup", "name": "Object Layer 1",
                "objects": [
                    {"id": 5, "x": 0, "y": 0, "width": 8, "height": 8, "type": "",
                     "properties": [
                        {"name": "description", "type": "string", "value": "key"},
                        {"name": "removable", "type": "string", "value": "true"}
                     ]}
                ]
            }]
        }"#;
        let map: crate::data::tiled::TiledMap = serde_json::from_str(json).unwrap();
        parts.maps.insert("town", map);
        // Mark the pickup already collected in this save.
        parts.save.mark_taken("town", 5);

        with_ctx(&mut console, &mut parts, |ctx| {
            walk.load_map_by_name(ctx, "town")
        });

        // The object is still in the loaded map (not filtered out), and reads as
        // taken — skipped at use-time, but present for the editor and a map save.
        assert_eq!(walk.current_map.objects.len(), 1, "taken object still loaded");
        assert!(
            WalkaroundState::object_taken(&walk.current_map.objects[0], "town", &parts.save),
            "and it reads as taken"
        );
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
        let trans = with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));

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
        let trans = with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));

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
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));

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

    /// A one-frame rising edge on the primary controller's A button, so a single
    /// bag `step` sees `just_pressed(pad.a)` (activating a button under the cursor).
    fn press_a(parts: &mut CtxParts) {
        parts.input.controllers[0].a = [true, false];
    }

    /// Placing a held item on the Drop button (cursor 9) and pressing A discards
    /// it: the item leaves the inventory, the held state clears, and the cursor
    /// returns to the grid (off the now-hidden buttons). Driven through the real
    /// `inventory_ui.step` so it exercises the A-on-button routing, not just the
    /// helper.
    #[test]
    fn bag_drop_button_removes_the_held_item() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        // Hold slot 0's item ("ff") with the cursor on the Drop button.
        walk.inventory_ui.state = InventoryUiState::Items(9, Some((0, "ff".into())));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.inventory_ui.step(ctx));

        assert_eq!(
            walk.inventory_ui.inventory.get(0),
            None,
            "the dropped item is removed from its slot",
        );
        assert!(walk.inventory_ui.pending_use.is_none(), "drop fires no use effect");
        match &walk.inventory_ui.state {
            InventoryUiState::Items(i, sel) => {
                assert!(sel.is_none(), "no longer holding after a drop");
                assert!(*i < 8, "cursor normalised back onto the grid, off the buttons");
            }
            other => panic!("still on the Items page after a drop, got {other:?}"),
        }
    }

    /// Using an item with no authored `on_use` denies: the deny path keeps the
    /// item held, the cursor on the Use button, and the bag open (nothing staged).
    #[test]
    fn bag_use_button_without_effect_denies_and_keeps_holding() {
        let mut console = TestConsole::new();
        // Default items (ff/lm/chegg) carry no `use`.
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();

        walk.inventory_ui.state = InventoryUiState::Items(8, Some((0, "ff".into())));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.inventory_ui.step(ctx));

        assert!(walk.inventory_ui.pending_use.is_none(), "no effect staged");
        assert_eq!(
            walk.inventory_ui.inventory.get(0),
            Some("ff"),
            "the item is untouched (using never consumes, and this one has no use)",
        );
        match &walk.inventory_ui.state {
            InventoryUiState::Items(i, sel) => {
                assert_eq!(*i, 8, "cursor stays on the Use button");
                assert!(sel.is_some(), "still holding after a denied use");
            }
            other => panic!("bag stays open on the Items page, got {other:?}"),
        }
    }

    /// Using an item that *has* an `on_use` stages its effect in `pending_use`,
    /// puts the held item back down (using never consumes it), and closes the bag
    /// so the effect plays out in the world.
    #[test]
    fn bag_use_button_with_effect_stages_it_and_closes() {
        use crate::data::eggdata::{GameItems, ItemDef};
        use std::collections::BTreeMap;

        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        // An item whose use fires a dialogue.
        let mut defs = BTreeMap::new();
        defs.insert(
            "potion".to_string(),
            ItemDef {
                sprite: 1,
                on_use: Some(UseDef::Dialogue("gulp".into())),
            },
        );
        parts.items = GameItems::from_data(&defs);

        let mut walk = WalkaroundState::new();
        walk.inventory_ui.inventory.items = [const { None }; 8];
        walk.inventory_ui.inventory.items[0] = Some("potion".into());
        walk.inventory_ui.state = InventoryUiState::Items(8, Some((0, "potion".into())));
        press_a(&mut parts);
        with_ctx(&mut console, &mut parts, |ctx| walk.inventory_ui.step(ctx));

        assert_eq!(
            walk.inventory_ui.pending_use,
            Some(UseDef::Dialogue("gulp".into())),
            "the item's use-effect is staged for the walkaround to fire",
        );
        assert!(
            matches!(walk.inventory_ui.state, InventoryUiState::Close),
            "the bag closes so the effect plays in the world",
        );
        assert_eq!(
            walk.inventory_ui.inventory.get(0),
            Some("potion"),
            "using does not consume the item",
        );
    }

    /// `step_inventory` drains a staged `pending_use` and fires it: a `Dialogue`
    /// use-effect opens the walkaround dialogue box (via the shared
    /// `fire_interaction` seam) the same frame, and clears `pending_use`.
    #[test]
    fn step_inventory_fires_a_staged_dialogue_use() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        // Register the dialogue the use-effect names so it resolves to messages.
        let script = crate::data::script::eggtext::parse("#dialogue gulp\n    Glug glug.\n")
            .expect("test script parses");
        parts.script.set_base(script);

        let mut walk = WalkaroundState::new();
        // The Use path has already closed the bag and staged the effect.
        walk.inventory_ui.state = InventoryUiState::Close;
        walk.inventory_ui.pending_use = Some(UseDef::Dialogue("gulp".into()));
        assert!(!walk.dialogue.is_active(), "no dialogue is up before firing");

        let trans = with_ctx(&mut console, &mut parts, |ctx| walk.step_inventory(ctx));

        assert_eq!(trans, None, "firing a use-effect drives no mode transition");
        assert!(
            walk.dialogue.is_active(),
            "the use-effect's dialogue box opened in the world",
        );
        assert!(
            walk.inventory_ui.pending_use.is_none(),
            "pending_use was drained by the fire",
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

    /// The `is_night` save flag drives the world palette through `step`: set it
    /// (as a dialogue `#set`, an object gate or a cutscene step would) and the
    /// next `step` repaints NIGHT_16; clear it and it repaints SWEETIE_16 — so
    /// flipping the flag flips day↔night live.
    #[test]
    fn is_night_flag_drives_world_palette() {
        use crate::data::save::IS_NIGHT_FLAG;
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map_with_objects(vec![]));

        // Day by default: the first step paints (and settles on) SWEETIE.
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(
            parts.draw.palettes[0][0],
            crate::platform::SWEETIE_16[0],
            "the day palette is the default"
        );

        // Setting the flag repaints night on the next step.
        parts.save.set_flag(IS_NIGHT_FLAG, true);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(
            parts.draw.palettes[0][0],
            crate::platform::NIGHT_16[0],
            "setting is_night paints the night palette"
        );

        // Clearing it repaints day again.
        parts.save.set_flag(IS_NIGHT_FLAG, false);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert_eq!(
            parts.draw.palettes[0][0],
            crate::platform::SWEETIE_16[0],
            "clearing is_night paints the day palette"
        );
    }

    /// The Digit7 debug key toggles night *through the flag* (not by painting the
    /// palette directly), so the change persists in the save and dialogue/gates
    /// can see it — and Digit6 toggles back to day the same way.
    #[test]
    fn night_debug_key_sets_flag_and_palette() {
        use crate::data::save::IS_NIGHT_FLAG;
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map_with_objects(vec![]));

        // Digit7: night. The flag is set and the palette is night. (A fresh
        // `EggInput` has no prior-frame keys, so this one press reads as an edge.)
        parts.input.press_key(ScanCode::Digit7);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert!(parts.save.flag(IS_NIGHT_FLAG), "Digit7 sets the is_night flag");
        assert_eq!(parts.draw.palettes[0][0], crate::platform::NIGHT_16[0]);

        // Digit6: back to day. Reset the input first (the host would `refresh`
        // between frames), so only Digit6 reads as pressed this step.
        parts.input = crate::platform::EggInput::new();
        parts.input.press_key(ScanCode::Digit6);
        with_ctx(&mut console, &mut parts, |ctx| walk.step(ctx, false));
        assert!(!parts.save.flag(IS_NIGHT_FLAG), "Digit6 clears the is_night flag");
        assert_eq!(parts.draw.palettes[0][0], crate::platform::SWEETIE_16[0]);
    }

    /// The background fill resolves the map's own colour — an index through the
    /// live palette, a literal RGB verbatim — and the walkaround `bg_colour`
    /// override (the runtime art/animation seam) wins over the map while set,
    /// dropping again on the next map load so a warp never carries it along.
    #[test]
    fn bg_colour_resolves_map_form_and_override() {
        use crate::render::image::Rgba;
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let mut walk = WalkaroundState::new();
        walk.load_map(&mut console, map_with_objects(vec![]));

        // Camera far into negative map space: the player lands off-canvas, so
        // the corner pixel below reads the bare background fill.
        let cam = Vec2::new(-1000, -1000);
        fn bg_pixel(
            console: &mut TestConsole,
            parts: &mut CtxParts,
            walk: &WalkaroundState,
            cam: Vec2,
        ) -> Rgba {
            use crate::draw_state::LayerId::BG;
            with_ctx(console, parts, |ctx| {
                ctx.draw.rgba(BG).fill(Rgba::TRANSPARENT);
                walk.draw_world(ctx, cam, &DebugInfo::default());
                ctx.draw.rgba(BG).get_pixel(0, 0)
            })
        }

        // Default: palette index 0, resolved through the live palette.
        assert_eq!(
            bg_pixel(&mut console, &mut parts, &walk, cam),
            Rgba::from_rgb(crate::platform::SWEETIE_16[0]),
            "an indexed background resolves through the palette"
        );

        // A literal map colour fills verbatim — no palette slot involved.
        walk.current_map.bg_colour = BgColour::Rgb([1, 2, 3]);
        assert_eq!(
            bg_pixel(&mut console, &mut parts, &walk, cam),
            Rgba::new(1, 2, 3, 255),
            "a literal map background fills verbatim"
        );

        // The runtime override wins over the map's colour while set...
        walk.bg_colour = Some(BgColour::Rgb([9, 8, 7]));
        assert_eq!(
            bg_pixel(&mut console, &mut parts, &walk, cam),
            Rgba::new(9, 8, 7, 255),
            "the walkaround override wins over the map"
        );

        // ...and the next map load drops it.
        walk.load_map(&mut console, map_with_objects(vec![]));
        assert_eq!(walk.bg_colour, None, "map load clears the override");
    }

    /// Arm `src`'s single cutscene on a fresh world at the origin, ready to
    /// replay — the setup `open_scrubber` performs before measuring.
    fn armed_scene(src: &str, console: &mut TestConsole, parts: &mut CtxParts) -> WalkaroundState {
        let def = crate::data::scene::parse(src)
            .expect("parse scene")
            .get_cutscene("t")
            .expect("cutscene t")
            .clone();
        let mut walk = WalkaroundState::new();
        walk.player().pos = Vec2::new(0, 0);
        with_ctx(console, parts, |ctx| walk.arm_cutscene(&def, ctx));
        walk
    }

    /// The snapshot ladder is a faithful seek index: for a scene long enough to
    /// span several rungs, seeking to *every* frame from the nearest rung yields
    /// exactly the world the naive replay-from-0 does — the invariant the ladder
    /// exists to preserve, made cheap.
    #[test]
    fn snapshot_ladder_seek_matches_naive_resim_at_every_frame() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        // Two sequential legs so the scene outlasts the small stride below.
        let walk = armed_scene(
            "#cutscene t\n    move\n        player: walk 40 0 in 20\n    move\n        player: walk 40 12 in 16",
            &mut console,
            &mut parts,
        );

        // A signature strong enough to catch any divergence in the replayed world.
        let sig = |w: &WalkaroundState| -> Vec<Vec2> { w.entities.iter().map(|e| e.pos).collect() };
        with_ctx(&mut console, &mut parts, |ctx| {
            let stride = 7;
            let replay = walk.replay_cutscene(stride, ctx);
            // 20 + 16 frame legs, sharing the frame the first move hands off to
            // the second (the same instant step boundary as back-to-back waits).
            assert_eq!(replay.total, 35, "20 + 16 legs sharing the hand-off frame");
            assert!(
                replay.keyframes.len() >= 5,
                "a 35-frame scene at stride 7 makes several rungs (got {})",
                replay.keyframes.len(),
            );
            for frame in 0..=replay.total + 3 {
                let via_ladder = replay.seek(frame, ctx);
                let naive = walk.sim_cutscene_to(frame, ctx);
                assert_eq!(
                    sig(&via_ladder),
                    sig(&naive),
                    "ladder seek diverges from the naive re-sim at frame {frame}",
                );
            }
        });
    }

    /// The ladder still seeks correctly when the stride exceeds the whole scene
    /// (a single rung at frame 0 — the small-scene degenerate case).
    #[test]
    fn snapshot_ladder_with_one_rung_still_seeks() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();
        let walk = armed_scene(
            "#cutscene t\n    move\n        player: walk 30 0 in 10",
            &mut console,
            &mut parts,
        );
        with_ctx(&mut console, &mut parts, |ctx| {
            let replay = walk.replay_cutscene(50, ctx);
            assert_eq!(replay.keyframes.len(), 1, "one rung: the scene is shorter than the stride");
            for frame in 0..=replay.total {
                assert_eq!(
                    replay.seek(frame, ctx).player_ref().pos,
                    walk.sim_cutscene_to(frame, ctx).player_ref().pos,
                    "single-rung seek matches naive at frame {frame}",
                );
            }
        });
    }

    /// Per-step offsets: `beats` records the start frame of each authored content
    /// step, in play order — a boundary each time a new frame-consuming step
    /// becomes active, with the opening beat pinned at 0.
    #[test]
    fn replay_reports_beat_offsets_per_content_step() {
        let mut console = TestConsole::new();
        let mut parts = CtxParts::new();

        // Two back-to-back waits: the second begins the frame the first ends.
        let waits = armed_scene("#cutscene t\n    wait 5\n    wait 7", &mut console, &mut parts);
        with_ctx(&mut console, &mut parts, |ctx| {
            let replay = waits.replay_cutscene(50, ctx);
            assert_eq!(replay.total, 11, "5 + 7, sharing the hand-off frame");
            assert_eq!(replay.beats, vec![0, 5], "wait #1 opens at 0, wait #2 at 5");
        });

        // A move then a wait: the move opens the scene, the wait begins as it ends.
        let move_wait = armed_scene(
            "#cutscene t\n    move\n        player: walk 30 0 in 10\n    wait 3",
            &mut console,
            &mut parts,
        );
        with_ctx(&mut console, &mut parts, |ctx| {
            let replay = move_wait.replay_cutscene(50, ctx);
            assert_eq!(replay.beats, vec![0, 10], "move at 0, wait at 10");
            assert_eq!(replay.total, 12, "10-frame move + 3-frame wait sharing frame 10");
        });
    }
}
