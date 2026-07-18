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
//! reads it back through the shared context (`Ctx::label`,
//! `Ctx::list`, `Ctx::get_dialogue`).
//!
//! A *base* language is always kept as a fallback. A *language* can be swapped
//! in at runtime; any key it doesn't define falls back to the base, so partial
//! translations work and switching is just another [`Script::set_language`].
//!
//! Portrait and sound references in dialogue are names (e.g. `"horror"`,
//! `"gain"`), resolved to values at install time against a
//! [`Portraits`](crate::data::portraits::Portraits) registry threaded in by the
//! caller (portraits are runtime data — see [`Script::set_base`]) and
//! [`sound::by_name`](crate::data::sound::by_name) (sound effects are not).

use std::collections::{BTreeSet, HashMap};

use serde::Deserialize;

pub mod eggtext;
pub mod message;

use crate::data::portraits::Portraits;
use crate::data::script::message::{ChoiceOption, Message, PortraitState, TextContent};
use crate::data::sound;

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
/// flag-gated `#if`/`#elif`/`#else` chain.
///
/// **On disk** the chain is stored flat, in the shape it's authored in: the
/// `#if`'s own condition and branch, an ordered `elifs` list, and the
/// optional trailing `#else` branch. `negated` (`#if not NAME`) is stored
/// per-condition (on `If` itself and on each [`ElifDef`]) rather than baked
/// into anything else.
///
/// **At resolution** ([`SegmentDef::resolve`]) the flat chain is rebuilt into
/// nested `TextContent::If` carriers, from the inside out: the `#else`
/// branch (or nothing, if absent) is innermost, each `#elif` wraps it as
/// another carrier, and the `#if` wraps last. Either way it still resolves to
/// exactly one carrier message, so the dialogue box always picks a branch
/// one flag at a time, live, against the save the moment playback reaches it
/// — not once, up front, when the conversation is fetched. Negation is also
/// resolved here, by swapping a branch's `then`/`otherwise` when its carrier
/// is built, so `TextContent::If` itself never needs to know about `not`.
///
/// Each branch (`then`, `otherwise`, and an [`ElifDef`]'s own `then`) is a
/// full [`DialogueDef`] rather than a bare [`Entry`] — so a branch can itself
/// contain further `#if` segments (the `.eggtext` parser's indentation rules
/// let a *nested* `#if` open strictly inside another; see
/// [`crate::data::script::eggtext`]'s module doc). A nested `If` inside a
/// branch resolves through this exact same machinery, recursively — the
/// dialogue box never needs to know a carrier came from a nested `#if`.
///
/// JSON: a plain [`Entry`], or `{ "if": "flag", "then": <entry-or-segments>,
/// "negated": bool, "elifs": [...], "else": <entry-or-segments> }`.
/// `negated`, `elifs` and `else` all default when absent, and `then`/`else`/
/// an `ElifDef`'s `then` accept either shape (untagged, same as
/// [`DialogueDef`] itself) — so the pre-nesting shape, where every branch was
/// a plain [`Entry`], still deserializes completely unchanged.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum SegmentDef {
    Plain(Entry),
    If {
        #[serde(rename = "if")]
        flag: String,
        #[serde(default)]
        negated: bool,
        then: DialogueDef,
        #[serde(default, rename = "else")]
        otherwise: Option<DialogueDef>,
        #[serde(default)]
        elifs: Vec<ElifDef>,
    },
}

/// One `#elif [not] NAME` branch of a [`SegmentDef::If`] chain: its own
/// condition (`flag`/`negated`) and the branch it guards, gathered flat
/// alongside its `#if` (see [`SegmentDef`]'s doc for how the chain nests at
/// resolution, and for why `then` is a full [`DialogueDef`]). JSON: `{
/// "flag": "name", "negated": bool, "then": <entry-or-segments> }`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ElifDef {
    pub flag: String,
    #[serde(default)]
    pub negated: bool,
    pub then: DialogueDef,
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

