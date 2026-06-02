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

use std::{
    fmt::Debug,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

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
pub enum StaticTextContent {
    Text(&'static str),
    Delayed(&'static str, u8),
    Delay(u8),
    Sound(&'static SfxData),
    Portrait(Option<&'static Portrait>),
    PausePortrait(Option<&'static Portrait>),
    Pause,
    AutoText(&'static str),
    Flip(bool),
}
impl StaticTextContent {
    pub fn is_auto(&self) -> bool {
        use StaticTextContent::*;
        !matches!(self, Text(_) | Pause)
    }
    pub fn is_skip(&self) -> bool {
        use StaticTextContent::*;
        matches!(self, Sound(_) | Portrait(_) | Flip(_))
    }
}

#[derive(Debug, Clone)]
pub enum TextContent {
    Text(String),
    Delayed(String, u8),
    Delay(u8),
    Sound(SfxData),
    Portrait(Option<Portrait>),
    PausePortrait(Option<Portrait>),
    Pause,
    AutoText(String),
    Flip(bool),
}
impl TextContent {
    pub fn is_auto(&self) -> bool {
        use TextContent::*;
        !matches!(self, Text(_) | Pause)
    }
    pub fn is_skip(&self) -> bool {
        use TextContent::*;
        matches!(self, Sound(_) | Portrait(_) | Flip(_))
    }
}

impl From<StaticTextContent> for TextContent {
    fn from(value: StaticTextContent) -> Self {
        match value {
            StaticTextContent::Text(x) => Self::Text(x.into()),
            StaticTextContent::Delayed(x, y) => Self::Delayed(x.into(), y),
            StaticTextContent::Delay(x) => Self::Delay(x),
            StaticTextContent::Sound(x) => Self::Sound(x.clone()),
            StaticTextContent::Portrait(x) => Self::Portrait(x.cloned()),
            StaticTextContent::PausePortrait(x) => Self::PausePortrait(x.cloned()),
            StaticTextContent::Pause => Self::Pause,
            StaticTextContent::AutoText(x) => Self::AutoText(x.into()),
            StaticTextContent::Flip(x) => Self::Flip(x),
        }
    }
}
/// A single "page" of dialogue: a run of text content shown under one
/// speaker (`portrait` + `flip_portrait`). `pause_when_done` controls whether
/// the player must press to continue to the *next* message, or whether it
/// auto-advances. The compile-time `StaticMessage` is converted to the
/// runtime-owned `Message` when an interaction fires.
#[derive(Debug, Clone)]
pub struct StaticMessage {
    pub content: &'static [StaticTextContent],
    pub portrait: Option<Portrait>,
    pub flip_portrait: bool,
    pub pause_when_done: bool,
}
impl StaticMessage {
    pub const fn default() -> Self {
        Self {
            content: &[],
            portrait: None,
            flip_portrait: false,
            pause_when_done: true,
        }
    }
    pub const fn with_content(self, content: &'static [StaticTextContent]) -> Self {
        Self { content, ..self }
    }
    pub const fn with_portrait(self, portrait: Portrait) -> Self {
        Self {
            portrait: Some(portrait),
            ..self
        }
    }
    pub const fn with_flip(self, flip_portrait: bool) -> Self {
        Self {
            flip_portrait,
            ..self
        }
    }
    /// Don't pause after this message: auto-advance straight into the next one.
    pub const fn no_pause(self) -> Self {
        Self {
            pause_when_done: false,
            ..self
        }
    }
}

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
    pub fn with_content(self, content: Vec<TextContent>) -> Self {
        Self { content, ..self }
    }
}

impl From<&StaticMessage> for Message {
    fn from(message: &StaticMessage) -> Self {
        Self {
            content: message.content.iter().cloned().map(Into::into).collect(),
            portrait: message.portrait.clone(),
            flip_portrait: message.flip_portrait,
            pause_when_done: message.pause_when_done,
        }
    }
}
impl From<StaticMessage> for Message {
    fn from(message: StaticMessage) -> Self {
        (&message).into()
    }
}
impl From<&str> for Message {
    fn from(text: &str) -> Self {
        Self {
            content: vec![TextContent::Text(text.to_string())],
            ..Message::default()
        }
    }
}
impl From<String> for Message {
    fn from(text: String) -> Self {
        Self {
            content: vec![TextContent::Text(text)],
            ..Message::default()
        }
    }
}

pub struct DialogueOptions {
    pub fixed: AtomicBool,
    pub box_width: AtomicUsize,
}
impl Default for DialogueOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogueOptions {
    pub const fn new() -> Self {
        Self {
            fixed: AtomicBool::new(false),
            box_width: AtomicUsize::new(200),
        }
    }
    pub fn fixed(&self) -> bool {
        self.fixed.load(Ordering::SeqCst)
    }
    pub fn small_text(&self, system: &mut impl ConsoleApi) -> bool {
        system.memory().small_text_on
    }
    pub fn box_width(&self) -> usize {
        self.box_width.load(Ordering::SeqCst)
    }
    pub fn set_options(&self, system: &mut impl ConsoleApi, fixed: bool, small_text: bool) {
        self.fixed.store(fixed, Ordering::SeqCst);
        system.memory().small_text_on = small_text;
    }
    pub fn get_options(&self, system: &mut impl ConsoleApi) -> PrintOptions {
        PrintOptions {
            fixed: self.fixed(),
            small_text: self.small_text(system),
            ..Default::default()
        }
    }
    pub fn toggle_small_text(&self, system: &mut impl ConsoleApi) {
        let save = system.memory();
        save.small_text_on = !save.small_text_on;
    }
    pub fn toggle_fixed(&self) {
        self.fixed
            .store(self.fixed.load(Ordering::SeqCst), Ordering::SeqCst);
    }
}
pub static DIALOGUE_OPTIONS: DialogueOptions = DialogueOptions::new();

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
            Some(text) => text.is_empty() || self.characters == text.len() - 1,
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
            self.next_text.push(TextContent::Text(string));
            false
        }
    }
    pub fn maybe_add_text(&mut self, system: &mut impl ConsoleApi, string: &'static str) {
        if self.current_text.is_none() {
            self.set_current_text(system, string);
        }
    }
    pub fn set_dialogue(&mut self, system: &mut impl ConsoleApi, dialogue: &[String]) {
        self.next_text = dialogue
            .iter()
            .rev()
            .map(|x| TextContent::Text(x.clone()))
            .collect();
        self.next_text(system, false);
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
            TextContent::Text(text) | TextContent::AutoText(text) => self.add_text(system, text),
            TextContent::Delay(x) => {
                if !manual_skip {
                    self.add_delay(x.into());
                }
                true
            }
            TextContent::Delayed(text, delay) => {
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
            TextContent::Sound(x) => {
                system.play_sound(x.clone());
                true
            }
            TextContent::Portrait(x) | TextContent::PausePortrait(x) => {
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
    pub fn text_len(&self) -> usize {
        self.current_text.as_ref().map_or(0, |x| x.len())
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
        self.characters = (self.characters + amount).min(self.text_len() - 1);
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
        if let Some(text) = &mut self.current_text {
            self.characters = text.len() - 1;
        }
    }
    pub fn set_options(&mut self, system: &mut impl ConsoleApi, fixed: bool, small_text: bool) {
        DIALOGUE_OPTIONS.set_options(system, fixed, small_text);
        let width = self.wrap_width();
        if let Some(text) = &mut self.current_text {
            *text = fit_default_paragraph(system, text, width);
        }
    }
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
        use crate::system::{HEIGHT, WIDTH};

        let w = self.width as i32;
        let h = 24;
        self.draw_dialogue_box_with_offset(draw_state, layer, system, string, timer, 14, -2, 4);
        let rect_fill = draw_state.colour(0);
        let rect_outline = draw_state.colour(3);
        //TODO: flexbox
        draw_state.rgba(layer).outlined_rect(
            (WIDTH - w) / 2 - 13,
            (HEIGHT - h) - 6,
            h + 4,
            h + 4,
            rect_fill,
            rect_outline,
        );
        draw_state.spr(
            layer,
            &PALETTE_MAP_IDENTITY,
            portrait,
            (WIDTH - w) / 2 - 13 + 2,
            (HEIGHT - h) - 6 + 2,
            StaticSpriteOptions {
                scale,
                transparent: &[0],
                w: sw,
                h: sh,
                ..Default::default()
            },
        );
    }

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
        use crate::system::{HEIGHT, WIDTH};

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
                (WIDTH - pw) / 2 - 13,
                (HEIGHT - h) - 6,
                h + 4,
                h + 4,
                dark,
                outline_colour,
            );
            height += 4;
            portrait.draw_offset(
                draw_state,
                layer,
                Vec2::new(((WIDTH - pw) / 2 - 15) as i16, ((HEIGHT - h) - 8) as i16),
            );
            draw_state.rgba(layer).stroke_rect(
                (WIDTH - pw) / 2 - 13,
                (HEIGHT - h) - 6,
                h + 4,
                h + 4,
                outline_colour,
            );
        }
        // Text box
        if self.dark_theme {
            draw_state.rgba(layer).outlined_rect(
                (WIDTH - w) / 2 + x - 2,
                (HEIGHT - h) - 4 + y - 2,
                w + 4,
                h + height + 4,
                darkish,
                dark,
            );
        }
        draw_state.rgba(layer).outlined_rect(
            (WIDTH - w) / 2 + x,
            (HEIGHT - h) - 4 + y,
            w,
            h + height,
            bg_colour,
            outline_colour,
        );
        let options = DIALOGUE_OPTIONS.get_options(system);
        let text: &str = if timer {
            &string[..=(print_timer)]
        } else {
            string
        };
        system.print_to(
            draw_state.rgba(layer),
            text,
            (WIDTH - w) / 2 + 3 + x,
            (HEIGHT - h) - 4 + 3 + y,
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
    let small_text = DIALOGUE_OPTIONS.small_text(system);
    fit_paragraph(
        system,
        string,
        wrap_width,
        DIALOGUE_OPTIONS.fixed(),
        small_text,
    )
}
