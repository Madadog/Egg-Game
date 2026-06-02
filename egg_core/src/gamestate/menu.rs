use crate::system::PrintOptions;
use crate::system::StaticSpriteOptions;
use crate::system::{HEIGHT, WIDTH};

use crate::camera::CameraBounds;
use crate::data::sound;
use crate::dialogue::DIALOGUE_OPTIONS;
use crate::system::{ConsoleApi, ConsoleHelper, just_pressed};
use crate::ui::{self, Content, Decoration, Style, Ui, UiBuilder};

use super::GameMode;
use super::inventory::InventoryUi;
use super::walkaround::WalkaroundState;

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
            draw_title: Some("game_title"),
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
    pub fn debug_options(system: &impl ConsoleApi) -> Self {
        let mut entries = vec![MenuEntry::Walk];
        entries.extend(
            (0..system.list("menu_debug_controls").len())
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
        draw_state: &mut crate::drawstate::DrawState,
        system: &mut impl ConsoleApi,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
    ) -> Option<GameMode> {
        let old_index = self.index;
        let entries = self.entries.len();
        let ui = self.build_ui(system);
        let mouse = system.mouse();
        let pad = system.controller();
        let mut clicked = false;
        if let Some(i) = ui.hit(mouse.pos()) {
            if mouse.moved() {
                self.index = i;
            }
            if just_pressed(mouse.left) {
                self.index = i;
                clicked = true;
            }
        }
        if just_pressed(pad.up) {
            self.index = old_index.checked_sub(1).unwrap_or(entries - 1);
        }
        if just_pressed(pad.down) {
            self.index = old_index.saturating_add(1) % entries;
        }
        let menu_index = self.index;
        if old_index != menu_index {
            self.exit_hover(old_index);
            system.play_sound(sound::CLICK);
        }
        let (index, action) = if just_pressed(pad.a) || clicked {
            (Some(menu_index), true)
        } else if just_pressed(pad.b) && self.back_entry.is_some() {
            (None, true)
        } else {
            (None, false)
        };
        if action {
            system.play_sound(sound::INTERACT);
            self.click(index, draw_state, walkaround_state, inventory_ui, system)
        } else {
            None
        }
    }
    pub fn entry_height(&self) -> i16 {
        if self.draw_title.is_some() { 88 } else { 40 }
    }
    /// Lay the menu out as a full-screen vertical column of selectable rows,
    /// one per entry and keyed by its index. Rebuilt each frame for both
    /// hit-testing (`step`) and drawing.
    pub fn build_ui(&self, system: &mut impl ConsoleApi) -> Ui<usize> {
        let small = DIALOGUE_OPTIONS.small_text(system);
        let texts: Vec<String> = self.entries.iter().map(|e| e.text(system)).collect();
        let mut builder = UiBuilder::new();
        let rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, _entry)| {
                let selected = i == self.index;
                builder.leaf(
                    Style { size: ui::full_width(8.0), ..Default::default() },
                    Content::Text {
                        text: texts[i].clone(),
                        color: if selected { 4 } else { 3 },
                        center: true,
                        small,
                    },
                    if selected { Decoration::fill(1) } else { Decoration::default() },
                    Some(i),
                )
            })
            .collect();
        let root = builder.container(
            Style {
                size: ui::size(WIDTH as f32, HEIGHT as f32),
                padding: ui::pad_lrtb(0.0, 0.0, self.entry_height() as f32, 0.0),
                ..ui::column(0.0)
            },
            Decoration::default(),
            None,
            &rows,
        );
        builder.finish(root)
    }
    pub fn click(
        &mut self,
        index: Option<usize>,
        draw_state: &mut crate::drawstate::DrawState,
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
                self.draw_title = Some("options_title");
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
                    system.reset_save_data();
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
                        draw_state.set_palette(&crate::system::SWEETIE_16);
                    }
                    1 => {
                        draw_state.set_palette(&crate::system::NIGHT_16);
                    }
                    2 => {
                        draw_state.set_palette(&crate::system::B_W);
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
                    6 => return Some(GameMode::MainMenu(MenuState::debug_options(system))),
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
        if let Reset(x) = &mut self.entries[index] {
            *x = 0
        }
    }
    fn hover(
        &self,
        draw_state: &mut crate::drawstate::DrawState,
        system: &mut impl ConsoleApi,
        index: usize,
    ) {
        use crate::drawstate::LayerId::*;
        use crate::system::drawing::Canvas;
        use MenuEntry::*;
        if let Reset(_) = self.entries[index] {
            let c2 = draw_state.colour(2);
            let c12 = draw_state.colour(12);
            let options = DIALOGUE_OPTIONS.get_options(system);
            draw_state.rgba(BG).fill_rect(60, 10, 120, 11, c2);
            system.print_to_centered(
                draw_state.rgba(BG),
                &system.label("options_lose_data"),
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
        use crate::drawstate::LayerId::*;
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::image::RgbaImage;

        let c0 = draw_state.colour(0);
        draw_state.rgba(BG).fill(c0);

        if let Some(key) = self.draw_title {
            draw_title_rgba(draw_state, system, 120, 53, &system.label(key), elapsed_frames);
        }

        self.build_ui(system).draw(draw_state, system, BG);
        self.hover(draw_state, system, self.index);

        let output = system.output_image();
        output.blit::<RgbaImage>(
            0,
            0,
            draw_state.rgba(BG),
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
    pub fn text(&self, system: &impl ConsoleApi) -> String {
        use MenuEntry::*;

        match self {
            Play => system.label("menu_play"),
            Options => system.label("menu_options"),
            MainMenu => system.label("menu_back"),
            FontSize => system.label("options_font_size"),
            Reset(x) => {
                if *x == 0 {
                    system.label("options_reset")
                } else {
                    system.label("options_reset_sure")
                }
            }
            Inventory => system.label("menu_back"),
            ExitToMenu => system.label("menu_exit"),
            _Space => String::new(),
            Debug(x) => (|list_key: &str, i: usize| system.list(list_key).into_iter().nth(i).unwrap_or_default())("menu_debug_controls", usize::from(*x)),
            MapTest => system.label("menu_map_test"),
            MusicTest => system.label("menu_music_test"),
            Walk => system.label("menu_play"),
            MapBankSelect(_, string) => string.clone(),
            _MusicSelect(_, string) => string.clone(),
        }
    }
}

/// Draw the centred game title, its underline, and the corner blurb onto any
/// canvas, returning the measured title width. The egg icon is blitted
/// separately by each caller — the indexed and RGBA paths differ.
fn draw_title_text<C: crate::system::drawing::Canvas>(
    canvas: &mut C,
    system: &impl ConsoleApi,
    x: i32,
    y: i32,
    game_title: &str,
    title_colour: C::Pixel,
    blurb_colour: C::Pixel,
) -> i32 {
    let opts = PrintOptions {
        scale: 1,
        ..Default::default()
    };
    let title_width = system.print_to(canvas, game_title, 999, 999, title_colour, opts.clone());
    system.print_to_centered(canvas, game_title, x, y + 23, title_colour, opts);
    system.print_to(
        canvas,
        &system.label("game_title_blurb"),
        3,
        3,
        blurb_colour,
        PrintOptions {
            scale: 1,
            small_text: true,
            ..Default::default()
        },
    );
    canvas.fill_rect(120 - title_width / 2, y + 19, title_width - 1, 2, title_colour);
    title_width
}

/// Indexed-canvas variant of the title screen, used by the migrated intro
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
    draw_title_text(canvas, system, x, y, game_title, 2u8, 14u8);
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

/// RGBA-canvas variant of the title screen, used by the migrated main menu.
pub fn draw_title_rgba(
    draw_state: &mut crate::drawstate::DrawState,
    system: &impl ConsoleApi,
    x: i32,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    use crate::drawstate::{LayerId::*, PALETTE_MAP_IDENTITY};
    let c2 = draw_state.colour(2);
    let c14 = draw_state.colour(14);
    draw_title_text(draw_state.rgba(BG), system, x, y, game_title, c2, c14);
    draw_state.spr(
        BG,
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
