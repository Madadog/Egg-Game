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
    drawstate::{DrawState, LayerId},
    position::Vec2,
    system::{ConsoleApi, ConsoleHelper},
};

use crate::system::{PrintOptions, StaticSpriteOptions};

use crate::data::{
    portraits::Portrait,
    sound::{self, SfxData},
};

#[derive(Debug, Clone)]
pub enum TextContent {
    /// A run of text.
    ///
    /// * `pause` — wait for a manual advance (keypress) before showing this
    ///   text. `false` flows it in automatically once the previous line is
    ///   done. (The old `AutoText` is just `pause: false`.)
    /// * `delay` — frames to wait before the text appears. `0` starts a fresh
    ///   page (clearing the box); `> 0` *appends* to the current page after the
    ///   delay, so a sentence can build up clause by clause. (The old `Delayed`
    ///   is just `delay > 0`.)
    Text { text: String, pause: bool, delay: u8 },
    Delay(u8),
    Sound(SfxData),
    Portrait(Option<Portrait>),
    Pause,
    Flip(bool),
}
impl TextContent {
    pub fn is_auto(&self) -> bool {
        use TextContent::*;
        !matches!(self, Text { pause: true, .. } | Pause)
    }
    pub fn is_skip(&self) -> bool {
        use TextContent::*;
        matches!(self, Sound(_) | Portrait(_) | Flip(_))
    }
    /// Plain text (stops on a manual advance unless reached via auto-advance).
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into(), pause: true, delay: 0 }
    }
    /// Text that auto-advances into a new frame once the previous line is done.
    pub fn auto(s: impl Into<String>) -> Self {
        Self::Text { text: s.into(), pause: false, delay: 0 }
    }
    /// Text appended to the current line after a `delay`-frame pause.
    pub fn delayed(s: impl Into<String>, delay: u8) -> Self {
        Self::Text { text: s.into(), pause: false, delay }
    }
}

/// A single "page" of dialogue: a run of text [`content`](Message::content)
/// shown under one speaker (`portrait` + `flip_portrait`). `pause_when_done`
/// controls whether the player must press to continue to the *next* message,
/// or whether it auto-advances. Dialogue is stored as `Vec<Message>` in the
/// registry built by [`crate::data::script`] and queued via
/// [`Dialogue::set_messages`].
#[derive(Debug, Clone)]
pub struct Message {
    pub content: Vec<TextContent>,
    pub portrait: Option<Portrait>,
    pub flip_portrait: bool,
    pub pause_when_done: bool,
}
impl Message {
    pub const fn default() -> Self {
        Self {
            content: Vec::new(),
            portrait: None,
            flip_portrait: false,
            pause_when_done: true,
        }
    }
    pub fn with_content(mut self, content: Vec<TextContent>) -> Self {
        self.content = content;
        self
    }
    pub fn with_portrait(mut self, portrait: Portrait) -> Self {
        self.portrait = Some(portrait);
        self
    }
    pub fn with_flip(mut self, flip_portrait: bool) -> Self {
        self.flip_portrait = flip_portrait;
        self
    }
    /// Don't pause after this message: auto-advance straight into the next one.
    pub fn no_pause(mut self) -> Self {
        self.pause_when_done = false;
        self
    }
    /// The message's text content concatenated into a plain string (ignoring
    /// sounds/portraits/flips/pauses). Used to read back list entries that were
    /// stored as single-line messages.
    pub fn to_plain_string(&self) -> String {
        let mut out = String::new();
        for item in &self.content {
            if let TextContent::Text { text, .. } = item {
                out.push_str(text);
            }
        }
        out
    }
}

impl From<&str> for Message {
    fn from(text: &str) -> Self {
        Self {
            content: vec![TextContent::text(text)],
            ..Message::default()
        }
    }
}
impl From<String> for Message {
    fn from(text: String) -> Self {
        Self {
            content: vec![TextContent::text(text)],
            ..Message::default()
        }
    }
}

