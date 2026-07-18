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
//! (one dialogue "page" / speaker turn each — always exactly one page: within
//! a message every text line *appends* onto the page that's already open,
//! there is no more mid-message "fresh page"). Within a message, lines become
//! content in order:
//!
//! * **Text lines** — `bare text` is stripped of surrounding whitespace;
//!   `"quoted text"` is preserved verbatim between the quotes. A trailing
//!   `#delay N` holds for `N` frames before the text *appends*; `0` (or no
//!   `#delay` at all) appends immediately. This now applies to a message's
//!   very first text line too — a `#delay` there holds before the page's
//!   first character appears, instead of being silently dropped.
//! * **Directives** — one or more `#word [arg]` on a line:
//!   * `#pic NAME` sets the speaker portrait; a later `#pic` switches it
//!     mid-message. `#pic none` explicitly clears it back to narration. A
//!     message with *no* `#pic` at all doesn't touch the portrait — it just
//!     carries over whatever was showing at the end of the previous message
//!     (and its side with it), all the way back to wherever one was last set
//!     in the conversation. Carry-over is resolved live, by the dialogue box,
//!     as each message plays — not by the parser — because an `#if` branch
//!     means the parser can't know what's "current" at a given point (see
//!     [`crate::data::script::message::PortraitState`]).
//!   * `#flip BOOL` chooses the portrait's side. Before any text it sets the
//!     message's side; after text it flips mid-message. Like the portrait
//!     itself, a message that sets neither `#pic` nor `#flip` carries over
//!     whatever side was in effect.
//!   * `#sound NAME` plays a sound effect at that point.
//!   * `#delay N` is a standalone `N`-frame pause.
//!   * `#speed N` sets the typewriter's pace (frames held between each
//!     revealed character) for all subsequent text in the dialogue — block
//!     scope from where it appears onward, like `#autoflip`, persisting
//!     across page breaks until another `#speed` changes it. `0` (the
//!     default) is the ordinary, unthrottled per-tick reveal.
//!   * `#nopause` flows straight on to the next message instead of waiting for
//!     the player to advance.
//!   * `#autoflip` (block scope, from where it appears onward) auto-alternates
//!     the portrait side whenever the speaker portrait changes, so two
//!     characters trade left/right automatically. An explicit `#flip` still
//!     overrides it for that message.
//!
//! ## Conditionals
//!
//! `#if NAME` (or `#if not NAME`) opens a branch on a declared `#flag`; zero
//! or more `#elif NAME` / `#elif not NAME` add further conditions, tried in
//! order; an optional trailing `#else` covers everything else; `#end` closes
//! the chain. `not` is reserved for this — `#flag not` is a parse error — so
//! `#if not NAME` can never be mistaken for testing a flag literally named
//! `not`. Nesting an `#if` inside another is not supported (yet).
//!
//! The whole chain resolves to a single carrier message: the branch is
//! chosen live, at *playback* time, against the actual save, the moment the
//! dialogue box reaches that point — not once, up front, when the
//! conversation is fetched. That's what lets an earlier `#choice`/`#set` in
//! the same conversation steer a later `#if` in it (see below).
//!
//! ```text
//! #flag liked_the_gift
//! #flag visited_before
//! #dialogue thanks
//!     #if liked_the_gift
//!     Thank you, I love it!
//!     #elif not visited_before
//!     Oh... well, thanks for stopping by, I guess.
//!     #else
//!     ...Thanks, I suppose.
//!     #end
//! ```
//!
//! ## Choices
//!
//! A `#choice` opens an interactive menu. It takes the rest of its message —
//! any text above it is the prompt shown with the options — and is a list of
//! `#option TEXT` lines, each followed by the `#set NAME BOOL` flags it writes
//! when picked (the same `#set` that fires inline elsewhere). It needs at least
//! two options; the picked option's flags then steer later dialogue through the
//! ordinary `#if` — evaluated live, at *playback* time, as the dialogue box
//! plays past it (a choice writes its flag the moment it's picked; the box
//! reads the same live save when it later reaches an `#if`), so a `#choice`
//! earlier in a conversation can steer an `#if` later in that very same
//! conversation. No `#end` — the block runs to the message's end (its blank
//! line).
//!
//! ```text
//! #flag chose_tea
//! #flag chose_coffee
//! #dialogue barista
//!     What'll it be?
//!     #choice
//!     #option Tea
//!     #set chose_tea true
//!     #option "Coffee, black"
//!     #set chose_coffee true
//! ```
//!
//! Escapes understood in text and labels: `\n` `\t` `\r` `\\` `\"` `\#`.

use std::collections::BTreeSet;

use super::{
    ChoiceOptionDef, ContentDef, DialogueDef, ElifDef, Entry, MessageDef, PortraitChange,
    ScriptFile, SegmentDef,
};

/// A parse failure, carrying the 1-based source line it occurred on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
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
    // `#flag` declarations must all precede the first entry; once any label /
    // `#list` / `#dialogue` has been seen this flips, and a later `#flag` is an
    // error (so the flag vocabulary is always readable from the top of the doc).
    let mut seen_entry = false;

    while let Some((idx, raw)) = lines.next() {
        let line_no = idx + 1;
        let logical = raw.trim_start();
        if logical.is_empty() || is_comment(logical) {
            continue;
        }
        if raw.starts_with([' ', '\t']) {
            return Err(ParseError::new(
                line_no,
                "indented line is not inside a block",
            ));
        }

        if let Some(header) = logical.strip_prefix('#') {
            let (kind, name) = split_first_word(header);
            // `#flag NAME` is a declaration, not a block: it takes no indented
            // body, just registers the name.
            if kind == "flag" {
                if name.is_empty() {
                    return Err(ParseError::new(line_no, "`#flag` needs a name"));
                }
                if name == "not" {
                    return Err(ParseError::new(
                        line_no,
                        "`not` is reserved and can't be a flag name (needed for `#if not NAME`)",
                    ));
                }
                if seen_entry {
                    return Err(ParseError::new(
                        line_no,
                        format!("`#flag {name}` must be declared before the first entry"),
                    ));
                }
                file.flags.insert(name.to_string());
                continue;
            }
            if name.is_empty() {
                return Err(ParseError::new(line_no, format!("`#{kind}` needs a name")));
            }
            seen_entry = true;
            let body = collect_block(&mut lines);
            match kind {
                "dialogue" => {
                    let dialogue = parse_dialogue(&body, &file.flags)?;
                    file.dialogue.insert(name.to_string(), dialogue);
                }
                "list" => {
                    file.lists.insert(name.to_string(), parse_list(&body)?);
                }
                other => {
                    return Err(ParseError::new(
                        line_no,
                        format!(
                            "unknown block `#{other}` (expected `#dialogue`, `#list` or `#flag`)"
                        ),
                    ));
                }
            }
        } else if let Some(eq) = logical.find('=') {
            seen_entry = true;
            let key = logical[..eq].trim();
            if key.is_empty() {
                return Err(ParseError::new(
                    line_no,
                    "label is missing a name before `=`",
                ));
            }
            file.labels.insert(
                key.to_string(),
                parse_value(&logical[eq + 1..], line_no)?,
            );
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
/// iterator positioned on the next column-0 line. Shared with the `.eggscene`
/// parser ([`crate::data::scene`]), which uses the same column-0-header /
/// indented-body block shape.
pub(crate) fn collect_block<'a, I>(lines: &mut std::iter::Peekable<I>) -> Vec<(usize, &'a str)>
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
            return Err(ParseError::new(
                line_no,
                "`#list` items can't be directives",
            ));
        }
        items.push(parse_value(logical, line_no)?);
    }
    Ok(items)
}

