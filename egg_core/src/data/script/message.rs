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

//! The data a conversation is made of: a [`Message`] (one "page" of dialogue,
//! under one speaker) and the [`TextContent`] items it is built from. This is
//! script data — the registry built by [`crate::data::script`] stores dialogue
//! as `Vec<Message>` — so it lives beside the script, not with the [`Dialogue`]
//! box widget (in [`crate::ui::dialogue`]) that plays it.

use crate::data::{portraits::Portrait, sound::SfxData};

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
    Text {
        text: String,
        pause: bool,
        delay: u8,
    },
    Delay(u8),
    Sound(SfxData),
    Portrait(Option<Portrait>),
    Pause,
    Flip(bool),
    /// Set (or clear) a named save flag when playback reaches this point — the
    /// `#set NAME BOOL` directive. Fires exactly like a [`Sound`](Self::Sound)
    /// (it is a `is_skip` item, consumed in place), so the flag flips at the
    /// observable moment the dialogue plays past it. See [`crate::data::script::eggtext`].
    SetFlag(String, bool),
    /// An interactive branch point — the `#choice` block. Presents `options` in
    /// the dialogue box and blocks playback (neither [`is_auto`](Self::is_auto)
    /// nor [`is_skip`](Self::is_skip)) until the player picks one; the picked
    /// option's flags are then written through the same [`SetFlag`](Self::SetFlag)
    /// machinery (`save.set_flag`) and playback continues. Follow-up text
    /// branches on those flags through the ordinary `#if` flatten on the next
    /// [`get_dialogue`](crate::data::script::Script::get_dialogue).
    Choice(Vec<ChoiceOption>),
}
impl TextContent {
    pub fn is_auto(&self) -> bool {
        use TextContent::*;
        !matches!(self, Text { pause: true, .. } | Pause | Choice(_))
    }
    pub fn is_skip(&self) -> bool {
        use TextContent::*;
        matches!(self, Sound(_) | Portrait(_) | Flip(_) | SetFlag(..))
    }
    /// Plain text (stops on a manual advance unless reached via auto-advance).
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text {
            text: s.into(),
            pause: true,
            delay: 0,
        }
    }
    /// Text that auto-advances into a new frame once the previous line is done.
    pub fn auto(s: impl Into<String>) -> Self {
        Self::Text {
            text: s.into(),
            pause: false,
            delay: 0,
        }
    }
    /// Text appended to the current line after a `delay`-frame pause.
    pub fn delayed(s: impl Into<String>, delay: u8) -> Self {
        Self::Text {
            text: s.into(),
            pause: false,
            delay,
        }
    }
}

/// A single "page" of dialogue: a run of text [`content`](Message::content)
/// shown under one speaker (`portrait` + `flip_portrait`). `pause_when_done`
/// controls whether the player must press to continue to the *next* message,
/// or whether it auto-advances. Dialogue is stored as `Vec<Message>` in the
/// registry built by [`crate::data::script`] and queued via
/// [`Dialogue::set_messages`](crate::ui::dialogue::Dialogue::set_messages).
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

/// One selectable option of a [`TextContent::Choice`] — the `#option` line of a
/// `#choice` block. `text` is what the menu shows; `sets` is the flags it writes
/// when picked, each a `(name, value)` applied exactly like a `#set`
/// ([`TextContent::SetFlag`]). An option may set zero flags (a "never mind" that
/// just closes the menu).
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    pub text: String,
    pub sets: Vec<(String, bool)>,
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
