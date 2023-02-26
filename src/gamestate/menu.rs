use crate::dialogue::DIALOGUE_OPTIONS;
use crate::dialogue_data::GAME_TITLE;
use crate::dialogue_data::OPTIONS_TITLE;
use crate::frames;
use crate::input_manager::*;
use crate::position::*;
use crate::sound;
use crate::tic80_core::*;
use crate::tic80_helpers::*;

use super::GameState;

#[derive(Debug)]
pub struct MenuState {
    index: usize,
    entries: Vec<MenuEntry>,
    draw_title: Option<&'static str>,
}
impl MenuState {
    pub fn new() -> Self {
        Self {
            index: 0,
            entries: vec![MenuEntry::Play, MenuEntry::Options],
            draw_title: Some(GAME_TITLE),
        }
    }
    pub fn step_main_menu(&mut self) -> Option<GameState> {
        let old_index = self.index;
        let (menu_index, clicked) =
            step_menu(self.entries.len(), self.entry_height(), &mut self.index);
        if old_index != menu_index {
            self.exit_hover(old_index);
            sound::CLICK.play()
        }
        if mem_btnp(4) || clicked {
            sound::INTERACT.play();
            return self.click(menu_index);
        };
        None
    }
    pub fn entry_height(&self) -> i16 {
        if self.draw_title.is_some() {
            88
        } else {
            40
        }
    }
    pub fn click(&mut self, index: usize) -> Option<GameState> {
        use MenuEntry::*;
        match &mut self.entries[index] {
            Play => return Some(GameState::Instructions(0)),
            Options => {
                self.index = 0;
                self.draw_title = Some(OPTIONS_TITLE);
                self.entries = vec![MainMenu, FontSize, Reset(0)];
            }
            MainMenu => {
                *self = MenuState::new();
            }
            FontSize => DIALOGUE_OPTIONS.toggle_small_text(),
            Reset(x) => {
                if *x == 0 {
                    *x += 1;
                } else {
                    crate::save::zero_pmem();
                    return Some(GameState::Animation(0));
                }
            }
        };
        None
    }
    pub fn exit_hover(&mut self, index: usize) {
        use MenuEntry::*;
        match &mut self.entries[index] {
            Reset(x) => {*x = 0},
            _ => {},
        }
    }
    fn hover(&self, index: usize) {
        use crate::dialogue_data::OPTIONS_LOSE_DATA;
        use MenuEntry::*;
        match self.entries[index] {
            Reset(_) => {
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
            _ => {}
        }
    }
    pub fn draw_main_menu(&self) {
        cls(0);

        if let Some(string) = self.draw_title {
            draw_title(120, 53, string);
        }

        let strings: Vec<&str> = self.entries.iter().map(|x| x.text()).collect();
        let current_option = self.index;
        draw_menu(&strings, 120, self.entry_height().into(), current_option);
        self.hover(current_option);
    }
}

#[derive(Debug)]
pub enum MenuEntry {
    Play,
    Options,
    MainMenu,
    FontSize,
    Reset(u8),
}
impl MenuEntry {
    pub fn text(&self) -> &'static str {
        use crate::dialogue_data::{
            MENU_BACK, MENU_OPTIONS, MENU_PLAY, OPTIONS_FONT_SIZE, OPTIONS_RESET,
            OPTIONS_RESET_SURE,
        };
        use MenuEntry::*;

        match self {
            Play => MENU_PLAY,
            Options => MENU_OPTIONS,
            MainMenu => MENU_BACK,
            FontSize => OPTIONS_FONT_SIZE,
            Reset(x) => {
                if *x == 0 {
                    OPTIONS_RESET
                } else {
                    OPTIONS_RESET_SURE
                }
            }
        }
    }
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

pub fn draw_title(x: i32, y: i32, game_title: &str) {
    use crate::dialogue_data::GAME_TITLE_BLURB;
    let game_title = &format!("{game_title}\0");
    let title_width = print_raw(
        game_title,
        999,
        999,
        PrintOptions {
            scale: 1,
            ..Default::default()
        },
    );
    print_raw_centered(
        game_title,
        x,
        y + 23,
        PrintOptions {
            scale: 1,
            color: 2,
            ..Default::default()
        },
    );
    print_raw(
        GAME_TITLE_BLURB,
        3,
        3,
        PrintOptions {
            scale: 1,
            color: 14,
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