/// A message's speaker portrait as authored: unspecified (`Keep`, the
/// default — no `#pic` in this message, so it carries over whatever was
/// showing before), explicitly cleared back to narration (`Clear`, `#pic
/// none`), or switched to a named portrait (`Set`, `#pic NAME`). Distinct from
/// `Option<Option<String>>` so "never mentioned" and "explicitly cleared"
/// can't be confused — that's exactly the distinction that lets
/// [`message::Message::portrait`](crate::data::script::message::Message::portrait)'s
/// carry-over survive an `#if` branch (see its doc, and
/// [`crate::data::script::eggtext`]'s module doc, for why carry-over can't
/// just be resolved here at parse/deserialize time).
///
/// JSON: `"keep"` (also what an absent `portrait` field on [`MessageDef`]
/// defaults to), `"clear"`, or `{"set": "y_oof"}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortraitChange {
    Keep,
    Clear,
    Set(String),
}

impl Default for PortraitChange {
    fn default() -> Self {
        Self::Keep
    }
}

impl PortraitChange {
    /// Resolve the authored name against `portraits`. An unresolvable name (a
    /// `#pic` referencing a portrait that doesn't exist) warns and resolves to
    /// [`PortraitState::Clear`] — mirrors [`resolve_portrait`]'s handling of
    /// the same case for a mid-message portrait switch.
    fn resolve(self, portraits: &Portraits) -> PortraitState {
        match self {
            PortraitChange::Keep => PortraitState::Keep,
            PortraitChange::Clear => PortraitState::Clear,
            PortraitChange::Set(name) => match portraits.get(&name) {
                Some(portrait) => PortraitState::Set(portrait),
                None => {
                    log::warn!("dialogue references unknown portrait {name:?}");
                    PortraitState::Clear
                }
            },
        }
    }
}

/// One "page" of a conversation under a single speaker.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MessageDef {
    /// See [`PortraitChange`].
    #[serde(default)]
    pub portrait: PortraitChange,
    /// This message's portrait side. `None` (the default) carries over
    /// whatever side was in effect; `Some(bool)` pins one — mirrors
    /// [`PortraitChange`]'s keep/set split, but as a bare `Option` since a
    /// side is a bool axis, not a named payload (see
    /// [`message::Message::flip_portrait`](crate::data::script::message::Message::flip_portrait)).
    #[serde(default)]
    pub flip: Option<bool>,
    /// Whether to wait for player input before the next message (default true).
    #[serde(default = "default_true")]
    pub pause: bool,
    pub content: Vec<ContentDef>,
}

/// A single content item within a message. Externally tagged, so JSON is
/// `{"auto": "..."}`, `{"delayed": ["...", 30]}`, `{"sound": "gain"}`,
/// `{"portrait": "y_oof"}`, `{"flip": true}`, `{"delay": 30}`,
/// `{"set_flag": ["name", true]}`, `{"cue": "name"}`, or `"pause"`.
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
    /// Shake the screen for N frames at ±AMP px — the `#shake FRAMES [AMP]`
    /// directive (the parser fills the default amplitude when omitted). JSON:
    /// `{"shake": [30, 2]}`.
    Shake(u32, i16),
    /// An interactive menu — the `#choice` block. JSON:
    /// `{"choice": [{"text": "Yes", "sets": [["flag", true]]}, ...]}`.
    Choice(Vec<ChoiceOptionDef>),
    /// The `#speed` directive: the typewriter's pace as a `(chars, frames)`
    /// rate — reveal that many characters every that many frames — for all
    /// subsequent text in the dialogue. Surface syntax `#speed 3` is `(3, 1)`,
    /// `#speed 1/10` is `(1, 10)`. JSON: `{"speed": [1, 10]}`.
    Speed(u8, u8),
    /// A named beat, marking a point in the conversation for scene
    /// choreography to subscribe to — the `#cue NAME` directive. JSON:
    /// `{"cue": "name"}`. State-flavoured like [`SetFlag`](Self::SetFlag)
    /// (fires even under a manual fast-forward — a skipped-past cue must
    /// still be banked, or the cutscene engine desyncs from dialogue), not
    /// time-flavoured like [`Shake`](Self::Shake). No upfront declaration
    /// (unlike a `#flag`): cue names are free-form here, cross-validated
    /// against the scene file's `on` handlers by the cutscene engine (see
    /// `crate::data::script::message::TextContent::Cue`).
    Cue(String),
}

/// One option of a [`ContentDef::Choice`]: its menu text and the flags it sets
/// when picked (`sets` defaults to none). JSON:
/// `{"text": "Tea", "sets": [["chose_tea", true]]}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChoiceOptionDef {
    pub text: String,
    #[serde(default)]
    pub sets: Vec<(String, bool)>,
}

