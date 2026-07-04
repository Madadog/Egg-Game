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

use std::fmt::Debug;

use crate::{
    data::save::SaveData,
    draw_state::{DrawState, LayerId},
    geometry::Vec2,
    platform::{ConsoleApi, ConsoleHelper},
};

use crate::render::{Font, PrintOptions, SpriteOptions, print_to_with_font};

use crate::data::portraits::Portrait;
use crate::data::script::message::{ChoiceOption, Message, TextContent};
use crate::data::sound;

/// The dialogue [`PrintOptions`]: defaults plus the caller's small-text setting
/// (the save flag `small_text_on`, passed in now that it's game state).
pub fn print_options(small_text: bool) -> PrintOptions {
    PrintOptions {
        small_text,
        ..Default::default()
    }
}

/// A live [`TextContent::Choice`]: the options being offered and the highlighted
/// index. Set when the box consumes a `Choice` item and cleared once the player
/// confirms (see [`Dialogue::confirm_choice`]). While it is `Some`, the box is
/// "choosing" — auto-advance is suspended and the driver routes directional
/// input to [`move_choice`](Dialogue::move_choice) instead of the world.
#[derive(Clone, Debug)]
pub struct ChoiceState {
    pub options: Vec<ChoiceOption>,
    pub selected: usize,
}

