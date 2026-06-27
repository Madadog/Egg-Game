// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
//
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

use std::mem;

use serde::{Deserialize, Serialize};

use crate::data::{sound, tiled::TiledMap};
use crate::geometry::{Hitbox, Vec2};
use crate::platform::{ConsoleApi, ConsoleHelper};
use crate::rand::Lcg64Xsh32;
use crate::render::{DrawParams, Flip, SpriteOptions};
use crate::world::interact::Interaction;
use crate::world::map::{Axis, LayerInfo, MapInfo};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    #[default]
    Loop,
    LoopRange(usize, usize),
    Hold,
}
impl LoopMode {
    /// Whether this is the default (`Loop`) — the serde `skip_serializing_if`
    /// guard that keeps a default loopmode out of the authored/dumped TOML.
    fn is_default(&self) -> bool {
        matches!(self, LoopMode::Loop)
    }
    pub fn loop_index(&self, index: usize, len: usize) -> usize {
        debug_assert!(len > 0);
        match self {
            LoopMode::Loop => index % len,
            &LoopMode::LoopRange(mut start, mut end) => {
                if index > end {
                    if start > end {
                        mem::swap(&mut start, &mut end);
                    }
                    let len = end - start + 1;
                    if len == 1 {
                        end
                    } else {
                        let zeroed_index = index - (end + 1);
                        start + (zeroed_index % len)
                    }
                } else {
                    index
                }
            }
            LoopMode::Hold => index.min(len - 1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpriteAnimation {
    frames: Vec<SpriteOptions>,
    #[serde(default, skip_serializing_if = "LoopMode::is_default")]
    loopmode: LoopMode,
}

impl SpriteAnimation {
    pub fn new(frames: Vec<SpriteOptions>, loopmode: LoopMode) -> Self {
        Self { frames, loopmode }
    }
    pub fn from_sprite_frames(frames: &[SpriteOptions]) -> Self {
        Self::new(frames.to_vec(), LoopMode::default())
    }
    pub fn from_sprite_ids(ids: &[i32], w: i32, h: i32) -> Self {
        let frames: Vec<SpriteOptions> = ids
            .iter()
            .map(|id| SpriteOptions {
                id: *id,
                w,
                h,
                transparent: Some(0),
                ..SpriteOptions::default()
            })
            .collect();
        Self::from_sprite_frames(&frames)
    }
    pub fn from_base_sprite_id(id: i32, len: i32, w: i32, h: i32) -> Self {
        let frames: Vec<SpriteOptions> = (id..(id + len * w))
            .step_by(w as usize)
            .map(|id| SpriteOptions {
                id,
                w,
                h,
                transparent: Some(0),
                ..SpriteOptions::default()
            })
            .collect();
        Self::from_sprite_frames(&frames)
    }
    pub fn with_flip(mut self, flip: Flip) -> Self {
        self.frames
            .iter_mut()
            .for_each(|frame| frame.flip = flip.clone());
        self
    }
    pub fn with_x_offset(mut self, x_offset: i32) -> Self {
        self.frames
            .iter_mut()
            .for_each(|frame| frame.x_offset = x_offset);
        self
    }
    pub fn with_loopmode(self, loopmode: LoopMode) -> Self {
        Self { loopmode, ..self }
    }
    pub fn frames(&self) -> &[SpriteOptions] {
        &self.frames
    }
    pub fn get_frame(&self, i: usize) -> &SpriteOptions {
        &self.frames()[self.loopmode.loop_index(i, self.frames().len())]
    }
}

/// How a sprite set turns a shell's heading into the grid cell it faces — a
/// property of the *art*, not the entity. A mirror / front-back set wants each
/// axis read on its own; a 4-way set wants to commit to one axis so a diagonal
/// doesn't spin it around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacingPolicy {
    /// Sticky horizontal + live vertical: every row and column is meaningful
    /// (a mirror/front-back set).
    PerAxis,
    /// Commit to the axis you start moving along; a diagonal off it doesn't
    /// re-aim you, and you only switch once that axis goes idle — the natural
    /// 4-way feel (the player and the dog).
    Committed,
}

/// Walk animations for each of the eight headings, indexed by the *sign* of the
/// movement vector via a flattened 3×3 grid — `grid[(dy.signum()+1)*3 +
/// (dx.signum()+1)]`, rows up/level/down and columns left/centre/right. The
/// centre cell is the resting/idle pose. Horizontal flip is baked into each cell,
/// so a humanoid's pre-mirrored west sits in the west cell and a sideways critter
/// mirrors its whole left column. `facing` decides how a heading resolves to a
/// cell (see [`FacingPolicy`]).
///
/// This is the authored form: a preset's `walk` in `data.toml` deserialises
/// straight into one of these — the nine cells and the facing policy in full, no
/// shorthand pattern layer. Verbose on purpose (the format is GUI-emitted), so
/// what ships is exactly what the runtime reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkSprites {
    grid: [SpriteAnimation; 9],
    facing: FacingPolicy,
}
impl WalkSprites {
    pub fn dir_to_sprite(&self, dir: (i8, i8)) -> &SpriteAnimation {
        // `signum`, not an exact match, so a heading of any magnitude (e.g.
        // noclip's scaled deltas) still buckets into one of the nine cells.
        let ix = |v: i8| (v.signum() + 1) as usize;
        &self.grid[ix(dir.1) * 3 + ix(dir.0)]
    }
    /// Resolve the cell direction to display from a shell's live `dir`, sticky
    /// `sticky_dir`, and committed `axis`, per this set's [`FacingPolicy`].
    fn lookup_dir(&self, dir: (i8, i8), sticky_dir: (i8, i8), axis: FacingAxis) -> (i8, i8) {
        match self.facing {
            // Sticky horizontal holds the mirror through vertical-only moves; live
            // vertical keeps side-vs-front tracking the current heading.
            FacingPolicy::PerAxis => (sticky_dir.0, dir.1),
            // A pure cardinal along the committed axis, so a diagonal never reaches
            // the grid's diagonal cells and the facing can't flip mid-stride.
            FacingPolicy::Committed => match axis {
                FacingAxis::Horizontal => (sticky_dir.0, 0),
                FacingAxis::Vertical => (0, sticky_dir.1),
            },
        }
    }
    /// A static, unhatched egg: the single frame `524` in every one of the nine
    /// cells, per-axis facing (it never mirrors or animates). Built inline — the
    /// one walk grid still constructed in code; every other creature's grid is
    /// authored in `data.toml` and deserialised straight into a [`WalkSprites`].
    fn egg() -> Self {
        let egg = SpriteAnimation::from_sprite_ids(&[524], 1, 1);
        Self {
            grid: std::array::from_fn(|_| egg.clone()),
            facing: FacingPolicy::PerAxis,
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub enum MoveMode {
    Player,
    /// Memoryless wander: a small chance each step to re-pick a random heading,
    /// otherwise keep the current one. Used by NPC shells.
    Wander,
    /// Inert until `timer` drains, then the shell *becomes* `hatches_into` in
    /// place (keeping its position) — an egg hatching into any preset. The
    /// `PresetId` makes this (and so `MoveMode`) no longer `Copy`.
    Egg {
        timer: Timer,
        hatches_into: PresetId,
    },
    /// Dwell wander: commit to a random heading for a spell, idle for a spell,
    /// repeat — the critter gait (see [`CreatureState`]).
    Amble(CreatureState),
    /// Follows its leader: each step it snaps onto a sample of the leader's
    /// [`trail`](Shell::trail) (the breadcrumb buffer), `slot` picking which
    /// sample (its place in the leader's `companions`). Driven by the leader,
    /// not self — the top-level entity loop skips it. The growable interruption
    /// seam: a future transient mode (knockback/puppeting) lives alongside this
    /// arm without it assuming a companion is always following.
    Companion {
        slot: usize,
    },
}

/// How a cutscene (and, later, the editor) addresses a live entity: by stable
/// string id — an authored map creature — or by a well-known role: the player,
/// or one of its companions by slot. Resolved against the entity tree by
/// [`WalkaroundState::resolve`](crate::gamestate::walkaround::WalkaroundState::resolve).
/// Replaces the old positional `cutscene_token`/`entities[0]` addressing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityId {
    /// A shell whose [`Shell::id`] matches (first match wins; ids should be
    /// unique per map).
    Id(String),
    /// The player entity (`entities[0]`).
    Player,
    /// The player's companion in this slot (`entities[0].companions[slot]`).
    PlayerCompanion(usize),
}

/// The data-store key identifying a creature archetype: the name a
/// [`PresetDef`](crate::data::eggdata::PresetDef) is filed under in
/// [`Presets`](crate::data::eggdata::Presets), and what a [`Shell::preset`] / an
/// egg's [`MoveMode::Egg`] records for persistence. Stored and serialised as a
/// bare string (`"critter"`), so a save is just the name and survives the
/// creature set changing in `data.toml`.
///
/// A typo'd or data-removed id is caught at the (fallible) store lookup, not at
/// compile time — the price of an open, data-defined set (vs. the old closed
/// enum). The constants below name the built-ins engine code spawns directly.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PresetId(pub String);

impl PresetId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn ellie() -> Self {
        Self::new("ellie")
    }
    pub fn may() -> Self {
        Self::new("may")
    }
    pub fn dog() -> Self {
        Self::new("dog")
    }
    pub fn bro() -> Self {
        Self::new("bro")
    }
    pub fn critter() -> Self {
        Self::new("critter")
    }
}
impl std::fmt::Display for PresetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<&str> for PresetId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

#[derive(Debug, Clone)]
pub struct ShellSprites {
    pub walk: WalkSprites,
    pub others: Vec<SpriteAnimation>,
}
impl ShellSprites {
    fn new(walk: WalkSprites, other_ids: &[i32], w: i32, h: i32) -> Self {
        Self {
            walk,
            others: vec![SpriteAnimation::from_sprite_ids(other_ids, w, h)],
        }
    }
    pub fn egg() -> Self {
        Self::new(WalkSprites::egg(), &[524], 1, 1)
    }
    /// A minimal stand-in for a shell whose real sprites are (re)attached from
    /// its preset later — the `#[serde(skip)]` default and the fallback for an
    /// unknown preset id. Reuses the egg's static look.
    pub fn placeholder() -> Self {
        Self::egg()
    }
}

/// Which axis a [`Committed`](FacingPolicy::Committed) (4-way) sprite is aimed
/// along. Flips only when that axis goes idle (see [`Shell::face`]), so moving
/// diagonally off your start direction doesn't spin the sprite around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacingAxis {
    Horizontal,
    Vertical,
}

/// A controllable game entity.
///
/// `PartialEq`/`Eq` deliberately ignore `sprites` (hand-written below): the
/// sprites are *derived* from `preset` and skipped in serialisation, so they're
/// never the deciding factor in whether the save changed. Entity persistence
/// (de)serialises every field except `sprites`, which is reattached from the
/// `preset` on load (see [`reattach_sprites`](Self::reattach_sprites)).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shell {
    /// coords are (x, y)
    pub dir: (i8, i8),
    /// Last non-zero movement sign per axis, each held while that axis is idle.
    /// `.0` is the sprite's horizontal facing (drives the walk-grid mirror, so a
    /// vertical-only mover keeps its last left/right look); a `Committed` set also
    /// reads `.1` for its vertical facing. Maintained via [`face`](Self::face).
    pub sticky_dir: (i8, i8),
    /// The axis a 4-way sprite is committed to facing along (see [`FacingAxis`]);
    /// unread by per-axis sets. Maintained via [`face`](Self::face).
    pub facing_axis: FacingAxis,
    pub hp: u8,
    pub local_hitbox: Hitbox,
    pub pos: Vec2,
    pub walking: bool,
    pub walktime: u16,
    pub flip_controls: Axis,
    pub pet_timer: Option<u8>,
    /// The archetype this shell is an instance of (its [`PresetId`]). The store
    /// key the *derived* `sprites` resolve from, and the handle entity
    /// persistence stores to rebuild a shell; an egg carries the preset it will
    /// hatch into. Held alongside the materialised `sprites` — the maps pattern
    /// (resolve once, keep the working copy) rather than a per-frame registry
    /// lookup; `sprites` is the cache, `preset` the key.
    pub preset: PresetId,
    /// Derived from [`preset`](Self::preset), so it's skipped in serialisation
    /// (a placeholder is parsed, then [`reattach_sprites`](Self::reattach_sprites)
    /// rebuilds the real set on load) rather than baking the art into every save.
    #[serde(skip, default = "ShellSprites::placeholder")]
    pub sprites: ShellSprites,
    pub move_mode: MoveMode,
    /// Sprite outline: `Some(colour)` strokes a 1px border in that palette index,
    /// `None` draws the bare sprite. When present, [`draw_params`](Self::draw_params)
    /// lifts the sprite one extra pixel so the ring sits on the hitbox baseline.
    /// `#[serde(default)]` so shells saved before the field keep the historical
    /// outlined look.
    #[serde(default = "default_outline")]
    pub outline: Option<u8>,
    /// Nested followers (depth-1): each is a full [`Shell`] with
    /// [`MoveMode::Companion`], snapped onto this shell's [`trail`](Self::trail)
    /// every step. Any shell can lead. Serialised with its leader, so a parked
    /// map creature carries its companions; an empty `Vec` is the common case.
    #[serde(default)]
    pub companions: Vec<Shell>,
    /// Breadcrumb buffer of this shell's recent `(pos, dir)` — its companions
    /// follow by sampling it. Transient runtime state (rebuilt as it walks), so
    /// it's skipped in serialisation and ignored by `PartialEq`, like `sprites`.
    #[serde(skip)]
    pub trail: CompanionTrail<16>,
    /// An intrinsic interaction this shell offers when another acts on it (the
    /// dog's pet). `None` for everything else. Skipped in serialisation /
    /// `PartialEq` (it isn't `Serialize`/`Eq`); set when the shell is spawned
    /// into its role, like `sprites`.
    #[serde(skip)]
    pub interaction: Option<Interaction>,
    /// Stable identity for cutscene addressing ([`EntityId::Id`]): an authored
    /// map-creature name. `None` for the player, companions, and anonymous
    /// creatures (which resolve by role/slot instead).
    #[serde(default)]
    pub id: Option<String>,
}

/// Serde default for [`Shell::outline`]: the historical `Some(1)` every shell
/// drew with before the field existed, so old saves load unchanged.
fn default_outline() -> Option<u8> {
    Some(1)
}

impl PartialEq for Shell {
    /// Compares every field **except** `sprites` — they're derived from `preset`
    /// (equal presets ⇒ equal sprites) and skipped in serialisation, so comparing
    /// them is redundant work that can't change whether the save file differs.
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir
            && self.sticky_dir == other.sticky_dir
            && self.facing_axis == other.facing_axis
            && self.hp == other.hp
            && self.local_hitbox == other.local_hitbox
            && self.pos == other.pos
            && self.walking == other.walking
            && self.walktime == other.walktime
            && self.flip_controls == other.flip_controls
            && self.pet_timer == other.pet_timer
            && self.preset == other.preset
            && self.move_mode == other.move_mode
            && self.outline == other.outline
            && self.companions == other.companions
            && self.id == other.id
        // `trail` and `interaction` are transient/derived (skipped in serde),
        // so they're excluded here too — like `sprites`.
    }
}
impl Eq for Shell {}
impl Default for Shell {
    /// The built-in `ellie`, spawned from the embedded `data.toml`. Used for the
    /// player entity (with `move_mode` overridden to [`MoveMode::Player`]) and as
    /// a benign fallback. Cheap enough — only called at construction, never per
    /// frame; a deserialised shell's `sprites` use [`ShellSprites::placeholder`].
    fn default() -> Self {
        crate::data::eggdata::Presets::builtin()
            .spawn(&PresetId::ellie())
            .expect("built-in presets define `ellie`")
    }
}
impl Shell {
    /// The direction this shell's sprite faces — the walk-grid cell — after
    /// applying the sprite set's [`FacingPolicy`] to the live/sticky/committed
    /// heading. Distinct from [`dir`](Self::dir), which stays literal for movement
    /// and interaction.
    pub fn facing_dir(&self) -> (i8, i8) {
        self.sprites
            .walk
            .lookup_dir(self.dir, self.sticky_dir, self.facing_axis)
    }
    pub fn sprite_options(&self) -> (SpriteOptions, i32) {
        let facing = self.facing_dir();
        // Anim cadence follows the *shown* facing, not the raw heading, so a
        // committed side view stays 4fps even while moving diagonally.
        let timer = if facing.1 == 0 {
            // sideways anim 4fps
            self.walktime.div_ceil(15)
        } else {
            // up/down anim at 3fps
            self.walktime.div_ceil(20)
        };
        let y_offset = (timer % 2) as i32;
        // petting animation
        if let Some(t) = self.pet_timer {
            let t = (t / 20 % 2) as usize;
            let mut sprite = self.sprites.others[0].get_frame(t).clone();
            // Mirror toward the petter's facing, composed with the strip's own
            // authored flip — so the dog's `others` (drawn when the player shell
            // is set to the dog) face the right way without inverting the player's.
            let mirror = self.dir.0 <= 0;
            sprite.flip = if sprite.flip.x() ^ mirror {
                Flip::Horizontal
            } else {
                Flip::None
            };
            return (sprite, 0);
        }
        let sprite = self
            .sprites
            .walk
            .dir_to_sprite(facing)
            .get_frame(timer as usize)
            .clone();
        (sprite, y_offset)
    }
    pub fn draw_params(&self, offset: Vec2) -> DrawParams {
        let (sprite, bob) = self.sprite_options();
        // `pos` is the hitbox top-left. The bottom-aligned sprite is drawn up by
        // `8h - hitbox_h` (so its bottom row sits on the hitbox bottom) and, when
        // mirrored, left by `8w - hitbox_w` (pivoting the flip about the hitbox
        // centre so the footprint stays planted) — both derived from the tile
        // footprint + hitbox rather than a hand-tuned per-frame offset. A drawn
        // outline adds a 1px ring below the art, so lift one extra pixel to keep
        // the outline's bottom row on the hitbox baseline (covering it) rather
        // than poking a pixel below the feet.
        let outline_lift = i16::from(self.outline.is_some());
        let anchor_y = sprite.h as i16 * 8 - self.local_hitbox.h + outline_lift;
        let anchor_x = if sprite.flip.x() {
            sprite.w as i16 * 8 - self.local_hitbox.w
        } else {
            0
        };
        DrawParams::new(
            sprite.id,
            i32::from(self.pos.x - offset.x - anchor_x),
            i32::from(self.pos.y - offset.y - anchor_y) - bob,
            sprite,
            self.outline,
            0,
        )
    }
    pub fn with_pos(self, pos: Vec2) -> Self {
        Self { pos, ..self }
    }
    pub fn with_move_mode(self, move_mode: MoveMode) -> Self {
        Self { move_mode, ..self }
    }
    /// Rebuild the (serialisation-skipped) [`sprites`](Self::sprites) from this
    /// shell's archetype after it's loaded from a save, where `sprites` came back
    /// as a placeholder. An unhatched egg keeps its egg sprites (its current
    /// form); anything else resolves its [`preset`](Self::preset) in `presets`,
    /// falling back to a placeholder if the data no longer defines that id.
    pub fn reattach_sprites(&mut self, presets: &crate::data::eggdata::Presets) {
        self.sprites = if matches!(self.move_mode, MoveMode::Egg { .. }) {
            ShellSprites::egg()
        } else {
            presets
                .get(&self.preset)
                .map(|def| def.build_sprites())
                .unwrap_or_else(ShellSprites::placeholder)
        };
        // Companions are nested shells whose `sprites` were skipped too — rebuild
        // each from its own preset (depth follows the nesting).
        for companion in &mut self.companions {
            companion.reattach_sprites(presets);
        }
        // `trail` is serde-skipped, so a freshly-loaded leader's is all-zero —
        // seed it at this shell's position so its companions regroup here on the
        // next step instead of snapping to (0, 0) until the trail refills.
        self.trail.fill(self.pos, self.dir);
    }
    pub fn replace(&mut self, shell: Shell) {
        *self = shell.with_pos(self.pos).with_move_mode(self.move_mode.clone());
    }
    pub fn hitbox(&self) -> Hitbox {
        self.local_hitbox.offset(self.pos)
    }
    pub fn apply_walk_direction(&mut self, mut dx: i16, mut dy: i16) -> (i16, i16) {
        if dx == 0 && dy == 0 {
            return (dx, dy);
        };

        match self.flip_controls {
            Axis::None => {}
            Axis::X => dx *= -1,
            Axis::Y => dy *= -1,
            Axis::Both => {
                dx *= -1;
                dy *= -1
            }
        }

        self.face((dx as i8, dy as i8));

        (dx, dy)
    }
    /// Set the facing direction, keeping [`sticky_dir`](Self::sticky_dir) (the
    /// per-axis last non-zero heading) in sync. The single point for changing
    /// where a shell faces, so the sprite mirror and the literal `dir` can't drift
    /// apart — used by walking and by cutscene `FacePlayer`.
    pub fn face(&mut self, dir: (i8, i8)) {
        self.dir = dir;
        if dir.0 != 0 {
            self.sticky_dir.0 = dir.0.signum();
        }
        if dir.1 != 0 {
            self.sticky_dir.1 = dir.1.signum();
        }
        // Commit to the axis we're moving along, switching only once it goes idle,
        // so a diagonal off your start direction doesn't re-aim a 4-way sprite.
        let committed_moving = match self.facing_axis {
            FacingAxis::Horizontal => dir.0 != 0,
            FacingAxis::Vertical => dir.1 != 0,
        };
        if !committed_moving {
            if dir.0 != 0 {
                self.facing_axis = FacingAxis::Horizontal;
            } else if dir.1 != 0 {
                self.facing_axis = FacingAxis::Vertical;
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn walk(
        &mut self,
        system: &mut impl ConsoleApi,
        mut dx: i16,
        mut dy: i16,
        noclip: bool,
        current_map: &MapInfo,
        tiles: Option<&TiledMap>,
    ) -> (i16, i16) {
        use crate::world::map::layer_collides;

        if dx == 0 && dy == 0 {
            return (dx, dy);
        };

        (dx, dy) = self.apply_walk_direction(dx, dy);

        if noclip {
            return (dx, dy);
        };

        if (self.walktime + 15).is_multiple_of(20) {
            system.play_sound(sound::FOOTSTEP_PLAIN.with_note(17));
        }

        // No tile source loaded for this map (e.g. the empty default map):
        // nothing to collide with, so walk freely. (Collision reads the layers'
        // own colliders, not the tile data, so the handle itself is unused —
        // its presence is the "this map has a tile source" signal.)
        let Some(_tiles) = tiles else {
            return (dx, dy);
        };

        // Player position + intended movement
        let player_hitbox = self.hitbox();
        let delta_hitbox = player_hitbox.offset_xy(-1, -1).grow(2, 2);

        // Collide
        let points_dx = player_hitbox.dx_corners(dx);
        let points_dx_up = player_hitbox.offset_xy(0, -1).dx_corners(dx);
        let points_dx_down = player_hitbox.offset_xy(0, 1).dx_corners(dx);
        let (mut dx_collision_x, mut dx_collision_up, mut dx_collision_down) =
            (false, false, false);
        let points_dy = player_hitbox.dy_corners(dy);
        let points_dy_left = player_hitbox.offset_xy(-1, 0).dy_corners(dy);
        let points_dy_right = player_hitbox.offset_xy(1, 0).dy_corners(dy);
        let (mut dy_collision_y, mut dy_collision_left, mut dy_collision_right) =
            (false, false, false);
        let point_diag = player_hitbox.dd_corner(Vec2::new(dx, dy));
        let mut diagonal_collision = false;
        for layer in current_map.layers.iter() {
            let layer_hitbox = layer.hitbox();
            if !layer_hitbox.touches(delta_hitbox) {
                continue;
            }
            [dx_collision_x, dx_collision_up, dx_collision_down] = test_many_points(
                layer,
                [points_dx, points_dx_up, points_dx_down],
                [dx_collision_x, dx_collision_up, dx_collision_down],
            );
            [dy_collision_y, dy_collision_left, dy_collision_right] = test_many_points(
                layer,
                [points_dy, points_dy_left, points_dy_right],
                [dy_collision_y, dy_collision_left, dy_collision_right],
            );
            if let Some(point_diag) = point_diag
                && layer_collides(point_diag, layer)
            {
                diagonal_collision = true;
            }
        }
        slide_ramp(
            dx_collision_x,
            dx_collision_down,
            dx_collision_up,
            &mut dx,
            &mut dy,
        );
        slide_ramp(
            dy_collision_y,
            dy_collision_right,
            dy_collision_left,
            &mut dy,
            &mut dx,
        );
        if diagonal_collision && dx != 0 && dy != 0 {
            dx = 0;
            dy = 0;
        }

        (dx, dy)
    }
    /// Apply a per-axis movement step, recording the breadcrumb a leader's
    /// companions follow. Every shell owns its [`trail`](Self::trail) and pushes
    /// its pre-move position onto it as it walks (so anything can lead); a shell
    /// with no companions just leaves an unread trail. `(0, 0)` stops the trail
    /// (companions settle) and switches the sprite to idle.
    pub fn apply_motion(&mut self, dx: i16, dy: i16) {
        if dx == 0 && dy == 0 {
            self.trail.stop();
            self.animate_stop();
        } else {
            self.trail
                .push(Vec2::new(self.pos.x, self.pos.y), (self.dir.0, self.dir.1));
            self.pos.x += dx;
            self.pos.y += dy;
            self.animate_walk();
        }
    }
    /// Snap each [`MoveMode::Companion`] follower onto a sample of this shell's
    /// [`trail`](Self::trail) — the per-step follow update a leader runs after it
    /// moves. Position, facing and walk cadence all come from the leader's
    /// breadcrumb (so the gait stays synced); the `slot` picks how far back along
    /// the trail this companion rides. Depth-1: a companion's own followers (if
    /// any) aren't recursed here.
    pub fn update_companions(&mut self) {
        let trail = &self.trail;
        let walktime = trail.walktime();
        for (i, companion) in self.companions.iter_mut().enumerate() {
            let slot = match companion.move_mode {
                MoveMode::Companion { slot } => slot,
                _ => i,
            };
            let (pos, dir) = trail.sample(slot);
            companion.pos = pos;
            companion.face(dir);
            companion.walktime = u16::from(walktime);
        }
    }
    pub fn animate_walk(&mut self) {
        self.walktime = self.walktime.wrapping_add(1);
        self.walking = true;
    }
    pub fn animate_stop(&mut self) {
        self.walktime = 0;
        self.walking = false;
    }
}

// presets
impl Shell {
    /// Assemble a shell from its archetype id and the parts a preset resolves to
    /// (hitbox, derived sprites, spawn behaviour), recording the id on
    /// [`preset`](Self::preset). The single funnel the data store
    /// ([`Presets::spawn`](crate::data::eggdata::Presets::spawn)) and the built-in
    /// [`egg`](Self::egg) build through, so every shell knows what it is.
    pub fn from_parts(
        preset: PresetId,
        local_hitbox: Hitbox,
        sprites: ShellSprites,
        move_mode: MoveMode,
    ) -> Self {
        Self {
            preset,
            // Hitbox-top-left spawn (new anchor). Was (62, 23) when `pos` meant
            // sprite-top-left; +10 (the old player hitbox.y) keeps the new-game
            // bedroom spawn at the same world standing position as before.
            pos: Vec2::new(62, 33),
            local_hitbox,
            hp: 3,
            dir: (0, 1),
            sticky_dir: (0, 1),
            facing_axis: FacingAxis::Vertical,
            walktime: 0,
            walking: false,
            flip_controls: Axis::None,
            pet_timer: None,
            sprites,
            move_mode,
            outline: Some(1),
            companions: Vec::new(),
            trail: CompanionTrail::new(),
            interaction: None,
            id: None,
        }
    }
    /// An egg that hatches into `hatches_into` after a fixed delay. The egg form
    /// is built-in (not a `data.toml` preset): static egg sprites + a tiny
    /// hitbox, with the target archetype recorded on both [`preset`](Self::preset)
    /// and [`MoveMode::Egg`] so the hatch can spawn it from the store.
    pub fn egg(hatches_into: PresetId) -> Self {
        Self::from_parts(
            hatches_into.clone(),
            Hitbox::new(0, 0, 8, 8),
            ShellSprites::egg(),
            MoveMode::Egg {
                timer: Timer(255),
                hatches_into,
            },
        )
    }
}

fn test_many_points(
    layer: &LayerInfo,
    points: [Option<[Vec2; 2]>; 3],
    mut side_flags: [bool; 3],
) -> [bool; 3] {
    use crate::world::map::layer_collides;
    for (i, points) in points.iter().enumerate() {
        if let Some(points) = points {
            points.iter().for_each(|point| {
                if layer_collides(*point, layer) {
                    side_flags[i] = true;
                }
            });
        };
    }
    side_flags
}

/// Logic for sliding on 1 pixel ramps.
///
/// If there is a forwards collision but no diagonal one,
/// this function will move in the first available
/// diagonal direction.
fn slide_ramp(
    main_axis_collides: bool,
    plus_side_collides: bool,
    minus_side_collides: bool,
    main_axis_delta: &mut i16,
    side_axis_delta: &mut i16,
) {
    if !main_axis_collides {
        return;
    }
    if *side_axis_delta == 0 {
        if !plus_side_collides {
            *side_axis_delta = 1;
        } else if !minus_side_collides {
            *side_axis_delta = -1;
        } else {
            *main_axis_delta = 0;
        }
    } else {
        *main_axis_delta = 0;
    }
}

/// A pettable shell's intrinsic interaction marker (the dog's
/// [`Shell::interaction`]). Its `Pet` coordinates are placeholders — the live
/// pet rebuilds them from the dog's current position via [`pet_interaction`] at
/// fire time, so the stored value only flags "this shell is pettable".
pub fn pet_marker() -> Interaction {
    Interaction::Func(crate::world::interact::InteractFn::Pet(
        Vec2::new(0, 0),
        None,
    ))
}

/// Build the dog's pet interaction from live positions: the target the player
/// walks to (nudged a pixel when approached from the right) and which side it
/// pets from. Extracted from the old `Companion::interact`; both the live pet
/// (the companion step fallback) and an authored pet feed it. Phase A folds this
/// into a `move_beside` + `interact` cutscene.
pub fn pet_interaction(position: Vec2, direction: (i8, i8), player_position: Vec2) -> Interaction {
    use crate::world::interact::InteractFn;
    let mut pixel = 0;
    let offset = if direction.1 == 0 {
        direction.0 > 0
    } else {
        let x = player_position.x > position.x;
        if x {
            pixel -= 1;
        }
        x
    };
    let position = position + Vec2::new(pixel, 0);
    Interaction::Func(InteractFn::Pet(position, Some(offset)))
}

#[derive(Clone, Debug)]
pub struct CompanionTrail<const N: usize> {
    positions: [Vec2; N],
    directions: [(i8, i8); N],
    walktime: u8,
}
impl<const N: usize> Default for CompanionTrail<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CompanionTrail<N> {
    pub const fn new() -> Self {
        Self {
            positions: [Vec2::new(0, 0); N],
            directions: [(0, 0); N],
            walktime: 0,
        }
    }
    /// When player moves, rotate all positions towards start of buffer, add new position end of buffer.
    pub fn push(&mut self, position: Vec2, direction: (i8, i8)) {
        self.positions.rotate_left(1);
        self.directions.rotate_left(1);

        // Array always has at least one element (N >= 1)
        *self.positions.last_mut().unwrap() = position;
        *self.directions.last_mut().unwrap() = direction;
        self.walktime = self.walktime.wrapping_add(1);
    }
    /// When player stops moving, tell animations to switch to idle pose.
    pub fn stop(&mut self) {
        self.walktime = 0;
    }
    /// Moves all companions to the same point.
    pub fn fill(&mut self, position: Vec2, direction: (i8, i8)) {
        self.positions.fill(position);
        self.directions.fill(direction);
    }
    /// Seed the buffer as a straight path from `tail` (the oldest sample, where a
    /// slot-0 follower rides) to `head` (the newest, next to the leader), facing
    /// `direction`. Hands companions back *in place* after something suspended
    /// them (a cutscene): a follower resumes from `tail` and trails toward `head`
    /// instead of snapping to a stale breadcrumb.
    pub fn fill_toward(&mut self, tail: Vec2, head: Vec2, direction: (i8, i8)) {
        let last = (N - 1).max(1) as i16;
        for (k, slot) in self.positions.iter_mut().enumerate() {
            let k = k as i16;
            slot.x = tail.x + (head.x - tail.x) * k / last;
            slot.y = tail.y + (head.y - tail.y) * k / last;
        }
        self.directions.fill(direction);
        self.walktime = 0;
    }
    pub fn mid(&self) -> (Vec2, (i8, i8)) {
        (
            self.positions[self.positions.len() / 2],
            self.directions[self.directions.len() / 2],
        )
    }
    pub fn oldest(&self) -> (Vec2, (i8, i8)) {
        (self.positions[0], self.directions[0])
    }
    pub fn latest(&self) -> (Vec2, (i8, i8)) {
        // Array always has at least one element (N >= 1)
        (
            *self.positions.last().unwrap(),
            *self.directions.last().unwrap(),
        )
    }
    pub fn walktime(&self) -> u8 {
        self.walktime
    }
    /// The `(pos, dir)` a companion in `slot` follows. Slot 0 — the lone dog, and
    /// the nearest of several — rides the oldest breadcrumb (the trail's tail, `N`
    /// steps back), settling a fixed gap behind the leader; deeper slots ride more
    /// recent samples. Preserves the old breadcrumb follow distance.
    pub fn sample(&self, slot: usize) -> (Vec2, (i8, i8)) {
        match slot {
            0 => self.oldest(),
            _ => self.mid(),
        }
    }
}

/// A creature's wander state, driven by [`CreatureState::step`] and selected by
/// [`MoveMode::Amble`]: dwell idle for a spell, then walk a random heading for a
/// spell, and back. The unhatched phase lives in [`MoveMode::Egg`], not here —
/// these critters are spawned (as eggs) by the `add_creatures` interaction, see
/// [`crate::world::interact::InteractFn::AddCreatures`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreatureState {
    Idle(Timer),
    Walking(Timer, Vec2),
}
impl CreatureState {
    /// Whether the creature is mid-walk (vs idling). The walk animation is driven
    /// off this *state* rather than the every-third-tick displacement, so the
    /// gait cycles smoothly instead of flickering on the idle ticks.
    pub fn is_walking(&self) -> bool {
        matches!(self, CreatureState::Walking(..))
    }
    /// Advance one step, returning the intended `(dx, dy)` for the shell to walk
    /// (the caller applies collision). Idle yields no motion and eventually flips
    /// to Walking; Walking nudges one pixel every third tick along its chosen
    /// heading, then eventually flips back to Idle.
    pub fn step(&mut self, rng: &mut Lcg64Xsh32) -> (i16, i16) {
        match self {
            CreatureState::Idle(timer) => {
                if timer.tick() {
                    *self = CreatureState::Walking(
                        Timer(rng.rand_u8().min(80)),
                        Vec2::new(
                            (rng.rand_u8() % 3) as i16 - 1,
                            (rng.rand_u8() % 3) as i16 - 1,
                        ),
                    );
                }
                (0, 0)
            }
            CreatureState::Walking(timer, vec) => {
                if timer.tick() {
                    *self = CreatureState::Idle(Timer(rng.rand_u8().min(80)));
                    (0, 0)
                } else if timer.0 % 3 == 0 {
                    (vec.x, vec.y)
                } else {
                    (0, 0)
                }
            }
        }
    }
}

/// A small countdown in fixed steps, saturating at zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer(pub u8);

impl Timer {
    pub fn tick_amt(&mut self, amount: u8) -> bool {
        self.0 = self.0.saturating_sub(amount);
        self.0 == 0
    }
    pub fn tick(&mut self) -> bool {
        self.tick_amt(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_snaps_to_the_leaders_trail_tail() {
        // A leader with one slot-0 companion: after the leader walks east, the
        // companion rides the oldest breadcrumb (the trail tail) — a fixed gap
        // behind, the dog-follow feel.
        let mut leader = Shell::default();
        leader.pos = Vec2::new(50, 50);
        let mut dog = Shell::default();
        dog.move_mode = MoveMode::Companion { slot: 0 };
        leader.companions.push(dog);
        for _ in 0..20 {
            leader.face((1, 0));
            leader.apply_motion(1, 0);
        }
        leader.update_companions();
        let (tail, _) = leader.trail.sample(0);
        assert_eq!(
            leader.companions[0].pos, tail,
            "companion rides the trail tail",
        );
        assert!(
            leader.companions[0].pos.x < leader.pos.x,
            "companion trails behind the leader",
        );
    }

    /// Spawn a built-in shell from the embedded `data.toml` — the data is the
    /// only source of creatures now (the walk-grid tests build `WalkSprites`
    /// fixtures directly, see [`grid`], since the pattern constructors are gone).
    fn spawn(name: &str) -> Shell {
        crate::data::eggdata::Presets::builtin()
            .spawn(&PresetId::new(name))
            .unwrap_or_else(|| panic!("built-in `{name}`"))
    }

    /// Build a [`WalkSprites`] fixture from an explicit 3×3 grid — one
    /// single-frame animation per cell (its sprite `id` and `flip`), in
    /// `dir_to_sprite` order (up row, level row, down row; left/centre/right) —
    /// plus a facing policy. The builder-free way these tests pin grid bucketing
    /// and facing-policy behaviour now the shorthand pattern constructors are gone.
    fn grid(cells: [(i32, Flip); 9], facing: FacingPolicy) -> WalkSprites {
        let cell = |(id, flip): (i32, Flip)| {
            SpriteAnimation::from_sprite_ids(&[id], 1, 1).with_flip(flip)
        };
        WalkSprites {
            grid: cells.map(cell),
            facing,
        }
    }

    /// A shell wearing a given walk grid, started facing down on the vertical
    /// axis (the [`Shell::from_parts`] defaults) — lets the facing-policy tests
    /// drive [`Shell::face`]/[`Shell::facing_dir`] on an explicit fixture rather
    /// than a `data.toml` preset.
    fn shell_with(walk: WalkSprites) -> Shell {
        let mut shell = spawn("critter");
        shell.sprites.walk = walk;
        shell.dir = (0, 1);
        shell.sticky_dir = (0, 1);
        shell.facing_axis = FacingAxis::Vertical;
        shell
    }

    #[test]
    fn critter_preset_ambles_and_egg_carries_its_target() {
        assert!(matches!(
            spawn("critter").move_mode,
            MoveMode::Amble(CreatureState::Idle(_))
        ));
        // An egg is in Egg mode and carries what it will become.
        let egg = Shell::egg(PresetId::critter());
        assert_eq!(egg.preset, PresetId::critter());
        assert!(matches!(egg.move_mode, MoveMode::Egg { .. }));
        // The store spawns a wandering NPC for a non-critter preset.
        assert!(matches!(spawn("dog").move_mode, MoveMode::Wander));
    }

    /// Every shell records its archetype on `preset`: the store stamps the id it
    /// spawned, and an egg carries the preset it will hatch into — the handle
    /// entity persistence stores to rebuild a shell.
    #[test]
    fn shells_carry_their_preset() {
        for name in ["ellie", "may", "dog", "bro", "critter"] {
            assert_eq!(spawn(name).preset, PresetId::new(name), "{name} stamps its id");
        }
        // An egg's archetype is what it becomes.
        assert_eq!(Shell::egg(PresetId::critter()).preset, PresetId::critter());
    }

    /// `PresetId` serialises as a bare string (`"critter"`), so a save is just the
    /// name and survives the creature set changing in `data.toml`.
    #[test]
    fn preset_id_serialises_as_a_bare_string() {
        assert_eq!(
            serde_json::to_string(&PresetId::critter()).unwrap(),
            "\"critter\""
        );
        let back: PresetId = serde_json::from_str("\"ellie\"").unwrap();
        assert_eq!(back, PresetId::ellie());
    }

    /// A shell serialises every field but its (derived) sprites, and
    /// `reattach_sprites` rebuilds them from the preset on the way back — so a
    /// persisted creature keeps its position/state and looks right again.
    #[test]
    fn shell_serde_skips_and_reattaches_sprites() {
        let presets = crate::data::eggdata::Presets::builtin();
        let mut critter = spawn("critter").with_pos(Vec2::new(40, 7));
        critter.walktime = 5;
        let json = serde_json::to_string(&critter).unwrap();
        assert!(!json.contains("sprites"), "derived sprites are not serialised");

        let mut back: Shell = serde_json::from_str(&json).unwrap();
        assert_eq!(back.preset, PresetId::critter());
        assert_eq!(back.pos, Vec2::new(40, 7));
        assert_eq!(back.walktime, 5);
        assert!(matches!(back.move_mode, MoveMode::Amble(_)));
        // Sprites round-trip via the preset, not the bytes (compared by Debug,
        // since `ShellSprites` has no `PartialEq`).
        back.reattach_sprites(&presets);
        assert_eq!(
            format!("{:?}", back.sprites),
            format!("{:?}", spawn("critter").sprites),
            "sprites rebuilt from the preset"
        );
    }

    /// An unhatched egg reattaches its *egg* sprites (its current form), not the
    /// hatched form of the preset it will become.
    #[test]
    fn egg_reattaches_egg_sprites_not_hatched_form() {
        let presets = crate::data::eggdata::Presets::builtin();
        let json = serde_json::to_string(&Shell::egg(PresetId::dog())).unwrap();
        let mut back: Shell = serde_json::from_str(&json).unwrap();
        assert_eq!(back.preset, PresetId::dog(), "archetype is what it becomes");
        assert!(matches!(back.move_mode, MoveMode::Egg { .. }));
        back.reattach_sprites(&presets);
        assert_eq!(
            format!("{:?}", back.sprites),
            format!("{:?}", ShellSprites::egg()),
            "an unhatched egg keeps its egg sprites"
        );
        assert_ne!(
            format!("{:?}", back.sprites),
            format!("{:?}", spawn("dog").sprites),
            "not the hatched dog's sprites"
        );
    }

    #[test]
    fn amble_idles_then_walks_on_every_third_tick() {
        use crate::rand::Lcg64Xsh32;
        let mut rng = Lcg64Xsh32::default();

        // Idle holds still, then flips to Walking once its timer drains.
        let mut state = CreatureState::Idle(Timer(1));
        assert_eq!(state.step(&mut rng), (0, 0));
        assert!(matches!(state, CreatureState::Walking(..)));

        // A long walk in a fixed heading nudges ≤1px/axis on every third tick and
        // stays put on the two before it.
        let mut state = CreatureState::Walking(Timer(90), Vec2::new(1, -1));
        let steps: Vec<(i16, i16)> = (0..3).map(|_| state.step(&mut rng)).collect();
        assert_eq!(steps, vec![(0, 0), (0, 0), (1, -1)]);
    }

    #[test]
    fn eight_way_grid_buckets_and_mirrors() {
        let flip_of = |w: &WalkSprites, dir| w.dir_to_sprite(dir).get_frame(0).flip.clone();
        let n = || Flip::None;
        let h = || Flip::Horizontal;

        // A humanoid-shaped grid (the ellie/may/bro layout): the level row is
        // [west-mirrored, idle, east], so east is unflipped, west pre-mirrored,
        // and `signum` buckets any magnitude — a noclip-scaled heading still
        // faces right.
        let ellie = grid(
            [
                (771, n()), (771, n()), (771, n()),
                (832, h()), (771, n()), (832, n()),
                (768, n()), (768, n()), (768, n()),
            ],
            FacingPolicy::Committed,
        );
        assert_eq!(flip_of(&ellie, (1, 0)), Flip::None);
        assert_eq!(flip_of(&ellie, (-1, 0)), Flip::Horizontal);
        assert_eq!(flip_of(&ellie, (4, 0)), Flip::None);

        // A sideways-shaped grid (the critter layout): the whole left column
        // mirrors — including the diagonal cells a vertical mover lands on via a
        // sticky facing — and nothing else.
        let critter = grid(
            [
                (688, h()), (688, n()), (688, n()),
                (688, h()), (688, n()), (688, n()),
                (688, h()), (688, n()), (688, n()),
            ],
            FacingPolicy::PerAxis,
        );
        for dy in [-1, 0, 1] {
            assert_eq!(flip_of(&critter, (-1, dy)), Flip::Horizontal);
            assert_eq!(flip_of(&critter, (1, dy)), Flip::None);
        }
    }

    #[test]
    fn face_keeps_horizontal_through_vertical_moves() {
        let mut shell = spawn("critter");
        shell.face((-1, 0)); // face left
        assert_eq!(shell.sticky_dir.0, -1);

        // A straight-down move holds the horizontal facing, but `dir` stays
        // literal (it's what the player's interact hitbox would read).
        shell.face((0, 1));
        assert_eq!(shell.sticky_dir.0, -1);
        assert_eq!(shell.dir, (0, 1));

        // Moving right flips the sticky facing back.
        shell.face((1, 0));
        assert_eq!(shell.sticky_dir.0, 1);
    }

    #[test]
    fn compass_locks_facing_to_initial_axis() {
        // A `Committed` grid (the player/humanoid policy): `facing_dir` resolves
        // off the committed axis, so the cell contents don't matter — only the
        // policy does.
        let uniform = std::array::from_fn(|_| (0, Flip::None));
        let mut shell = shell_with(grid(uniform, FacingPolicy::Committed));

        shell.face((1, 0)); // start moving right
        assert_eq!(shell.facing_dir(), (1, 0)); // east

        shell.face((1, -1)); // add up: a diagonal off the committed axis
        assert_eq!(shell.facing_dir(), (1, 0)); // still east, not north

        shell.face((0, -1)); // release right, now purely up
        assert_eq!(shell.facing_dir(), (0, -1)); // axis released → north
    }

    #[test]
    fn sideways_stays_per_axis() {
        // A `PerAxis` grid (the critter policy): sticky horizontal, live vertical —
        // unaffected by the committed-axis policy.
        let uniform = std::array::from_fn(|_| (0, Flip::None));
        let mut shell = shell_with(grid(uniform, FacingPolicy::PerAxis));
        shell.face((-1, 0));
        shell.face((0, 1)); // straight down while last-facing left
        assert_eq!(shell.facing_dir(), (-1, 1)); // mirror held, vertical live
    }
}