fn default_true() -> bool {
    true
}

impl ContentDef {
    fn resolve(self, portraits: &Portraits) -> Option<TextContent> {
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
            ContentDef::Portrait(name) => TextContent::Portrait(resolve_portrait(name, portraits)),
            ContentDef::Pause => TextContent::Pause,
            ContentDef::Flip(b) => TextContent::Flip(b),
            ContentDef::SetFlag(name, value) => TextContent::SetFlag(name, value),
            ContentDef::Shake(frames, amplitude) => TextContent::Shake { frames, amplitude },
            ContentDef::Choice(options) => TextContent::Choice(
                options
                    .into_iter()
                    .map(|o| ChoiceOption {
                        text: o.text,
                        sets: o.sets,
                    })
                    .collect(),
            ),
            ContentDef::Speed(chars, frames) => TextContent::Speed { chars, frames },
            ContentDef::Cue(name) => TextContent::Cue(name),
        })
    }
}

impl MessageDef {
    fn resolve(self, portraits: &Portraits) -> Message {
        Message {
            content: self
                .content
                .into_iter()
                .filter_map(|c| c.resolve(portraits))
                .collect(),
            portrait: self.portrait.resolve(portraits),
            flip_portrait: self.flip,
            pause_when_done: self.pause,
        }
    }
}

impl Entry {
    fn resolve(self, portraits: &Portraits) -> Vec<Message> {
        match self {
            Entry::Line(s) => vec![Message::from(s)],
            Entry::Pages(pages) => pages.into_iter().map(Message::from).collect(),
            Entry::Conversation { messages } => messages
                .into_iter()
                .map(|m| m.resolve(portraits))
                .collect(),
        }
    }
}

impl SegmentDef {
    /// A `Plain` segment resolves to its messages as-is. An `If`/`elif`/`else`
    /// chain resolves to exactly one carrier message, built inside-out: the
    /// `#else` branch (or nothing) is innermost, each `#elif` wraps it as
    /// another [`TextContent::If`] carrier, and the `#if` wraps last — so
    /// however many `#elif`s were authored, the dialogue box still walks one
    /// carrier at a time, live, against the save when playback gets there.
    /// All the carrier `Message`s' own fields (portrait/pause/flip) are
    /// `Message::default()` and never read: the box treats an `If` item
    /// specially rather than displaying the carrier itself (see
    /// `Dialogue::consume_text_content` in `egg_ui`).
    fn resolve(self, portraits: &Portraits) -> Vec<Message> {
        match self {
            SegmentDef::Plain(entry) => entry.resolve(portraits),
            SegmentDef::If {
                flag,
                negated,
                then,
                otherwise,
                elifs,
            } => {
                let mut rest = otherwise.map(|e| e.resolve(portraits)).unwrap_or_default();
                for elif in elifs.into_iter().rev() {
                    let carrier =
                        if_carrier(elif.flag, elif.negated, elif.then.resolve(portraits), rest);
                    rest = vec![Message {
                        content: vec![carrier],
                        ..Message::default()
                    }];
                }
                vec![Message {
                    content: vec![if_carrier(flag, negated, then.resolve(portraits), rest)],
                    ..Message::default()
                }]
            }
        }
    }
}

/// Build one runtime `#if`/`#elif` carrier. [`TextContent::If`] always means
/// "if `flag` is true show `then`, else show `otherwise`", so resolving a
/// negated condition (`#if not`/`#elif not`) is just a matter of swapping
/// which resolved branch plays which role — `TextContent::If` itself never
/// needs to represent `not`.
fn if_carrier(flag: String, negated: bool, then: Vec<Message>, otherwise: Vec<Message>) -> TextContent {
    if negated {
        TextContent::If {
            flag,
            then: otherwise,
            otherwise: then,
        }
    } else {
        TextContent::If {
            flag,
            then,
            otherwise,
        }
    }
}

impl DialogueDef {
    fn resolve(self, portraits: &Portraits) -> Vec<Message> {
        match self {
            DialogueDef::Plain(entry) => entry.resolve(portraits),
            DialogueDef::Segments { segments } => segments
                .into_iter()
                .flat_map(|s| s.resolve(portraits))
                .collect(),
        }
    }
}

fn resolve_portrait(
    name: Option<String>,
    portraits: &Portraits,
) -> Option<crate::data::portraits::Portrait> {
    let name = name?;
    let portrait = portraits.get(&name);
    if portrait.is_none() {
        log::warn!("dialogue references unknown portrait {name:?}");
    }
    portrait
}

