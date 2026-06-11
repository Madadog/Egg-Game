use crate::system::PrintOptions;
use crate::system::SpriteOptions;

use crate::Ctx;
use crate::camera::CameraBounds;
use crate::data::script::Script;
use crate::data::sound;
use crate::dialogue::print_options;
use crate::map::MapStore;
use crate::system::{ConsoleApi, ConsoleHelper, just_pressed};
use crate::ui::{Ui, UiBuilder};

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
    pub fn debug_options(script: &Script) -> Self {
        let mut entries = vec![MenuEntry::Walk];
        entries.extend(
            (0..script.list("menu_debug_controls").len())
                .map(|x| MenuEntry::Debug(x as u8))
                .chain([MenuEntry::MapTest]),
        );
        Self {
            entries,
            draw_title: None,
            back_entry: Some(MenuEntry::Walk),
            ..Self::new()
        }
    }
    /// The debug map-test menu: one entry per loaded modern map (legacy maps
    /// are reachable through their warps; modern maps need an entry point).
    pub fn map_select(maps: &MapStore) -> Self {
        let entries = std::iter::once(MenuEntry::Debug(6))
            .chain(
                maps.names()
                    .into_iter()
                    .filter(|name| maps.is_modern(name))
                    .map(|name| MenuEntry::MapSelect(name.to_string())),
            )
            .collect();
        Self {
            entries,
            draw_title: None,
            back_entry: Some(MenuEntry::Debug(6)),
            ..Self::new()
        }
    }
    pub fn step_main_menu(
        &mut self,
        ctx: &mut Ctx<impl ConsoleApi>,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
    ) -> Option<GameMode> {
        let old_index = self.index;
        let entries = self.entries.len();
        let ui = self.build_ui(ctx.system, ctx.script);
        let mouse = ctx.system.mouse();
        let pad = ctx.system.controller();
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
            ctx.system.play_sound(sound::CLICK);
        }
        let (index, action) = if just_pressed(pad.a) || clicked {
            (Some(menu_index), true)
        } else if just_pressed(pad.b) && self.back_entry.is_some() {
            (None, true)
        } else {
            (None, false)
        };
        if action {
            ctx.system.play_sound(sound::INTERACT);
            self.click(index, ctx, walkaround_state, inventory_ui)
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
    pub fn build_ui(&self, system: &mut impl ConsoleApi, script: &Script) -> Ui<usize> {
        let small = system.memory().small_text_on;
        let texts: Vec<String> = self.entries.iter().map(|e| e.text(script)).collect();
        let screen = (system.width() as f32, system.height() as f32);
        let mut builder = UiBuilder::new();
        let rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, _entry)| {
                let selected = i == self.index;
                builder
                    .text(texts[i].as_str())
                    .color(if selected { 4 } else { 3 })
                    .center()
                    .small(small)
                    .full_width(8.0)
                    .fill_if(selected, 1)
                    .key(i)
                    .id()
            })
            .collect();
        let root = builder
            .column(0.0, rows)
            .size(screen.0, screen.1)
            .pad_lrtb(0.0, 0.0, self.entry_height() as f32, 0.0)
            .id();
        builder.finish(root, screen)
    }
    pub fn click(
        &mut self,
        index: Option<usize>,
        ctx: &mut Ctx<impl ConsoleApi>,
        walkaround_state: &mut WalkaroundState,
        inventory_ui: &mut InventoryUi,
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
            FontSize => {
                let save = ctx.system.memory();
                save.small_text_on = !save.small_text_on;
            }
            Reset(x) => {
                if *x == 0 {
                    *x += 1;
                } else {
                    ctx.system.reset_save_data();
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
                        ctx.draw.set_palette(&crate::system::SWEETIE_16);
                    }
                    1 => {
                        ctx.draw.set_palette(&crate::system::NIGHT_16);
                    }
                    2 => {
                        ctx.draw.set_palette(&crate::system::B_W);
                    }
                    3 => {
                        *walk.cam_state() = CameraBounds::free();
                    }
                    4 => {
                        walk.execute_interact_fn(&crate::interact::InteractFn::ToggleDog, ctx.system);
                    }
                    5 => {
                        walk.execute_interact_fn(
                            &crate::interact::InteractFn::AddCreatures(1),
                            ctx.system,
                        );
                    }
                    6 => return Some(GameMode::MainMenu(MenuState::debug_options(ctx.script))),
                    _ => {}
                }
            }
            Walk => return Some(GameMode::Walkaround),
            MapTest => return Some(GameMode::MainMenu(MenuState::map_select(ctx.maps))),
            MapSelect(name) => {
                walkaround_state.load_map_by_name(ctx.system, &ctx.draw.indexed_sprites, ctx.maps, name);
            }
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
        script: &Script,
        index: usize,
    ) {
        use crate::drawstate::LayerId::*;
        use crate::system::drawing::Canvas;
        use MenuEntry::*;
        if let Reset(_) = self.entries[index] {
            let c2 = draw_state.colour(2);
            let c12 = draw_state.colour(12);
            let options = print_options(system);
            let lose_data = script.label("options_lose_data");
            draw_state.rgba(BG).fill_rect(60, 10, 120, 11, c2);
            system.print_to_centered(
                draw_state.rgba(BG),
                &lose_data,
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
    pub fn draw_main_menu(&self, ctx: &mut Ctx<impl ConsoleApi>, elapsed_frames: i32) {
        use crate::drawstate::LayerId::*;
        use crate::system::drawing::{Canvas, EdgePolicy, Transform};
        use crate::system::drawing::image::RgbaImage;

        let c0 = ctx.draw.colour(0);
        ctx.draw.rgba(BG).fill(c0);

        if let Some(key) = self.draw_title {
            let title = ctx.script.label(key);
            draw_title_rgba(ctx.draw, ctx.system, ctx.script, 120, 53, &title, elapsed_frames);
        }

        self.build_ui(ctx.system, ctx.script).draw(ctx.draw, ctx.system, BG);
        self.hover(ctx.draw, ctx.system, ctx.script, self.index);

        let output = ctx.system.output_image();
        output.blit::<RgbaImage>(
            0,
            0,
            ctx.draw.rgba(BG),
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
    MapSelect(String),
    Walk,
}
impl MenuEntry {
    pub fn text(&self, script: &Script) -> String {
        use MenuEntry::*;

        match self {
            Play => script.label("menu_play"),
            Options => script.label("menu_options"),
            MainMenu => script.label("menu_back"),
            FontSize => script.label("options_font_size"),
            Reset(x) => {
                if *x == 0 {
                    script.label("options_reset")
                } else {
                    script.label("options_reset_sure")
                }
            }
            Inventory => script.label("menu_back"),
            ExitToMenu => script.label("menu_exit"),
            _Space => String::new(),
            Debug(x) => script
                .list_get("menu_debug_controls", usize::from(*x))
                .unwrap_or_default(),
            MapTest => script.label("menu_map_test"),
            Walk => script.label("menu_play"),
            MapSelect(name) => name.clone(),
        }
    }
}

/// Draw the centred game title, its underline, and the corner blurb onto any
/// canvas, returning the measured title width. The egg icon is blitted
/// separately by each caller — the indexed and RGBA paths differ.
#[allow(clippy::too_many_arguments)]
fn draw_title_text<C: crate::system::drawing::Canvas>(
    canvas: &mut C,
    system: &impl ConsoleApi,
    script: &Script,
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
        &script.label("game_title_blurb"),
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
#[allow(clippy::too_many_arguments)]
pub fn draw_title_indexed(
    canvas: &mut crate::system::drawing::image::IndexedImage,
    indexed_sprites: &crate::system::drawing::image::IndexedImage,
    system: &impl ConsoleApi,
    script: &Script,
    x: i32,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    draw_title_text(canvas, system, script, x, y, game_title, 2u8, 14u8);
    canvas.spr(
        indexed_sprites,
        534,
        120 - 8,
        y + ((elapsed_frames / 30) % 2),
        SpriteOptions {
            transparent: Some(0),
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
    script: &Script,
    x: i32,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    use crate::drawstate::{LayerId::*, PALETTE_MAP_IDENTITY};
    let c2 = draw_state.colour(2);
    let c14 = draw_state.colour(14);
    draw_title_text(draw_state.rgba(BG), system, script, x, y, game_title, c2, c14);
    draw_state.spr(
        BG,
        &PALETTE_MAP_IDENTITY,
        534,
        120 - 8,
        y + ((elapsed_frames / 30) % 2),
        SpriteOptions {
            transparent: Some(0),
            scale: 1,
            w: 2,
            h: 2,
            ..Default::default()
        },
    );
}