/// A structural marker line inside a `#dialogue` body: the `#if`/`#elif`/
/// `#else`/`#end` conditional delimiters, which split the body at message
/// boundaries.
enum Marker {
    If { flag: String, negated: bool },
    Elif { flag: String, negated: bool },
    Else,
    End,
}

/// One scanned piece of a `#dialogue` body in document order: a blank-line-
/// delimited message group, or a conditional [`Marker`].
enum BodyItem<'a> {
    Group(Vec<(usize, &'a str)>),
    Marker(usize, Marker),
}

/// A `#dialogue` body. Without conditionals it reduces to the tightest plain
/// shape ([`Entry::Line`] for a lone bare line, [`Entry::Pages`] for several),
/// wrapped as a single-segment [`DialogueDef::Plain`]. With `#if`/`#elif`/
/// `#else`/`#end` it becomes a [`DialogueDef::Segments`] list: unconditional
/// runs and flag-gated chains. Each `#if` chain resolves (see
/// [`SegmentDef::resolve`] in `crate::data::script`) to a single carrier
/// message the dialogue box picks a branch from live, at *playback* time —
/// not once, up front, at [`get_dialogue`]. `flags` is the declared
/// vocabulary, against which every `#set`/`#if`/`#elif` name is checked.
///
/// [`get_dialogue`]: crate::data::script::Script::get_dialogue
fn parse_dialogue(
    body: &[(usize, &str)],
    flags: &BTreeSet<String>,
) -> Result<DialogueDef, ParseError> {
    // Scan the body into message groups and conditional markers. A blank line OR
    // a marker line ends the current message group.
    fn flush<'a>(current: &mut Vec<(usize, &'a str)>, items: &mut Vec<BodyItem<'a>>) {
        if !current.is_empty() {
            items.push(BodyItem::Group(std::mem::take(current)));
        }
    }
    let mut items: Vec<BodyItem> = Vec::new();
    let mut current: Vec<(usize, &str)> = Vec::new();
    for &(line_no, raw) in body {
        let logical = raw.trim_start();
        if logical.is_empty() {
            flush(&mut current, &mut items);
        } else if is_comment(logical) {
            continue;
        } else if let Some(marker) = parse_marker(logical, line_no)? {
            flush(&mut current, &mut items);
            items.push(BodyItem::Marker(line_no, marker));
        } else {
            current.push((line_no, logical));
        }
    }
    flush(&mut current, &mut items);

    // Resolve every message group in document order so `#autoflip`/`#flip` side
    // tracking spans the whole entry exactly as before, then thread the resolved
    // messages back through the markers to build segments.
    let mut resolver = AutoflipState::default();
    let mut builder = SegmentBuilder::default();
    for item in &items {
        match item {
            BodyItem::Group(group) => {
                let def = resolver.resolve(parse_message(group, flags)?);
                builder.push_message(def);
            }
            BodyItem::Marker(line_no, marker) => builder.marker(*line_no, marker, flags)?,
        }
    }
    builder.finish()
}

/// Recognise a `#if [not] NAME` / `#elif [not] NAME` / `#else` / `#end` line.
/// Returns `None` for any other line (ordinary message content). A bare
/// `#if`/`#elif` with no name, a `not` with nothing after it, or a stray
/// extra word are all parse errors.
fn parse_marker(logical: &str, line_no: usize) -> Result<Option<Marker>, ParseError> {
    let Some(rest) = logical.strip_prefix('#') else {
        return Ok(None);
    };
    let (keyword, arg) = split_first_word(rest);
    Ok(Some(match keyword {
        "if" => {
            let (flag, negated) = parse_condition(arg, line_no, "if")?;
            Marker::If { flag, negated }
        }
        "elif" => {
            let (flag, negated) = parse_condition(arg, line_no, "elif")?;
            Marker::Elif { flag, negated }
        }
        "else" => Marker::Else,
        "end" => Marker::End,
        _ => return Ok(None),
    }))
}

/// Parse the `[not] NAME` argument shared by `#if`/`#elif`: a bare flag name,
/// or `not` followed by one. `not` is only recognised as a whole word (via
/// [`split_first_word`]), so a flag whose name merely *starts with* `not`
/// (e.g. `nothing`) is never mistaken for negation — and `not` itself can
/// never be a declared flag name (see the `#flag` check in [`parse`]), so
/// `#if not` with nothing after it is unambiguously an error rather than a
/// (impossible) test of a flag literally named `not`.
fn parse_condition(
    arg: &str,
    line_no: usize,
    keyword: &str,
) -> Result<(String, bool), ParseError> {
    if arg.is_empty() {
        return Err(ParseError::new(line_no, format!("`#{keyword}` needs a flag name")));
    }
    let (first, rest) = split_first_word(arg);
    let (name_part, negated) = if first == "not" {
        if rest.is_empty() {
            return Err(ParseError::new(
                line_no,
                format!("`#{keyword} not` needs a flag name"),
            ));
        }
        (rest, true)
    } else {
        (arg, false)
    };
    let (flag, extra) = split_first_word(name_part);
    if !extra.is_empty() {
        return Err(ParseError::new(
            line_no,
            format!("`#{keyword}` has an unexpected extra word {extra:?}"),
        ));
    }
    Ok((flag.to_string(), negated))
}

