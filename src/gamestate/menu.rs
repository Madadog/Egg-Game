use crate::dialogue::DIALOGUE_OPTIONS;
use crate::frames;
use crate::input_manager::*;
use crate::position::*;
use crate::tic80::*;
use crate::tic_helpers::*;

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

        draw_title(120, 53);

        let strings = [MENU_PLAY, MENU_OPTIONS];
        let current_option = self.index;
        draw_menu(&strings, 120, 88, current_option);
    }
    pub fn step_options(&mut self) -> bool {
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
