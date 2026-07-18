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
//! box widget (in `ui::dialogue`) that plays it.

use crate::data::{portraits::Portrait, sound::SfxData};

#[derive(Debug, Clone)]
pub enum TextContent {
    /// A run of text.
    ///
    /// * `pause` — wait for a manual advance (keypress) before showing this
    ///   text. `false` flows it in automatically once the previous line is
    ///   done. (The old `AutoText` is just `pause: false`.)
    /// * `delay` — frames to hold before the text *appends* to the page
    ///   that's already open — or, if this is the first text since the last
    ///   [`Clear`](Self::Clear), *opens* it. `0` appends/opens at once; `> 0`
    ///   holds first. Every text line within a message appends onto the same
    ///   page — there is no more mid-message "fresh page" — and `delay` is now
    ///   meaningful even on a page's very first line (it used to be silently
    ///   dropped there, since there was nothing yet to append to).
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
    /// Shake the screen for `frames` frames at up to ±`amplitude` px when
    /// playback reaches this point — the `#shake FRAMES [AMP]` directive.
    /// Fires like a [`Sound`](Self::Sound): the widget banks it as
    /// `pending_shake` and the world's camera driver picks it up (see
    /// `Dialogue` and
    /// [`Shake`](crate::world::camera::Shake)). Time-flavoured like
    /// [`Delay`](Self::Delay), so a manual fast-forward drops it.
    Shake { frames: u32, amplitude: i16 },
    /// An interactive branch point — the `#choice` block. Presents `options` in
    /// the dialogue box and blocks playback (neither [`is_auto`](Self::is_auto)
    /// nor [`is_skip`](Self::is_skip)) until the player picks one; the picked
    /// option's flags are then written through the same [`SetFlag`](Self::SetFlag)
    /// machinery (`save.set_flag`) and playback continues. Follow-up text
    /// branches on those flags through the ordinary `#if` — evaluated at
    /// *playback* time (see [`If`](Self::If)), so a later `#if` in the very same
    /// conversation already sees the flag the choice just set.
    Choice(Vec<ChoiceOption>),
    /// The runtime `#if` — both branches are carried into the playback queue as
    /// one carrier item, and the dialogue box picks one against the live save
    /// the moment playback reaches it (not once, up front, when the
    /// conversation is fetched). That is what lets an earlier `#choice`/`#set`
    /// in the same conversation steer a later `#if` in it: the flag is live by
    /// the time this item is consumed. `otherwise` is empty when the source had
    /// no `#else`. See [`crate::data::script::eggtext`] and
    /// `Dialogue::consume_text_content` (`egg_ui`) for the two ends of this.
    If {
        flag: String,
        then: Vec<Message>,
        otherwise: Vec<Message>,
    },
    /// The page-break boundary: `lower_messages` (`egg_ui`) inserts one of
    /// these before every real message, clearing the box's revealed text —
    /// but *not* the portrait/side, which carry over (see [`Message::portrait`]
    /// / [`Message::flip_portrait`]) — so a new page always starts blank
    /// regardless of what the previous page showed, and any text after it is
    /// unambiguously that new page's, not an append to the old one.
    /// Synthesized at lowering time; never authored directly.
    Clear,
    /// The `#speed N` directive: sets the typewriter's pace (frames held
    /// between each revealed character) for all subsequent text in this
    /// dialogue — block-scoped from where it appears onward, like
    /// `#autoflip`, and it persists across page breaks within the same
    /// conversation until another `#speed` changes it. `0` is the default:
    /// the ordinary, unthrottled per-tick reveal.
    Speed(u8),
}
impl TextContent {
    pub fn is_auto(&self) -> bool {
        use TextContent::*;
        !matches!(self, Text { pause: true, .. } | Pause | Choice(_))
    }
    /// Consumed in place rather than shown directly. `If` belongs here too: the
    /// carrier itself never renders — consuming it splices the chosen branch's
    /// content into the queue, and `next_text`'s `is_skip` recursion flows
    /// straight into that spliced content in the same call.
    pub fn is_skip(&self) -> bool {
        use TextContent::*;
        matches!(
            self,
            Sound(_)
                | Portrait(_)
                | Flip(_)
                | SetFlag(..)
                | Shake { .. }
                | If { .. }
                | Clear
                | Speed(_)
        )
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

/// A message's speaker portrait at *playback* time — the runtime counterpart
/// of [`PortraitChange`](crate::data::script::PortraitChange), which this
/// resolves from (a name against a live [`Portraits`](crate::data::portraits::Portraits)
/// registry instead of a bare `String`).
///
/// Three states rather than `Option<Portrait>` because "this message never
/// mentioned a portrait" and "this message explicitly cleared it" mean
/// different things once a message can no longer just flatten "whatever the
/// portrait is right now" at parse time — an `#if` branch means the parser
/// can't know what's current at a given point in the conversation (see
/// [`crate::data::script::eggtext`]'s module doc). So carry-over is resolved
/// live instead: the `Dialogue` widget (`egg_ui`) holds the actual current
/// portrait/side across a conversation and folds each message's `Keep`/
/// `Clear`/`Set` against it as playback reaches it.
#[derive(Debug, Clone, PartialEq)]
pub enum PortraitState {
    /// No `#pic` in this message: carry over whatever portrait (and side) was
    /// showing at the end of the previous message.
    Keep,
    /// `#pic none`: explicitly show no portrait (narration), regardless of
    /// what was showing before.
    Clear,
    /// `#pic NAME`: switch to this portrait.
    Set(Portrait),
}

/// A single "page" of dialogue: a run of text [`content`](Message::content)
/// shown under one speaker (`portrait` + `flip_portrait`). `pause_when_done`
/// controls whether the player must press to continue to the *next* message,
/// or whether it auto-advances. Dialogue is stored as `Vec<Message>` in the
/// registry built by [`crate::data::script`] and queued via
/// `Dialogue::set_messages`.
#[derive(Debug, Clone)]
pub struct Message {
    pub content: Vec<TextContent>,
    /// This message's speaker portrait — see [`PortraitState`]. `Keep` (the
    /// default — no `#pic` in this message) carries over whatever was
    /// showing; that carry-over is applied live, by the `Dialogue` widget,
    /// not resolved here.
    pub portrait: PortraitState,
    /// This message's portrait side. `None` (the default) carries over
    /// whatever side was in effect — mirrors [`PortraitState::Keep`], but as
    /// a bare `Option` since a side is a bool axis, not a named payload.
    /// `Some(bool)` pins one, from an explicit `#flip` or an `#autoflip`
    /// resolution.
    pub flip_portrait: Option<bool>,
    pub pause_when_done: bool,
}
impl Message {
    pub const fn default() -> Self {
        Self {
            content: Vec::new(),
            portrait: PortraitState::Keep,
            flip_portrait: None,
            pause_when_done: true,
        }
    }
    pub fn with_content(mut self, content: Vec<TextContent>) -> Self {
        self.content = content;
        self
    }
    pub fn with_portrait(mut self, portrait: Portrait) -> Self {
        self.portrait = PortraitState::Set(portrait);
        self
    }
    pub fn with_flip(mut self, flip_portrait: bool) -> Self {
        self.flip_portrait = Some(flip_portrait);
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