// --- resolved, in-memory script ---

/// One resolved language. Portrait/sound names are turned into values and every
/// keyed entry — dialogue *and* lists — becomes a flat `Vec<Message>`; the
/// dialogue-vs-list distinction only exists in the JSON file. An `#if` in a
/// dialogue entry survives resolution as a single carrier `Message` holding a
/// [`TextContent::If`] (see [`SegmentDef::resolve`]) rather than being picked
/// here — that happens at playback time, in the dialogue box, against the live
/// save. `labels` stay separate because they're printed directly rather than
/// run through the dialogue system.
#[derive(Debug, Clone, Default)]
struct Language {
    labels: HashMap<String, String>,
    /// Both `dialogue` and `lists` JSON sections, keyed together. A list entry
    /// is stored as one single-line [`Message`] per string.
    entries: HashMap<String, Vec<Message>>,
    /// The original, unresolved dialogue defs, kept verbatim so the in-game
    /// dialogue editor can load a key back into an editable draft and classify
    /// it (a resolved `Vec<Message>` has lost the `#if` structure and the
    /// authored shape). Dialogue only — lists live in `entries` and aren't here,
    /// so `raw_dialogue.keys()` is exactly the dialogue key set.
    raw_dialogue: HashMap<String, DialogueDef>,
    /// The flag vocabulary this language declared. Merged base+active is what
    /// [`Script::flags`] reports.
    flags: BTreeSet<String>,
}