/// Tracks portrait-side state (`#autoflip` + explicit `#flip`) across a whole
/// `#dialogue`, turning each [`ParsedMessage`] into a final [`MessageDef`].
#[derive(Default)]
struct AutoflipState {
    autoflip: bool,
    side: bool,
    last_portrait: Option<String>,
}

impl AutoflipState {
    fn resolve(&mut self, parsed: ParsedMessage) -> MessageDef {
        let mut def = parsed.def;

        if parsed.autoflip && !self.autoflip {
            self.autoflip = true;
            self.side = false;
            self.last_portrait = None;
        }

        if let Some(explicit) = parsed.flip {
            def.flip = Some(explicit);
            if let PortraitChange::Set(portrait) = &def.portrait {
                self.side = explicit;
                self.last_portrait = Some(portrait.clone());
            }
        } else if self.autoflip
            && let PortraitChange::Set(portrait) = &def.portrait
        {
            if self
                .last_portrait
                .as_ref()
                .is_some_and(|last| last != portrait)
            {
                self.side = !self.side;
            }
            def.flip = Some(self.side);
            self.last_portrait = Some(portrait.clone());
        }

        drop_redundant_flips(&mut def);
        def
    }
}

/// Assembles resolved messages and `#if`/`#elif`/`#else`/`#end` markers into a
/// [`DialogueDef`]. Tracks at most one open conditional chain (no nesting) and
/// the branch (`#if`'s own `then`, the current `#elif`'s, or `#else`)
/// currently collecting messages.
#[derive(Default)]
struct SegmentBuilder {
    segments: Vec<SegmentDef>,
    /// The unconditional run collecting messages outside any `#if`.
    plain: Vec<MessageDef>,
    /// The open conditional chain, if any.
    open: Option<OpenIf>,
}

/// The currently-open `#if`/`#elif`/`#else` chain, awaiting its `#end`.
struct OpenIf {
    flag: String,
    negated: bool,
    then: Vec<MessageDef>,
    /// Completed `#elif` branches in document order; the last one (if any) is
    /// where a message currently routes to, unless `in_else`.
    elifs: Vec<OpenElif>,
    otherwise: Vec<MessageDef>,
    /// Whether `#else` has been seen: newly gathered messages go to
    /// `otherwise`, and a further `#elif` is now an error.
    in_else: bool,
    /// The line the `#if` opened on, so a missing `#end` points back at it.
    open_line: usize,
}

/// One in-progress `#elif [not] NAME` branch: its condition, plus the
/// messages gathered for it since (reduced to an [`Entry`] only once the
/// whole chain closes at `#end`).
struct OpenElif {
    flag: String,
    negated: bool,
    then: Vec<MessageDef>,
}

impl SegmentBuilder {
    /// Route a resolved message to the open branch, or to the plain run.
    fn push_message(&mut self, def: MessageDef) {
        match &mut self.open {
            Some(open) if open.in_else => open.otherwise.push(def),
            Some(open) => match open.elifs.last_mut() {
                Some(elif) => elif.then.push(def),
                None => open.then.push(def),
            },
            None => self.plain.push(def),
        }
    }

    fn marker(
        &mut self,
        line_no: usize,
        marker: &Marker,
        flags: &BTreeSet<String>,
    ) -> Result<(), ParseError> {
        match marker {
            Marker::If { flag, negated } => {
                if self.open.is_some() {
                    return Err(ParseError::new(
                        line_no,
                        "`#if` cannot be nested inside another `#if`",
                    ));
                }
                check_flag(flag, line_no, flags)?;
                // Close the current plain run so the conditional slots in order.
                self.flush_plain();
                self.open = Some(OpenIf {
                    flag: flag.clone(),
                    negated: *negated,
                    then: Vec::new(),
                    elifs: Vec::new(),
                    otherwise: Vec::new(),
                    in_else: false,
                    open_line: line_no,
                });
            }
            Marker::Elif { flag, negated } => {
                let Some(open) = &mut self.open else {
                    return Err(ParseError::new(line_no, "`#elif` without a matching `#if`"));
                };
                if open.in_else {
                    return Err(ParseError::new(line_no, "`#elif` can't follow `#else`"));
                }
                check_flag(flag, line_no, flags)?;
                open.elifs.push(OpenElif {
                    flag: flag.clone(),
                    negated: *negated,
                    then: Vec::new(),
                });
            }
            Marker::Else => {
                let Some(open) = &mut self.open else {
                    return Err(ParseError::new(line_no, "`#else` without a matching `#if`"));
                };
                if open.in_else {
                    return Err(ParseError::new(line_no, "a second `#else` in one `#if`"));
                }
                open.in_else = true;
            }
            Marker::End => {
                let Some(open) = self.open.take() else {
                    return Err(ParseError::new(line_no, "`#end` without a matching `#if`"));
                };
                self.segments.push(SegmentDef::If {
                    flag: open.flag,
                    negated: open.negated,
                    then: reduce_entry(open.then),
                    otherwise: (!open.otherwise.is_empty()).then(|| reduce_entry(open.otherwise)),
                    elifs: open
                        .elifs
                        .into_iter()
                        .map(|elif| ElifDef {
                            flag: elif.flag,
                            negated: elif.negated,
                            then: reduce_entry(elif.then),
                        })
                        .collect(),
                });
            }
        }
        Ok(())
    }

    /// Flush the pending plain run as a `Plain` segment, if non-empty.
    fn flush_plain(&mut self) {
        if !self.plain.is_empty() {
            self.segments
                .push(SegmentDef::Plain(reduce_entry(std::mem::take(
                    &mut self.plain,
                ))));
        }
    }

    fn finish(mut self) -> Result<DialogueDef, ParseError> {
        if let Some(open) = &self.open {
            // The body ended with a conditional still open; point at the `#if`.
            return Err(ParseError::new(
                open.open_line,
                "`#if` is missing its closing `#end`",
            ));
        }
        // A body with no conditionals is one plain run: keep the compact
        // single-entry shape so existing entries round-trip unchanged.
        if self.segments.is_empty() {
            return Ok(DialogueDef::Plain(reduce_entry(self.plain)));
        }
        self.flush_plain();
        Ok(DialogueDef::Segments {
            segments: self.segments,
        })
    }
}

