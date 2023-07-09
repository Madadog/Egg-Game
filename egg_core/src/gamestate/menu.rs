use crate::camera::CameraBounds;
use crate::data::dialogue_data::GAME_TITLE;
use crate::data::dialogue_data::OPTIONS_TITLE;
use crate::data::sound;
use crate::dialogue::DIALOGUE_OPTIONS;
use crate::position::*;
use tic80_api::core::*;
use tic80_api::helpers::input_manager::*;
use tic80_api::helpers::*;

use super::EggInput;
use super::EggMemory;
use super::inventory;
use super::inventory::Inventory;
use super::inventory::InventoryUi;
use super::walkaround::WalkaroundState;
use super::GameState;

#[derive(Debug)]
pub struct MenuState {
    index: usize,
    entries: Vec<MenuEntry>,
    draw_title: Option<&'static str>,
    back_entry: Option<MenuEntry>,
}
impl MenuState {
    pub fn new() -> Self {
        Self {
            index: 0,
            entries: vec![MenuEntry::Play, MenuEntry::Options],
            draw_title: Some(GAME_TITLE),
            back_entry: None,
        }
    }
    pub fn inventory_options() -> Self {
        Self {
            entries: vec![
                MenuEntry::Inventory,
                MenuEntry::FontSize,
                MenuEntry::ExitToMenu,
            ],
            draw_title: None,
            back_entry: Some(MenuEntry::Inventory),
            ..Self::new()
        }
    }
    pub fn debug_options() -> Self {
        let mut entries = vec![MenuEntry::Walk];
        entries.extend(
            (0..crate::data::dialogue_data::MENU_DEBUG_CONTROLS.len())
                .map(|x| MenuEntry::Debug(x as u8)),
        );
        Self {
            entries,
            draw_title: None,
            back_entry: Some(MenuEntry::Walk),
            ..Self::new()
        }
    }
    pub fn step_main_menu(
        &mut self,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
        memory: &mut EggMemory,
        input: &EggInput,
    ) -> Option<GameState> {
        println!("Stepping menu");
        let old_index = self.index;
        let (menu_index, clicked) =
            step_menu(self.entries.len(), self.entry_height(), &mut self.index, input);
        if old_index != menu_index {
            self.exit_hover(old_index);
            sound::CLICK.play()
        }
        let (index, action) = if input.mem_btnp(4) || clicked {
            (Some(menu_index), true)
        } else if input.mem_btnp(5) && self.back_entry.is_some() {
            (None, true)
        } else {
            (None, false)
        };
        println!("Stepping menu done");
        if action {
            println!("interaction");
            sound::INTERACT.play();
            self.click(index, walkaround_state, inventory_ui, memory)
        } else {
            println!("returning none");
            None
        }
    }
    pub fn entry_height(&self) -> i16 {
        if self.draw_title.is_some() {
            88
        } else {
            40
        }
    }
    pub fn click(
        &mut self,
        index: Option<usize>,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
        memory: &mut EggMemory
    ) -> Option<GameState> {
        use MenuEntry::*;
        let x = if let Some(index) = index {
            &mut self.entries[index]
        } else if let Some(entry) = &mut self.back_entry {
            entry
        } else {
            return None;
        };
        match x {
            Play => return Some(GameState::Instructions(0)),
            Options => {
                self.index = 0;
                self.draw_title = Some(OPTIONS_TITLE);
                self.entries = vec![MainMenu, FontSize, Reset(0)];
                self.back_entry = Some(MainMenu);
            }
            MainMenu | ExitToMenu => {
                *self = MenuState::new();
            }
            FontSize => DIALOGUE_OPTIONS.toggle_small_text(),
            Reset(x) => {
                if *x == 0 {
                    *x += 1;
                } else {
                    crate::data::save::zero_pmem();
                    return Some(GameState::Animation(0));
                }
            }
            Inventory => {
                inventory_ui.state = crate::gamestate::inventory::InventoryUiState::PageSelect(2);
                return Some(GameState::Inventory);
            }
            Space => {}
            Debug(x) => {
                let mut walk = walkaround_state;
                match x {
                    0 => {
                        set_palette(tic80_api::helpers::SWEETIE_16);
                    }
                    1 => {
                        set_palette(tic80_api::helpers::NIGHT_16);
                    }
                    2 => {
                        set_palette(tic80_api::helpers::B_W);
                    }
                    3 => {
                        *walk.cam_state() = CameraBounds::free();
                    }
                    4 => {
                        walk.execute_interact_fn(&crate::interact::InteractFn::ToggleDog, memory);
                    }
                    5 => {
                        walk.execute_interact_fn(
                            &crate::interact::InteractFn::AddCreatures(1),
                            memory,
                        );
                    }
                    _ => {}
                }
            }
            Walk => return Some(GameState::Walkaround),
        };
        None
    }
    pub fn exit_hover(&mut self, index: usize) {
        use MenuEntry::*;
        match &mut self.entries[index] {
            Reset(x) => *x = 0,
            _ => {}
        }
    }
    fn hover(&self, index: usize) {
        use crate::data::dialogue_data::OPTIONS_LOSE_DATA;
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
    pub fn draw_main_menu(&self, elapsed_frames: i32) {
        cls(0);

        if let Some(string) = self.draw_title {
            draw_title(120, 53, string, elapsed_frames);
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
    Inventory,
    ExitToMenu,
    Space,
    Debug(u8),
    Walk,
}
impl MenuEntry {
    pub fn text(&self) -> &'static str {
        use crate::data::dialogue_data::*;
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
            Inventory => MENU_BACK,
            ExitToMenu => MENU_EXIT,
            Space => "\0",
            Debug(x) => MENU_DEBUG_CONTROLS[usize::from(*x)],
            Walk => MENU_PLAY,
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

pub fn step_menu(entries: usize, y: i16, index: &mut usize, input: &EggInput) -> (usize, bool) {
    let old_index = *index;

    let mouse_pos = Vec2::new(input.mouse().x, input.mouse().y);
    let mouse_delta = input.mouse_delta();
    let mut clicked = false;
    for i in 0..entries {
        if Hitbox::new(0, y + 8 * i as i16, 240, 8).touches_point(mouse_pos) {
            clicked = mouse_delta.left;
            if mouse_delta.x != 0 || mouse_delta.y != 0 || clicked {
                *index = i;
            }
        }
    }
    if input.mem_btnp(0) {
        match old_index.checked_sub(1) {
            Some(x) => *index = x,
            None => *index = entries - 1,
        }
    }
    if input.mem_btnp(1) {
        *index = old_index.saturating_add(1) % entries;
    }

    (*index, clicked)
}

pub fn draw_title(x: i32, y: i32, game_title: &str, elapsed_frames: i32) {
    use crate::data::dialogue_data::GAME_TITLE_BLURB;
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
        y + ((elapsed_frames / 30) % 2),
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