#[derive(Clone)]
pub struct Dialogue {
    pub current_text: Option<String>,
    pub characters: usize,
    pub next_text: Vec<TextContent>,
    pub width: usize,
    pub delay: usize,
    pub print_time: Option<usize>,
    pub portrait: Option<Portrait>,
    pub dark_theme: bool,
    pub flip_portrait: bool,
    /// The open choice menu, if the box is currently awaiting a selection.
    pub choice: Option<ChoiceState>,
    /// A `#shake FRAMES [AMP]` playback just passed, waiting for the world's
    /// camera driver to pick it up (`(frames, amplitude)`). The widget can't
    /// reach the camera itself, so it banks the request here; the walkaround
    /// takes it when it centres the camera. Overwritten (not stacked) if
    /// another fires first; ignored by camera-less hosts of the box.
    pub pending_shake: Option<(u32, i16)>,
}
impl Dialogue {
    pub const fn default() -> Self {
        Self {
            current_text: None,
            next_text: Vec::new(),
            characters: 0,
            width: 200,
            delay: 0,
            print_time: None,
            portrait: None,
            dark_theme: false,
            flip_portrait: false,
            choice: None,
            pending_shake: None,
        }
    }
    pub fn with_width(self, width: usize) -> Self {
        Self { width, ..self }
    }
    pub fn is_line_done(&self) -> bool {
        match &self.current_text {
            Some(_) => self.characters >= self.char_count().saturating_sub(1),
            None => true,
        }
    }
    fn set_current_text(&mut self, font: &Font, save: &SaveData, string: &str) {
        self.current_text = Some(self.fit_text(font, save.small_text_on, string));
        self.characters = 0;
        self.print_time = Some(0);
    }
    pub fn add_text(&mut self, font: &Font, save: &SaveData, string: String) -> bool {
        if self.current_text.is_none() || self.is_line_done() {
            self.set_current_text(font, save, &string);
            true
        } else {
            self.next_text.push(TextContent::text(string));
            false
        }
    }
    pub fn maybe_add_text(&mut self, font: &Font, save: &SaveData, string: &'static str) {
        if self.current_text.is_none() {
            self.set_current_text(font, save, string);
        }
    }
    /// Queue a sequence of [`Message`]s. Each message sets its speaker
    /// (portrait + flip) before emitting its content; a `Pause` is inserted
    /// between messages whose `pause_when_done` is set, so the player must
    /// press to continue (otherwise it auto-advances). No pause is added after
    /// the final message — closing the box handles that.
    ///
    /// `save` is threaded through playback (not just for the wrap setting
    /// `small_text_on`): a [`TextContent::SetFlag`] item — authored as `#set` —
    /// writes its named flag the moment it is consumed, so passing `&mut save`
    /// is what lets dialogue mutate progress as it plays.
    pub fn set_messages(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        messages: &[Message],
    ) {
        let mut queue: Vec<TextContent> = Vec::new();
        let last = messages.len().saturating_sub(1);
        for (i, message) in messages.iter().enumerate() {
            queue.push(TextContent::Portrait(message.portrait.clone()));
            queue.push(TextContent::Flip(message.flip_portrait));
            queue.extend(message.content.iter().cloned());
            if message.pause_when_done && i != last {
                queue.push(TextContent::Pause);
            }
        }
        self.next_text = queue.into_iter().rev().collect();
        self.next_text(system, font, save, false);
    }
    pub fn next_text(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        manual_skip: bool,
    ) -> bool {
        if let Some(text_content) = self.next_text.pop() {
            // trace!(format!("Popping text content: {:?}", text_content), 12);
            let skip = text_content.is_skip();
            let val = self.consume_text_content(system, font, save, text_content, manual_skip);
            if skip {
                self.next_text(system, font, save, manual_skip)
            } else {
                val
            }
        } else {
            false
        }
    }
    pub fn consume_text_content(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        text_content: TextContent,
        manual_skip: bool,
    ) -> bool {
        match text_content {
            // `delay > 0` appends to the current page after a beat; `delay == 0`
            // starts a fresh page. See [`TextContent::Text`].
            TextContent::Text { text, delay, .. } if delay > 0 => {
                let wrap_width = self.wrap_width();
                if let Some(string) = &mut self.current_text {
                    string.push_str(&text);
                    *string = fit_default_paragraph(font, string, wrap_width, save.small_text_on);
                    if !manual_skip {
                        self.add_delay(delay.into());
                    }
                } else {
                    self.add_text(font, save, text);
                }
                true
            }
            TextContent::Text { text, .. } => self.add_text(font, save, text),
            TextContent::Delay(x) => {
                if !manual_skip {
                    self.add_delay(x.into());
                }
                true
            }
            TextContent::Sound(x) => {
                system.play_sound(x.clone());
                true
            }
            TextContent::Portrait(x) => {
                if let Some(portrait) = x {
                    self.portrait = Some(portrait.clone());
                } else {
                    self.portrait = None;
                };
                true
            }
            TextContent::Pause => true,
            TextContent::Flip(x) => {
                self.flip_portrait = x;
                true
            }
            // Fire the named flag in place — same timing as a Sound item.
            TextContent::SetFlag(name, value) => {
                save.set_flag(&name, value);
                true
            }
            // Bank the shake for the camera driver — but, being time-flavoured
            // like a `Delay`, it is dropped on a manual fast-forward: skipping
            // a page shouldn't jolt the screen.
            TextContent::Shake { frames, amplitude } => {
                if !manual_skip {
                    self.pending_shake = Some((frames, amplitude));
                }
                true
            }
            // Open the menu and stop: playback blocks here until the driver calls
            // [`confirm_choice`](Self::confirm_choice). `Choice` is neither auto
            // nor skip, so `next_text` returns after consuming it.
            TextContent::Choice(options) => {
                self.choice = Some(ChoiceState {
                    options,
                    selected: 0,
                });
                true
            }
        }
    }
    pub fn fit_text(&self, font: &Font, small_text: bool, string: &str) -> String {
        fit_default_paragraph(font, string, self.wrap_width(), small_text)
    }
    pub fn wrap_width(&self) -> usize {
        self.width - 3
    }
    pub fn close(&mut self) {
        *self = Self {
            width: self.width,
            ..Self::default()
        };
        self.next_text.shrink_to_fit();
    }
    /// Characters (not bytes) in the current text. [`characters`](Self::characters)
    /// is a char index, so all typewriter bookkeeping uses this count.
    pub fn char_count(&self) -> usize {
        self.current_text.as_ref().map_or(0, |x| x.chars().count())
    }
    pub fn tick(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        amount: usize,
    ) {
        if let Some(text) = &mut self.current_text {
            if self.delay != 0 {
                self.delay = self.delay.saturating_sub(amount);
                return;
            }
            let mut silent_char = false;
            if let Some(char) = text.chars().nth(self.characters) {
                if char == '.' {
                    self.delay += 4;
                }
                if char.is_ascii_control() {
                    silent_char = true
                }
            }
            if let Some(print_time) = &mut self.print_time {
                *print_time += 1;
                if !silent_char && *print_time % 2 == 0 && !self.is_line_done() {
                    system.play_sound(sound::pop());
                }
            }
            self.step_text(amount);
            self.delay += 1;
        }
        if self.is_line_done() && self.can_autoadvance() {
            self.next_text(system, font, save, false);
        }
    }
    pub fn step_text(&mut self, amount: usize) {
        self.characters = (self.characters + amount).min(self.char_count().saturating_sub(1));
    }
    pub fn can_autoadvance(&self) -> bool {
        // A pending choice blocks the queue: don't tick past it into whatever
        // follows until the player has picked.
        if self.choice.is_some() {
            return false;
        }
        if let Some(content) = self.next_text.last() {
            content.is_auto()
        } else {
            false
        }
    }
    pub fn add_delay(&mut self, amount: usize) {
        self.delay = self.delay.saturating_add(amount);
    }
    pub fn skip(&mut self, system: &mut impl ConsoleApi, font: &Font, save: &mut SaveData) {
        while self.can_autoadvance() {
            self.next_text(system, font, save, true);
        }
        self.finish_line();
    }
    /// Jump the typewriter to the last character of the current line, revealing
    /// it all at once.
    pub fn finish_line(&mut self) {
        if self.current_text.is_some() {
            self.characters = self.char_count().saturating_sub(1);
        }
    }
    /// Whether the box is awaiting a choice selection (a `#choice` menu is open).
    /// While true the driver routes directional input to [`move_choice`](Self::move_choice)
    /// and confirm to [`confirm_choice`](Self::confirm_choice), not the world.
    pub fn is_choosing(&self) -> bool {
        self.choice.is_some()
    }
    /// Whether the box is doing anything — showing text, holding queued content,
    /// or waiting on a choice. Callers gate world input / movement on this; a
    /// choice with no prompt has no `current_text`, so it must be counted too.
    pub fn is_active(&self) -> bool {
        self.current_text.is_some() || !self.next_text.is_empty() || self.choice.is_some()
    }
    /// Move the choice highlight by `delta` rows (negative up), wrapping around
    /// the ends. A no-op if no choice is open.
    pub fn move_choice(&mut self, delta: i32) {
        if let Some(choice) = &mut self.choice {
            let len = choice.options.len() as i32;
            if len > 0 {
                let next = (choice.selected as i32 + delta).rem_euclid(len);
                choice.selected = next as usize;
            }
        }
    }
    /// Confirm the highlighted option: write its flags through `save.set_flag`
    /// (the same path as `#set`), clear the menu, and resume playback with the
    /// content that followed the `#choice`. Returns whatever [`next_text`](Self::next_text)
    /// returns (whether a further line opened). A no-op returning `false` if no
    /// choice is open.
    pub fn confirm_choice(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
    ) -> bool {
        let Some(choice) = self.choice.take() else {
            return false;
        };
        if let Some(option) = choice.options.get(choice.selected) {
            for (name, value) in &option.sets {
                save.set_flag(name, *value);
            }
        }
        self.next_text(system, font, save, false)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_dialogue_portrait(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        font: &Font,
        small_text: bool,
        string: &str,
        timer: bool,
        portrait: i32,
        scale: i32,
        sw: i32,
        sh: i32,
    ) {
        use crate::draw_state::PALETTE_MAP_IDENTITY;
        use crate::render::Canvas;
        // Measure the render target, matching `draw_dialogue_box_with_offset` so
        // this variant's portrait stays aligned with the text box on any
        // framebuffer (not just the main window).
        let (screen_w, screen_h) = draw_state.size();

        let w = self.width as i32;
        let h = 24;
        self.draw_dialogue_box_with_offset(
            draw_state, layer, font, small_text, string, timer, 14, -2, 4,
        );
        let rect_fill = draw_state.colour(0);
        let rect_outline = draw_state.colour(3);
        //TODO: flexbox
        draw_state.rgba(layer).outlined_rect(
            (screen_w - w) / 2 - 13,
            (screen_h - h) - 6,
            h + 4,
            h + 4,
            rect_fill,
            rect_outline,
        );
        draw_state.spr(
            layer,
            &PALETTE_MAP_IDENTITY,
            portrait,
            (screen_w - w) / 2 - 13 + 2,
            (screen_h - h) - 6 + 2,
            SpriteOptions {
                scale,
                transparent: Some(0),
                w: sw,
                h: sh,
                ..Default::default()
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_dialogue_box_with_offset(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        // Drawing only measures/prints text, so it takes the loaded [`Font`]
        // directly (not the console) — which lets the text-editor previewer call
        // this from its own `&self` draw, split-borrowing the font and the canvas.
        font: &Font,
        small_text: bool,
        string: &str,
        timer: bool,
        mut x: i32,
        mut y: i32,
        mut height: i32,
    ) {
        use crate::render::Canvas;
        // Measure against the surface being drawn into, not the host's main
        // window: an off-screen render target (an extra editor view) has its own
        // framebuffer size, so the box must re-centre against that to land at the
        // bottom-middle of *that* view. For the main window the two are identical,
        // so in-game rendering is unchanged.
        let (screen_w, screen_h) = draw_state.size();

        let print_timer = self.characters;
        let w = self.width as i32;
        let h = 24;

        let outline_colour = draw_state.colour(if self.dark_theme { 1u8 } else { 3 });
        let bg_colour = draw_state.colour(if self.dark_theme { 1u8 } else { 2 });
        let dark = draw_state.colour(0);
        let darkish = draw_state.colour(1);
        let bright = draw_state.colour(12);

        // Portrait
        if let Some(portrait) = &self.portrait {
            let pw = if self.flip_portrait {
                x -= 12;
                -w
            } else {
                x += 14;
                w
            };
            y -= 2;
            draw_state.rgba(layer).outlined_rect(
                (screen_w - pw) / 2 - 13,
                (screen_h - h) - 6,
                h + 4,
                h + 4,
                dark,
                outline_colour,
            );
            height += 4;
            crate::ui::portrait::draw_offset(
                portrait,
                draw_state,
                layer,
                Vec2::new(
                    ((screen_w - pw) / 2 - 15) as i16,
                    ((screen_h - h) - 8) as i16,
                ),
            );
            draw_state.rgba(layer).stroke_rect(
                (screen_w - pw) / 2 - 13,
                (screen_h - h) - 6,
                h + 4,
                h + 4,
                outline_colour,
            );
        }
        // Text box
        if self.dark_theme {
            draw_state.rgba(layer).outlined_rect(
                (screen_w - w) / 2 + x - 2,
                (screen_h - h) - 4 + y - 2,
                w + 4,
                h + height + 4,
                darkish,
                dark,
            );
        }
        draw_state.rgba(layer).outlined_rect(
            (screen_w - w) / 2 + x,
            (screen_h - h) - 4 + y,
            w,
            h + height,
            bg_colour,
            outline_colour,
        );
        let options = print_options(small_text);
        let text: &str = if timer {
            revealed(string, print_timer)
        } else {
            string
        };
        print_to_with_font(
            font,
            draw_state.rgba(layer),
            text,
            (screen_w - w) / 2 + 3 + x,
            (screen_h - h) - 4 + 3 + y,
            bright,
            PrintOptions {
                color: 12,
                ..options
            },
        );
    }

    pub fn draw_dialogue_box(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        font: &Font,
        small_text: bool,
        string: &str,
        timer: bool,
    ) {
        self.draw_dialogue_box_with_offset(
            draw_state, layer, font, small_text, string, timer, 0, 0, 0,
        )
    }

    /// Draw the open `#choice` menu as a bordered panel stacked just above the
    /// dialogue box: one option per row, the highlighted one filled in the
    /// bright selection colour (palette #9, the same unmistakable blue the text
    /// editor uses) with a `>` cursor. A no-op when no choice is open, so the
    /// draw site can call it unconditionally after the box.
    pub fn draw_choice(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        font: &Font,
        small_text: bool,
    ) {
        use crate::render::Canvas;
        let Some(choice) = &self.choice else {
            return;
        };

        let (screen_w, screen_h) = draw_state.size();
        let w = self.width as i32;
        let box_h = 24;
        let row_h = 8;
        let pad = 3;

        let panel_h = choice.options.len() as i32 * row_h + pad * 2;
        let x = (screen_w - w) / 2;
        // Sit just above where the dialogue box lands (a 2px gap), so a prompt
        // page and its options read as one stacked unit.
        let box_top = (screen_h - box_h) - 4;
        let y = box_top - panel_h - 2;

        // Resolve colours before the mutable canvas borrow (mirrors the box).
        let bg = draw_state.colour(if self.dark_theme { 1 } else { 2 });
        let outline = draw_state.colour(if self.dark_theme { 1 } else { 3 });
        let text_col = draw_state.colour(12);
        let sel = draw_state.colour(9);

        let options = print_options(small_text);
        let canvas = draw_state.rgba(layer);
        canvas.outlined_rect(x, y, w, panel_h, bg, outline);
        for (i, option) in choice.options.iter().enumerate() {
            let row_y = y + pad + i as i32 * row_h;
            let selected = i == choice.selected;
            if selected {
                canvas.fill_rect(x + 1, row_y - 1, w - 2, row_h, sel);
            }
            let marker = if selected { ">" } else { " " };
            let line = format!("{marker} {}", option.text);
            print_to_with_font(
                font,
                canvas,
                &line,
                x + pad,
                row_y,
                text_col,
                PrintOptions {
                    color: 12,
                    ..options.clone()
                },
            );
        }
    }
}

impl Debug for Dialogue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialogue")
            .field("current_text", &self.current_text)
            .field("characters", &self.characters)
            .field("next_text", &self.next_text.len())
            .field("width", &self.width)
            .field("delay", &self.delay)
            .field("print_time", &self.print_time)
            .field("portrait", &self.portrait)
            .field("choice", &self.choice)
            .finish()
    }
}

pub fn print_width(font: &Font, string: &str, fixed: bool, small_font: bool) -> i32 {
    crate::render::text_width(
        font,
        string,
        PrintOptions {
            fixed,
            small_text: small_font,
            ..Default::default()
        },
    )
}

pub fn take_words(string: &str, count: usize, skip: usize) -> String {
    string.split_inclusive(' ').skip(skip).take(count).collect()
}

/// The prefix of `text` containing characters `0..=chars` (the whole string if
/// `chars` runs past the end). The typewriter reveal indexes *characters*, so
/// slicing bytes would split — and panic on — multibyte text.
pub fn revealed(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars.saturating_add(1))
        .map_or(text, |(i, _)| &text[..i])
}

/// Clamps a string to the specified width (with the TIC-80 font). Returns a string,
/// the number of fitting words, and a bool for if the whole string fit.
pub fn fit_string(
    font: &Font,
    string: &str,
    wrap_width: usize,
    start_word: usize,
    fixed: bool,
    small_font: bool,
) -> (String, usize, bool) {
    let len = string.split_inclusive(' ').skip(start_word).count();
    let mut line_length = 0;
    for i in 1..=len {
        let taken = &take_words(string, i, start_word);
        if print_width(font, taken, fixed, small_font) as usize > wrap_width {
            break;
        } else {
            line_length = i
        };
    }
    // Always make progress: when even the first word is wider than `wrap_width`,
    // still take that one word (it overflows the line) so the caller's `skip`
    // advances. Otherwise `fit_paragraph` would `skip += 0` forever, pushing a
    // newline each pass until it exhausts memory.
    if line_length == 0 && len > 0 {
        line_length = 1;
    }
    (
        take_words(string, line_length, start_word),
        line_length,
        line_length == len,
    )
}

pub fn fit_paragraph(
    font: &Font,
    string: &str,
    wrap_width: usize,
    fixed: bool,
    small_font: bool,
) -> String {
    let len = string.split_inclusive(' ').count();
    let mut paragraph = String::new();
    let mut skip = 0;
    while skip < len {
        let (string, x, all_fits) = fit_string(font, string, wrap_width, skip, fixed, small_font);
        skip += x;
        paragraph.push_str(&string);
        if all_fits {
            return paragraph;
        }
        paragraph.push('\n');
    }
    paragraph
}

/// Wrap `string` to `wrap_width` using the caller's small-text setting (the
/// save flag `small_text_on`, passed in now that persistence is game state).
pub fn fit_default_paragraph(
    font: &Font,
    string: &str,
    wrap_width: usize,
    small_text: bool,
) -> String {
    fit_paragraph(font, string, wrap_width, false, small_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::NullConsole;

    /// A `#shake` item banks its request for the camera driver as it is played
    /// past — but a manual fast-forward drops it (time-flavoured, like a
    /// `Delay`): skipping a page shouldn't jolt the screen.
    #[test]
    fn shake_banks_unless_manually_skipped() {
        let mut dialogue = Dialogue::default();
        let mut save = SaveData::default();
        let font = crate::render::Font::blank();
        let shake = TextContent::Shake {
            frames: 30,
            amplitude: 4,
        };
        assert!(shake.is_skip(), "fires in place like a sound");

        let consumed = dialogue.consume_text_content(
            &mut NullConsole::new(),
            &font,
            &mut save,
            shake.clone(),
            false,
        );
        assert!(consumed);
        assert_eq!(dialogue.pending_shake, Some((30, 4)));

        // Manual skip: the request is dropped, not banked.
        dialogue.pending_shake = None;
        dialogue.consume_text_content(&mut NullConsole::new(), &font, &mut save, shake, true);
        assert_eq!(dialogue.pending_shake, None);
    }

    /// A dialogue whose live line is `text`, bypassing the wrap/fit path
    /// (which needs a console to measure glyphs).
    fn with_text(text: &str) -> Dialogue {
        Dialogue {
            current_text: Some(text.to_string()),
            ..Dialogue::default()
        }
    }

    #[test]
    fn revealed_is_char_indexed() {
        let text = "café né?"; // 8 chars, 10 bytes
        assert_eq!(revealed(text, 0), "c");
        assert_eq!(revealed(text, 3), "café");
        assert_eq!(revealed(text, 6), "café né");
        assert_eq!(revealed(text, 7), text);
        assert_eq!(revealed(text, 100), text);
        assert_eq!(revealed("", 0), "");
    }

    #[test]
    fn is_line_done_counts_chars_not_bytes() {
        let mut dialogue = with_text("café né?");
        dialogue.characters = 6;
        assert!(!dialogue.is_line_done());
        dialogue.characters = 7;
        assert!(dialogue.is_line_done());
        assert!(with_text("").is_line_done());
        assert!(Dialogue::default().is_line_done());
    }

    #[test]
    fn step_text_clamps_to_last_char() {
        let mut dialogue = with_text("né");
        dialogue.step_text(10);
        assert_eq!(dialogue.characters, 1);
        // An empty line must not underflow the clamp.
        let mut empty = with_text("");
        empty.step_text(1);
        assert_eq!(empty.characters, 0);
    }

    #[test]
    fn finish_line_lands_on_last_char() {
        let mut dialogue = with_text("café né?");
        dialogue.finish_line();
        assert_eq!(dialogue.characters, 7);
        assert!(dialogue.is_line_done());
        let mut empty = with_text("");
        empty.finish_line();
        assert_eq!(empty.characters, 0);
    }

    #[test]
    fn choice_menu_moves_wraps_confirms_and_sets_the_flag() {
        use crate::data::script::message::ChoiceOption;
        use crate::platform::NullConsole;

        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        let options = vec![
            ChoiceOption {
                text: "Tea".into(),
                sets: vec![("chose_tea".into(), true)],
            },
            ChoiceOption {
                text: "Coffee".into(),
                sets: vec![("chose_coffee".into(), true)],
            },
            ChoiceOption {
                text: "Nothing".into(),
                sets: vec![],
            },
        ];
        let messages = vec![Message::default().with_content(vec![
            TextContent::text("What'll it be?"),
            TextContent::Choice(options),
        ])];
        d.set_messages(&mut console, &font, &mut save, &messages);

        // The prompt page shows first; the box is active but not yet choosing.
        assert!(d.is_active());
        assert!(!d.is_choosing());
        // Advancing past the prompt opens the menu at the first option.
        d.next_text(&mut console, &font, &mut save, false);
        assert!(d.is_choosing());
        assert_eq!(d.choice.as_ref().unwrap().selected, 0);
        // Up from the top wraps to the last option; down wraps back.
        d.move_choice(-1);
        assert_eq!(d.choice.as_ref().unwrap().selected, 2);
        d.move_choice(1);
        assert_eq!(d.choice.as_ref().unwrap().selected, 0);
        // Highlight "Coffee" and confirm: it writes that option's flag, clears
        // the menu, and (nothing followed) leaves the box ready to close.
        d.move_choice(1);
        let opened = d.confirm_choice(&mut console, &font, &mut save);
        assert!(!opened, "no content followed the choice");
        assert!(!d.is_choosing());
        assert!(save.flag("chose_coffee"));
        assert!(!save.flag("chose_tea"));
    }

    #[test]
    fn a_pending_choice_blocks_auto_advance() {
        use crate::data::script::message::ChoiceOption;
        // Even with auto-text queued after it, the box must not tick past an open
        // choice.
        let mut d = Dialogue {
            current_text: Some("prompt".into()),
            choice: Some(ChoiceState {
                options: vec![ChoiceOption {
                    text: "ok".into(),
                    sets: vec![],
                }],
                selected: 0,
            }),
            next_text: vec![TextContent::auto("after")],
            ..Dialogue::default()
        };
        assert!(!d.can_autoadvance());
        // Clearing the choice lets the queued auto-text advance again.
        d.choice = None;
        assert!(d.can_autoadvance());
    }

    #[test]
    fn fit_paragraph_terminates_on_an_over_wide_word() {
        // A fully-opaque atlas makes every glyph 8px wide, so even a single
        // character is wider than this 4px wrap width — the case that used to
        // spin `fit_paragraph` forever (`skip += 0`). It must return promptly and
        // still emit both words (each on its own overflowing line).
        let mut font = Font::blank();
        font.image_mut().data_mut().fill(255);
        font.refresh();
        let out = fit_paragraph(&font, "AAA BBB", 4, false, false);
        assert!(out.contains("AAA"), "got {out:?}");
        assert!(out.contains("BBB"), "got {out:?}");
    }
}