/// A `#set`/`#if` may only name a declared `#flag`; otherwise it is a
/// line-pointed parse error.
fn check_flag(name: &str, line_no: usize, flags: &BTreeSet<String>) -> Result<(), ParseError> {
    if flags.contains(name) {
        Ok(())
    } else {
        Err(ParseError::new(
            line_no,
            format!("undeclared flag {name:?} (add `#flag {name}` at the top)"),
        ))
    }
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

fn parse_message(
    lines: &[(usize, &str)],
    flags: &BTreeSet<String>,
) -> Result<ParsedMessage, ParseError> {
    let mut def = MessageDef {
        portrait: PortraitChange::Keep,
        flip: None,
        pause: true,
        content: Vec::new(),
    };
    let mut flip: Option<bool> = None;
    let mut autoflip = false;
    let mut have_portrait = false;
    let mut have_text = false;

    for (idx, &(line_no, logical)) in lines.iter().enumerate() {
        // `#choice` opens an interactive menu. It consumes the rest of the
        // message (its `#option`/`#set` lines), so the whole block stays in one
        // page with any preceding text as the prompt; nothing may follow it.
        if let Some(args) = strip_directive(logical, "choice") {
            if !args.is_empty() {
                return Err(ParseError::new(
                    line_no,
                    "`#choice` takes no arguments (put the prompt on the line above)",
                ));
            }
            let options = parse_choice(&lines[idx + 1..], line_no, flags)?;
            def.content.push(ContentDef::Choice(options));
            break;
        }
        // `#set NAME BOOL` takes two arguments, so it is parsed as a whole line
        // rather than through `directive_segments` (which only ever captures
        // one argument per directive, erroring on a second rather than
        // dropping it — no help for a directive whose grammar wants two).
        if let Some(rest) = logical.strip_prefix("#set")
            && (rest.is_empty() || rest.starts_with([' ', '\t']))
        {
            let (name, bool_arg) = split_first_word(rest);
            if name.is_empty() {
                return Err(ParseError::new(line_no, "`#set` needs `NAME BOOL`"));
            }
            check_flag(name, line_no, flags)?;
            let value = parse_bool(Some(bool_arg.trim()), line_no).map_err(|_| {
                ParseError::new(line_no, "`#set` needs `NAME true` or `NAME false`")
            })?;
            def.content
                .push(ContentDef::SetFlag(name.to_string(), value));
            continue;
        }
        // `#shake FRAMES [AMP]` can take two arguments, so like `#set` it is
        // parsed as a whole line rather than through `directive_segments`
        // (same reason: two arguments, one-arg-per-directive splitter).
        if let Some(rest) = logical.strip_prefix("#shake")
            && (rest.is_empty() || rest.starts_with([' ', '\t']))
        {
            let (frames, amplitude) = parse_shake(rest, line_no)?;
            def.content.push(ContentDef::Shake(frames, amplitude));
            continue;
        }
        if logical.starts_with('#') {
            for (name, arg) in directive_segments(logical, line_no)? {
                match name {
                    "pic" => {
                        if have_portrait {
                            // Mid-message switch: unaffected by keep/clear —
                            // there's no "keep" mid-message, only "clear" or
                            // "set", exactly as before.
                            let portrait = match arg {
                                None | Some("none") | Some("-") => None,
                                Some(name) => Some(name.to_string()),
                            };
                            def.content.push(ContentDef::Portrait(portrait));
                        } else {
                            def.portrait = match arg {
                                None | Some("none") | Some("-") => PortraitChange::Clear,
                                Some(name) => PortraitChange::Set(name.to_string()),
                            };
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
                        let name =
                            arg.ok_or_else(|| ParseError::new(line_no, "`#sound` needs a name"))?;
                        def.content.push(ContentDef::Sound(name.to_string()));
                    }
                    "delay" => def.content.push(ContentDef::Delay(parse_u8(arg, line_no)?)),
                    "speed" => def.content.push(ContentDef::Speed(parse_u8(arg, line_no)?)),
                    "nopause" => {
                        if let Some(extra) = arg {
                            return Err(ParseError::new(
                                line_no,
                                format!("`#nopause` takes no arguments, found {extra:?}"),
                            ));
                        }
                        def.pause = false;
                    }
                    "autoflip" => {
                        if let Some(extra) = arg {
                            return Err(ParseError::new(
                                line_no,
                                format!("`#autoflip` takes no arguments, found {extra:?}"),
                            ));
                        }
                        autoflip = true;
                    }
                    other => {
                        return Err(ParseError::new(
                            line_no,
                            format!("unknown directive `#{other}`"),
                        ));
                    }
                }
            }
        } else {
            // Every text line — including a message's very first — appends
            // onto the page (see the module doc): `#delay` just decides
            // whether that append is immediate or held. No more special-
            // casing the opening line.
            let (text, delay) = split_text(logical, line_no)?;
            have_text = true;
            match delay {
                Some(delay) => def.content.push(ContentDef::Delayed(text, delay)),
                None => def.content.push(ContentDef::Text(text)),
            }
        }
    }

    Ok(ParsedMessage {
        def,
        flip,
        autoflip,
    })
}

/// The `#option`/`#set` lines following a `#choice` header, as an option list.
/// Each `#option TEXT` opens an option (text stripped/unescaped, `"quotes"`
/// honoured like any dialogue line); the `#set NAME BOOL` lines beneath it are
/// the flags it writes when picked — the exact same `#set` machinery the box
/// fires elsewhere. A `#choice` needs at least two options, each with non-empty
/// text; a `#set` before the first `#option`, or any other line, is a
/// line-pointed error, as is a `#set` naming an undeclared flag.
fn parse_choice(
    lines: &[(usize, &str)],
    header_line: usize,
    flags: &BTreeSet<String>,
) -> Result<Vec<ChoiceOptionDef>, ParseError> {
    let mut options: Vec<ChoiceOptionDef> = Vec::new();
    for &(line_no, logical) in lines {
        if is_comment(logical) {
            continue;
        }
        if let Some(rest) = strip_directive(logical, "option") {
            let text = parse_value(rest, line_no)?;
            if text.is_empty() {
                return Err(ParseError::new(line_no, "`#option` needs display text"));
            }
            options.push(ChoiceOptionDef {
                text,
                sets: Vec::new(),
            });
        } else if let Some(rest) = strip_directive(logical, "set") {
            let (name, bool_arg) = split_first_word(rest);
            if name.is_empty() {
                return Err(ParseError::new(line_no, "`#set` needs `NAME BOOL`"));
            }
            check_flag(name, line_no, flags)?;
            let value = parse_bool(Some(bool_arg.trim()), line_no).map_err(|_| {
                ParseError::new(line_no, "`#set` needs `NAME true` or `NAME false`")
            })?;
            let Some(option) = options.last_mut() else {
                return Err(ParseError::new(
                    line_no,
                    "`#set` inside `#choice` must follow an `#option`",
                ));
            };
            option.sets.push((name.to_string(), value));
        } else {
            return Err(ParseError::new(
                line_no,
                "only `#option` and `#set` may appear inside `#choice`",
            ));
        }
    }
    if options.len() < 2 {
        return Err(ParseError::new(
            header_line,
            "a `#choice` needs at least two `#option`s",
        ));
    }
    Ok(options)
}

/// If `logical` is the directive `#word` — bare, or `#word` followed by
/// whitespace and args — return the trimmed argument text (`""` when bare).
/// Returns `None` for a different word, so `#option` never matches `#optionx`.
fn strip_directive<'a>(logical: &'a str, word: &str) -> Option<&'a str> {
    let rest = logical.strip_prefix('#')?.strip_prefix(word)?;
    if rest.is_empty() || rest.starts_with([' ', '\t']) {
        Some(rest.trim())
    } else {
        None
    }
}

