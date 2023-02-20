use crate::dialogue::DIALOGUE_OPTIONS;
use crate::dialogue_data::*;
use crate::gamestate::Game;
use crate::input_manager::{any_btnpr, mem_btn, mem_btnp};
use crate::interact::{InteractFn, Interaction};
use crate::inventory::INVENTORY;
use crate::map::Axis;
use crate::map_data::{BEDROOM, DEFAULT_MAP_SET, SUPERMARKET, TEST_PEN, WILDERNESS};
use crate::player::{Companion, CompanionList, CompanionTrail, Player};
use crate::tic80::*;
use crate::tic_helpers::*;
use crate::{camera::Camera, dialogue::Dialogue, gamestate::GameState, map::MapSet};
use crate::{debug_info, print, trace, BG_COLOUR, SYNC_HELPER};

pub struct WalkaroundState<'a> {
    player: Player,
    companion_trail: CompanionTrail,
    companion_list: CompanionList,
    map_animations: Vec<(u16, usize)>,
    camera: Camera,
    current_map: &'a MapSet<'a>,
    dialogue: Dialogue,
}
impl<'a> WalkaroundState<'a> {
    pub const fn new() -> Self {
        Self {
            player: Player::const_default(),
            companion_trail: CompanionTrail::new(),
            companion_list: CompanionList::new(),
            map_animations: Vec::new(),
            camera: Camera::const_default(),
            current_map: &DEFAULT_MAP_SET,
            dialogue: Dialogue::const_default(),
        }
    }
    pub fn load_map(&mut self, map: &'a MapSet<'static>) {
        let map1 = &map.maps[0];
        self.camera =
            Camera::from_map_size(map1.w as u8, map1.h as u8, map1.sx as i16, map1.sy as i16);
        self.current_map = map;
        *BG_COLOUR.write().unwrap() = map.bg_colour;
        if let Some(track) = map.music_track {
            music(track as i32, MusicOptions::default());
        };
        if map.bank != SYNC_HELPER.read().unwrap().last_bank() {
            let x = SYNC_HELPER
                .write()
                .unwrap()
                .sync(1 | 4 | 8 | 16 | 32 | 64 | 128, map.bank);
            if x.is_err() {
                let bank = map.bank;
                trace!(
                    format!("COULD NOT SYNC TO BANK {bank} THIS IS A BUG BTW"),
                    12
                );
            }
        }

        self.map_animations.clear();
        for _ in map.interactables {
            self.map_animations.push((0, 0));
        }
    }
    pub fn cam_x(&self) -> i32 {
        self.camera.pos.x.into()
    }
    pub fn cam_y(&self) -> i32 {
        self.camera.pos.y.into()
    }
    pub fn execute_interact_fn(&mut self, interact: &InteractFn) -> Option<&'static str> {
        match interact {
            InteractFn::ToggleDog => {
                self.companion_trail.fill(self.player.pos, self.player.dir);
                if self.companion_list.has(Companion::Dog) {
                    self.companion_list.remove(Companion::Dog);
                    Some(DOG_RELINQUISHED)
                } else {
                    self.companion_list.add(Companion::Dog);
                    Some(DOG_OBTAINED)
                }
            }
            _ => Some(HOUSE_BACKYARD_DOGHOUSE),
        }
    }
}

