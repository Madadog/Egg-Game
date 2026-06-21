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
//! it — via the [`eggtext`](crate::data::script::eggtext) DSL, or straight from JSON —
//! into a [`ScriptFile`] and installs it into the [`Script`] registry it owns
//! (via [`Script::set_base`] / [`Script::set_language`]); gameplay code
//! reads it back through the shared context ([`Ctx::label`](crate::Ctx::label),
//! [`Ctx::list`](crate::Ctx::list), [`Ctx::get_dialogue`](crate::Ctx::get_dialogue)).
//!
//! A *base* language is always kept as a fallback. A *language* can be swapped
//! in at runtime; any key it doesn't define falls back to the base, so partial
//! translations work and switching is just another [`Script::set_language`].
//!
//! Portrait and sound references in dialogue are names (e.g. `"horror"`,
//! `"gain"`), resolved to values at install time via
//! [`portraits::by_name`](crate::data::portraits::by_name) and
//! [`sound::by_name`](crate::data::sound::by_name).

use std::collections::{BTreeSet, HashMap};

use serde::Deserialize;

pub mod eggtext;
pub mod message;

use crate::data::save::SaveData;
use crate::data::script::message::{Message, TextContent};
use crate::data::{portraits, sound};

// --- on-disk schema (deserialized as-is, names not yet resolved) ---

/// A whole language file. Every section is optional so a language overlay can
/// define only what it overrides.
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
    pub dialogue: HashMap<String, DialogueDef>,
    /// The named save flags this script declares (`#flag NAME`, or a top-level
    /// `"flags": [...]` array in JSON). The vocabulary `#set`/`#if` may name and
    /// an in-game editor autocompletes against; the resolved [`Script`] re-exposes
    /// it via [`Script::flags`].
    #[serde(default)]
    pub flags: BTreeSet<String>,
}

/// A dialogue map value: either a plain conversation (an [`Entry`]) or, for
/// entries that branch on a save flag, an explicit list of [`SegmentDef`]s
/// (`{ "segments": [...] }`). Distinguished by JSON shape — a `{ "segments" }`
/// object is the only thing that isn't a plain entry, so every pre-existing
/// dialogue value keeps parsing unchanged.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum DialogueDef {
    Plain(Entry),
    Segments { segments: Vec<SegmentDef> },
}

/// One piece of a dialogue body: an unconditional run of messages, or a
/// flag-gated `#if`. A conditional includes its whole `then` branch when the
/// named flag is set, otherwise its `else` branch (empty if absent); the choice
/// is made at [`Ctx::get_dialogue`](crate::Ctx::get_dialogue) time against the
/// live save. JSON: a plain [`Entry`], or `{ "if": "flag", "then": <entry>,
/// "else": <entry> }` (the `else` key optional).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum SegmentDef {
    Plain(Entry),
    If {
        #[serde(rename = "if")]
        flag: String,
        then: Entry,
        #[serde(default, rename = "else")]
        otherwise: Option<Entry>,
    },
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
/// `{"portrait": "y_oof"}`, `{"flip": true}`, `{"delay": 30}`,
/// `{"set_flag": ["name", true]}`, or `"pause"`.
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
    /// Set (or clear) a named save flag when playback reaches this point — the
    /// `#set NAME BOOL` directive. JSON: `{"set_flag": ["name", true]}`.
    SetFlag(String, bool),
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
            ContentDef::SetFlag(name, value) => TextContent::SetFlag(name, value),
        })
    }
}