/// Drop mid-message `#flip`s that don't actually change the current side, so a
/// defensive `#flip false` on an already-unflipped speaker emits nothing. Only
/// possible when the message's own starting side is actually known
/// (`def.flip` is `Some`, from an explicit `#flip` or `#autoflip`) — a message
/// that carries over its side (`None`) can't have its mid-message flips
/// checked for redundancy at parse time, since what the side actually *is*
/// depends on playback, so they're all kept.
fn drop_redundant_flips(def: &mut MessageDef) {
    let Some(mut side) = def.flip else {
        return;
    };
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
/// message is "plain" when it is a lone unstyled line of text — no portrait or
/// flip mentioned (`Keep`/`None`, i.e. it doesn't touch either), waits for a
/// manual advance; an all-plain conversation is a [`Entry::Line`] (one
/// message) or [`Entry::Pages`] (more). `Keep`/`None` are exactly what a bare
/// conversation's messages already resolve to (see
/// [`message::Message::default`](crate::data::script::message::Message::default)),
/// so this still round-trips unchanged.
fn reduce_entry(messages: Vec<MessageDef>) -> Entry {
    let plain: Option<Vec<String>> = messages
        .iter()
        .map(|message| match message.content.as_slice() {
            [ContentDef::Text(text)]
                if message.portrait == PortraitChange::Keep
                    && message.flip.is_none()
                    && message.pause =>
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
//
// A handful of these (`is_comment`, `split_first_word`, `parse_value`,
// `quoted_span`, `unescape`, `escape`) are `pub(crate)` so the sibling
// `.eggscene` parser ([`crate::data::scene`]) reuses the exact same line
// scanner / quoting / escaping rules rather than duplicating them.

pub(crate) fn is_comment(logical: &str) -> bool {
    logical.starts_with("//")
}

/// Split off the first whitespace-delimited word, returning it and the trimmed
/// remainder.
pub(crate) fn split_first_word(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, ""),
    }
}

/// Break a directive line into `(name, first-arg)` segments, e.g.
/// `#pic y_oof #flip false` → `[("pic", Some("y_oof")), ("flip", Some("false"))]`.
/// Any token the grammar doesn't consume is a line-pointed [`ParseError`]
/// rather than being silently dropped: a second bare word for a directive
/// that only takes one argument (`#sound gain loss`), or a stray word that
/// isn't a `#directive` at all.
fn directive_segments(
    logical: &str,
    line_no: usize,
) -> Result<Vec<(&str, Option<&str>)>, ParseError> {
    let mut segments = Vec::new();
    let mut tokens = logical.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let Some(name) = token.strip_prefix('#') else {
            return Err(ParseError::new(
                line_no,
                format!("unexpected token {token:?} (expected a `#directive`)"),
            ));
        };
        let mut arg = None;
        while let Some(next) = tokens.peek() {
            if next.starts_with('#') {
                break;
            }
            if arg.is_none() {
                arg = Some(*next);
                tokens.next();
            } else {
                return Err(ParseError::new(
                    line_no,
                    format!("`#{name}` has an unexpected extra argument {next:?}"),
                ));
            }
        }
        segments.push((name, arg));
    }
    Ok(segments)
}

/// A standalone string value (label value or list item): the verbatim contents
/// of a `"quoted"` span, otherwise the trimmed line. Escapes are resolved.
pub(crate) fn parse_value(s: &str, line_no: usize) -> Result<String, ParseError> {
    let s = s.trim();
    Ok(match quoted_span(s, line_no)? {
        Some((inner, _)) => unescape(inner),
        None => unescape(s),
    })
}

/// A dialogue text line: its resolved text and an optional trailing `#delay N`.
fn split_text(logical: &str, line_no: usize) -> Result<(String, Option<u8>), ParseError> {
    if let Some((inner, after)) = quoted_span(logical, line_no)? {
        Ok((unescape(inner), parse_trailing_delay(after, line_no)?))
    } else {
        let (text, delay) = peel_trailing_delay(logical);
        Ok((unescape(text.trim()), delay))
    }
}

/// If `s` opens with a double quote, return the span between it and its
/// closing double quote, plus whatever trails the closing quote. The closing
/// quote is the line's one *unescaped* `"` remaining after the opening one:
/// finding none before the line ends is a missing-closing-quote
/// [`ParseError`]; finding more than one means an interior quote the author
/// forgot to escape (e.g. `"a" and "b"`, which used to silently fold into one
/// verbatim string via `str::rfind` matching the *last* quote in the line) —
/// also an error, asking for `\"` instead.
pub(crate) fn quoted_span(
    s: &str,
    line_no: usize,
) -> Result<Option<(&str, &str)>, ParseError> {
    let s = s.trim_start();
    let Some(rest) = s.strip_prefix('"') else {
        return Ok(None);
    };
    let mut quotes = Vec::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '"' => quotes.push(i),
            _ => {}
        }
    }
    match quotes.as_slice() {
        [] => Err(ParseError::new(
            line_no,
            "quoted text is missing its closing `\"`",
        )),
        &[close] => Ok(Some((&rest[..close], &rest[close + 1..]))),
        _ => Err(ParseError::new(
            line_no,
            "unescaped `\"` inside quoted text — escape it as `\\\"`",
        )),
    }
}

