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

use std::sync::RwLock;

use crate::dialogue::DIALOGUE_OPTIONS;
use crate::inventory::{InventoryUiState, INVENTORY};
use crate::position::{Hitbox, Vec2};
use crate::rand;
use crate::{tic80::*, WALKAROUND_STATE};

use crate::tic_helpers::*;

use crate::frames;
use crate::input_manager::{any_btnp, mem_btn, mem_btnp, mouse_delta};

pub enum GameState {
    Instructions(u16),
    Walkaround,
    Animation(u16),
    MainMenu(MenuState),
    Options(MenuState),
    Inventory,
}
impl GameState {
    pub fn run(&mut self) {
        match self {
            Self::Instructions(i) => {
                *i += 1;
                if (*i > 60 || get_pmem(0) != 0) && any_btnp() {
                    *self = Self::Walkaround;
                }
                draw_instructions();
            }
            Self::Walkaround => {
                let next = WALKAROUND_STATE.write().unwrap().step();
                WALKAROUND_STATE.read().unwrap().draw();
                if let Some(state) = next {
                    *self = state;
                }
            }
            Self::Animation(x) => {
                if get_pmem(0) != 0 {
                    *self = Self::MainMenu(MenuState::new());
                    return;
                };
                if mem_btn(4) {
                    *x += 1;
                }
                if mem_btn(5) {
                    *x += 1000;
                }
                if draw_animation(*x) {
                    *x += 1;
                } else {
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::MainMenu(state) => {
                match state.step_main_menu() {
                    Some(MainMenuOption::Play) => *self = Self::Instructions(0),
                    Some(MainMenuOption::Options) => *self = Self::Options(MenuState::new()),
                    None => state.draw_main_menu(),
                };
            }
            Self::Options(state) => {
                if state.step_options() {
                    state.draw_options();
                } else {
                    *self = Self::MainMenu(MenuState::new());
                }
            }
            Self::Inventory => {
                INVENTORY.write().unwrap().step();
                if matches!(INVENTORY.read().unwrap().state, InventoryUiState::Close) {
                    *self = Self::Walkaround;
                } else {
                    INVENTORY.read().unwrap().draw();
                }
            }
        }
    }
}

pub trait Game {
    fn step(&mut self) -> Option<GameState> {
        None
    }
    fn draw(&self);
}

pub fn step_walkaround() -> Option<GameState> {
    None
}

pub fn draw_walkaround() {}

pub fn draw_instructions() {
    cls(0);
    let string = crate::dialogue_data::INSTRUCTIONS;
    let small_text = DIALOGUE_OPTIONS.small_text();
    rect_outline(7, 15, 226, 100, 1, 2);
    print_raw(
        string,
        11,
        21,
        PrintOptions {
            color: 0,
            small_text,
            ..Default::default()
        },
    );
    print_raw(
        string,
        10,
        20,
        PrintOptions {
            color: 12,
            small_text,
            ..Default::default()
        },
    );
}

pub fn draw_animation(t: u16) -> bool {
    let steps: &[u16] = &[0, 1, 700, 760];
    let index = steps.iter().position(|&x| x >= t);
    let local_time = index.map(|x| t - steps[x.saturating_sub(1)]);
    match index {
        Some(0) => {
            cls(0);
            set_palette([[0; 3]; 16]);
            // fade_palette(SWEETIE_16, [[0; 3]; 16], 256/50 * t);
            true
        }
        Some(1) => {
            music(3, MusicOptions::default());
            draw_ovr(|| {
                set_palette([[0; 3]; 16]);
                circb(90, 38, 4, 4);
                circb(90, 36, 3, 4);
                circ(90, 38, 3, 12);
                circ(90, 36, 2, 12);
                for _ in 0..420 {
                    pix(rand() as i32 % 240, rand() as i32 % 136, 12)
                }
            });
            true
        }
        Some(2) => {
            let local_time = local_time.unwrap();
            let max_time = 700.0 - 60.0;
            fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
            draw_ovr(|| {
                fade_palette([[0; 3]; 16], SWEETIE_16, local_time * 2);
                let t = (local_time as f32 / max_time).powf(0.02);
                let size = 200.0 / (max_time + 1.0 - t * max_time).powi(2).max(1.0);
                let t = size as i32;
                set_palette_colour(15, [0x0F; 3]);
                circ(120, 68, t, 15);
                circb(120, 68, t, 2);
                if local_time > 400 {
                    if local_time < 450 {
                        if local_time % 3 == 0 {
                            screen_offset((rand() % 2 - 1) as i8, (rand() % 2 - 1) as i8);
                        }
                    } else {
                        screen_offset((rand() % 2 - 1) as i8, (rand() % 2 - 1) as i8);
                    }
                }
            });
            true
        }
        Some(3) => {
            screen_offset(0, 0);
            fade_palette_colour(15, [0x0F; 3], [26, 28, 44], local_time.unwrap() * 10);
            cls(15);
            draw_ovr(|| {
                cls(0);
                fade_palette([[0x0F; 3]; 16], SWEETIE_16, local_time.unwrap() * 10);
                draw_title(120, 50)
            });
            true
        }
        _ => {
            music(
                -1,
                MusicOptions {
                    frame: 1,
                    ..Default::default()
                },
            );
            set_pmem(0, 1);
            screen_offset(0, 0);
            set_palette(SWEETIE_16);
            cls(0);
            draw_title(120, 50);
            draw_ovr(|| cls(0));
            false
        }
    }
}

static MENU_STATE: RwLock<MenuState> = RwLock::new(MenuState::new());

pub struct MenuState {
    index: usize,
    reset_protector: usize,
}
impl MenuState {
    pub const fn new() -> Self {
        Self {
            index: 0,
            reset_protector: 0,
        }
    }
    pub fn step_main_menu(&mut self) -> Option<MainMenuOption> {
        let (menu_index, clicked) = step_menu(2, 88, &mut self.index);
        if mem_btnp(4) || clicked {
            self.index = 0;
            match menu_index {
                0 => return Some(MainMenuOption::Play),
                1 => return Some(MainMenuOption::Options),
                _ => {}
            };
        }
        None
    }
    pub fn draw_main_menu(&self) {
        use crate::dialogue_data::{MENU_OPTIONS, MENU_PLAY};
        cls(0);

        draw_title(120, 50);

        let strings = [MENU_PLAY, MENU_OPTIONS];
        let current_option = self.index;
        draw_menu(&strings, 120, 88, current_option);
    }
    fn step_options(&mut self) -> bool {
        let (menu_index, clicked) = step_menu(3, 40, &mut self.index);
        if menu_index != 2 {
            self.reset_protector = 0;
        };
        if mem_btnp(4) || clicked {
            match menu_index {
                0 => return false,
                1 => {
                    DIALOGUE_OPTIONS.toggle_small_text();
                }
                2 => {
                    if self.reset_protector == 0 {
                        self.reset_protector += 1;
                    } else {
                        self.index = 0;
                        self.reset_protector = 0;
                        unsafe {
                            for byte in (*PERSISTENT_RAM).iter_mut() {
                                *byte = 0;
                            }
                        }
                        return false;
                    };
                }
                _ => {}
            };
        }
        true
    }

    pub fn draw_options(&self) {
        cls(0);
        use crate::dialogue_data::{
            MENU_BACK, OPTIONS_FONT_SIZE, OPTIONS_LOSE_DATA, OPTIONS_RESET, OPTIONS_RESET_SURE,
        };
        let reset_string = if self.reset_protector == 0 {
            OPTIONS_RESET
        } else {
            OPTIONS_RESET_SURE
        };
        let strings = [MENU_BACK, OPTIONS_FONT_SIZE, reset_string];
        let current_option = self.index;
        if current_option == 2 {
            rect(60, 10, 120, 11, 2);
            print_raw_centered(
                OPTIONS_LOSE_DATA,
                120,
                13,
                PrintOptions {
                    color: 12,
                    ..DIALOGUE_OPTIONS.get_options()
                },
            );
        }
        draw_menu(&strings, 120, 40, current_option);
    }
}

pub enum MainMenuOption {
    Play,
    Options,
}

pub fn draw_menu(entries: &[&str], x: i32, y: i32, current_option: usize) {
    for (i, string) in entries.iter().enumerate() {
        let color = if i == current_option { 4 } else { 3 };
        if i == current_option {
            rect(0, y + i as i32 * 8 - 1, 240, 8, 1);
        }
        print_raw_centered(
            string,
            x,
            y + i as i32 * 8,
            PrintOptions {
                color,
                ..DIALOGUE_OPTIONS.get_options()
            },
        );
    }
}

pub fn step_menu(entries: usize, y: i16, index: &mut usize) -> (usize, bool) {
    let old_index = *index;

    let mouse_pos = Vec2::new(mouse().x, mouse().y);
    let mouse_delta = mouse_delta();
    let mut clicked = false;
    for i in 0..entries {
        if Hitbox::new(0, y + 8 * i as i16, 240, 8).touches_point(mouse_pos) {
            clicked = mouse_delta.left;
            if mouse_delta.x != 0 || mouse_delta.y != 0 || clicked {
                *index = i;
            }
        }
    }
    if mem_btnp(0) {
        *index = old_index.saturating_sub(1);
    }
    if mem_btnp(1) {
        *index = old_index.saturating_add(1).min(entries - 1);
    }

    (*index, clicked)
}

pub fn draw_title(x: i32, y: i32) {
    use crate::dialogue_data::{GAME_TITLE, GAME_TITLE_BLURB};
    let title_width = print_raw(
        GAME_TITLE,
        999,
        999,
        PrintOptions {
            scale: 1,
            ..Default::default()
        },
    );
    print_raw_centered(
        GAME_TITLE,
        x,
        y + 23,
        PrintOptions {
            scale: 1,
            color: 2,
            ..Default::default()
        },
    );
    print_raw_centered(
        GAME_TITLE_BLURB,
        x,
        y + 30,
        PrintOptions {
            scale: 1,
            color: 13,
            small_text: true,
            ..Default::default()
        },
    );

    rect(120 - title_width / 2, y + 19, title_width - 1, 2, 2);

    blit_segment(8);
    spr(
        1086,
        120 - 8,
        y + ((frames() / 30) % 2),
        SpriteOptions {
            transparent: &[0],
            scale: 1,
            w: 2,
            h: 2,
            ..Default::default()
        },
    );
    blit_segment(4);
}
