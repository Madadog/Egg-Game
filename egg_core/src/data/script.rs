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

//! Runtime-loaded game text — UI labels, string lists, and dialogue — read from
//! a per-language script file (e.g. `assets/script/en.eggtext`). The host parses
//! it — via the [`eggtext`](crate::data::eggtext) DSL, or straight from JSON —
//! into a [`ScriptFile`] and installs it into the [`Script`] registry it owns
//! (via [`Script::set_base`] / [`Script::set_language`]); gameplay code
//! reads it back through the console (`system.label(..)`, `system.get_dialogue(..)`,
//! `system.print_label(..)`).
//!
//! A *base* language is always kept as a fallback. A *language* can be swapped
//! in at runtime; any key it doesn't define falls back to the base, so partial
//! translations work and switching is just another [`Script::set_language`].
//!
//! Portrait and sound references in dialogue are names (e.g. `"horror"`,
//! `"gain"`), resolved to values at install time via
//! [`portraits::by_name`](crate::data::portraits::by_name) and
//! [`sound::by_name`](crate::data::sound::by_name).

use std::collections::HashMap;

use serde::Deserialize;

use crate::data::{portraits, sound};
use crate::dialogue::{Message, TextContent};

// --- on-disk schema (deserialized as-is, names not yet resolved) ---

/// A whole language file. All three sections are optional so a language overlay
/// can define only what it overrides.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ScriptFile {
    /// Single strings printed directly (menu items, titles, item names…).
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Ordered string lists (e.g. debug-menu entries).
    #[serde(default)]
    pub lists: HashMap<String, Vec<String>>,
    /// Dialogue-box conversations.
    #[serde(default)]
    pub dialogue: HashMap<String, Entry>,
}

/// A dialogue entry: a single line, a sequence of manually-advanced pages, or a
/// full conversation (`{ "messages": [...] }`). Distinguished by JSON shape.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Line(String),
    Pages(Vec<String>),
    Conversation { messages: Vec<MessageDef> },
}

/// One "page" of a conversation under a single speaker.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MessageDef {
    /// Portrait name, or absent for narration.
    #[serde(default)]
    pub portrait: Option<String>,
    #[serde(default)]
    pub flip: bool,
    /// Whether to wait for player input before the next message (default true).
    #[serde(default = "default_true")]
    pub pause: bool,
    pub content: Vec<ContentDef>,
}

/// A single content item within a message. Externally tagged, so JSON is
/// `{"auto": "..."}`, `{"delayed": ["...", 30]}`, `{"sound": "gain"}`,
/// `{"portrait": "y_oof"}`, `{"flip": true}`, `{"delay": 30}`, or `"pause"`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentDef {
    /// Plain text (advances manually unless reached via auto-advance).
    Text(String),
    /// Text that auto-advances into view.
    Auto(String),
    /// Text appended after a frame delay.
    Delayed(String, u8),
    /// A pause of N frames.
    Delay(u8),
    /// Play a sound by name.
    Sound(String),
    /// Switch (or clear, if null) the portrait mid-message.
    Portrait(Option<String>),
    /// Wait for player input.
    Pause,
    /// Flip the portrait side.
    Flip(bool),
}

fn default_true() -> bool {
    true
}

impl ContentDef {
    fn resolve(self) -> Option<TextContent> {
        Some(match self {
            ContentDef::Text(s) => TextContent::text(s),
            ContentDef::Auto(s) => TextContent::auto(s),
            ContentDef::Delayed(s, d) => TextContent::delayed(s, d),
            ContentDef::Delay(d) => TextContent::Delay(d),
            ContentDef::Sound(name) => match sound::by_name(&name) {
                Some(sfx) => TextContent::Sound(sfx),
                None => {
                    log::warn!("dialogue references unknown sound {name:?}");
                    return None;
                }
            },
            ContentDef::Portrait(name) => TextContent::Portrait(resolve_portrait(name)),
            ContentDef::Pause => TextContent::Pause,
            ContentDef::Flip(b) => TextContent::Flip(b),
        })
    }
}