impl Language {
    fn resolve(file: ScriptFile, portraits: &Portraits) -> Self {
        let raw_dialogue = file.dialogue.clone();
        let mut entries: HashMap<String, Vec<Message>> = file
            .dialogue
            .into_iter()
            .map(|(key, entry)| (key, entry.resolve(portraits)))
            .collect();
        for (key, lines) in file.lists {
            let messages = lines.into_iter().map(Message::from).collect();
            entries.insert(key, messages);
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
    /// startup with the default language file. `portraits` is the runtime
    /// registry portrait names resolve against (see
    /// [`reresolve_portraits`](Self::reresolve_portraits) for what happens when
    /// it later changes).
    pub fn set_base(&mut self, file: ScriptFile, portraits: &Portraits) {
        let language = Language::resolve(file, portraits);
        self.active = language.clone();
        self.base = language;
    }

    /// Swap the active language at runtime. Keys it doesn't define fall back to
    /// the base language installed by [`Script::set_base`].
    pub fn set_language(&mut self, file: ScriptFile, portraits: &Portraits) {
        self.active = Language::resolve(file, portraits);
    }

    /// Re-resolve every installed dialogue's portrait names against a fresh
    /// `portraits` registry. Portrait names are baked into `Message`s once, at
    /// [`set_base`](Self::set_base)/[`set_language`](Self::set_language) time —
    /// so when `data.toml` reloads with different portrait data, the dialogue
    /// already installed in `entries` is stale until it's re-baked here. Lists
    /// carry no portraits and aren't in `raw_dialogue`, so only dialogue needs
    /// this; re-resolution is authoritative, so a name absent from the new
    /// registry resolves to `None` rather than keeping its last-good value.
    pub fn reresolve_portraits(&mut self, portraits: &Portraits) {
        for language in [&mut self.base, &mut self.active] {
            for (key, def) in &language.raw_dialogue {
                language
                    .entries
                    .insert(key.clone(), def.clone().resolve(portraits));
            }
        }
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

    /// A dialogue conversation's messages, falling back to the `default` entry
    /// then an empty conversation for unknown keys. Any `#if` in it comes back
    /// as an unpicked [`TextContent::If`] carrier — branch selection happens at
    /// *playback* time, in the dialogue box, against the live save (not here),
    /// so a `#choice`/`#set` earlier in the same conversation is visible to a
    /// later `#if` in it.
    pub fn get_dialogue(&self, key: &str) -> Vec<Message> {
        self.entry(key)
            .or_else(|| self.entry("default"))
            .cloned()
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
    /// structure and shape rather than resolving it to a carrier item.
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

    /// A list entry's messages. Used by [`list`](Self::list)/
    /// [`list_get`](Self::list_get) to read them back as plain text; lists are
    /// authored unconditionally, so there's no `#if` carrier to worry about.
    fn list_messages(&self, key: &str) -> Option<Vec<Message>> {
        self.entry(key).cloned()
    }

    /// Look up a keyed entry (dialogue or list) as its resolved messages, active
    /// language then base.
    fn entry(&self, key: &str) -> Option<&Vec<Message>> {
        self.active
            .entries
            .get(key)
            .or_else(|| self.base.entries.get(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::eggdata;
    use crate::data::script::eggtext;

    /// Install a script from `.eggtext` source (the DSL resolves to the same
    /// [`ScriptFile`] the JSON loader produces), against the built-in portraits.
    fn script(src: &str) -> Script {
        let mut script = Script::new();
        script.set_base(eggtext::parse(src).expect("parse eggtext"), &Portraits::builtin());
        script
    }

    /// The concatenated plain text of a resolved conversation.
    fn plain(messages: &[Message]) -> String {
        messages.iter().map(Message::to_plain_string).collect()
    }

    #[test]
    fn get_dialogue_resolves_the_if_branch_into_a_carrier() {
        // No save is threaded through resolution any more: both branches ride
        // along in a single `TextContent::If` carrier message, unpicked. Branch
        // selection is egg_ui's job, at playback time (see
        // `Dialogue::consume_text_content`), which is what lets a `#choice`/
        // `#set` earlier in the *same* conversation steer this later `#if` — see
        // the end-to-end test in `egg_ui::dialogue`.
        let script = script(
            "#flag seen\n\
             #dialogue d\n\
             \x20   #if seen\n\
             \x20   After.\n\
             \x20   #else\n\
             \x20   Before.\n\
             \x20   #end",
        );

        let convo = script.get_dialogue("d");
        assert_eq!(convo.len(), 1, "the whole #if resolves to one carrier message");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "seen");
                assert_eq!(plain(then), "After.");
                assert_eq!(plain(otherwise), "Before.");
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    /// `is_night` is an ordinary declared flag, so dialogue branches on it like
    /// any other — confirming the day/night state (now a plain flag, not a typed
    /// bool) is reachable from `#if is_night` once `#flag is_night` is declared.
    #[test]
    fn is_night_flag_resolves_into_an_if_carrier() {
        use crate::data::save::IS_NIGHT_FLAG;
        let script = script(
            "#flag is_night\n\
             #dialogue d\n\
             \x20   #if is_night\n\
             \x20   Good evening.\n\
             \x20   #else\n\
             \x20   Good morning.\n\
             \x20   #end",
        );

        let convo = script.get_dialogue("d");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, IS_NIGHT_FLAG);
                assert_eq!(plain(then), "Good evening.");
                assert_eq!(plain(otherwise), "Good morning.");
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    /// `#if not NAME` swaps which resolved branch plays `then` vs `otherwise`
    /// in the runtime carrier, since `TextContent::If` itself always means
    /// "show `then` when the flag is true".
    #[test]
    fn if_not_swaps_the_branches_at_resolution() {
        let script = script(
            "#flag seen\n\
             #dialogue d\n\
             \x20   #if not seen\n\
             \x20   Before.\n\
             \x20   #else\n\
             \x20   After.\n\
             \x20   #end",
        );
        let convo = script.get_dialogue("d");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "seen");
                assert_eq!(plain(then), "After.", "then fires when the flag IS true, i.e. `not seen` is false");
                assert_eq!(
                    plain(otherwise),
                    "Before.",
                    "otherwise fires when the flag is false, i.e. `not seen` is true"
                );
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    /// An `#if`/`#elif`/`#else` chain still resolves to exactly one carrier
    /// message; the `#elif` nests one level inside the `#if`'s `otherwise`.
    #[test]
    fn if_elif_else_chain_resolves_to_nested_carriers() {
        let script = script(
            "#flag a\n#flag b\n\
             #dialogue d\n\
             \x20   #if a\n\
             \x20   A branch.\n\
             \x20   #elif b\n\
             \x20   B branch.\n\
             \x20   #else\n\
             \x20   Else branch.\n\
             \x20   #end",
        );
        let convo = script.get_dialogue("d");
        assert_eq!(convo.len(), 1, "the whole chain resolves to one carrier message");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "a");
                assert_eq!(plain(then), "A branch.");
                assert_eq!(otherwise.len(), 1, "the #elif nests inside the #if's otherwise branch");
                match &otherwise[0].content[..] {
                    [TextContent::If { flag, then, otherwise }] => {
                        assert_eq!(flag, "b");
                        assert_eq!(plain(then), "B branch.");
                        assert_eq!(plain(otherwise), "Else branch.");
                    }
                    other => panic!("expected a nested If carrier for the #elif, got {other:?}"),
                }
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    /// The pre-`#elif` JSON shape — just `if`/`then`/`else`, no `negated` or
    /// `elifs` keys — still deserializes, defaulting the new fields.
    #[test]
    fn old_if_json_shape_without_elif_or_negated_still_deserializes() {
        let json = r#"{
            "dialogue": {
                "d": { "segments": [
                    { "if": "seen", "then": "After.", "else": "Before." }
                ] }
            },
            "flags": ["seen"]
        }"#;
        let file: ScriptFile = serde_json::from_str(json).expect("old shape still deserializes");
        let DialogueDef::Segments { segments } = &file.dialogue["d"] else {
            panic!("expected segments");
        };
        assert_eq!(
            segments[0],
            SegmentDef::If {
                flag: "seen".into(),
                negated: false,
                then: DialogueDef::Plain(Entry::Line("After.".into())),
                otherwise: Some(DialogueDef::Plain(Entry::Line("Before.".into()))),
                elifs: vec![],
            },
        );
    }

    /// The old flat shape — every branch a plain [`Entry`], no branch
    /// containing a nested `#if` — resolves identically whether or not the
    /// schema *supports* nesting: `then`/`else` becoming a full
    /// [`DialogueDef`] (wave 4) doesn't change what a plain-`Entry` branch
    /// resolves to, since `DialogueDef::Plain(entry).resolve(..)` is exactly
    /// `entry.resolve(..)`.
    #[test]
    fn old_flat_json_shape_round_trips_through_resolve_unchanged() {
        let json = r#"{
            "dialogue": {
                "d": { "segments": [
                    { "if": "seen", "then": "After.", "else": "Before." }
                ] }
            },
            "flags": ["seen"]
        }"#;
        let file: ScriptFile = serde_json::from_str(json).expect("old shape still deserializes");
        let script = {
            let mut script = Script::new();
            script.set_base(file, &Portraits::builtin());
            script
        };
        let convo = script.get_dialogue("d");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "seen");
                assert_eq!(plain(then), "After.");
                assert_eq!(plain(otherwise), "Before.");
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    #[test]
    fn get_dialogue_resolves_an_if_without_else_to_an_empty_otherwise() {
        let script = script(
            "#flag seen\n\
             #dialogue d\n\
             \x20   Intro.\n\n\
             \x20   #if seen\n\
             \x20   Extra.\n\
             \x20   #end",
        );
        // The unconditional message, then the If carrier.
        let convo = script.get_dialogue("d");
        assert_eq!(convo.len(), 2);
        assert_eq!(convo[0].to_plain_string(), "Intro.");
        match &convo[1].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "seen");
                assert_eq!(plain(then), "Extra.");
                assert!(
                    otherwise.is_empty(),
                    "no #else means an empty otherwise branch"
                );
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    #[test]
    fn set_flag_item_survives_resolution() {
        let script = script("#flag seen\n#dialogue d\n    #set seen true\n    Hi.");
        let convo = script.get_dialogue("d");
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
        assert_eq!(plain(&script.get_dialogue("missing")), "Nothing here.");
    }

    #[test]
    fn lists_read_back_as_plain_entries() {
        let script = script("#list things\n    one\n    two\n    three");
        assert_eq!(script.list("things"), ["one", "two", "three"]);
        assert_eq!(script.list_get("things", 1).as_deref(), Some("two"));
        assert_eq!(script.list_get("things", 9), None);
    }

    #[test]
    fn choice_resolves_to_a_runtime_choice_item() {
        // A `#choice` resolves as message content like any other directive; the
        // conversation carries a runtime `Choice` with each option's flags.
        let script = script(
            "#flag chose_tea\n\
             #dialogue ask\n\
             \x20   Tea or coffee?\n\
             \x20   #choice\n\
             \x20   #option Tea\n\
             \x20   #set chose_tea true\n\
             \x20   #option Coffee",
        );
        let convo = script.get_dialogue("ask");
        let options = convo
            .iter()
            .flat_map(|m| &m.content)
            .find_map(|c| match c {
                TextContent::Choice(options) => Some(options),
                _ => None,
            })
            .expect("a Choice content item");
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].text, "Tea");
        assert_eq!(options[0].sets, vec![("chose_tea".to_string(), true)]);
        assert!(options[1].sets.is_empty());
    }

    #[test]
    fn a_pick_flag_and_a_later_if_share_the_same_flag_name() {
        // Resolution-level half of the story: `ask`'s choice and `react`'s `#if`
        // resolve independently of each other and of any save — but they name
        // the same flag, so when a real save picks up the choice's `sets` and
        // the dialogue box later evaluates the `#if` against it, the two agree.
        // The actual runtime hookup (a choice's flag steering a *same*-
        // conversation `#if`, playback-side) is the end-to-end test in
        // `egg_ui::dialogue`, since resolution no longer touches a save at all.
        let script = script(
            "#flag chose_tea\n\
             #dialogue ask\n\
             \x20   #choice\n\
             \x20   #option Tea\n\
             \x20   #set chose_tea true\n\
             \x20   #option Coffee\n\
             #dialogue react\n\
             \x20   #if chose_tea\n\
             \x20   Enjoy your tea.\n\
             \x20   #else\n\
             \x20   Coffee it is.\n\
             \x20   #end",
        );

        let convo = script.get_dialogue("ask");
        let options = convo
            .iter()
            .flat_map(|m| &m.content)
            .find_map(|c| match c {
                TextContent::Choice(o) => Some(o.clone()),
                _ => None,
            })
            .expect("a Choice");
        assert_eq!(options[0].sets, vec![("chose_tea".to_string(), true)]);

        let convo = script.get_dialogue("react");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "chose_tea", "names the same flag the choice sets");
                assert_eq!(plain(then), "Enjoy your tea.");
                assert_eq!(plain(otherwise), "Coffee it is.");
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    /// A portrait name resolves against whatever [`Portraits`] registry is
    /// threaded through — and [`Script::reresolve_portraits`] re-bakes already
    /// installed dialogue against a *new* registry, since the portrait name was
    /// baked into the `Message` at install time. A name the new registry drops
    /// entirely resolves to [`PortraitState::Clear`]: re-resolution is
    /// authoritative, not last-good-wins per message.
    #[test]
    fn reresolve_portraits_rebakes_installed_dialogue() {
        let spr_id = |script: &Script| match &script.get_dialogue("d")[0].portrait {
            PortraitState::Set(p) => Some(p.sprite.cells[0].spr_id),
            _ => None,
        };

        let v1 = eggdata::parse("[portraits.p]\nspr_id = 1\noffset = [0, 0]\n").expect("parse");
        let mut script = Script::new();
        script.set_base(
            eggtext::parse("#dialogue d\n    #pic p\n    Hi.").expect("parse eggtext"),
            &Portraits::from_data(&v1),
        );
        assert_eq!(spr_id(&script), Some(1), "resolves against the registry at set_base time");

        // A data.toml reload that redefines `p` re-bakes the installed message.
        let v2 = eggdata::parse("[portraits.p]\nspr_id = 99\noffset = [0, 0]\n").expect("parse");
        script.reresolve_portraits(&Portraits::from_data(&v2));
        assert_eq!(spr_id(&script), Some(99), "reresolve_portraits picks up the new cells");

        // A reload that drops `p` entirely clears the portrait, not keeps it.
        let v3 = eggdata::parse("[portraits.other]\nspr_id = 5\noffset = [0, 0]\n").expect("parse");
        script.reresolve_portraits(&Portraits::from_data(&v3));
        assert_eq!(spr_id(&script), None, "a name the new registry drops resolves to None");
    }

    // --- wave 4: nested `#if` resolution ---

    /// A nested, negated `#if` — inside the outer `#if`'s `then` branch —
    /// resolves through the exact same carrier machinery one level deeper:
    /// the inner carrier rides inside the outer's `then` messages, and
    /// `negated` still swaps which resolved branch plays `then`/`otherwise`
    /// at its own level, independent of the outer condition.
    #[test]
    fn nested_if_resolves_one_carrier_inside_another_level_by_level() {
        let script = script(
            "#flag outer\n#flag inner\n\
             #dialogue d\n\
             \x20   #if outer\n\
             \x20       #if not inner\n\
             \x20           Deep then.\n\
             \x20       #else\n\
             \x20           Deep else.\n\
             \x20   #else\n\
             \x20       Shallow.",
        );
        let convo = script.get_dialogue("d");
        assert_eq!(convo.len(), 1, "the whole nested chain is one top-level carrier");
        match &convo[0].content[..] {
            [TextContent::If { flag, then, otherwise }] => {
                assert_eq!(flag, "outer");
                assert_eq!(plain(otherwise), "Shallow.");
                assert_eq!(then.len(), 1, "outer's `then` holds exactly the inner carrier");
                match &then[0].content[..] {
                    [TextContent::If { flag, then, otherwise }] => {
                        assert_eq!(flag, "inner");
                        // `#if not inner`: negation swaps which resolved
                        // branch plays which role.
                        assert_eq!(plain(then), "Deep else.");
                        assert_eq!(plain(otherwise), "Deep then.");
                    }
                    other => panic!("expected a nested If carrier, got {other:?}"),
                }
            }
            other => panic!("expected a single If carrier, got {other:?}"),
        }
    }

    /// `assets/script/en.eggtext`'s `debug_portrait2` is authored *scoped*
    /// (branch content indented deeper than its `#if`/`#else`). Reflowing the
    /// same conversation *flat* (content at the `#if`'s own depth) must
    /// resolve to the exact same messages — proving the two authoring styles
    /// are genuinely interchangeable, not just individually valid.
    #[test]
    fn debug_portrait2_style_scoped_and_flat_resolve_identically() {
        let scoped = script(
            "#flag INSULT\n\
             #dialogue d\n\
             \x20   #pic m_close\n\
             \x20   ... Hmm...\n\n\
             \x20   #if INSULT\n\
             \x20       #pic m_narrow\n\
             \x20       ... Low hanging fruit.\n\n\
             \x20       #pic m_smug\n\
             \x20       As expected from an insect like yourself.\n\
             \x20   #else\n\
             \x20       #pic m_normal\n\
             \x20       Hey, it isn't all that bad.\n\n\
             \x20       #pic m_smile\n\
             \x20       ... At least you're not stuck in the floor like me.\n\
             \x20   #end",
        );
        let flat = script(
            "#flag INSULT\n\
             #dialogue d\n\
             \x20   #pic m_close\n\
             \x20   ... Hmm...\n\n\
             \x20   #if INSULT\n\
             \x20   #pic m_narrow\n\
             \x20   ... Low hanging fruit.\n\n\
             \x20   #pic m_smug\n\
             \x20   As expected from an insect like yourself.\n\
             \x20   #else\n\
             \x20   #pic m_normal\n\
             \x20   Hey, it isn't all that bad.\n\n\
             \x20   #pic m_smile\n\
             \x20   ... At least you're not stuck in the floor like me.\n\
             \x20   #end",
        );
        assert_eq!(
            format!("{:#?}", scoped.get_dialogue("d")),
            format!("{:#?}", flat.get_dialogue("d")),
        );
    }

    /// The flip side of the previous test: `house_stairwell_painting` is
    /// authored *flat*; reflowing it *scoped* must resolve identically.
    #[test]
    fn house_stairwell_painting_style_flat_and_scoped_resolve_identically() {
        let flat = script(
            "#flag house_stairwell_window_interacted\n\
             #dialogue d\n\
             \x20   #if house_stairwell_window_interacted\n\
             \x20   A purple circle hovers listlessly over a collection of purple lumps. You feel nothing in particular.\n\
             \x20   #else\n\
             \x20   \"Your uncle's magnum opus: \\\"Sunrise over town\\\". \"\n\
             \x20   4x11 px, painted with a minimalist two-tone palette.#delay 30\n\
             \x20   #end",
        );
        let scoped = script(
            "#flag house_stairwell_window_interacted\n\
             #dialogue d\n\
             \x20   #if house_stairwell_window_interacted\n\
             \x20       A purple circle hovers listlessly over a collection of purple lumps. You feel nothing in particular.\n\
             \x20   #else\n\
             \x20       \"Your uncle's magnum opus: \\\"Sunrise over town\\\". \"\n\
             \x20       4x11 px, painted with a minimalist two-tone palette.#delay 30\n\
             \x20   #end",
        );
        assert_eq!(
            format!("{:#?}", flat.get_dialogue("d")),
            format!("{:#?}", scoped.get_dialogue("d")),
        );
    }
}