/// Parse the text that follows a closing quote: nothing, or a `#delay N`.
fn parse_trailing_delay(after: &str, line_no: usize) -> Result<Option<u8>, ParseError> {
    let after = after.trim();
    if after.is_empty() {
        return Ok(None);
    }
    let Some(rest) = after.strip_prefix('#') else {
        return Err(ParseError::new(
            line_no,
            format!("unexpected text after quote: {after:?}"),
        ));
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

/// The arguments of `#shake FRAMES [AMP]`: a frame count, and an optional pixel
/// amplitude defaulting to the cutscene `shake` verb's
/// [`DEFAULT_SHAKE_AMPLITUDE`](crate::data::scene::DEFAULT_SHAKE_AMPLITUDE) —
/// the two spell the same effect, so they share the default.
fn parse_shake(rest: &str, line_no: usize) -> Result<(u32, i16), ParseError> {
    let mut tokens = rest.split_whitespace();
    let frames: u32 = tokens
        .next()
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| ParseError::new(line_no, "`#shake` needs a frame count"))?;
    if frames == 0 {
        return Err(ParseError::new(line_no, "`#shake 0` — needs ≥1 frame"));
    }
    let amplitude: i16 = match tokens.next() {
        Some(t) => t
            .parse()
            .map_err(|_| ParseError::new(line_no, "`#shake` amplitude must be a pixel count"))?,
        None => crate::data::scene::DEFAULT_SHAKE_AMPLITUDE,
    };
    if tokens.next().is_some() {
        return Err(ParseError::new(
            line_no,
            "`#shake` takes `FRAMES [AMP]` — too many arguments",
        ));
    }
    Ok((frames, amplitude))
}

/// Resolve backslash escapes; an unknown escape keeps its backslash.
pub(crate) fn unescape(s: &str) -> String {
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

    /// The single dialogue entry `src` defines, unwrapped to its plain [`Entry`]
    /// (the shape of every non-conditional entry).
    fn dialogue(src: &str) -> Entry {
        match dialogue_def(src) {
            DialogueDef::Plain(entry) => entry,
            other => panic!("expected a plain entry, got {other:?}"),
        }
    }

    /// The single dialogue map value `src` defines, in its on-disk
    /// [`DialogueDef`] shape (so conditional-segment tests can inspect it).
    fn dialogue_def(src: &str) -> DialogueDef {
        let file = parse(src).expect("parse");
        file.dialogue
            .into_values()
            .next()
            .expect("one dialogue entry")
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
        assert_eq!(
            dialogue("#dialogue d\n    Just one line."),
            Entry::Line("Just one line.".into())
        );
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
            vec![
                ContentDef::Text("Hi".into()),
                ContentDef::Delayed(" there".into(), 5)
            ],
        );
    }

    #[test]
    fn every_text_line_keeps_its_own_delay() {
        // Unlike the old model, a `#delay` on a message's very first text line
        // is no longer dropped — it's meaningful now (a hold before the
        // page's first character appears), just like on any later line.
        let messages = convo("#dialogue d\n    \"T\" #delay 10\n    \"h\" #delay 10");
        assert_eq!(
            messages[0].content,
            vec![
                ContentDef::Delayed("T".into(), 10),
                ContentDef::Delayed("h".into(), 10)
            ],
        );
    }

    /// Two bare lines in one message both land as plain (undelayed) `Text`
    /// items, in order — at *playback* time these append onto the same page
    /// (see `egg_ui::dialogue`'s tests), but structurally the parser treats
    /// every text line the same regardless of position.
    #[test]
    fn consecutive_bare_lines_both_become_text_items() {
        let messages = convo("#dialogue d\n    Hello\n    there.");
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Text("Hello".into()), ContentDef::Text("there.".into())],
        );
    }

    /// A blank line always yields a new message, and — since it mentions no
    /// `#pic` — that message's portrait is `Keep`, not narration: it carries
    /// over whatever the previous message showed rather than clearing it.
    #[test]
    fn blank_line_yields_a_new_message_with_portrait_keep() {
        let messages = convo("#dialogue d\n    #pic y_oof\n    Hi.\n\n    Bye.");
        assert_eq!(messages[1].portrait, PortraitChange::Keep);
        assert_eq!(messages[1].flip, None);
    }

    /// `#pic none` as a message's first `#pic` explicitly clears the
    /// portrait — distinct from a message that never mentions `#pic` at all
    /// (`Keep`).
    #[test]
    fn pic_none_yields_clear() {
        let messages = convo("#dialogue d\n    #pic none\n    Hi.");
        assert_eq!(messages[0].portrait, PortraitChange::Clear);
    }

    /// `#speed N` emits an inline `Speed` content item wherever it appears —
    /// no special block-scope bookkeeping at parse time, since (like
    /// `#sound`/`#set`) it's simply carried forward by the `Dialogue` widget
    /// as playback reaches it (see `egg_ui::dialogue`'s tests for the actual
    /// cross-page persistence).
    #[test]
    fn speed_emits_inline() {
        let messages = convo("#dialogue d\n    #speed 10\n    Sloooow.");
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Speed(10), ContentDef::Text("Sloooow.".into())],
        );
    }

    #[test]
    fn sound_and_escapes_interleave() {
        let messages = convo("#dialogue d\n    #sound gain\n    Found it...!\\n");
        assert_eq!(
            messages[0].content,
            vec![
                ContentDef::Sound("gain".into()),
                ContentDef::Text("Found it...!\n".into())
            ],
        );
    }

    /// `#shake FRAMES [AMP]` fires in content order like `#sound`, defaults its
    /// amplitude to the cutscene verb's, and rejects misauthored forms with a
    /// line-pointed error.
    #[test]
    fn shake_parses_with_optional_amplitude() {
        let messages = convo("#dialogue d\n    #shake 30\n    Whoa!");
        assert_eq!(
            messages[0].content,
            vec![
                ContentDef::Shake(30, crate::data::scene::DEFAULT_SHAKE_AMPLITUDE),
                ContentDef::Text("Whoa!".into())
            ],
        );
        let messages = convo("#dialogue d\n    Rumble...\n    #shake 45 6");
        assert_eq!(messages[0].content[1], ContentDef::Shake(45, 6));

        // Misauthored forms point at their line.
        assert_eq!(parse("#dialogue d\n    #shake").unwrap_err().line, 2);
        assert_eq!(parse("#dialogue d\n    #shake 0").unwrap_err().line, 2);
        assert_eq!(parse("#dialogue d\n    #shake many").unwrap_err().line, 2);
        assert_eq!(parse("#dialogue d\n    #shake 30 x").unwrap_err().line, 2);
        assert_eq!(parse("#dialogue d\n    #shake 30 2 9").unwrap_err().line, 2);
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
        let flips: Vec<Option<bool>> = messages.iter().map(|m| m.flip).collect();
        // a→false, b→toggles true, b again→stays true, a→toggles false. Every
        // message here sets a portrait, so autoflip always resolves a side —
        // none of these are `None` (carry-over).
        assert_eq!(flips, vec![Some(false), Some(true), Some(true), Some(false)]);
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
        let flips: Vec<Option<bool>> = messages.iter().map(|m| m.flip).collect();
        // a→false, b→true, c forced false (resets the side), d→toggles true.
        assert_eq!(flips, vec![Some(false), Some(true), Some(false), Some(true)]);
    }

    #[test]
    fn comments_and_nopause() {
        let messages = convo(
            "#dialogue d\n    // a comment\n    #nopause\n    flowing\n\n    // another\n    done",
        );
        assert!(!messages[0].pause);
        assert!(messages[1].pause);
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Text("flowing".into())]
        );
    }

    #[test]
    fn errors_point_at_the_line() {
        assert_eq!(parse("ok = 1\n   stray").unwrap_err().line, 2);
        assert_eq!(parse("#dialogue d\n    #bogus x").unwrap_err().line, 2);
        assert_eq!(parse("#wat name").unwrap_err().line, 1);
    }

    #[test]
    fn flags_declared_at_the_top_register() {
        let file = parse("#flag one\n#flag two\n\ngame_title = hi").unwrap();
        assert!(file.flags.contains("one"));
        assert!(file.flags.contains("two"));
        assert_eq!(file.flags.len(), 2);
    }

    #[test]
    fn flag_after_an_entry_errors_at_the_line() {
        // The first entry is line 1; the stray `#flag` is line 2.
        let err = parse("game_title = hi\n#flag late").unwrap_err();
        assert_eq!(err.line, 2);
        // …and a `#flag` after a `#dialogue` entry, too.
        let err = parse("#dialogue d\n    Hi\n#flag late").unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn set_directive_emits_a_set_flag_item() {
        let messages = convo("#flag seen\n#dialogue d\n    #set seen true\n    Hello.");
        assert_eq!(
            messages[0].content,
            vec![
                ContentDef::SetFlag("seen".into(), true),
                ContentDef::Text("Hello.".into())
            ],
        );
    }

    #[test]
    fn set_undeclared_flag_errors_at_the_line() {
        let err = parse("#dialogue d\n    #set nope true\n    Hi.").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn set_needs_a_bool() {
        let err = parse("#flag seen\n#dialogue d\n    #set seen maybe\n    Hi.").unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn if_else_end_build_a_conditional_segment() {
        let def = dialogue_def(
            "#flag seen\n\
             #dialogue d\n\
             \x20   #if seen\n\
             \x20   After.\n\
             \x20   #else\n\
             \x20   Before.\n\
             \x20   #end",
        );
        let DialogueDef::Segments { segments } = def else {
            panic!("expected segments, got {def:?}");
        };
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0],
            SegmentDef::If {
                flag: "seen".into(),
                negated: false,
                then: Entry::Line("After.".into()),
                otherwise: Some(Entry::Line("Before.".into())),
                elifs: vec![],
            },
        );
    }

    #[test]
    fn if_without_else_has_no_otherwise() {
        let def = dialogue_def("#flag seen\n#dialogue d\n    #if seen\n    Yes.\n    #end");
        let DialogueDef::Segments { segments } = def else {
            panic!("expected segments");
        };
        assert_eq!(
            segments[0],
            SegmentDef::If {
                flag: "seen".into(),
                negated: false,
                then: Entry::Line("Yes.".into()),
                otherwise: None,
                elifs: vec![],
            },
        );
    }

    #[test]
    fn plain_runs_around_a_conditional_are_kept_in_order() {
        // intro (plain) → #if → outro (plain): three segments in document order.
        let def = dialogue_def(
            "#flag seen\n\
             #dialogue d\n\
             \x20   Intro.\n\n\
             \x20   #if seen\n\
             \x20   Branch.\n\
             \x20   #end\n\n\
             \x20   Outro.",
        );
        let DialogueDef::Segments { segments } = def else {
            panic!("expected segments");
        };
        assert_eq!(segments.len(), 3);
        assert!(matches!(&segments[0], SegmentDef::Plain(Entry::Line(s)) if s == "Intro."));
        assert!(matches!(&segments[1], SegmentDef::If { flag, .. } if flag == "seen"));
        assert!(matches!(&segments[2], SegmentDef::Plain(Entry::Line(s)) if s == "Outro."));
    }

    #[test]
    fn if_undeclared_flag_errors_at_the_line() {
        let err = parse("#dialogue d\n    #if nope\n    Hi.\n    #end").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn missing_end_errors_at_the_if_line() {
        let err = parse("#flag seen\n#dialogue d\n    #if seen\n    Hi.").unwrap_err();
        // The error points back at the unclosed `#if` (line 3), not the body end.
        assert_eq!(err.line, 3);
    }

    #[test]
    fn nested_if_errors_at_the_inner_line() {
        let err = parse(
            "#flag a\n#flag b\n#dialogue d\n    #if a\n    x\n    #if b\n    y\n    #end\n    #end",
        )
        .unwrap_err();
        // The inner `#if` is line 6.
        assert_eq!(err.line, 6);
    }

    #[test]
    fn else_or_end_without_if_errors_at_the_line() {
        assert_eq!(
            parse("#dialogue d\n    Hi.\n    #else").unwrap_err().line,
            3
        );
        assert_eq!(parse("#dialogue d\n    Hi.\n    #end").unwrap_err().line, 3);
    }

    #[test]
    fn if_not_negates_and_swaps_nothing_at_parse_time() {
        // `negated` just rides along on the flat schema; the swap happens at
        // resolution (see `crate::data::script::tests`), not here.
        let def = dialogue_def(
            "#flag seen\n\
             #dialogue d\n\
             \x20   #if not seen\n\
             \x20   Before.\n\
             \x20   #else\n\
             \x20   After.\n\
             \x20   #end",
        );
        let DialogueDef::Segments { segments } = def else {
            panic!("expected segments");
        };
        assert_eq!(
            segments[0],
            SegmentDef::If {
                flag: "seen".into(),
                negated: true,
                then: Entry::Line("Before.".into()),
                otherwise: Some(Entry::Line("After.".into())),
                elifs: vec![],
            },
        );
    }

    #[test]
    fn elif_chain_parses_into_ordered_elifs() {
        let def = dialogue_def(
            "#flag a\n#flag b\n\
             #dialogue d\n\
             \x20   #if a\n\
             \x20   A.\n\
             \x20   #elif not b\n\
             \x20   B.\n\
             \x20   #else\n\
             \x20   C.\n\
             \x20   #end",
        );
        let DialogueDef::Segments { segments } = def else {
            panic!("expected segments");
        };
        assert_eq!(
            segments[0],
            SegmentDef::If {
                flag: "a".into(),
                negated: false,
                then: Entry::Line("A.".into()),
                otherwise: Some(Entry::Line("C.".into())),
                elifs: vec![ElifDef {
                    flag: "b".into(),
                    negated: true,
                    then: Entry::Line("B.".into()),
                }],
            },
        );
    }

    #[test]
    fn elif_without_a_matching_if_errors_at_the_line() {
        let err = parse("#dialogue d\n    #elif a\n    Bye.").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn elif_after_else_errors_at_the_line() {
        let err = parse(
            "#flag a\n#flag b\n#dialogue d\n    #if a\n    X.\n    #else\n    Y.\n    #elif b\n    Z.\n    #end",
        )
        .unwrap_err();
        assert_eq!(err.line, 8);
    }

    #[test]
    fn if_not_and_elif_not_need_a_flag_name() {
        assert_eq!(
            parse("#dialogue d\n    #if not\n    Hi.\n    #end")
                .unwrap_err()
                .line,
            2
        );
        assert_eq!(
            parse("#flag a\n#dialogue d\n    #if a\n    Hi.\n    #elif not\n    Bye.\n    #end")
                .unwrap_err()
                .line,
            5
        );
    }

    #[test]
    fn flag_named_not_is_reserved() {
        let err = parse("#flag not").unwrap_err();
        assert_eq!(err.line, 1);
    }

    #[test]
    fn stray_directive_token_errors_at_the_line() {
        let err = parse("#dialogue d\n    #pic y_oof stray_typo\n    Hi.").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn sound_extra_argument_errors_at_the_line() {
        let err = parse("#dialogue d\n    #sound gain loss\n    Hi.").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn multi_directive_line_still_parses() {
        // `#pic y_oof #flip false` must keep working: each directive gets its
        // own single argument, split at the next `#`.
        let messages = convo("#dialogue d\n    #pic y_oof #flip false\n    Hi.");
        assert_eq!(messages[0].portrait, PortraitChange::Set("y_oof".into()));
        assert_eq!(messages[0].flip, Some(false));
    }

    #[test]
    fn unescaped_interior_quote_errors_at_the_line() {
        // Old (`str::rfind`-based) behaviour silently matched the *last*
        // quote in the line, folding the middle into one verbatim string.
        let err = parse("#dialogue d\n    \"a\" and \"b\"").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn quote_missing_its_closing_mark_errors_at_the_line() {
        // The trailing `\"` is an *escaped* quote, not a terminator, so there
        // is no unescaped closing quote at all — the author forgot one.
        let err = parse("#dialogue d\n    \"abc\\\"").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn the_real_corpus_parses() {
        parse(include_str!("../../../../../assets/script/en.eggtext")).expect("parse en.eggtext");
    }

    #[test]
    fn choice_block_parses_prompt_options_and_sets() {
        // The prompt text and the `#choice` stay in one message; each `#option`
        // carries its display text and the `#set`s beneath it.
        let messages = convo(
            "#flag tea\n#flag coffee\n#dialogue d\n\
             \x20   What'll it be?\n\
             \x20   #choice\n\
             \x20   #option Tea\n\
             \x20   #set tea true\n\
             \x20   #option \"Coffee, black\"\n\
             \x20   #set coffee true",
        );
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].content,
            vec![
                ContentDef::Text("What'll it be?".into()),
                ContentDef::Choice(vec![
                    ChoiceOptionDef {
                        text: "Tea".into(),
                        sets: vec![("tea".into(), true)],
                    },
                    ChoiceOptionDef {
                        text: "Coffee, black".into(),
                        sets: vec![("coffee".into(), true)],
                    },
                ]),
            ],
        );
    }

    #[test]
    fn choice_option_may_set_several_flags() {
        let messages = convo(
            "#flag a\n#flag b\n#dialogue d\n\
             \x20   #choice\n\
             \x20   #option Both\n\
             \x20   #set a true\n\
             \x20   #set b false\n\
             \x20   #option Neither",
        );
        assert_eq!(
            messages[0].content,
            vec![ContentDef::Choice(vec![
                ChoiceOptionDef {
                    text: "Both".into(),
                    sets: vec![("a".into(), true), ("b".into(), false)],
                },
                ChoiceOptionDef {
                    text: "Neither".into(),
                    sets: vec![],
                },
            ])],
        );
    }

    #[test]
    fn choice_needs_two_options() {
        // One option → error pointing at the `#choice` header (line 3).
        let err = parse("#flag a\n#dialogue d\n    #choice\n    #option Only\n    #set a true")
            .unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn choice_set_undeclared_flag_errors_at_the_line() {
        let err =
            parse("#dialogue d\n    #choice\n    #option A\n    #set nope true\n    #option B")
                .unwrap_err();
        assert_eq!(err.line, 4);
    }

    #[test]
    fn choice_option_needs_text() {
        let err = parse("#flag a\n#dialogue d\n    #choice\n    #option\n    #option B")
            .unwrap_err();
        assert_eq!(err.line, 4);
    }

    #[test]
    fn choice_takes_no_arguments() {
        let err = parse("#dialogue d\n    #choice now\n    #option A\n    #option B").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn choice_set_before_option_errors() {
        let err =
            parse("#flag a\n#dialogue d\n    #choice\n    #set a true\n    #option A\n    #option B")
                .unwrap_err();
        assert_eq!(err.line, 4);
    }

    #[test]
    fn choice_rejects_stray_lines() {
        let err = parse("#dialogue d\n    #choice\n    #option A\n    loose text\n    #option B")
            .unwrap_err();
        assert_eq!(err.line, 4);
    }
}
