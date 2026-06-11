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

//! `.eggtext` — a small, indentation-aware DSL for authoring game text. It
//! parses into the very same [`ScriptFile`] that the JSON loader produces, so
//! it is just a friendlier *front end* for the existing [`crate::data::script`]
//! registry: parse `.eggtext`, hand the [`ScriptFile`] to [`Script::set_base`]
//! /[`Script::set_language`], done. No new runtime types, no extra dependency
//! (the format is line-oriented, which a hand-written scanner handles more
//! readably than a combinator library would).
//!
//! [`Script::set_base`]: crate::data::script::Script::set_base
//! [`Script::set_language`]: crate::data::script::Script::set_language
//!
//! # The format
//!
//! Three kinds of top-level item, distinguished by their first character at
//! column 0. Blank lines and `//` comments are ignored.
//!
//! ```text
//! // A label: a single string printed directly (menu items, titles, ...).
//! game_title = "super unfinished EGG GAME"
//!
//! // A list: an ordered set of strings (e.g. a debug menu).
//! #list menu_debug_controls
//!     Palette 1
//!     Palette 2
//!
//! // A dialogue conversation.
//! #dialogue house_kitchen_sink
//!     #sound gain
//!     Found something down the drain...!\n
//!
//!     #sound loss
//!     "... You left it there."
//! ```
//!
//! ## Inside a `#dialogue` block
//!
//! Indented lines form the body. **Blank lines split the body into messages**
//! (one dialogue "page" / speaker turn each). Within a message, lines become
//! content in order:
//!
//! * **Text lines** — `bare text` is stripped of surrounding whitespace;
//!   `"quoted text"` is preserved verbatim between the quotes. A trailing
//!   `#delay N` makes the text *appear after `N` frames, appended* to the
//!   current message (a piecemeal reveal). The first text line of a message
//!   opens its box (any `#delay` on it is ignored, as there is nothing to
//!   append to yet); a later text line *without* `#delay` starts a fresh page.
//! * **Directives** — one or more `#word [arg]` on a line:
//!   * `#pic NAME` sets the speaker portrait. The first `#pic` is the message's
//!     portrait; a later `#pic` switches it mid-message. `#pic none` clears it.
//!   * `#flip BOOL` chooses the portrait's side. Before any text it sets the
//!     message's side; after text it flips mid-message.
//!   * `#sound NAME` plays a sound effect at that point.
//!   * `#delay N` is a standalone `N`-frame pause.
//!   * `#nopause` flows straight on to the next message instead of waiting for
//!     the player to advance.
//!   * `#autoflip` (block scope, from where it appears onward) auto-alternates
//!     the portrait side whenever the speaker portrait changes, so two
//!     characters trade left/right automatically. An explicit `#flip` still
//!     overrides it for that message.
//!
//! Escapes understood in text and labels: `\n` `\t` `\r` `\\` `\"` `\#`.

use super::script::{ContentDef, Entry, MessageDef, ScriptFile};

/// A parse failure, carrying the 1-based source line it occurred on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self { line, message: message.into() }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parse a whole `.eggtext` source into a [`ScriptFile`].
pub fn parse(src: &str) -> Result<ScriptFile, ParseError> {
    let mut file = ScriptFile::default();
    let mut lines = src.lines().enumerate().peekable();

    while let Some((idx, raw)) = lines.next() {
        let line_no = idx + 1;
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            continue;
        }
        if raw.starts_with([' ', '\t']) {
            return Err(ParseError::new(line_no, "indented line is not inside a block"));
        }

        if let Some(header) = logical.strip_prefix('#') {
            let (kind, name) = split_first_word(header);
            if name.is_empty() {
                return Err(ParseError::new(line_no, format!("`#{kind}` needs a name")));
            }
            let body = collect_block(&mut lines);
            match kind {
                "dialogue" => {
                    file.dialogue.insert(name.to_string(), parse_dialogue(&body)?);
                }
                "list" => {
                    file.lists.insert(name.to_string(), parse_list(&body)?);
                }
                other => {
                    return Err(ParseError::new(
                        line_no,
                        format!("unknown block `#{other}` (expected `#dialogue` or `#list`)"),
                    ));
                }
            }
        } else if let Some(eq) = logical.find('=') {
            let key = logical[..eq].trim();
            if key.is_empty() {
                return Err(ParseError::new(line_no, "label is missing a name before `=`"));
            }
            file.labels.insert(key.to_string(), parse_value(&logical[eq + 1..]));
        } else {
            return Err(ParseError::new(
                line_no,
                "expected a label (`key = \"value\"`) or a block (`#dialogue name`)",
            ));
        }
    }

    Ok(file)
}