impl<'a> Game for WalkaroundState<'a> {
    fn step(&mut self) -> Option<GameState> {
        for (anim, interact) in self
            .map_animations
            .iter_mut()
            .zip(self.current_map.interactables.iter())
        {
            if let Some(sprite) = &interact.sprite {
                anim.0 += 1; //timer
                if anim.0 > sprite.frames[anim.1].length {
                    anim.0 = 0;
                    anim.1 += 1; //index
                    if anim.1 >= sprite.frames.len() {
                        anim.1 = 0;
                    }
                }
            }
        }

        if keyp(28, -1, -1) {
            self.load_map(&SUPERMARKET);
        }
        if keyp(29, -1, -1) {
            self.load_map(&WILDERNESS);
        }
        if keyp(30, -1, -1) {
            self.load_map(&TEST_PEN);
        }
        if keyp(31, -1, -1) {
            self.load_map(&BEDROOM);
        }
        if keyp(33, -1, -1) {
            set_palette(crate::tic_helpers::SWEETIE_16);
        }
        if keyp(34, -1, -1) {
            set_palette(crate::tic_helpers::NIGHT_16);
        }
        {
            let small_text = DIALOGUE_OPTIONS.small_text();
            if keyp(36, -1, -1) {
                self.dialogue.set_options(false, !small_text);
            }
        }

        // Get keyboard inputs
        let (mut dx, mut dy) = (0, 0);
        let mut interact = false;
        if matches!(self.dialogue.text, None) {
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
                INVENTORY.write().unwrap().open();
                return Some(GameState::Inventory);
            }
        } else {
            self.dialogue.tick(1);
            if mem_btn(4) {
                self.dialogue.tick(2);
            }
            if mem_btnp(5) {
                self.dialogue.skip();
            }
        }
        if mem_btnp(4) && self.dialogue.is_done() {
            interact = true;
            if matches!(self.dialogue.text, Some(_)) {
                interact = false;
                self.dialogue.close();
            }
            trace!("Attempting interact...", 11);
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

        let (dx, dy) = self.player.walk(dx, dy, noclip, self.current_map);
        self.player.apply_motion(dx, dy, &mut self.companion_trail);

        // Set after player.dir has updated
        let interact_hitbox = self
            .player
            .hitbox()
            .offset_xy(self.player.dir.0.into(), self.player.dir.1.into());

        let mut warp_target = None;
        for warp in self.current_map.warps.iter() {
            if self.player.hitbox().touches(warp.from)
                || (interact && interact_hitbox.touches(warp.from))
            {
                warp_target = Some(warp.clone());
                break;
            }
        }
        if let Some(target) = warp_target {
            self.player.pos = target.to;
            self.player.flip_controls = target.flip;
            self.companion_trail.fill(self.player.pos, self.player.dir);
            if let Some(new_map) = target.map {
                self.load_map(new_map);
            }
        } else if interact {
            for item in self.current_map.interactables.iter() {
                if interact_hitbox.touches(item.hitbox) {
                    match &item.interaction {
                        Interaction::Text(x) => {
                            trace!(format!("{x:?}"), 12);
                            self.dialogue.set_text(x);
                        }
                        Interaction::Func(x) => {
                            trace!(format!("{x:?}"), 12);
                            if let Some(dialogue) = self.execute_interact_fn(x) {
                                self.dialogue.set_text(dialogue);
                            };
                        }
                        x => {
                            trace!(format!("{x:?}"), 12);
                        }
                    }
                }
            }
        }

        self.camera
            .center_on(self.player.pos.x + 4, self.player.pos.y + 8);
        None
    }
    fn draw(&self) {
        // draw bg
        palette_map_reset();
        cls(*crate::BG_COLOUR.read().unwrap());
        let palette_map_rotation = self.current_map.palette_rotation;
        for (i, layer) in self.current_map.maps.iter().enumerate() {
            if let Some(amount) = palette_map_rotation.get(i) {
                palette_map_rotate(*amount)
            } else {
                palette_map_rotate(0)
            }
            blit_segment(layer.blit_segment);
            let mut options: MapOptions = layer.clone().into();
            options.sx -= self.cam_x();
            options.sy -= self.cam_y();
            if debug_info().map_info {
                rectb(options.sx, options.sy, options.w * 8, options.h * 8, 9);
            }
            map(options.into());
        }
        blit_segment(4);
        // draw sprites from least to greatest y
        let mut sprites: Vec<(i32, i32, i32, SpriteOptions, Option<u8>, u8)> = Vec::new();
        let player_sprite = self.player.sprite_index();
        let (player_x, player_y): (i32, i32) = (self.player.pos.x.into(), self.player.pos.y.into());
        sprites.push((
            player_sprite.0,
            player_x - self.cam_x(),
            player_y - player_sprite.2 - self.cam_y(),
            SpriteOptions {
                w: 1,
                h: 2,
                transparent: &[0],
                scale: 1,
                flip: player_sprite.1,
                ..Default::default()
            },
            Some(1),
            1,
        ));

        for (item, time) in self
            .current_map
            .interactables
            .iter()
            .zip(&self.map_animations)
        {
            if let Some(anim) = &item.sprite {
                sprites.push((
                    anim.frames[time.1].id.into(),
                    anim.frames[time.1].pos.x as i32 + item.hitbox.x as i32 - self.cam_x(),
                    anim.frames[time.1].pos.y as i32 + item.hitbox.y as i32 - self.cam_y(),
                    anim.frames[time.1].options.clone(),
                    anim.frames[time.1].outline,
                    anim.frames[time.1].palette_rotate,
                ));
            }
        }
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

        sprites.sort_by(|a, b| (a.2 + a.3.h * 8).partial_cmp(&(b.2 + b.3.h * 8)).unwrap());

        for options in sprites {
            palette_map_rotate(options.5);
            if let Some(outline) = options.4 {
                spr_outline(options.0, options.1, options.2, options.3, outline);
            } else {
                spr(options.0, options.1, options.2, options.3);
            }
        }

        // draw fg
        palette_map_reset();
        for (i, layer) in self.current_map.fg_maps.iter().enumerate() {
            if let Some(amount) = palette_map_rotation.get(i) {
                palette_map_rotate(*amount)
            } else {
                palette_map_rotate(0)
            }
            blit_segment(layer.blit_segment);
            let mut options: MapOptions = layer.clone().into();
            options.sx -= self.cam_x();
            options.sy -= self.cam_y();
            if debug_info().map_info {
                rectb(options.sx, options.sy, options.w * 8, options.h * 8, 9);
            }
            map(options);
        }
        if let Some(string) = &self.dialogue.text {
            self.dialogue.draw_dialogue_box(string, true);
        }
        if debug_info().map_info {
            for warp in self.current_map.warps.iter() {
                warp.from
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
        if debug_info().player_info {
            print!(
                format!("Player: {:#?}", self.player),
                0,
                0,
                PrintOptions {
                    small_text: true,
                    color: 11,
                    ..Default::default()
                }
            );
            print!(
                format!("Camera: {:#?}", self.camera),
                64,
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
