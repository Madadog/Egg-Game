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

use std::{fmt::format, sync::RwLock};

use crate::{
    animation::{AnimFrame, Animation},
    portraits::TalkPic,
    print_alloc, save,
    sound::{self, SfxData},
    tic80_core::SpriteOptions,
    trace, PrintOptions, tic80_helpers::{palette_map_rotate, palette_map_reset, spr_outline, blit_segment},
};

#[derive(Debug, Clone)]
pub enum TextContent {
    Text(&'static str),
    Delayed(&'static str, u8),
    Delay(u8),
    Sound(SfxData),
    Portrait(Option<&'static TalkPic>),
    Pause,
    AutoText(&'static str),
}
impl TextContent {
    pub fn is_auto(&self) -> bool {
        use TextContent::*;
        match self {
            Text(_) | Pause => false,
            _ => true,
        }
    }
    pub fn is_skip(&self) -> bool {
        use TextContent::*;
        match self {
            Sound(_) | Portrait(_) => true,
            _ => false,
        }
    }
}

pub struct DialogueOptions {
    pub fixed: RwLock<bool>,
    pub box_width: RwLock<usize>,
}
impl DialogueOptions {
    pub const fn new() -> Self {
        Self {
            fixed: RwLock::new(false),
            box_width: RwLock::new(200),
        }
    }
    pub fn fixed(&self) -> bool {
        *self.fixed.read().unwrap()
    }
    pub fn small_text(&self) -> bool {
        save::SMALL_TEXT_ON.is_true()
    }
    pub fn box_width(&self) -> usize {
        *self.box_width.read().unwrap()
    }
    pub fn set_options(&self, fixed: bool, small_text: bool) {
        *self.fixed.write().unwrap() = fixed;
        if small_text {
            save::SMALL_TEXT_ON.set_true()
        } else {
            save::SMALL_TEXT_ON.set_false()
        }
    }
    pub fn get_options(&self) -> PrintOptions {
        PrintOptions {
            fixed: self.fixed(),
            small_text: self.small_text(),
            ..Default::default()
        }
    }
    pub fn toggle_small_text(&self) {
        save::SMALL_TEXT_ON.toggle();
    }
    pub fn toggle_fixed(&self) {
        *self.fixed.write().unwrap() = !self.fixed()
    }
}
pub static DIALOGUE_OPTIONS: DialogueOptions = DialogueOptions::new();

pub struct Dialogue {
    pub current_text: Option<String>,
    pub characters: usize,
    pub next_text: Vec<TextContent>,
    pub width: usize,
    pub delay: usize,
    pub print_time: Option<usize>,
    pub portrait: Option<Animation<'static>>,
}
impl Dialogue {
    pub const fn const_default() -> Self {
        Self {
            current_text: None,
            next_text: Vec::new(),
            characters: 0,
            width: 200,
            delay: 0,
            print_time: None,
            portrait: None,
        }
    }
    pub fn with_width(self, width: usize) -> Self {
        Self { width, ..self }
    }
    pub fn is_line_done(&self) -> bool {
        match &self.current_text {
            Some(text) => text.len() == 0 || self.characters == text.len() - 1,
            None => true,
        }
    }
    fn set_current_text(&mut self, string: &str) {
        self.current_text = Some(self.fit_text(string));
        self.characters = 0;
        self.print_time = Some(0);
    }
    pub fn add_text(&mut self, string: &'static str) -> bool {
        if self.current_text.is_none() || self.is_line_done() {
            self.set_current_text(string);
            true
        } else {
            self.next_text.push(TextContent::Text(string));
            false
        }
    }
    pub fn maybe_add_text(&mut self, string: &'static str) {
        if self.current_text.is_none() {
            self.set_current_text(string);
        }
    }
    pub fn set_dialogue(&mut self, dialogue: &[&'static str]) {
        self.next_text = dialogue
            .iter()
            .rev()
            .map(|x| TextContent::Text(x))
            .collect();
        self.next_text();
    }
    pub fn set_enum_text(&mut self, dialogue: &[TextContent]) {
        self.next_text = dialogue.iter().rev().cloned().collect();
        self.next_text();
    }
    pub fn next_text(&mut self) -> bool {
        if let Some(text_content) = self.next_text.pop() {
            trace!(format!("Popping text content: {:?}", text_content), 12);
            let skip = text_content.is_skip();
            let val = self.consume_text_content(text_content);
            if skip {
                self.next_text()
            } else {
                val
            }
        } else {
            false
        }
    }
    pub fn consume_text_content(&mut self, text_content: TextContent) -> bool {
        match text_content {
            TextContent::Text(text) | TextContent::AutoText(text) => self.add_text(text),
            TextContent::Delay(x) => {
                self.add_delay(x.into());
                true
            }
            TextContent::Delayed(text, delay) => {
                let wrap_width = self.wrap_width();
                if let Some(string) = &mut self.current_text {
                    string.push_str(text);
                    *string = fit_default_paragraph(string, wrap_width);
                    self.add_delay(delay.into());
                } else {
                    self.add_text(text);
                }
                true
            }
            TextContent::Sound(x) => {
                x.play();
                true
            }
            TextContent::Portrait(x) => {
                self.portrait = if let Some(portrait) = x {
                    Some(portrait.clone().to_anim())
                } else {
                    None
                };
                true
            }
            TextContent::Pause => true,
        }
    }
    pub fn fit_text(&self, string: &str) -> String {
        fit_default_paragraph(string, self.wrap_width())
    }
    pub fn wrap_width(&self) -> usize {
        self.width - 3
    }
    pub fn close(&mut self) {
        *self = Self {
            width: self.width,
            ..Self::const_default()
        }
    }
    pub fn text_len(&self) -> usize {
        self.current_text.as_ref().map_or(0, |x| x.len())
    }
    pub fn tick(&mut self, amount: usize) {
        if let Some(text) = &mut self.current_text {
            // trace!(format!("delay = {}", self.delay),12);
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
            } else {
                trace!(format!("index was {}", self.characters), 12);
            }
            self.print_time = self.print_time.map(|x| x + 1);
            if !silent_char && self.print_time.unwrap() % 2 == 0 && !self.is_line_done() {
                sound::CLICK.with_volume(2).play();
            }
            self.step_text(amount);
            self.delay += 1;
        }
        // trace!(format!("self.is_line_done(): {},  self.can_autoadvance(): {}", self.is_line_done(), self.can_autoadvance()),12);
        if self.is_line_done() && self.can_autoadvance() {
            self.next_text();
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
    pub fn skip(&mut self) {
        while self.can_autoadvance() {
            self.next_text();
        }
        if let Some(text) = &mut self.current_text {
            self.characters = text.len() - 1;
        }
    }
    pub fn set_options(&mut self, fixed: bool, small_text: bool) {
        DIALOGUE_OPTIONS.set_options(fixed, small_text);
        let width = self.wrap_width();
        if let Some(text) = &mut self.current_text {
            *text = fit_default_paragraph(text, width);
        }
    }
    pub fn draw_dialogue_portrait(
        &self,
        string: &str,
        timer: bool,
        portrait: i32,
        scale: i32,
        sw: i32,
        sh: i32,
    ) {
        use crate::tic80_helpers::rect_outline;
        use crate::{spr, HEIGHT, WIDTH};

        let w = self.width as i32;
        let h = 24;
        self.draw_dialogue_box_with_offset(string, timer, 14, -2, 4);
        rect_outline((WIDTH - w) / 2 - 13, (HEIGHT - h) - 6, h + 4, h + 4, 0, 3);

        spr(
            portrait,
            (WIDTH - w) / 2 - 13 + 2,
            (HEIGHT - h) - 6 + 2,
            SpriteOptions {
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
        string: &str,
        timer: bool,
        mut x: i32,
        mut y: i32,
        mut height: i32,
    ) {
        use crate::tic80_helpers::rect_outline;
        use crate::tic80_core::rectb;
        use crate::{HEIGHT, WIDTH};

        let print_timer = self.characters;
        let w = self.width as i32;
        let h = 24;
        // Portrait
        if let Some(anim) = &self.portrait {
            rect_outline((WIDTH - w) / 2 - 13, (HEIGHT - h) - 6, h + 4, h + 4, 0, 3);
            let frame = anim.current_frame();
            x += 14;
            y -= 2;
            height += 4;
            let (x, y): (i32, i32) = (frame.pos.x.into(), frame.pos.y.into());
            blit_segment(4);
            palette_map_rotate(frame.palette_rotate);
            spr_outline(
                frame.spr_id.into(),
                (WIDTH - w) / 2 - 13 + 2 + x,
                (HEIGHT - h) - 6 + 2 + y,
                frame.options.clone(),
                frame.outline_colour.unwrap_or_default(),
            );
            palette_map_reset();
            rectb((WIDTH - w) / 2 - 13, (HEIGHT - h) - 6, h + 4, h + 4, 3);
        }
        // Text box
        rect_outline(
            (WIDTH - w) / 2 + x,
            (HEIGHT - h) - 4 + y,
            w,
            h + height,
            2,
            3,
        );
        print_alloc(
            if timer {
                &string[..=(print_timer)]
            } else {
                string
            },
            (WIDTH - w) / 2 + 3 + x,
            (HEIGHT - h) - 4 + 3 + y,
            PrintOptions {
                color: 12,
                ..DIALOGUE_OPTIONS.get_options()
            },
        );
    }
    pub fn draw_dialogue_box(&self, string: &str, timer: bool) {
        self.draw_dialogue_box_with_offset(string, timer, 0, 0, 0)
    }
}

pub fn print_width(string: &str, fixed: bool, small_font: bool) -> i32 {
    print_alloc(
        string,
        250,
        200,
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
        if print_width(taken, fixed, small_font) as usize > wrap_width {
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

pub fn fit_paragraph(string: &str, wrap_width: usize, fixed: bool, small_font: bool) -> String {
    let len = string.split_inclusive(' ').count();
    let mut paragraph = String::new();
    let mut skip = 0;
    while skip < len {
        let (string, x, all_fits) = fit_string(string, wrap_width, skip, fixed, small_font);
        skip += x;
        paragraph.push_str(&string);
        if all_fits {
            return paragraph;
        }
        paragraph.push('\n');
    }
    paragraph
}

pub fn fit_default_paragraph(string: &str, wrap_width: usize) -> String {
    fit_paragraph(
        string,
        wrap_width,
        DIALOGUE_OPTIONS.fixed(),
        DIALOGUE_OPTIONS.small_text(),
    )
}