impl MessageDef {
    fn resolve(self) -> Message {
        Message {
            content: self.content.into_iter().filter_map(ContentDef::resolve).collect(),
            portrait: resolve_portrait(self.portrait),
            flip_portrait: self.flip,
            pause_when_done: self.pause,
        }
    }
}

impl Entry {
    fn resolve(self) -> Vec<Message> {
        match self {
            Entry::Line(s) => vec![Message::from(s)],
            Entry::Pages(pages) => pages.into_iter().map(Message::from).collect(),
            Entry::Conversation { messages } => {
                messages.into_iter().map(MessageDef::resolve).collect()
            }
        }
    }
}

fn resolve_portrait(name: Option<String>) -> Option<portraits::Portrait> {
    let name = name?;
    let portrait = portraits::by_name(&name);
    if portrait.is_none() {
        log::warn!("dialogue references unknown portrait {name:?}");
    }
    portrait
}

// --- resolved, in-memory script ---

/// One resolved language. Portrait/sound names are turned into values and every
/// keyed entry — dialogue *and* lists — becomes a `Vec<Message>`; the
/// dialogue-vs-list distinction only exists in the JSON file. `labels` stay
/// separate because they're printed directly rather than run through the
/// dialogue system.
#[derive(Debug, Clone, Default)]
struct Language {
    labels: HashMap<String, String>,
    /// Both `dialogue` and `lists` JSON sections, keyed together. A list entry
    /// is stored as one single-line [`Message`] per string.
    entries: HashMap<String, Vec<Message>>,
}

impl Language {
    fn resolve(file: ScriptFile) -> Self {
        let mut entries: HashMap<String, Vec<Message>> = file
            .dialogue
            .into_iter()
            .map(|(key, entry)| (key, entry.resolve()))
            .collect();
        for (key, lines) in file.lists {
            entries.insert(key, lines.into_iter().map(Message::from).collect());
        }
        Language {
            labels: file.labels,
            entries,
        }
    }
}

/// The game's text registry, owned by the host console (no global state). Holds
/// a base/fallback language plus the currently active language; lookups try the
/// active language first, then fall back to the base.
#[derive(Debug, Clone, Default)]
pub struct Script {
    base: Language,
    active: Language,
}

impl Script {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the base/fallback language (also makes it active). Call once at
    /// startup with the default language file.
    pub fn set_base(&mut self, file: ScriptFile) {
        let language = Language::resolve(file);
        self.active = language.clone();
        self.base = language;
    }

    /// Swap the active language at runtime. Keys it doesn't define fall back to
    /// the base language installed by [`Script::set_base`].
    pub fn set_language(&mut self, file: ScriptFile) {
        self.active = Language::resolve(file);
    }

    /// A UI label, or `[key]` if undefined in both the active and base languages.
    pub fn label(&self, key: &str) -> String {
        self.active
            .labels
            .get(key)
            .or_else(|| self.base.labels.get(key))
            .cloned()
            .unwrap_or_else(|| format!("[{key}]"))
    }

    /// An ordered string list (e.g. debug-menu entries), or empty if undefined.
    /// Stored as single-line messages, read back as their plain text.
    pub fn list(&self, key: &str) -> Vec<String> {
        self.entry(key)
            .map(|msgs| msgs.iter().map(Message::to_plain_string).collect())
            .unwrap_or_default()
    }

    /// One entry of an ordered string list, or `None` if the key or index is
    /// undefined. Cheaper than [`Script::list`] when only one entry is wanted.
    pub fn list_get(&self, key: &str, index: usize) -> Option<String> {
        self.entry(key)
            .and_then(|msgs| msgs.get(index))
            .map(Message::to_plain_string)
    }

    /// A dialogue conversation, falling back to the `default` entry then an
    /// empty conversation for unknown keys.
    pub fn get_dialogue(&self, key: &str) -> Vec<Message> {
        self.entry(key)
            .or_else(|| self.entry("default"))
            .cloned()
            .unwrap_or_default()
    }

    /// Look up a keyed entry (dialogue or list), active language then base.
    fn entry(&self, key: &str) -> Option<&Vec<Message>> {
        self.active.entries.get(key).or_else(|| self.base.entries.get(key))
    }
}