impl MessageDef {
    fn resolve(self) -> Message {
        Message {
            content: self
                .content
                .into_iter()
                .filter_map(ContentDef::resolve)
                .collect(),
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

impl SegmentDef {
    fn resolve(self) -> Segment {
        match self {
            SegmentDef::Plain(entry) => Segment::Plain(entry.resolve()),
            SegmentDef::If {
                flag,
                then,
                otherwise,
            } => Segment::If {
                flag,
                then: then.resolve(),
                otherwise: otherwise.map(Entry::resolve).unwrap_or_default(),
            },
        }
    }
}

impl DialogueDef {
    fn resolve(self) -> Vec<Segment> {
        match self {
            DialogueDef::Plain(entry) => vec![Segment::Plain(entry.resolve())],
            DialogueDef::Segments { segments } => {
                segments.into_iter().map(SegmentDef::resolve).collect()
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

/// One resolved piece of a dialogue entry: a run of messages always shown, or a
/// flag-gated branch resolved per lookup against the live save (see
/// [`flatten`](Segment::flatten)). The `#if`/`#else`/`#end` model — and the only
/// reason a resolved entry is kept as segments rather than a flat `Vec<Message>`.
#[derive(Debug, Clone)]
enum Segment {
    Plain(Vec<Message>),
    /// `then` when `flag` is set, otherwise `otherwise` (which is empty when the
    /// `#if` had no `#else`).
    If {
        flag: String,
        then: Vec<Message>,
        otherwise: Vec<Message>,
    },
}

impl Segment {
    /// Flatten a segment list to the messages that actually play, choosing each
    /// conditional's branch by the live save flags. Lists (always a single
    /// [`Plain`](Segment::Plain) segment) flatten the same way with any save.
    fn flatten(segments: &[Segment], save: &SaveData) -> Vec<Message> {
        let mut out = Vec::new();
        for segment in segments {
            match segment {
                Segment::Plain(messages) => out.extend(messages.iter().cloned()),
                Segment::If {
                    flag,
                    then,
                    otherwise,
                } => {
                    let branch = if save.flag(flag) { then } else { otherwise };
                    out.extend(branch.iter().cloned());
                }
            }
        }
        out
    }
}

/// One resolved language. Portrait/sound names are turned into values and every
/// keyed entry — dialogue *and* lists — becomes a `Vec<Segment>`; the
/// dialogue-vs-list distinction only exists in the JSON file (a list is one
/// unconditional [`Plain`](Segment::Plain) segment). `labels` stay separate
/// because they're printed directly rather than run through the dialogue system.
#[derive(Debug, Clone, Default)]
struct Language {
    labels: HashMap<String, String>,
    /// Both `dialogue` and `lists` JSON sections, keyed together. A list entry
    /// is stored as one single-line [`Message`] per string, in one `Plain`
    /// segment.
    entries: HashMap<String, Vec<Segment>>,
    /// The original, unresolved dialogue defs, kept verbatim so the in-game
    /// dialogue editor can load a key back into an editable draft and classify
    /// it (a resolved `Vec<Segment>` has lost the `#if` structure and the
    /// authored shape). Dialogue only — lists live in `entries` and aren't here,
    /// so `raw_dialogue.keys()` is exactly the dialogue key set.
    raw_dialogue: HashMap<String, DialogueDef>,
    /// The flag vocabulary this language declared. Merged base+active is what
    /// [`Script::flags`] reports.
    flags: BTreeSet<String>,
}

impl Language {
    fn resolve(file: ScriptFile) -> Self {
        let raw_dialogue = file.dialogue.clone();
        let mut entries: HashMap<String, Vec<Segment>> = file
            .dialogue
            .into_iter()
            .map(|(key, entry)| (key, entry.resolve()))
            .collect();
        for (key, lines) in file.lists {
            let messages = lines.into_iter().map(Message::from).collect();
            entries.insert(key, vec![Segment::Plain(messages)]);
        }
        Language {
            labels: file.labels,
            entries,
            raw_dialogue,
            flags: file.flags,
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
    /// Stored as single-line messages, read back as their plain text. Lists are
    /// unconditional, so the (always single, `Plain`) segments need no save.
    pub fn list(&self, key: &str) -> Vec<String> {
        self.list_messages(key)
            .map(|msgs| msgs.iter().map(Message::to_plain_string).collect())
            .unwrap_or_default()
    }

    /// One entry of an ordered string list, or `None` if the key or index is
    /// undefined. Cheaper than [`Script::list`] when only one entry is wanted.
    pub fn list_get(&self, key: &str, index: usize) -> Option<String> {
        self.list_messages(key)
            .and_then(|msgs| msgs.into_iter().nth(index))
            .as_ref()
            .map(Message::to_plain_string)
    }

    /// A dialogue conversation resolved against the live `save` (its `#if`
    /// branches choose by `save.flags`), falling back to the `default` entry
    /// then an empty conversation for unknown keys.
    pub fn get_dialogue(&self, key: &str, save: &SaveData) -> Vec<Message> {
        self.entry(key)
            .or_else(|| self.entry("default"))
            .map(|segments| Segment::flatten(segments, save))
            .unwrap_or_default()
    }

    /// Every dialogue key the loaded base + active languages define, sorted —
    /// what the in-game dialogue browser lists. Excludes lists and labels.
    pub fn dialogue_keys(&self) -> Vec<String> {
        let keys: BTreeSet<&String> = self
            .active
            .raw_dialogue
            .keys()
            .chain(self.base.raw_dialogue.keys())
            .collect();
        keys.into_iter().cloned().collect()
    }

    /// A dialogue key's original, unresolved [`DialogueDef`] (active language
    /// then base), for the in-game editor to load into an editable draft. Unlike
    /// [`get_dialogue`](Self::get_dialogue) this keeps the authored `#if`
    /// structure and shape rather than flattening against a save.
    pub fn raw_dialogue(&self, key: &str) -> Option<&DialogueDef> {
        self.active
            .raw_dialogue
            .get(key)
            .or_else(|| self.base.raw_dialogue.get(key))
    }

    /// The merged flag vocabulary the loaded base + active languages declared —
    /// what an in-game editor autocompletes `#set`/`#if`/flag references against.
    pub fn flags(&self) -> BTreeSet<String> {
        self.base
            .flags
            .iter()
            .chain(self.active.flags.iter())
            .cloned()
            .collect()
    }

    /// A list entry's messages, flattening its unconditional segments. Used by
    /// [`list`](Self::list)/[`list_get`](Self::list_get), which have no save to
    /// resolve conditionals against — lists never carry any.
    fn list_messages(&self, key: &str) -> Option<Vec<Message>> {
        self.entry(key).map(|segments| {
            segments
                .iter()
                .flat_map(|segment| match segment {
                    Segment::Plain(messages) => messages.clone(),
                    // Lists are authored unconditionally; an `If` here would be a
                    // misuse, so take its `then` branch as a best effort.
                    Segment::If { then, .. } => then.clone(),
                })
                .collect()
        })
    }

    /// Look up a keyed entry (dialogue or list) as its resolved segments, active
    /// language then base.
    fn entry(&self, key: &str) -> Option<&Vec<Segment>> {
        self.active
            .entries
            .get(key)
            .or_else(|| self.base.entries.get(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::script::eggtext;

    /// Install a script from `.eggtext` source (the DSL resolves to the same
    /// [`ScriptFile`] the JSON loader produces).
    fn script(src: &str) -> Script {
        let mut script = Script::new();
        script.set_base(eggtext::parse(src).expect("parse eggtext"));
        script
    }

    /// The concatenated plain text of a resolved conversation.
    fn plain(messages: &[Message]) -> String {
        messages.iter().map(Message::to_plain_string).collect()
    }

    #[test]
    fn get_dialogue_picks_the_if_branch_by_flag() {
        let script = script(
            "#flag seen\n\
             #dialogue d\n\
             \x20   #if seen\n\
             \x20   After.\n\
             \x20   #else\n\
             \x20   Before.\n\
             \x20   #end",
        );

        let mut save = SaveData::default();
        // Flag unset → the `#else` branch.
        assert_eq!(plain(&script.get_dialogue("d", &save)), "Before.");
        // Flag set → the `#if` branch.
        save.set_flag("seen", true);
        assert_eq!(plain(&script.get_dialogue("d", &save)), "After.");
    }

    #[test]
    fn get_dialogue_skips_an_if_without_else_when_unset() {
        let script = script(
            "#flag seen\n\
             #dialogue d\n\
             \x20   Intro.\n\n\
             \x20   #if seen\n\
             \x20   Extra.\n\
             \x20   #end",
        );
        let mut save = SaveData::default();
        // No `#else`, flag unset → only the unconditional message survives.
        let convo = script.get_dialogue("d", &save);
        assert_eq!(convo.len(), 1);
        assert_eq!(plain(&convo), "Intro.");
        // Flag set → both messages, in order.
        save.set_flag("seen", true);
        let convo = script.get_dialogue("d", &save);
        assert_eq!(convo.len(), 2);
        assert_eq!(plain(&convo), "Intro.Extra.");
    }

    #[test]
    fn set_flag_item_survives_resolution() {
        let script = script("#flag seen\n#dialogue d\n    #set seen true\n    Hi.");
        let convo = script.get_dialogue("d", &SaveData::default());
        // The `#set` becomes a SetFlag content item ahead of the text, so it
        // fires the moment playback reaches it.
        assert!(matches!(
            convo[0].content.first(),
            Some(TextContent::SetFlag(name, true)) if name == "seen"
        ));
        assert!(matches!(
            convo[0].content.get(1),
            Some(TextContent::Text { .. })
        ));
    }

    #[test]
    fn flags_vocabulary_is_exposed() {
        let script = script("#flag one\n#flag two\ngame_title = hi");
        let flags = script.flags();
        assert!(flags.contains("one"));
        assert!(flags.contains("two"));
        assert_eq!(flags.len(), 2);
    }

    #[test]
    fn unknown_key_falls_back_to_default() {
        let script = script("#dialogue default\n    Nothing here.");
        assert_eq!(
            plain(&script.get_dialogue("missing", &SaveData::default())),
            "Nothing here."
        );
    }

    #[test]
    fn lists_read_back_through_segments() {
        // Lists resolve to one unconditional `Plain` segment; `list`/`list_get`
        // flatten it without needing a save.
        let script = script("#list things\n    one\n    two\n    three");
        assert_eq!(script.list("things"), ["one", "two", "three"]);
        assert_eq!(script.list_get("things", 1).as_deref(), Some("two"));
        assert_eq!(script.list_get("things", 9), None);
    }
}