/// The dialogue [`PrintOptions`]: defaults plus the save's small-text setting.
pub fn print_options(system: &mut impl ConsoleApi) -> PrintOptions {
    PrintOptions {
        small_text: system.memory().small_text_on,
        ..Default::default()
    }
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
    fn set_current_text(&mut self, system: &mut impl ConsoleApi, string: &str) {
        self.current_text = Some(self.fit_text(system, string));
        self.characters = 0;
        self.print_time = Some(0);
    }
    pub fn add_text(&mut self, system: &mut impl ConsoleApi, string: String) -> bool {
        if self.current_text.is_none() || self.is_line_done() {
            self.set_current_text(system, &string);
            true
        } else {
            self.next_text.push(TextContent::text(string));
            false
        }
    }
    pub fn maybe_add_text(&mut self, system: &mut impl ConsoleApi, string: &'static str) {
        if self.current_text.is_none() {
            self.set_current_text(system, string);
        }
    }
    /// Queue a sequence of [`Message`]s. Each message sets its speaker
    /// (portrait + flip) before emitting its content; a `Pause` is inserted
    /// between messages whose `pause_when_done` is set, so the player must
    /// press to continue (otherwise it auto-advances). No pause is added after
    /// the final message — closing the box handles that.
    pub fn set_messages(&mut self, system: &mut impl ConsoleApi, messages: &[Message]) {
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
        self.next_text(system, false);
    }
    pub fn next_text(&mut self, system: &mut impl ConsoleApi, manual_skip: bool) -> bool {
        if let Some(text_content) = self.next_text.pop() {
            // trace!(format!("Popping text content: {:?}", text_content), 12);
            let skip = text_content.is_skip();
            let val = self.consume_text_content(system, text_content, manual_skip);
            if skip {
                self.next_text(system, manual_skip)
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
                    *string = fit_default_paragraph(system, string, wrap_width);
                    if !manual_skip {
                        self.add_delay(delay.into());
                    }
                } else {
                    self.add_text(system, text);
                }
                true
            }
            TextContent::Text { text, .. } => self.add_text(system, text),
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
        }
    }
    pub fn fit_text(&self, system: &mut impl ConsoleApi, string: &str) -> String {
        fit_default_paragraph(system, string, self.wrap_width())
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
    pub fn tick(&mut self, system: &mut impl ConsoleApi, amount: usize) {
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
                    system.play_sound(sound::POP);
                }
            }
            self.step_text(amount);
            self.delay += 1;
        }
        if self.is_line_done() && self.can_autoadvance() {
            self.next_text(system, false);
        }
    }
    pub fn step_text(&mut self, amount: usize) {
        self.characters = (self.characters + amount).min(self.char_count().saturating_sub(1));
    }
    pub fn can_autoadvance(&self) -> bool {
        if let Some(content) = self.next_text.last() {
            content.is_auto()
        } else {
            false
        }
    }
    pub fn add_delay(&mut self, amount: usize) {
        self.delay = self.delay.saturating_add(amount);
    }
    pub fn skip(&mut self, system: &mut impl ConsoleApi) {
        while self.can_autoadvance() {
            self.next_text(system, true);
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
    #[allow(clippy::too_many_arguments)]
    pub fn draw_dialogue_portrait(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        system: &mut impl ConsoleApi,
        string: &str,
        timer: bool,
        portrait: i32,
        scale: i32,
        sw: i32,
        sh: i32,
    ) {
        use crate::drawstate::PALETTE_MAP_IDENTITY;
        use crate::system::drawing::Canvas;
        let (screen_w, screen_h) = (system.width(), system.height());

        let w = self.width as i32;
        let h = 24;
        self.draw_dialogue_box_with_offset(draw_state, layer, system, string, timer, 14, -2, 4);
        let rect_fill = draw_state.colour(0);
        let rect_outline = draw_state.colour(3);
        //TODO: flexbox
        draw_state.rgba(layer).outlined_rect(
            (screen_w -w) / 2 - 13,
            (screen_h -h) - 6,
            h + 4,
            h + 4,
            rect_fill,
            rect_outline,
        );
        draw_state.spr(
            layer,
            &PALETTE_MAP_IDENTITY,
            portrait,
            (screen_w -w) / 2 - 13 + 2,
            (screen_h -h) - 6 + 2,
            StaticSpriteOptions {
                scale,
                transparent: &[0],
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
        system: &mut impl ConsoleApi,
        string: &str,
        timer: bool,
        mut x: i32,
        mut y: i32,
        mut height: i32,
    ) {
        use crate::system::drawing::Canvas;
        let (screen_w, screen_h) = (system.width(), system.height());

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
                (screen_w -pw) / 2 - 13,
                (screen_h -h) - 6,
                h + 4,
                h + 4,
                dark,
                outline_colour,
            );
            height += 4;
            portrait.draw_offset(
                draw_state,
                layer,
                Vec2::new(((screen_w -pw) / 2 - 15) as i16, ((screen_h -h) - 8) as i16),
            );
            draw_state.rgba(layer).stroke_rect(
                (screen_w -pw) / 2 - 13,
                (screen_h -h) - 6,
                h + 4,
                h + 4,
                outline_colour,
            );
        }
        // Text box
        if self.dark_theme {
            draw_state.rgba(layer).outlined_rect(
                (screen_w -w) / 2 + x - 2,
                (screen_h -h) - 4 + y - 2,
                w + 4,
                h + height + 4,
                darkish,
                dark,
            );
        }
        draw_state.rgba(layer).outlined_rect(
            (screen_w -w) / 2 + x,
            (screen_h -h) - 4 + y,
            w,
            h + height,
            bg_colour,
            outline_colour,
        );
        let options = print_options(system);
        let text: &str = if timer {
            revealed(string, print_timer)
        } else {
            string
        };
        system.print_to(
            draw_state.rgba(layer),
            text,
            (screen_w -w) / 2 + 3 + x,
            (screen_h -h) - 4 + 3 + y,
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
        system: &mut impl ConsoleApi,
        string: &str,
        timer: bool,
    ) {
        self.draw_dialogue_box_with_offset(draw_state, layer, system, string, timer, 0, 0, 0)
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
            .finish()
    }
}

pub fn print_width(system: &impl ConsoleApi, string: &str, fixed: bool, small_font: bool) -> i32 {
    crate::system::text_width(
        system.font(),
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
    system: &mut impl ConsoleApi,
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
        if print_width(system, taken, fixed, small_font) as usize > wrap_width {
            break;
        } else {
            line_length = i
        };
    }
    (
        take_words(string, line_length, start_word),
        line_length,
        line_length == len,
    )
}

pub fn fit_paragraph(
    system: &mut impl ConsoleApi,
    string: &str,
    wrap_width: usize,
    fixed: bool,
    small_font: bool,
) -> String {
    let len = string.split_inclusive(' ').count();
    let mut paragraph = String::new();
    let mut skip = 0;
    while skip < len {
        let (string, x, all_fits) = fit_string(system, string, wrap_width, skip, fixed, small_font);
        skip += x;
        paragraph.push_str(&string);
        if all_fits {
            return paragraph;
        }
        paragraph.push('\n');
    }
    paragraph
}

pub fn fit_default_paragraph(
    system: &mut impl ConsoleApi,
    string: &str,
    wrap_width: usize,
) -> String {
    let small_text = system.memory().small_text_on;
    fit_paragraph(system, string, wrap_width, false, small_text)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
