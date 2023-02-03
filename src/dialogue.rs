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

use crate::{print_alloc, PrintOptions};

pub struct Dialogue {
    pub text: Option<String>,
    pub timer: usize,
    pub fixed: bool,
    pub small_text: bool,
}
impl Dialogue {
    pub const fn const_default() -> Self {
        Self {
            text: None,
            timer: 0,
            fixed: false,
            small_text: false,
        }
    }
    pub fn is_done(&self) -> bool {
        match &self.text {
            Some(text) => self.timer == text.len(),
            None => true,
        }
    }
    pub fn set_text(&mut self, string: &str) {
        self.text = Some(fit_paragraph(string, 194, self.fixed, self.small_text));
        self.timer = 0;
    }
    pub fn close(&mut self) {
        self.text = None;
        self.timer = 0;
    }
    pub fn tick(&mut self, amount: usize) {
        if let Some(text) = &mut self.text {
            self.timer = (self.timer + amount).min(text.len());
        }
    }
    pub fn skip(&mut self) {
        if let Some(text) = &mut self.text {
            self.timer = text.len();
        }
    }
    pub fn set_options(&mut self, fixed: bool, small_text: bool) {
        self.fixed = fixed;
        self.small_text = small_text;
        if let Some(text) = &mut self.text {
            *text = fit_paragraph(text, 196, self.fixed, self.small_text);
        }
    }
    pub fn toggle_small_text(&mut self) {
        self.small_text = !self.small_text
    }
    pub fn toggle_fixed(&mut self) {
        self.fixed = !self.fixed
    }
    pub fn get_options(&self) -> PrintOptions {
        PrintOptions {
            fixed: self.fixed,
            small_text: self.small_text,
            ..Default::default()
        }
    }
}

use crate::trace;
pub fn print_width(string: &str, fixed: bool, small_font: bool) -> i32 {
    let width = print_alloc(
        string,
        250,
        200,
        PrintOptions {
            fixed,
            small_text: small_font,
            ..Default::default()
        },
    );
    trace!(format!("{}", width), 12);
    width
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
    trace!(format!("len: {}", len), 12);
    for i in 1..=len {
        let taken = &take_words(&string, i, start_word);
        if print_width(taken, fixed, small_font) as usize > wrap_width {
            break;
        } else {
            line_length = i
        };
        trace!(format!("{}", taken), 12);
        trace!(format!("line length: {}", line_length), 12);
    }
    (take_words(&string, line_length, start_word), line_length)
}

pub fn fit_paragraph(string: &str, wrap_width: usize, fixed: bool, small_font: bool) -> String {
    let len = string.split_inclusive(' ').count();
    let mut paragraph = String::new();
    let mut skip = 0;
    while skip < len {
        let (string, x) = fit_string(&string, wrap_width, skip, fixed, small_font);
        skip += x;
        trace!(format!("Skip: {}", skip), 12);
        paragraph.push_str(&string);
        paragraph.push('\n');
        if x == 0 {
            return paragraph;
        }
    }
    paragraph
}
