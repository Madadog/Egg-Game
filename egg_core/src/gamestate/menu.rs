use crate::system::PrintOptions;
use crate::system::StaticSpriteOptions;
use crate::system::WIDTH;

use crate::camera::CameraBounds;
use crate::data::dialogue_data::GAME_TITLE;
use crate::data::dialogue_data::OPTIONS_TITLE;
use crate::data::sound;
use crate::dialogue::DIALOGUE_OPTIONS;
use crate::position::*;
use crate::system::{ConsoleApi, ConsoleHelper};

use super::inventory::InventoryUi;
use super::walkaround::WalkaroundState;
use super::GameMode;

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
                .map(|x| MenuEntry::Debug(x as u8))
                .chain([MenuEntry::MapTest, MenuEntry::MusicTest]),
        );
        Self {
            entries,
            draw_title: None,
            back_entry: Some(MenuEntry::Walk),
            ..Self::new()
        }
    }
    pub fn map_select() -> Self {
        let entries = vec![
            MenuEntry::Debug(6),
            MenuEntry::MapBankSelect(2, "Map Bank: 0".into()),
        ];
        Self {
            entries,
            draw_title: None,
            back_entry: Some(MenuEntry::Debug(6)),
            ..Self::new()
        }
    }
    pub fn step_main_menu(
        &mut self,
        system: &mut impl ConsoleApi,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
    ) -> Option<GameMode> {
        let old_index = self.index;
        let (menu_index, clicked) = step_menu(
            self.entries.len(),
            self.entry_height(),
            &mut self.index,
            system,
        );
        if old_index != menu_index {
            self.exit_hover(old_index);
            system.play_sound(sound::CLICK);
        }
        let (index, action) = if system.mem_btnp(4) || clicked {
            (Some(menu_index), true)
        } else if system.mem_btnp(5) && self.back_entry.is_some() {
            (None, true)
        } else {
            (None, false)
        };
        if action {
            system.play_sound(sound::INTERACT);
            self.click(index, walkaround_state, inventory_ui, system)
        } else {
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
        system: &mut impl ConsoleApi,
    ) -> Option<GameMode> {
        use MenuEntry::*;
        let x = if let Some(index) = index {
            &mut self.entries[index]
        } else if let Some(entry) = &mut self.back_entry {
            entry
        } else {
            return None;
        };
        match x {
            Play => return Some(GameMode::Instructions(0)),
            Options => {
                self.index = 0;
                self.draw_title = Some(OPTIONS_TITLE);
                self.entries = vec![MainMenu, FontSize, Reset(0)];
                self.back_entry = Some(MainMenu);
            }
            MainMenu | ExitToMenu => {
                *self = MenuState::new();
            }
            FontSize => DIALOGUE_OPTIONS.toggle_small_text(system),
            Reset(x) => {
                if *x == 0 {
                    *x += 1;
                } else {
                    system.zero_pmem();
                    return Some(GameMode::Animation(0));
                }
            }
            Inventory => {
                inventory_ui.state = crate::gamestate::inventory::InventoryUiState::PageSelect(2);
                return Some(GameMode::Inventory);
            }
            _Space => {}
            Debug(x) => {
                let walk = walkaround_state;
                match x {
                    0 => {
                        system.set_palette(crate::system::SWEETIE_16);
                    }
                    1 => {
                        system.set_palette(crate::system::NIGHT_16);
                    }
                    2 => {
                        system.set_palette(crate::system::B_W);
                    }
                    3 => {
                        *walk.cam_state() = CameraBounds::free();
                    }
                    4 => {
                        walk.execute_interact_fn(&crate::interact::InteractFn::ToggleDog, system);
                    }
                    5 => {
                        walk.execute_interact_fn(
                            &crate::interact::InteractFn::AddCreatures(1),
                            system,
                        );
                    }
                    6 => return Some(GameMode::MainMenu(MenuState::debug_options())),
                    _ => {}
                }
            }
            Walk => return Some(GameMode::Walkaround),
            MapTest => return Some(GameMode::MainMenu(MenuState::map_select())),
            MapBankSelect(_x, _) => {
                walkaround_state.load_map_bank(system, 2);
            }
            MusicTest => todo!(),
            _MusicSelect(_x, _) => todo!(),
        };
        None
    }
    pub fn exit_hover(&mut self, index: usize) {
        use MenuEntry::*;
        if let Reset(x) = &mut self.entries[index] { *x = 0 }
    }
    fn hover(
        &self,
        draw_state: &mut crate::drawstate::DrawState,
        system: &mut impl ConsoleApi,
        index: usize,
    ) {
        use crate::data::dialogue_data::OPTIONS_LOSE_DATA;
        use crate::drawstate::LayerId;
        use crate::system::drawing::Canvas;
        use MenuEntry::*;
        if let Reset(_) = self.entries[index] {
            let bg = LayerId::BG as usize;
            let c2 = draw_state.colour(2);
            let c12 = draw_state.colour(12);
            let options = DIALOGUE_OPTIONS.get_options(system);
            draw_state.rgba_canvas[bg].fill_rect(60, 10, 120, 11, c2);
            system.print_to_centered(
                &mut draw_state.rgba_canvas[bg],
                OPTIONS_LOSE_DATA,
                120,
                13,
                c12,
                PrintOptions {
                    color: 12,
                    ..options
                },
            );
        }
    }
    pub fn draw_main_menu(
        &self,
        draw_state: &mut crate::drawstate::DrawState,
        system: &mut impl ConsoleApi,
        elapsed_frames: i32,
    ) {
        use crate::drawstate::LayerId;
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::image::RgbaImage;

        let bg = LayerId::BG as usize;
        let c0 = draw_state.colour(0);
        draw_state.rgba_canvas[bg].fill(c0);

        if let Some(string) = self.draw_title {
            draw_title_rgba(draw_state, system, 120, 53, string, elapsed_frames);
        }

        let strings: Vec<&str> = self.entries.iter().map(|x| x.text()).collect();
        let current_option = self.index;
        draw_menu(
            draw_state,
            system,
            &strings,
            120,
            self.entry_height().into(),
            current_option,
        );
        self.hover(draw_state, system, current_option);

        let output = system.output_image();
        output.blit::<RgbaImage>(
            0,
            0,
            &draw_state.rgba_canvas[bg],
            EdgePolicy::Transparent,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
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
    _Space,
    Debug(u8),
    MapTest,
    MapBankSelect(u8, String),
    MusicTest,
    _MusicSelect(u8, String),
    Walk,
}
impl MenuEntry {
    pub fn text(&self) -> &str {
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
            _Space => "\0",
            Debug(x) => MENU_DEBUG_CONTROLS[usize::from(*x)],
            MapTest => MENU_MAP_TEST[0],
            MusicTest => MENU_MUSIC_TEST[0],
            Walk => MENU_PLAY,
            MapBankSelect(_, string) => string,
            _MusicSelect(_, string) => string,
        }
    }
}

pub fn draw_menu(
    draw_state: &mut crate::drawstate::DrawState,
    system: &mut impl ConsoleApi,
    entries: &[&str],
    x: i32,
    y: i32,
    current_option: usize,
) {
    use crate::drawstate::LayerId;
    use crate::system::drawing::Canvas;
    let bg = LayerId::BG as usize;
    let c1 = draw_state.colour(1);
    let c3 = draw_state.colour(3);
    let c4 = draw_state.colour(4);
    let options = DIALOGUE_OPTIONS.get_options(system);
    for (i, string) in entries.iter().enumerate() {
        let color = if i == current_option { c4 } else { c3 };
        if i == current_option {
            draw_state.rgba_canvas[bg].fill_rect(0, y + i as i32 * 8 - 1, WIDTH, 8, c1);
        }
        system.print_to_centered(
            &mut draw_state.rgba_canvas[bg],
            string,
            x,
            y + i as i32 * 8,
            color,
            PrintOptions {
                color: if i == current_option { 4 } else { 3 },
                ..options.clone()
            },
        );
    }
}

pub fn step_menu(
    entries: usize,
    y: i16,
    index: &mut usize,
    system: &mut impl ConsoleApi,
) -> (usize, bool) {
    let old_index = *index;

    let mouse_pos = Vec2::new(system.mouse().x, system.mouse().y);
    let mouse_delta = system.mouse_delta();
    let mut clicked = false;
    for i in 0..entries {
        if Hitbox::new(0, y + 8 * i as i16, WIDTH as i16, 8).touches_point(mouse_pos) {
            clicked = mouse_delta.left;
            if mouse_delta.x != 0 || mouse_delta.y != 0 || clicked {
                *index = i;
            }
        }
    }
    if system.mem_btnp(0) {
        match old_index.checked_sub(1) {
            Some(x) => *index = x,
            None => *index = entries - 1,
        }
    }
    if system.mem_btnp(1) {
        *index = old_index.saturating_add(1) % entries;
    }

    (*index, clicked)
}

/// Indexed-canvas variant of [`draw_title`], used by the migrated intro
/// animation so the palette fades apply uniformly to the title pixels.
/// `canvas` is the target indexed layer; `indexed_sprites` is the sprite
/// sheet for the egg icon.
pub fn draw_title_indexed(
    canvas: &mut crate::system::image::IndexedImage,
    indexed_sprites: &crate::system::image::IndexedImage,
    system: &impl ConsoleApi,
    x: i32,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    use crate::data::dialogue_data::GAME_TITLE_BLURB;
    use crate::system::drawing::Canvas;
    let game_title_z = format!("{game_title}\0");
    let title_width = system.print_to(
        canvas,
        &game_title_z,
        999,
        999,
        2u8,
        PrintOptions {
            scale: 1,
            ..Default::default()
        },
    );
    system.print_to_centered(
        canvas,
        &game_title_z,
        x,
        y + 23,
        2u8,
        PrintOptions {
            scale: 1,
            ..Default::default()
        },
    );
    system.print_to(
        canvas,
        GAME_TITLE_BLURB,
        3,
        3,
        14u8,
        PrintOptions {
            scale: 1,
            small_text: true,
            ..Default::default()
        },
    );
    canvas.fill_rect(120 - title_width / 2, y + 19, title_width - 1, 2, 2);
    canvas.spr(
        indexed_sprites,
        534,
        120 - 8,
        y + ((elapsed_frames / 30) % 2),
        StaticSpriteOptions {
            transparent: &[0],
            scale: 1,
            w: 2,
            h: 2,
            ..Default::default()
        },
    );
}

/// RGBA-canvas variant of [`draw_title_indexed`], used by the migrated
/// main menu.
pub fn draw_title_rgba(
    draw_state: &mut crate::drawstate::DrawState,
    system: &impl ConsoleApi,
    x: i32,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    use crate::data::dialogue_data::GAME_TITLE_BLURB;
    use crate::drawstate::{LayerId, PALETTE_MAP_IDENTITY};
    use crate::system::drawing::Canvas;
    let bg = LayerId::BG as usize;
    let c2 = draw_state.colour(2);
    let c14 = draw_state.colour(14);
    let game_title_z = format!("{game_title}\0");
    let title_width = system.print_to(
        &mut draw_state.rgba_canvas[bg],
        &game_title_z,
        999,
        999,
        c2,
        PrintOptions {
            scale: 1,
            ..Default::default()
        },
    );
    system.print_to_centered(
        &mut draw_state.rgba_canvas[bg],
        &game_title_z,
        x,
        y + 23,
        c2,
        PrintOptions {
            scale: 1,
            color: 2,
            ..Default::default()
        },
    );
    system.print_to(
        &mut draw_state.rgba_canvas[bg],
        GAME_TITLE_BLURB,
        3,
        3,
        c14,
        PrintOptions {
            scale: 1,
            color: 14,
            small_text: true,
            ..Default::default()
        },
    );
    draw_state.rgba_canvas[bg].fill_rect(120 - title_width / 2, y + 19, title_width - 1, 2, c2);
    draw_state.spr(
        LayerId::BG,
        &PALETTE_MAP_IDENTITY,
        534,
        120 - 8,
        y + ((elapsed_frames / 30) % 2),
        StaticSpriteOptions {
            transparent: &[0],
            scale: 1,
            w: 2,
            h: 2,
            ..Default::default()
        },
    );
}
