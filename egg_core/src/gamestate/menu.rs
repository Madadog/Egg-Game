use crate::render::PrintOptions;
use crate::render::SpriteOptions;
use crate::render::{Font, print_to_centered_with_font, print_to_with_font};

use crate::Ctx;
use crate::data::save::SaveData;
use crate::data::script::Script;
use crate::data::sound;
use crate::platform::{ConsoleApi, ConsoleHelper, just_pressed};
use crate::ui::dialogue::print_options;
use crate::ui::layout::{Ui, UiBuilder};
use crate::world::camera::CameraBounds;
use crate::world::map::MapStore;

use super::GameMode;
use super::walkaround::WalkaroundState;
use super::walkaround::inventory::InventoryUi;

#[derive(Debug)]
pub struct MenuState {
    index: usize,
    entries: Vec<MenuEntry>,
    draw_title: Option<&'static str>,
    back_entry: Option<MenuEntry>,
}
impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
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
    /// The debug map-test menu: one entry per loaded map, so any map can be
    /// jumped to directly rather than only reached through its warps.
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
    ) -> Option<GameMode> {
        let old_index = self.index;
        let entries = self.entries.len();
        let ui = self.build_ui(&*ctx);
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
            self.click(index, ctx, walkaround_state)
        } else {
            None
        }
    }
    pub fn entry_height(&self) -> i16 {
        if self.draw_title.is_some() { 88 } else { 40 }
    }
    /// Lay the menu out as a full-screen vertical column of selectable rows,
    /// one per entry and keyed by its index. Rebuilt each frame for both
    /// hit-testing (`step`) and drawing. A pure read-only builder: it only reads
    /// `ctx` (the save's small-text flag, the screen size, the script), so it
    /// takes `&Ctx` rather than the old `&mut ConsoleApi` (which it needed only
    /// for the now-removed `memory()`).
    pub fn build_ui<S: ConsoleApi>(&self, ctx: &Ctx<S>) -> Ui<usize> {
        let small = ctx.save.small_text_on;
        let texts: Vec<String> = self
            .entries
            .iter()
            .map(|e| e.text(ctx.script, ctx.save))
            .collect();
        // Centre the menu against the render target (the framebuffer being drawn
        // into), so it re-centres at any window size — and stays consistent
        // between this layout pass and the matching hit-test pass in `step`.
        let (sw, sh) = ctx.draw.size();
        let screen = (sw as f32, sh as f32);
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
        // Shift the entries down by the same `d` as the title (0 at the base
        // height), so the title+menu block stays vertically centred — and keeps
        // its canonical gap below the title — at any framebuffer height.
        let d = (sh - crate::platform::HEIGHT) / 2;
        let root = builder
            .column(0.0, rows)
            .size(screen.0, screen.1)
            .pad_lrtb(0.0, 0.0, (self.entry_height() as i32 + d) as f32, 0.0)
            .id();
        builder.finish(root, screen)
    }
    pub fn click(
        &mut self,
        index: Option<usize>,
        ctx: &mut Ctx<impl ConsoleApi>,
        walkaround_state: &mut WalkaroundState,
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
            Play => return Some(GameMode::Instructions),
            Options => {
                self.index = 0;
                self.draw_title = Some("options_title");
                self.entries = vec![MainMenu, FontSize, AutoDoors, Reset(0)];
                self.back_entry = Some(MainMenu);
            }
            // Back to the main menu from the in-place Options sub-screen — same
            // mode, just reset content.
            MainMenu => {
                *self = MenuState::new();
            }
            // Leaving the inventory's options menu: a real mode change back to
            // the title's main menu, rebuilt by `enter`.
            ExitToMenu => return Some(GameMode::MainMenu),
            FontSize => {
                ctx.save.small_text_on = !ctx.save.small_text_on;
            }
            AutoDoors => {
                // `manual_doors` is the stored preference; the menu phrases it
                // positively ("automatic doors"), so the toggle inverts it.
                ctx.save.manual_doors = !ctx.save.manual_doors;
            }
            Reset(x) => {
                if *x == 0 {
                    *x += 1;
                } else {
                    *ctx.save = SaveData::default();
                    // Erasing zeroes the save, but the LIVE inventory lives on
                    // `walkaround_state.inventory_ui`, and `run` re-syncs
                    // `save.inventory = inventory_ui…to_save()` at the end of every
                    // frame — so without this the stale items would be written
                    // straight back over the just-erased default and the erase
                    // undone. Rebuild it to the fresh starting items (ff/lm/chegg),
                    // matching `new_game`'s `*self = Self::new()` for the
                    // walkaround. (The walkaround itself, including its parked
                    // `map_entities`, is reset by `new_game` on the ensuing
                    // fresh-game path, and no `save()` runs between here and there
                    // to re-gather stale creatures.)
                    walkaround_state.inventory_ui = InventoryUi::new();
                    return Some(GameMode::Animation);
                }
            }
            Inventory => {
                // Re-open the bag overlay on its options page and resume the
                // walkaround: the bag is no longer a mode, so setting its state
                // (which `is_open` reads as open) and returning to Walkaround
                // makes the overlay step + draw itself again.
                walkaround_state.inventory_ui.state =
                    crate::gamestate::walkaround::inventory::InventoryUiState::PageSelect(2);
                return Some(GameMode::Walkaround);
            }
            _Space => {}
            Debug(x) => {
                let walk = walkaround_state;
                match x {
                    0 => {
                        ctx.draw.set_palette(&crate::platform::SWEETIE_16);
                    }
                    1 => {
                        ctx.draw.set_palette(&crate::platform::NIGHT_16);
                    }
                    2 => {
                        ctx.draw.set_palette(&crate::platform::B_W);
                    }
                    3 => {
                        *walk.cam_state() = CameraBounds::free();
                    }
                    4 => {
                        // The bag lives on `walk`, which `execute_interact_fn` also
                        // borrows mutably, so lift it out and put it straight back.
                        let mut inventory = std::mem::take(&mut walk.inventory_ui.inventory);
                        walk.execute_interact_fn(
                            &crate::world::interact::InteractFn::ToggleDog,
                            ctx.system,
                            &mut inventory,
                            ctx.presets,
                        );
                        walk.inventory_ui.inventory = inventory;
                    }
                    5 => {
                        let mut inventory = std::mem::take(&mut walk.inventory_ui.inventory);
                        walk.execute_interact_fn(
                            &crate::world::interact::InteractFn::AddCreatures(1),
                            ctx.system,
                            &mut inventory,
                            ctx.presets,
                        );
                        walk.inventory_ui.inventory = inventory;
                    }
                    6 => return Some(GameMode::DebugMenu),
                    _ => {}
                }
            }
            Walk => return Some(GameMode::Walkaround),
            MapTest => return Some(GameMode::MapSelect),
            MapSelect(name) => {
                walkaround_state.load_map_by_name(ctx, name);
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
        draw_state: &mut crate::draw_state::DrawState,
        font: &Font,
        script: &Script,
        small_text: bool,
        index: usize,
    ) {
        use crate::draw_state::LayerId::*;
        use crate::render::Canvas;
        use MenuEntry::*;
        if let Reset(_) = self.entries[index] {
            let c2 = draw_state.colour(2);
            let c12 = draw_state.colour(12);
            let options = print_options(small_text);
            let lose_data = script.label("options_lose_data");
            // A 120px-wide tooltip, centred on the framebuffer (60px margins at
            // the base 240 width).
            let (w, _) = draw_state.size();
            draw_state
                .rgba(BG)
                .fill_rect((w - 120) / 2, 10, 120, 11, c2);
            print_to_centered_with_font(
                font,
                draw_state.rgba(BG),
                &lose_data,
                w / 2,
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
        use crate::draw_state::LayerId::*;
        use crate::render::image::RgbaImage;
        use crate::render::{Canvas, EdgePolicy, Transform};

        let c0 = ctx.draw.colour(0);
        ctx.draw.rgba(BG).fill(c0);

        if let Some(key) = self.draw_title {
            let title = ctx.script.label(key);
            // Centre the whole 136-tall title+menu composition vertically on the
            // framebuffer (`d` = 0 at the base height, so the canonical look is
            // unchanged), keeping the title's canonical gap above the entries —
            // `build_ui` shifts the entries by the same `d`.
            let d = (ctx.draw.size().1 - crate::platform::HEIGHT) / 2;
            draw_title_rgba(
                ctx.draw,
                ctx.font,
                ctx.script,
                53 + d,
                &title,
                elapsed_frames,
            );
        }

        self.build_ui(&*ctx).draw(ctx.draw, ctx.font, BG);
        self.hover(
            ctx.draw,
            ctx.font,
            ctx.script,
            ctx.save.small_text_on,
            self.index,
        );

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
    /// Toggle `SaveData::manual_doors` — whether `Interact`-mode warps stop
    /// opening on touch (see `Trigger::warp_fires`).
    AutoDoors,
    Reset(u8),
    Inventory,
    ExitToMenu,
    _Space,
    Debug(u8),
    MapTest,
    MapSelect(String),
    Walk,
}
/// Render a toggle entry's label with its live on/off state — the menu's
/// `[x]`/`[ ]` checkbox convention. The menu UI is rebuilt every frame, so the
/// box updates the moment the entry is activated.
fn checkbox(label: String, on: bool) -> String {
    format!("{label} [{}]", if on { "x" } else { " " })
}

impl MenuEntry {
    /// The entry's display text. Toggle entries read `save` to show their
    /// current state as a `[x]`/`[ ]` checkbox.
    pub fn text(&self, script: &Script, save: &SaveData) -> String {
        use MenuEntry::*;

        match self {
            Play => script.label("menu_play"),
            Options => script.label("menu_options"),
            MainMenu => script.label("menu_back"),
            FontSize => checkbox(script.label("options_font_size"), save.small_text_on),
            // Phrased positively ("automatic doors"), so the box shows the
            // inverse of the stored `manual_doors` preference.
            AutoDoors => checkbox(script.label("options_auto_doors"), !save.manual_doors),
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
fn draw_title_text<C: crate::render::Canvas>(
    canvas: &mut C,
    font: &Font,
    script: &Script,
    y: i32,
    game_title: &str,
    title_colour: C::Pixel,
    blurb_colour: C::Pixel,
) -> i32 {
    // Centre on the target canvas's own width, so the title tracks the
    // framebuffer size (e.g. a mirror-resized window) rather than a fixed
    // 240-wide screen. The corner blurb stays anchored to the top-left.
    let cx = canvas.width() as i32 / 2;
    let opts = PrintOptions {
        scale: 1,
        ..Default::default()
    };
    let title_width = crate::render::text_width(font, game_title, opts.clone());
    print_to_centered_with_font(font, canvas, game_title, cx, y + 23, title_colour, opts);
    print_to_with_font(
        font,
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
    canvas.fill_rect(
        cx - title_width / 2,
        y + 19,
        title_width - 1,
        2,
        title_colour,
    );
    title_width
}

/// Indexed-canvas variant of the title screen, used by the migrated intro
/// animation so the palette fades apply uniformly to the title pixels.
/// `canvas` is the target indexed layer; `indexed_sprites` is the sprite
/// sheet for the egg icon.
#[allow(clippy::too_many_arguments)]
pub fn draw_title_indexed(
    canvas: &mut crate::render::image::IndexedImage,
    indexed_sprites: &crate::render::image::IndexedImage,
    font: &Font,
    script: &Script,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    draw_title_text(canvas, font, script, y, game_title, 2u8, 14u8);
    let egg_x = canvas.width() as i32 / 2 - 8;
    canvas.spr(
        indexed_sprites,
        534,
        egg_x,
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
    draw_state: &mut crate::draw_state::DrawState,
    font: &Font,
    script: &Script,
    y: i32,
    game_title: &str,
    elapsed_frames: i32,
) {
    use crate::draw_state::{LayerId::*, PALETTE_MAP_IDENTITY};
    let c2 = draw_state.colour(2);
    let c14 = draw_state.colour(14);
    draw_title_text(draw_state.rgba(BG), font, script, y, game_title, c2, c14);
    let egg_x = draw_state.size().0 / 2 - 8;
    draw_state.spr(
        BG,
        &PALETTE_MAP_IDENTITY,
        534,
        egg_x,
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