/// Pull the indented (and blank) lines that make up a block body, leaving the
/// iterator positioned on the next column-0 line.
fn collect_block<'a, I>(lines: &mut std::iter::Peekable<I>) -> Vec<(usize, &'a str)>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut body = Vec::new();
    while let Some(&(idx, raw)) = lines.peek() {
        if raw.trim().is_empty() || raw.starts_with([' ', '\t']) {
            body.push((idx + 1, raw));
            lines.next();
        } else {
            break;
        }
    }
    body
}

/// A `#list` body: one string item per non-blank line.
fn parse_list(body: &[(usize, &str)]) -> Result<Vec<String>, ParseError> {
    let mut items = Vec::new();
    for &(line_no, raw) in body {
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            continue;
        }
        if logical.starts_with('#') {
            return Err(ParseError::new(line_no, "`#list` items can't be directives"));
        }
        items.push(parse_value(logical));
    }
    Ok(items)
}

/// A `#dialogue` body: blank-line-separated messages, reduced to the tightest
/// [`Entry`] variant that represents them (a bare single-line conversation
/// becomes [`Entry::Line`], several bare lines become [`Entry::Pages`]).
fn parse_dialogue(body: &[(usize, &str)]) -> Result<Entry, ParseError> {
    // Group lines into messages, dropping comments. A blank line ends a message.
    let mut groups: Vec<Vec<(usize, &str)>> = Vec::new();
    let mut current: Vec<(usize, &str)> = Vec::new();
    for &(line_no, raw) in body {
        let logical = raw.trim_start();
        if logical.is_empty() {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else if !is_comment(logical) {
            current.push((line_no, logical));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    // Parse each message, then resolve portrait sides (honouring `#autoflip`).
    let mut messages: Vec<MessageDef> = Vec::new();
    let mut autoflip = false;
    let mut side = false;
    let mut last_portrait: Option<String> = None;
    for group in &groups {
        let parsed = parse_message(group)?;
        let mut def = parsed.def;

        if parsed.autoflip && !autoflip {
            autoflip = true;
            side = false;
            last_portrait = None;
        }

        if let Some(explicit) = parsed.flip {
            def.flip = explicit;
            if def.portrait.is_some() {
                side = explicit;
                last_portrait = def.portrait.clone();
            }
        } else if autoflip && let Some(portrait) = &def.portrait {
            if last_portrait.as_ref().is_some_and(|last| last != portrait) {
                side = !side;
            }
            def.flip = side;
            last_portrait = Some(portrait.clone());
        }

        drop_redundant_flips(&mut def);
        messages.push(def);
    }

    Ok(reduce_entry(messages))
}

/// One message mid-parse: its [`MessageDef`] plus directives whose effect needs
/// resolving across the whole conversation (`#flip` side, `#autoflip` toggle).
struct ParsedMessage {
    def: MessageDef,
    /// An explicit message-level `#flip` (set before any text), overriding
    /// `#autoflip`.
    flip: Option<bool>,
    /// Whether this message turned `#autoflip` on.
    autoflip: bool,
}

fn parse_message(lines: &[(usize, &str)]) -> Result<ParsedMessage, ParseError> {
    let mut def = MessageDef { portrait: None, flip: false, pause: true, content: Vec::new() };
    let mut flip: Option<bool> = None;
    let mut autoflip = false;
    let mut have_portrait = false;
    let mut have_text = false;

    for &(line_no, logical) in lines {
        if logical.starts_with('#') {
            for (name, arg) in directive_segments(logical) {
                match name {
                    "pic" => {
                        let portrait = match arg {
                            None | Some("none") | Some("-") => None,
                            Some(name) => Some(name.to_string()),
                        };
                        if have_portrait {
                            def.content.push(ContentDef::Portrait(portrait));
                        } else {
                            def.portrait = portrait;
                            have_portrait = true;
                        }
                    }
                    "flip" => {
                        let value = parse_bool(arg, line_no)?;
                        if have_text {
                            def.content.push(ContentDef::Flip(value));
                        } else {
                            flip = Some(value);
                        }
                    }
                    "sound" => {
                        let name = arg
                            .ok_or_else(|| ParseError::new(line_no, "`#sound` needs a name"))?;
                        def.content.push(ContentDef::Sound(name.to_string()));
                    }
                    "delay" => def.content.push(ContentDef::Delay(parse_u8(arg, line_no)?)),
                    "nopause" => def.pause = false,
                    "autoflip" => autoflip = true,
                    other => {
                        return Err(ParseError::new(line_no, format!("unknown directive `#{other}`")));
                    }
                }
            }
        } else {
            let (text, delay) = split_text(logical, line_no)?;
            if !have_text {
                // The opening text of a message; a `#delay` here has nothing to
                // append to, so it is ignored.
                def.content.push(ContentDef::Text(text));
                have_text = true;
            } else if let Some(delay) = delay {
                def.content.push(ContentDef::Delayed(text, delay));
            } else {
                def.content.push(ContentDef::Text(text));
            }
        }
    }

    Ok(ParsedMessage { def, flip, autoflip })
}

/// Drop mid-message `#flip`s that don't actually change the current side, so a
/// defensive `#flip false` on an already-unflipped speaker emits nothing.
fn drop_redundant_flips(def: &mut MessageDef) {
    let mut side = def.flip;
    def.content.retain(|content| match content {
        ContentDef::Flip(value) => {
            if *value == side {
                false
            } else {
                side = *value;
                true
            }
        }
        _ => true,
    });
}

/// Collapse a parsed conversation to the simplest equivalent [`Entry`]. A
/// message is "plain" when it is a lone unstyled line of text; an all-plain
/// conversation is a [`Entry::Line`] (one message) or [`Entry::Pages`] (more).
fn reduce_entry(messages: Vec<MessageDef>) -> Entry {
    let plain: Option<Vec<String>> = messages
        .iter()
        .map(|message| match message.content.as_slice() {
            [ContentDef::Text(text)]
                if message.portrait.is_none() && !message.flip && message.pause =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();

    match plain {
        Some(mut lines) if lines.len() == 1 => Entry::Line(lines.pop().unwrap()),
        Some(lines) if !lines.is_empty() => Entry::Pages(lines),
        _ => Entry::Conversation { messages },
    }
}

// --- line-level helpers ---

fn is_comment(logical: &str) -> bool {
    logical.starts_with("//")
}

/// Split off the first whitespace-delimited word, returning it and the trimmed
/// remainder.
fn split_first_word(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, ""),
    }
}

/// Break a directive line into `(name, first-arg)` segments, e.g.
/// `#pic y_oof #flip false` → `[("pic", Some("y_oof")), ("flip", Some("false"))]`.
fn directive_segments(logical: &str) -> Vec<(&str, Option<&str>)> {
    let mut segments = Vec::new();
    let mut tokens = logical.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let Some(name) = token.strip_prefix('#') else { continue };
        let mut arg = None;
        while let Some(next) = tokens.peek() {
            if next.starts_with('#') {
                break;
            }
            if arg.is_none() {
                arg = Some(*next);
            }
            tokens.next();
        }
        segments.push((name, arg));
    }
    segments
}

/// A standalone string value (label value or list item): the verbatim contents
/// of a `"quoted"` span, otherwise the trimmed line. Escapes are resolved.
fn parse_value(s: &str) -> String {
    let s = s.trim();
    match quoted_span(s) {
        Some((inner, _)) => unescape(inner),
        None => unescape(s),
    }
}

/// A dialogue text line: its resolved text and an optional trailing `#delay N`.
fn split_text(logical: &str, line_no: usize) -> Result<(String, Option<u8>), ParseError> {
    if let Some((inner, after)) = quoted_span(logical) {
        Ok((unescape(inner), parse_trailing_delay(after, line_no)?))
    } else {
        let (text, delay) = peel_trailing_delay(logical);
        Ok((unescape(text.trim()), delay))
    }
}

/// If `s` opens with a double quote, return the span between it and the final
/// double quote, plus whatever trails the closing quote.
fn quoted_span(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let rest = s.strip_prefix('"')?;
    let close = rest.rfind('"')?;
    Some((&rest[..close], &rest[close + 1..]))
}

/// Parse the text that follows a closing quote: nothing, or a `#delay N`.
fn parse_trailing_delay(after: &str, line_no: usize) -> Result<Option<u8>, ParseError> {
    let after = after.trim();
    if after.is_empty() {
        return Ok(None);
    }
    let Some(rest) = after.strip_prefix('#') else {
        return Err(ParseError::new(line_no, format!("unexpected text after quote: {after:?}")));
    };
    let (keyword, arg) = split_first_word(rest);
    if keyword == "delay" {
        Ok(Some(parse_u8(Some(arg), line_no)?))
    } else {
        Err(ParseError::new(
            line_no,
            format!("only `#delay` may follow quoted text, found `#{keyword}`"),
        ))
    }
}

/// Peel a trailing `#delay N` off a bare text line, if present.
fn peel_trailing_delay(s: &str) -> (&str, Option<u8>) {
    let trimmed = s.trim_end();
    if let Some(hash) = trimmed.rfind('#') {
        let (keyword, arg) = split_first_word(&trimmed[hash + 1..]);
        if keyword == "delay"
            && let Ok(delay) = arg.parse::<u8>()
        {
            return (trimmed[..hash].trim_end(), Some(delay));
        }
    }
    (trimmed, None)
}

fn parse_bool(arg: Option<&str>, line_no: usize) -> Result<bool, ParseError> {
    match arg {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(ParseError::new(line_no, "`#flip` needs `true` or `false`")),
    }
}

fn parse_u8(arg: Option<&str>, line_no: usize) -> Result<u8, ParseError> {
    arg.and_then(|a| a.parse().ok())
        .ok_or_else(|| ParseError::new(line_no, "expected a number 0-255"))
}

/// Resolve backslash escapes; an unknown escape keeps its backslash.
fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('#') => out.push('#'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialogue(src: &str) -> Entry {
        let file = parse(src).expect("parse");
        file.dialogue.into_values().next().expect("one dialogue entry")
    }

    fn convo(src: &str) -> Vec<MessageDef> {
        match dialogue(src) {
            Entry::Conversation { messages } => messages,
            other => panic!("expected a conversation, got {other:?}"),
        }
    }

    #[test]
    fn labels_are_quoted_or_bare() {
        let file = parse("a = \"hi there\"\nb = bare value  \nc = \"  spaced  \"").unwrap();
        assert_eq!(file.labels["a"], "hi there");
        assert_eq!(file.labels["b"], "bare value");
        assert_eq!(file.labels["c"], "  spaced  ");
    }

    #[test]
    fn lists_collect_items() {
        let file = parse("#list things\n    one\n    two\n    three").unwrap();
        assert_eq!(file.lists["things"], ["one", "two", "three"]);
    }

    #[test]
    fn single_line_reduces_to_line() {
        assert_eq!(dialogue("#dialogue d\n    Just one line."), Entry::Line("Just one line.".into()));
    }

    #[test]
    fn blank_separated_lines_reduce_to_pages() {
        assert_eq!(
            dialogue("#dialogue d\n    First.\n\n    Second."),
            Entry::Pages(vec!["First.".into(), "Second.".into()]),
        );
    }

    #[test]
    fn bare_text_is_stripped_quoted_is_verbatim() {
        // Bare first line stripped; quoted continuation keeps its leading space.
        let messages = convo("#dialogue d\n    Hi\n    \" there\" #delay 5");
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Text("Hi".into()), ContentDef::Delayed(" there".into(), 5)],
        );
    }

    #[test]
    fn first_text_ignores_delay_rest_append() {
        // Mirrors `town_wide`: the opening segment is plain text, later ones are delayed.
        let messages = convo("#dialogue d\n    \"T\" #delay 10\n    \"h\" #delay 10");
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Text("T".into()), ContentDef::Delayed("h".into(), 10)],
        );
    }

    #[test]
    fn sound_and_escapes_interleave() {
        let messages = convo("#dialogue d\n    #sound gain\n    Found it...!\\n");
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Sound("gain".into()), ContentDef::Text("Found it...!\n".into())],
        );
    }

    #[test]
    fn autoflip_alternates_and_keeps_redundant_flips_out() {
        let messages = convo(
            "#dialogue d\n\
            \x20   #autoflip\n\
            \x20   #pic a\n\
            \x20   left\n\n\
            \x20   #pic b\n\
            \x20   right\n\n\
            \x20   #pic b\n\
            \x20   still right\n\n\
            \x20   #pic a\n\
            \x20   left again\n\
            \x20   #pic c #flip false\n\
            \x20   mid-switch",
        );
        let flips: Vec<bool> = messages.iter().map(|m| m.flip).collect();
        // a→false, b→toggles true, b again→stays true, a→toggles false.
        assert_eq!(flips, vec![false, true, true, false]);
        // The defensive `#flip false` matches the current side, so no Flip leaks
        // in; the portrait switch and the following text stay.
        assert_eq!(
            messages[3].content,
            vec![
                ContentDef::Text("left again".into()),
                ContentDef::Portrait(Some("c".into())),
                ContentDef::Text("mid-switch".into()),
            ],
        );
    }

    #[test]
    fn explicit_flip_overrides_autoflip() {
        let messages = convo(
            "#dialogue d\n\
            \x20   #autoflip\n\
            \x20   #pic a\n\
            \x20   one\n\n\
            \x20   #pic b\n\
            \x20   two\n\n\
            \x20   #pic c #flip false\n\
            \x20   three\n\n\
            \x20   #pic d\n\
            \x20   four",
        );
        let flips: Vec<bool> = messages.iter().map(|m| m.flip).collect();
        // a→false, b→true, c forced false (resets the side), d→toggles true.
        assert_eq!(flips, vec![false, true, false, true]);
    }

    #[test]
    fn comments_and_nopause() {
        let messages = convo(
            "#dialogue d\n    // a comment\n    #nopause\n    flowing\n\n    // another\n    done",
        );
        assert!(!messages[0].pause);
        assert!(messages[1].pause);
        assert_eq!(messages[0].content, vec![ContentDef::Text("flowing".into())]);
    }

    #[test]
    fn errors_point_at_the_line() {
        assert_eq!(parse("ok = 1\n   stray").unwrap_err().line, 2);
        assert_eq!(parse("#dialogue d\n    #bogus x").unwrap_err().line, 2);
        assert_eq!(parse("#wat name").unwrap_err().line, 1);
    }

    /// The whole authored `en.eggtext` must parse to exactly what the JSON
    /// loader produces from `en.json`, key by key (so a mismatch names the key).
    #[test]
    fn eggtext_matches_en_json() {
        let dsl = parse(include_str!("../../../assets/script/en.eggtext")).expect("parse eggtext");
        let json: ScriptFile =
            serde_json::from_str(include_str!("../../../assets/script/en.json")).expect("parse json");

        for (key, value) in &json.labels {
            assert_eq!(dsl.labels.get(key), Some(value), "label {key:?}");
        }
        for (key, value) in &json.lists {
            assert_eq!(dsl.lists.get(key), Some(value), "list {key:?}");
        }
        for (key, value) in &json.dialogue {
            assert_eq!(dsl.dialogue.get(key), Some(value), "dialogue {key:?}");
        }

        assert_eq!(dsl.labels.len(), json.labels.len(), "label count");
        assert_eq!(dsl.lists.len(), json.lists.len(), "list count");
        assert_eq!(dsl.dialogue.len(), json.dialogue.len(), "dialogue count");
    }
}
