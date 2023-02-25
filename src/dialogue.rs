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

use std::sync::RwLock;

use crate::{print_alloc, save, sound, tic80_core::SpriteOptions, PrintOptions};

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
    pub text: Option<String>,
    pub buffer: Vec<TextContent>,
    pub characters: usize,
    pub width: usize,
    pub delay: usize,
    pub print_time: Option<usize>,
}
impl Dialogue {
    pub const fn const_default() -> Self {
        Self {
            text: None,
            buffer: Vec::new(),
            characters: 0,
            width: 200,
            delay: 0,
            print_time: None,
        }
    }
    pub fn with_width(self, width: usize) -> Self {
        Self { width, ..self }
    }
    pub fn is_line_done(&self) -> bool {
        match &self.text {
            Some(text) => self.characters == text.len(),
            None => true,
        }
    }
    pub fn set_current_text(&mut self, string: &str) {
        self.text = Some(self.fit_text(string));
        self.characters = 0;
        self.print_time = Some(0);
    }
    pub fn try_set_text(&mut self, string: &str) -> bool {
        if self.text.is_none() || self.is_line_done() {
            self.set_current_text(string);
            true
        } else {
            false
        }
    }
    pub fn set_dialogue(&mut self, dialogue: &[&'static str]) {
        self.buffer = dialogue.iter().rev().map(|x| TextContent::Text(x)).collect();
        self.next_text();
    }
    pub fn set_enum_text(&mut self, dialogue: &[TextContent]) {
        self.buffer = dialogue.iter().rev().cloned().collect();
        self.next_text();
    }
    pub fn next_text(&mut self) -> bool {
        if let Some(text_content) = self.buffer.pop() {
            self.consume_text_content(text_content)
        } else {
            false
        }
    }
    pub fn consume_text_content(&mut self, text_content: TextContent) -> bool {
        match text_content {
            TextContent::Text(x) => {
                self.try_set_text(x)
            },
            TextContent::Delay(x) => {
                self.add_delay(x as usize);
                false
            },
        }
    }
    pub fn fit_text(&self, string: &str) -> String {
        fit_default_paragraph(string, self.wrap_width())
    }
    pub fn wrap_width(&self) -> usize {
        self.width - 3
    }
    pub fn close(&mut self) {
        self.text = None;
        self.buffer.clear();
        self.characters = 0;
        self.delay = 0;
        self.print_time = None;
    }
    pub fn tick(&mut self, amount: usize) {
        if self.characters == self.text.as_ref().map_or(0, |x| x.len()) {return}
        self.print_time = self.print_time.map(|x| x + 1);
        if let Some(text) = &mut self.text {
            if self.print_time.unwrap() % 4 == 0 {
                sound::CLICK.with_volume(2).play();
            }
            if self.delay != 0 {
                self.delay = self.delay.saturating_sub(amount);
                return;
            }
            if text.chars().nth(self.characters).unwrap() == '.' {
                self.delay += 4;
            }
            self.characters = (self.characters + amount).min(text.len());
            self.delay += 1;
        }
    }
    pub fn add_delay(&mut self, amount: usize) {
        self.delay.saturating_add(amount);
    }
    pub fn skip(&mut self) {
        if let Some(text) = &mut self.text {
            self.characters = text.len();
        }
    }
    pub fn set_options(&mut self, fixed: bool, small_text: bool) {
        DIALOGUE_OPTIONS.set_options(fixed, small_text);
        let width = self.wrap_width();
        if let Some(text) = &mut self.text {
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
        x: i32,
        y: i32,
        height: i32,
    ) {
        use crate::tic80_helpers::rect_outline;
        use crate::{HEIGHT, WIDTH};

        let print_timer = self.characters;
        let w = self.width as i32;
        let h = 24;
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
                &string[..(print_timer)]
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

/// Clamps a string to the specified width (with the TIC-80 font). Returns a string and
/// the number of fitting words.
pub fn fit_string(
    string: &str,
    wrap_width: usize,
    start_word: usize,
    fixed: bool,
    small_font: bool,
) -> (String, usize) {
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
    (take_words(string, line_length, start_word), line_length)
}

pub fn fit_paragraph(string: &str, wrap_width: usize, fixed: bool, small_font: bool) -> String {
    let len = string.split_inclusive(' ').count();
    let mut paragraph = String::new();
    let mut skip = 0;
    while skip < len {
        let (string, x) = fit_string(string, wrap_width, skip, fixed, small_font);
        skip += x;
        paragraph.push_str(&string);
        paragraph.push('\n');
        if x == 0 {
            return paragraph;
        }
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

#[derive(Debug, Clone)]
pub enum TextContent {
    Text(&'static str),
    Delay(u8),
}