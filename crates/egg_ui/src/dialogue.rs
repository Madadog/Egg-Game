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

use std::fmt::Debug;

use egg_platform::{ConsoleApi, ConsoleHelper};
use egg_render::geometry::Vec2;
use egg_world::data::save::SaveData;
use egg_world::draw_state::{DrawState, LayerId};

use egg_render::{Flip, Font, PrintOptions, SpriteOptions, print_to_with_font, text_width};

use egg_world::data::portraits::Portrait;
use egg_world::data::script::message::{ChoiceOption, Message, PortraitState, TextContent};
use egg_world::data::sound;

/// The dialogue [`PrintOptions`]: defaults plus the caller's small-text setting
/// (the save flag `small_text_on`, passed in now that it's game state).
pub fn print_options(small_text: bool) -> PrintOptions {
    PrintOptions {
        small_text,
        ..Default::default()
    }
}

/// Lower a sequence of [`Message`]s into the forward-order [`TextContent`]
/// queue [`Dialogue::next_text`] pops from (the caller reverses it into the
/// stack). Shared by [`Dialogue::set_messages`], which lowers a whole fetched
/// conversation, and the `TextContent::If` arm of
/// [`Dialogue::consume_text_content`], which lowers just the chosen branch and
/// splices it back in — same rules either way.
///
/// Per message: a message whose content is *exactly one* [`TextContent::If`]
/// item is a branch carrier, not a real page — it pushes just that item, with
/// no `Clear`/`Portrait`/`Flip` (those would visibly clobber the box the
/// instant playback reaches it, even when the branch it resolves to turns out
/// empty) and never a trailing `Pause` (that's decided where the branch is
/// consumed — see the `If` arm — since it depends on what follows the `#end`,
/// which this function alone can't see). It still counts as "a following
/// message" for the *previous* message's own pause rule below, same as any
/// other message would.
///
/// Any other message: a [`Clear`](TextContent::Clear) (the page-break — every
/// message gets one, not just messages after the first, since it's what makes
/// a page's opening text unambiguous regardless of that text's own `#delay`
/// — see its doc), then a `Portrait`/`Flip` item *only* when this message
/// actually sets one (`PortraitState::Clear`/`Set`, `flip_portrait: Some`) —
/// a `Keep` portrait / `None` flip pushes neither, so the box's current
/// portrait/side simply survives untouched, which is exactly what carries it
/// across the page break. Then the message's content, then a `Pause` if
/// `pause_when_done` and it isn't the last message. No pause follows the very
/// last message — the caller (closing the box, or the `If` arm splicing more
/// content after a branch) handles that.
fn lower_messages(messages: &[Message]) -> Vec<TextContent> {
    let mut queue: Vec<TextContent> = Vec::new();
    let last = messages.len().saturating_sub(1);
    for (i, message) in messages.iter().enumerate() {
        if let [TextContent::If { .. }] = message.content.as_slice() {
            queue.extend(message.content.iter().cloned());
            continue;
        }
        queue.push(TextContent::Clear);
        match &message.portrait {
            PortraitState::Keep => {}
            PortraitState::Clear => queue.push(TextContent::Portrait(None)),
            PortraitState::Set(portrait) => {
                queue.push(TextContent::Portrait(Some(portrait.clone())))
            }
        }
        if let Some(flip) = message.flip_portrait {
            queue.push(TextContent::Flip(flip));
        }
        queue.extend(message.content.iter().cloned());
        if message.pause_when_done && i != last {
            queue.push(TextContent::Pause);
        }
    }
    queue
}

/// The portrait/side actually shown for each of `messages`, folding every
/// message's `Keep`/`Clear`/`Set` (and flip's `None`/`Some`) against whatever
/// came before it — the same carry-over rule [`lower_messages`] bakes into
/// the queue for a *live* [`Dialogue`] to apply as it plays. For a caller that
/// instead browses a flat, already-resolved conversation by index without a
/// live widget (the map editor's Dialog panel — see
/// `egg_editor::map::step::resolve_if_carriers`, which flattens `#if`
/// carriers first so every entry here is an ordinary message), this computes
/// the same result eagerly, one `(portrait, flip)` pair per message.
pub fn resolve_portrait_carry(messages: &[Message]) -> Vec<(Option<Portrait>, bool)> {
    let mut portrait: Option<Portrait> = None;
    let mut flip = false;
    messages
        .iter()
        .map(|message| {
            match &message.portrait {
                PortraitState::Keep => {}
                PortraitState::Clear => portrait = None,
                PortraitState::Set(p) => portrait = Some(p.clone()),
            }
            if let Some(f) = message.flip_portrait {
                flip = f;
            }
            (portrait.clone(), flip)
        })
        .collect()
}

/// A live [`TextContent::Choice`]: the options being offered and the highlighted
/// index. Set when the box consumes a `Choice` item and cleared once the player
/// confirms (see [`Dialogue::confirm_choice`]). While it is `Some`, the box is
/// "choosing" — auto-advance is suspended and the driver routes directional
/// input to [`move_choice`](Dialogue::move_choice) instead of the world.
#[derive(Clone, Debug)]
pub struct ChoiceState {
    pub options: Vec<ChoiceOption>,
    pub selected: usize,
}

#[derive(Clone)]
pub struct Dialogue {
    pub current_text: Option<String>,
    pub characters: usize,
    pub next_text: Vec<TextContent>,
    pub width: usize,
    pub delay: usize,
    pub print_time: Option<usize>,
    pub portrait: Option<Portrait>,
    pub dark_theme: bool,
    pub flip_portrait: bool,
    /// The typewriter's pace: frames held between each revealed character —
    /// the `#speed N` directive (`0` is the default, unthrottled per-tick
    /// reveal). Persists across pages within one conversation; only another
    /// `#speed` (or [`Dialogue::close`], which resets everything for a new
    /// conversation) changes it — see [`TextContent::Speed`].
    pub speed: u8,
    /// The open choice menu, if the box is currently awaiting a selection.
    pub choice: Option<ChoiceState>,
    /// A `#shake FRAMES [AMP]` playback just passed, waiting for the world's
    /// camera driver to pick it up (`(frames, amplitude)`). The widget can't
    /// reach the camera itself, so it banks the request here; the walkaround
    /// takes it when it centres the camera. Overwritten (not stacked) if
    /// another fires first; ignored by camera-less hosts of the box.
    pub pending_shake: Option<(u32, i16)>,
}
impl Dialogue {
    pub const fn default() -> Self {
        Self {
            current_text: None,
            next_text: Vec::new(),
            characters: 0,
            width: 200,
            delay: 0,
            print_time: None,
            portrait: None,
            dark_theme: false,
            flip_portrait: false,
            speed: 0,
            choice: None,
            pending_shake: None,
        }
    }
    pub fn with_width(self, width: usize) -> Self {
        Self { width, ..self }
    }
    pub fn is_line_done(&self) -> bool {
        match &self.current_text {
            Some(_) => self.characters >= self.char_count().saturating_sub(1),
            None => true,
        }
    }
    fn set_current_text(&mut self, font: &Font, save: &SaveData, string: &str) {
        self.current_text = Some(self.fit_text(font, save.small_text_on, string));
        self.characters = 0;
        self.print_time = Some(0);
    }
    pub fn maybe_add_text(&mut self, font: &Font, save: &SaveData, string: &'static str) {
        if self.current_text.is_none() {
            self.set_current_text(font, save, string);
        }
    }
    /// Queue a sequence of [`Message`]s (see [`lower_messages`]).
    ///
    /// `save` is threaded through playback (not just for the wrap setting
    /// `small_text_on`): a [`TextContent::SetFlag`] item — authored as `#set` —
    /// writes its named flag the moment it is consumed, and a
    /// [`TextContent::If`] item reads the same live `save` the moment
    /// playback reaches it, so passing `&mut save` is what lets dialogue both
    /// mutate progress as it plays *and* branch on progress set earlier in the
    /// very same conversation.
    pub fn set_messages(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        messages: &[Message],
    ) {
        self.next_text = lower_messages(messages).into_iter().rev().collect();
        self.next_text(system, font, save, false);
    }
    pub fn next_text(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        manual_skip: bool,
    ) -> bool {
        if let Some(text_content) = self.next_text.pop() {
            // trace!(format!("Popping text content: {:?}", text_content), 12);
            let skip = text_content.is_skip();
            let val = self.consume_text_content(system, font, save, text_content, manual_skip);
            if skip {
                self.next_text(system, font, save, manual_skip)
            } else {
                val
            }
        } else {
            false
        }
    }
    pub fn consume_text_content(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        text_content: TextContent,
        manual_skip: bool,
    ) -> bool {
        match text_content {
            // Every text item appends onto the page that's already open — or,
            // if this is the first text since the last `Clear`, opens it (see
            // [`TextContent::Text`]). `delay` holds for that many frames
            // first either way; a manual skip drops the hold, same as it
            // always has for an append.
            TextContent::Text { text, delay, .. } => {
                let wrap_width = self.wrap_width();
                if let Some(string) = &mut self.current_text {
                    string.push_str(&text);
                    *string = fit_default_paragraph(font, string, wrap_width, save.small_text_on);
                } else {
                    self.set_current_text(font, save, &text);
                }
                if delay > 0 && !manual_skip {
                    self.add_delay(delay.into());
                }
                true
            }
            // The page-break: blank the box's revealed text, but leave the
            // portrait/side untouched so they carry into whatever comes next
            // (see [`TextContent::Clear`]). Also drops any stale typewriter
            // hold left over from the page that just ended, so it can't bleed
            // into the next page's pacing.
            TextContent::Clear => {
                self.current_text = None;
                self.characters = 0;
                self.delay = 0;
                self.print_time = None;
                true
            }
            // `#speed N`: set the typewriter's pace for everything revealed
            // from here on — persists until another `#speed` changes it (see
            // [`TextContent::Speed`]).
            TextContent::Speed(n) => {
                self.speed = n;
                true
            }
            TextContent::Delay(x) => {
                if !manual_skip {
                    self.add_delay(x.into());
                }
                true
            }
            TextContent::Sound(x) => {
                system.play_sound(x.clone());
                true
            }
            TextContent::Portrait(x) => {
                if let Some(portrait) = x {
                    self.portrait = Some(portrait.clone());
                } else {
                    self.portrait = None;
                };
                true
            }
            TextContent::Pause => true,
            TextContent::Flip(x) => {
                self.flip_portrait = x;
                true
            }
            // Fire the named flag in place — same timing as a Sound item.
            TextContent::SetFlag(name, value) => {
                save.set_flag(&name, value);
                true
            }
            // Bank the shake for the camera driver — but, being time-flavoured
            // like a `Delay`, it is dropped on a manual fast-forward: skipping
            // a page shouldn't jolt the screen.
            TextContent::Shake { frames, amplitude } => {
                if !manual_skip {
                    self.pending_shake = Some((frames, amplitude));
                }
                true
            }
            // Open the menu and stop: playback blocks here until the driver calls
            // [`confirm_choice`](Self::confirm_choice). `Choice` is neither auto
            // nor skip, so `next_text` returns after consuming it.
            TextContent::Choice(options) => {
                self.choice = Some(ChoiceState {
                    options,
                    selected: 0,
                });
                true
            }
            // The runtime `#if`: pick a branch against the *live* `save` right
            // now, at the moment playback reaches it — not once, up front, when
            // the conversation was fetched. This is what lets a `#choice`/`#set`
            // earlier in the same conversation steer this. Being `is_skip`, the
            // `next_text` recursion flows straight into the spliced branch in
            // the same call, so nothing is displayed for the carrier itself.
            TextContent::If {
                flag,
                then,
                otherwise,
            } => {
                let branch = if save.flag(&flag) { &then } else { &otherwise };
                let mut spliced = lower_messages(branch);
                // The pause separating the branch's last page from whatever
                // follows the `#end` — only when both sides actually exist
                // (mirrors the ordinary between-messages pause rule). One
                // accepted edge case: an `#if` whose chosen branch is empty,
                // sitting at the very end of a conversation, costs one extra
                // (silent, no visible change) advance press versus the old
                // flatten-at-fetch behaviour — the pause after the *preceding*
                // page was already emitted statically and can't be retracted
                // now that we know the branch turned out empty. Everywhere else
                // press counts are identical.
                if !spliced.is_empty()
                    && branch.last().is_some_and(|m| m.pause_when_done)
                    && !self.next_text.is_empty()
                {
                    spliced.push(TextContent::Pause);
                }
                self.next_text.extend(spliced.into_iter().rev());
                true
            }
        }
    }
    pub fn fit_text(&self, font: &Font, small_text: bool, string: &str) -> String {
        fit_default_paragraph(font, string, self.wrap_width(), small_text)
    }
    pub fn wrap_width(&self) -> usize {
        self.width - 3
    }
    pub fn close(&mut self) {
        *self = Self {
            width: self.width,
            ..Self::default()
        };
        self.next_text.shrink_to_fit();
    }
    /// Characters (not bytes) in the current text. [`characters`](Self::characters)
    /// is a char index, so all typewriter bookkeeping uses this count.
    pub fn char_count(&self) -> usize {
        self.current_text.as_ref().map_or(0, |x| x.chars().count())
    }
    pub fn tick(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
        amount: usize,
    ) {
        if let Some(text) = &mut self.current_text {
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
            }
            if let Some(print_time) = &mut self.print_time {
                *print_time += 1;
                if !silent_char && *print_time % 2 == 0 && !self.is_line_done() {
                    system.play_sound(sound::pop());
                }
            }
            self.step_text(amount);
            // `#speed 0` (the default) preserves the ordinary one-frame gap
            // between characters; `#speed N` holds for `N` frames instead —
            // see [`TextContent::Speed`].
            self.delay += self.speed.max(1) as usize;
        }
        if self.is_line_done() && self.can_autoadvance() {
            self.next_text(system, font, save, false);
        }
    }
    pub fn step_text(&mut self, amount: usize) {
        self.characters = (self.characters + amount).min(self.char_count().saturating_sub(1));
    }
    pub fn can_autoadvance(&self) -> bool {
        // A pending choice blocks the queue: don't tick past it into whatever
        // follows until the player has picked.
        if self.choice.is_some() {
            return false;
        }
        if let Some(content) = self.next_text.last() {
            content.is_auto()
        } else {
            false
        }
    }
    pub fn add_delay(&mut self, amount: usize) {
        self.delay = self.delay.saturating_add(amount);
    }
    pub fn skip(&mut self, system: &mut impl ConsoleApi, font: &Font, save: &mut SaveData) {
        while self.can_autoadvance() {
            self.next_text(system, font, save, true);
        }
        self.finish_line();
    }
    /// Jump the typewriter to the last character of the current line, revealing
    /// it all at once.
    pub fn finish_line(&mut self) {
        if self.current_text.is_some() {
            self.characters = self.char_count().saturating_sub(1);
        }
    }
    /// Whether the box is awaiting a choice selection (a `#choice` menu is open).
    /// While true the driver routes directional input to [`move_choice`](Self::move_choice)
    /// and confirm to [`confirm_choice`](Self::confirm_choice), not the world.
    pub fn is_choosing(&self) -> bool {
        self.choice.is_some()
    }
    /// Whether the box is doing anything — showing text, holding queued content,
    /// or waiting on a choice. Callers gate world input / movement on this; a
    /// choice with no prompt has no `current_text`, so it must be counted too.
    pub fn is_active(&self) -> bool {
        self.current_text.is_some() || !self.next_text.is_empty() || self.choice.is_some()
    }
    /// Move the choice highlight by `delta` rows (negative up), wrapping around
    /// the ends. A no-op if no choice is open.
    pub fn move_choice(&mut self, delta: i32) {
        if let Some(choice) = &mut self.choice {
            let len = choice.options.len() as i32;
            if len > 0 {
                let next = (choice.selected as i32 + delta).rem_euclid(len);
                choice.selected = next as usize;
            }
        }
    }
    /// Confirm the highlighted option: write its flags through `save.set_flag`
    /// (the same path as `#set`), clear the menu, and resume playback with the
    /// content that followed the `#choice`. Returns whatever [`next_text`](Self::next_text)
    /// returns (whether a further line opened). A no-op returning `false` if no
    /// choice is open.
    pub fn confirm_choice(
        &mut self,
        system: &mut impl ConsoleApi,
        font: &Font,
        save: &mut SaveData,
    ) -> bool {
        let Some(choice) = self.choice.take() else {
            return false;
        };
        if let Some(option) = choice.options.get(choice.selected) {
            for (name, value) in &option.sets {
                save.set_flag(name, *value);
            }
        }
        self.next_text(system, font, save, false)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_dialogue_portrait(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        font: &Font,
        small_text: bool,
        string: &str,
        timer: bool,
        portrait: i32,
        scale: i32,
        sw: i32,
        sh: i32,
    ) {
        use egg_render::Canvas;
        use egg_world::draw_state::PALETTE_MAP_IDENTITY;
        // Measure the render target, matching `draw_dialogue_box_with_offset` so
        // this variant's portrait stays aligned with the text box on any
        // framebuffer (not just the main window).
        let (screen_w, screen_h) = draw_state.size();

        let w = self.width as i32;
        let h = 24;
        self.draw_dialogue_box_with_offset(
            draw_state, layer, font, small_text, string, timer, 14, -2, 4,
        );
        let rect_fill = draw_state.colour(0);
        let rect_outline = draw_state.colour(3);
        //TODO: flexbox
        draw_state.rgba(layer).outlined_rect(
            (screen_w - w) / 2 - 13,
            (screen_h - h) - 6,
            h + 4,
            h + 4,
            rect_fill,
            rect_outline,
        );
        draw_state.spr(
            layer,
            &PALETTE_MAP_IDENTITY,
            portrait,
            (screen_w - w) / 2 - 13 + 2,
            (screen_h - h) - 6 + 2,
            SpriteOptions {
                scale,
                transparent: Some(0),
                w: sw,
                h: sh,
                ..Default::default()
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_dialogue_box_with_offset(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        // Drawing only measures/prints text, so it takes the loaded [`Font`]
        // directly (not the console) — which lets the text-editor previewer call
        // this from its own `&self` draw, split-borrowing the font and the canvas.
        font: &Font,
        small_text: bool,
        string: &str,
        timer: bool,
        mut x: i32,
        mut y: i32,
        mut height: i32,
    ) {
        use egg_render::Canvas;
        // Measure against the surface being drawn into, not the host's main
        // window: an off-screen render target (an extra editor view) has its own
        // framebuffer size, so the box must re-centre against that to land at the
        // bottom-middle of *that* view. For the main window the two are identical,
        // so in-game rendering is unchanged.
        let (screen_w, screen_h) = draw_state.size();

        let print_timer = self.characters;
        let w = self.width as i32;
        let h = 24;

        let outline_colour = draw_state.colour(if self.dark_theme { 1u8 } else { 3 });
        let bg_colour = draw_state.colour(if self.dark_theme { 1u8 } else { 2 });
        let dark = draw_state.colour(0);
        let darkish = draw_state.colour(1);
        let bright = draw_state.colour(12);

        // Portrait
        if let Some(portrait) = &self.portrait {
            let (pw, flip) = if self.flip_portrait {
                x -= 12;
                (-w, Flip::Horizontal)
            } else {
                x += 14;
                (w, Flip::None)
            };
            y -= 2;
            draw_state.rgba(layer).outlined_rect(
                (screen_w - pw) / 2 - 13,
                (screen_h - h) - 6,
                h + 4,
                h + 4,
                dark,
                outline_colour,
            );
            height += 4;
            crate::portrait::draw_offset(
                portrait,
                draw_state,
                layer,
                Vec2::new(
                    ((screen_w - pw) / 2 - 15) as i16,
                    ((screen_h - h) - 8) as i16,
                ),
                None,
                flip,
            );
            draw_state.rgba(layer).stroke_rect(
                (screen_w - pw) / 2 - 13,
                (screen_h - h) - 6,
                h + 4,
                h + 4,
                outline_colour,
            );
        }
        // Text box
        if self.dark_theme {
            draw_state.rgba(layer).outlined_rect(
                (screen_w - w) / 2 + x - 2,
                (screen_h - h) - 4 + y - 2,
                w + 4,
                h + height + 4,
                darkish,
                dark,
            );
        }
        draw_state.rgba(layer).outlined_rect(
            (screen_w - w) / 2 + x,
            (screen_h - h) - 4 + y,
            w,
            h + height,
            bg_colour,
            outline_colour,
        );
        let options = print_options(small_text);
        let text: &str = if timer {
            revealed(string, print_timer)
        } else {
            string
        };
        print_to_with_font(
            font,
            draw_state.rgba(layer),
            text,
            (screen_w - w) / 2 + 3 + x,
            (screen_h - h) - 4 + 3 + y,
            bright,
            PrintOptions {
                color: 12,
                ..options
            },
        );
    }

    pub fn draw_dialogue_box(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        font: &Font,
        small_text: bool,
        string: &str,
        timer: bool,
    ) {
        self.draw_dialogue_box_with_offset(
            draw_state, layer, font, small_text, string, timer, 0, 0, 0,
        )
    }

    /// Draw the open `#choice` menu as a bordered panel stacked just above the
    /// dialogue box: one option per row, the highlighted one filled in the
    /// bright selection colour (palette #9, the same unmistakable blue the text
    /// editor uses) with a `>` cursor. The panel is as wide as the largest
    /// option (existing padding intent) but never wider than the dialogue box
    /// itself (`self.width`) — an option too wide to fit at that clamped width
    /// wraps onto extra rows instead of overflowing (see [`wrap_choice_options`]),
    /// with continuation rows indented to align under the first row's text and
    /// the selection highlight stretched to cover every row of the option. A
    /// no-op when no choice is open, so the draw site can call it
    /// unconditionally after the box.
    pub fn draw_choice(
        &self,
        draw_state: &mut DrawState,
        layer: LayerId,
        font: &Font,
        small_text: bool,
    ) {
        use egg_render::Canvas;
        let Some(choice) = &self.choice else {
            return;
        };

        let (screen_w, screen_h) = draw_state.size();
        let options = print_options(small_text);
        let (w, wrapped) =
            wrap_choice_options(font, &choice.options, small_text, self.width as i32);
        let box_h = 24;
        let row_h = 8;
        let pad = 3;
        let prefix_w = choice_marker_width(font, options.clone());

        let total_rows: i32 = wrapped.iter().map(|lines| lines.len() as i32).sum();
        let panel_h = total_rows * row_h + pad;
        let x = (screen_w - w) / 2;
        // Sit just above where the dialogue box lands (a 2px gap), so a prompt
        // page and its options read as one stacked unit.
        let box_top = (screen_h - box_h) - 4;
        let y = box_top - panel_h - 2;

        // Resolve colours before the mutable canvas borrow (mirrors the box).
        let bg = draw_state.colour(if self.dark_theme { 1 } else { 2 });
        let outline = draw_state.colour(if self.dark_theme { 1 } else { 3 });
        let text_col = draw_state.colour(12);
        let sel = draw_state.colour(3);

        let canvas = draw_state.rgba(layer);
        canvas.outlined_rect(x, y, w, panel_h, bg, outline);
        let mut row = 0;
        for (i, lines) in wrapped.iter().enumerate() {
            let selected = i == choice.selected;
            let row_y = y + pad + row * row_h;
            if selected {
                // Stretch the highlight over every wrapped row of the option,
                // not just its first.
                canvas.fill_rect(x + 1, row_y - 2, w - 2, lines.len() as i32 * row_h + 1, sel);
            }
            for (j, line) in lines.iter().enumerate() {
                let line_y = row_y + j as i32 * row_h;
                // The `>`/` ` marker only ever sits on an option's first row;
                // continuation rows are indented past where it would be, so
                // their text lines up with the first row's.
                let (text, line_x) = if j == 0 {
                    let marker = if selected { ">" } else { " " };
                    (format!("{marker} {line}"), x + pad)
                } else {
                    (line.clone(), x + pad + prefix_w)
                };
                print_to_with_font(
                    font,
                    canvas,
                    &text,
                    line_x,
                    line_y,
                    text_col,
                    PrintOptions {
                        color: 12,
                        ..options.clone()
                    },
                );
            }
            row += lines.len() as i32;
        }
    }
}

/// Width of the `"> "` marker prefix drawn before every option's first row
/// (and reserved as the indent before every continuation row). The selected
/// `">"` and unselected `" "` markers are both a single glyph before the
/// space, so one measurement covers both.
fn choice_marker_width(font: &Font, options: PrintOptions) -> i32 {
    text_width(font, "> ", options)
}

/// Wrap each `#choice` option to fit a panel no wider than `max_width`,
/// returning that panel's width alongside every option's rendered lines (one
/// entry per option, one `String` per row).
///
/// The *unclamped* width is still the widest `"> option <"` — the existing
/// padding intent — but capped to `max_width` so the panel drawn in
/// [`Dialogue::draw_choice`] never overruns the dialogue box it sits above.
/// Only an option that can't fit inside the clamped width on one line breaks
/// onto more, via [`fit_default_paragraph`]; short options that already fit
/// come back as a single untouched line, so the common case (all short
/// options) renders exactly as before. The wrap width reserves the panel's
/// left padding and the `"> "` marker prefix so wrapped text can't run past
/// the panel border.
fn wrap_choice_options(
    font: &Font,
    options: &[ChoiceOption],
    small_text: bool,
    max_width: i32,
) -> (i32, Vec<Vec<String>>) {
    let print_opts = print_options(small_text);
    const PAD: i32 = 3;

    let unclamped_w = options
        .iter()
        .map(|x| text_width(font, &format!("> {} <", x.text), print_opts.clone()))
        .max()
        .unwrap_or_default()
        + 4;
    let w = unclamped_w.min(max_width);

    let wrap_width = (w - PAD - choice_marker_width(font, print_opts.clone())).max(1) as usize;

    let lines = options
        .iter()
        .map(|opt| {
            if text_width(font, &opt.text, print_opts.clone()) <= wrap_width as i32 {
                vec![opt.text.clone()]
            } else {
                fit_default_paragraph(font, &opt.text, wrap_width, small_text)
                    .lines()
                    .map(str::to_string)
                    .collect()
            }
        })
        .collect();

    (w, lines)
}

impl Debug for Dialogue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialogue")
            .field("current_text", &self.current_text)
            .field("characters", &self.characters)
            .field("next_text", &self.next_text.len())
            .field("width", &self.width)
            .field("delay", &self.delay)
            .field("print_time", &self.print_time)
            .field("portrait", &self.portrait)
            .field("speed", &self.speed)
            .field("choice", &self.choice)
            .finish()
    }
}

pub fn print_width(font: &Font, string: &str, fixed: bool, small_font: bool) -> i32 {
    egg_render::text_width(
        font,
        string,
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

/// The prefix of `text` containing characters `0..=chars` (the whole string if
/// `chars` runs past the end). The typewriter reveal indexes *characters*, so
/// slicing bytes would split — and panic on — multibyte text.
pub fn revealed(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars.saturating_add(1))
        .map_or(text, |(i, _)| &text[..i])
}

/// Clamps a string to the specified width (with the TIC-80 font). Returns a string,
/// the number of fitting words, and a bool for if the whole string fit.
pub fn fit_string(
    font: &Font,
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
        if print_width(font, taken, fixed, small_font) as usize > wrap_width {
            break;
        } else {
            line_length = i
        };
    }
    // Always make progress: when even the first word is wider than `wrap_width`,
    // still take that one word (it overflows the line) so the caller's `skip`
    // advances. Otherwise `fit_paragraph` would `skip += 0` forever, pushing a
    // newline each pass until it exhausts memory.
    if line_length == 0 && len > 0 {
        line_length = 1;
    }
    (
        take_words(string, line_length, start_word),
        line_length,
        line_length == len,
    )
}

pub fn fit_paragraph(
    font: &Font,
    string: &str,
    wrap_width: usize,
    fixed: bool,
    small_font: bool,
) -> String {
    let len = string.split_inclusive(' ').count();
    let mut paragraph = String::new();
    let mut skip = 0;
    while skip < len {
        let (string, x, all_fits) = fit_string(font, string, wrap_width, skip, fixed, small_font);
        skip += x;
        paragraph.push_str(&string);
        if all_fits {
            return paragraph;
        }
        paragraph.push('\n');
    }
    paragraph
}

/// Wrap `string` to `wrap_width` using the caller's small-text setting (the
/// save flag `small_text_on`, passed in now that persistence is game state).
pub fn fit_default_paragraph(
    font: &Font,
    string: &str,
    wrap_width: usize,
    small_text: bool,
) -> String {
    fit_paragraph(font, string, wrap_width, false, small_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egg_platform::NullConsole;

    /// A `#shake` item banks its request for the camera driver as it is played
    /// past — but a manual fast-forward drops it (time-flavoured, like a
    /// `Delay`): skipping a page shouldn't jolt the screen.
    #[test]
    fn shake_banks_unless_manually_skipped() {
        let mut dialogue = Dialogue::default();
        let mut save = SaveData::default();
        let font = egg_render::Font::blank();
        let shake = TextContent::Shake {
            frames: 30,
            amplitude: 4,
        };
        assert!(shake.is_skip(), "fires in place like a sound");

        let consumed = dialogue.consume_text_content(
            &mut NullConsole::new(),
            &font,
            &mut save,
            shake.clone(),
            false,
        );
        assert!(consumed);
        assert_eq!(dialogue.pending_shake, Some((30, 4)));

        // Manual skip: the request is dropped, not banked.
        dialogue.pending_shake = None;
        dialogue.consume_text_content(&mut NullConsole::new(), &font, &mut save, shake, true);
        assert_eq!(dialogue.pending_shake, None);
    }

    /// A dialogue whose live line is `text`, bypassing the wrap/fit path
    /// (which needs a console to measure glyphs).
    fn with_text(text: &str) -> Dialogue {
        Dialogue {
            current_text: Some(text.to_string()),
            ..Dialogue::default()
        }
    }

    #[test]
    fn revealed_is_char_indexed() {
        let text = "café né?"; // 8 chars, 10 bytes
        assert_eq!(revealed(text, 0), "c");
        assert_eq!(revealed(text, 3), "café");
        assert_eq!(revealed(text, 6), "café né");
        assert_eq!(revealed(text, 7), text);
        assert_eq!(revealed(text, 100), text);
        assert_eq!(revealed("", 0), "");
    }

    #[test]
    fn is_line_done_counts_chars_not_bytes() {
        let mut dialogue = with_text("café né?");
        dialogue.characters = 6;
        assert!(!dialogue.is_line_done());
        dialogue.characters = 7;
        assert!(dialogue.is_line_done());
        assert!(with_text("").is_line_done());
        assert!(Dialogue::default().is_line_done());
    }

    #[test]
    fn step_text_clamps_to_last_char() {
        let mut dialogue = with_text("né");
        dialogue.step_text(10);
        assert_eq!(dialogue.characters, 1);
        // An empty line must not underflow the clamp.
        let mut empty = with_text("");
        empty.step_text(1);
        assert_eq!(empty.characters, 0);
    }

    #[test]
    fn finish_line_lands_on_last_char() {
        let mut dialogue = with_text("café né?");
        dialogue.finish_line();
        assert_eq!(dialogue.characters, 7);
        assert!(dialogue.is_line_done());
        let mut empty = with_text("");
        empty.finish_line();
        assert_eq!(empty.characters, 0);
    }

    #[test]
    fn choice_menu_moves_wraps_confirms_and_sets_the_flag() {
        use egg_platform::NullConsole;
        use egg_world::data::script::message::ChoiceOption;

        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        let options = vec![
            ChoiceOption {
                text: "Tea".into(),
                sets: vec![("chose_tea".into(), true)],
            },
            ChoiceOption {
                text: "Coffee".into(),
                sets: vec![("chose_coffee".into(), true)],
            },
            ChoiceOption {
                text: "Nothing".into(),
                sets: vec![],
            },
        ];
        let messages = vec![Message::default().with_content(vec![
            TextContent::text("What'll it be?"),
            TextContent::Choice(options),
        ])];
        d.set_messages(&mut console, &font, &mut save, &messages);

        // The prompt page shows first; the box is active but not yet choosing.
        assert!(d.is_active());
        assert!(!d.is_choosing());
        // Advancing past the prompt opens the menu at the first option.
        d.next_text(&mut console, &font, &mut save, false);
        assert!(d.is_choosing());
        assert_eq!(d.choice.as_ref().unwrap().selected, 0);
        // Up from the top wraps to the last option; down wraps back.
        d.move_choice(-1);
        assert_eq!(d.choice.as_ref().unwrap().selected, 2);
        d.move_choice(1);
        assert_eq!(d.choice.as_ref().unwrap().selected, 0);
        // Highlight "Coffee" and confirm: it writes that option's flag, clears
        // the menu, and (nothing followed) leaves the box ready to close.
        d.move_choice(1);
        let opened = d.confirm_choice(&mut console, &font, &mut save);
        assert!(!opened, "no content followed the choice");
        assert!(!d.is_choosing());
        assert!(save.flag("chose_coffee"));
        assert!(!save.flag("chose_tea"));
    }

    #[test]
    fn a_pending_choice_blocks_auto_advance() {
        use egg_world::data::script::message::ChoiceOption;
        // Even with auto-text queued after it, the box must not tick past an open
        // choice.
        let mut d = Dialogue {
            current_text: Some("prompt".into()),
            choice: Some(ChoiceState {
                options: vec![ChoiceOption {
                    text: "ok".into(),
                    sets: vec![],
                }],
                selected: 0,
            }),
            next_text: vec![TextContent::auto("after")],
            ..Dialogue::default()
        };
        assert!(!d.can_autoadvance());
        // Clearing the choice lets the queued auto-text advance again.
        d.choice = None;
        assert!(d.can_autoadvance());
    }

    #[test]
    fn choice_wrap_clamps_panel_width_and_wraps_an_overlong_option() {
        // Every glyph forced to 8px (same trick as the test below), so
        // wrapping is driven purely by character count, not font metrics.
        let mut font = Font::blank();
        font.image_mut().data_mut().fill(255);
        font.refresh();

        let long_text = "A very long option that must wrap";
        let options = vec![
            ChoiceOption {
                text: "Hi".into(),
                sets: vec![],
            },
            ChoiceOption {
                text: long_text.into(),
                sets: vec![],
            },
        ];

        // Unclamped, the long option would need a much wider panel than 60px
        // — the clamp must kick in.
        let (w, lines) = wrap_choice_options(&font, &options, false, 60);
        assert!(w <= 60, "panel width {w} exceeded the 60px clamp");

        // The short option already fits: it comes back untouched as a single
        // row, so it renders exactly as it did before wrapping existed.
        assert_eq!(lines[0], vec!["Hi".to_string()]);

        // The long option can't fit the clamped panel on one line, so it must
        // break onto more than one row instead of overflowing.
        assert!(
            lines[1].len() > 1,
            "expected the long option to wrap, got {:?}",
            lines[1]
        );
        // Wrapping must not drop any words.
        let rejoined = lines[1].join(" ");
        for word in long_text.split_whitespace() {
            assert!(
                rejoined.contains(word),
                "lost word {word:?} while wrapping: {rejoined:?}"
            );
        }
    }

    #[test]
    fn fit_paragraph_terminates_on_an_over_wide_word() {
        // A fully-opaque atlas makes every glyph 8px wide, so even a single
        // character is wider than this 4px wrap width — the case that used to
        // spin `fit_paragraph` forever (`skip += 0`). It must return promptly and
        // still emit both words (each on its own overflowing line).
        let mut font = Font::blank();
        font.image_mut().data_mut().fill(255);
        font.refresh();
        let out = fit_paragraph(&font, "AAA BBB", 4, false, false);
        assert!(out.contains("AAA"), "got {out:?}");
        assert!(out.contains("BBB"), "got {out:?}");
    }

    // --- runtime `#if` (a `#choice`/`#set` earlier in a conversation steering
    // a later `#if` in the very same conversation, resolved live as playback
    // reaches it — see `TextContent::If` and `lower_messages`) ---

    /// Parse `.eggtext` `src` and resolve dialogue key `key` from it, against
    /// the built-in portraits — same route as `egg_world`'s own script tests
    /// (`crate::data::script::mod::tests::script`), just reachable from here as
    /// a normal dependency rather than an in-crate helper.
    fn dialogue_from(src: &str, key: &str) -> Vec<Message> {
        use egg_world::data::portraits::Portraits;
        use egg_world::data::script::{Script, eggtext};
        let mut script = Script::new();
        script.set_base(
            eggtext::parse(src).expect("parse eggtext"),
            &Portraits::builtin(),
        );
        script.get_dialogue(key)
    }

    /// Advance playback past a pause to the next real content, looping
    /// `next_text` until `current_text` changes, a choice opens, or the queue
    /// empties — a lone `Pause` (or an `If` resolving to an empty branch) is
    /// consumed without visibly changing anything, so one driver "press" can
    /// take more than one `next_text` call to land on the next real page. Must
    /// stop the instant a choice opens rather than looping past it: unlike
    /// production (which gates every `next_text` call on `!is_choosing()`),
    /// nothing here would otherwise stop `next_text` from popping straight
    /// through an open menu.
    ///
    /// Calls [`Dialogue::finish_line`] first, same as the real driver only
    /// calling `next_text` once `is_line_done()` — without it, a `Text` item
    /// reached mid-typewriter would just get requeued by
    /// [`Dialogue::add_text`] rather than shown (`current_text` is still
    /// `Some` and not yet "done").
    fn advance(d: &mut Dialogue, console: &mut impl ConsoleApi, font: &Font, save: &mut SaveData) -> bool {
        d.finish_line();
        let before = d.current_text.clone();
        loop {
            if !d.next_text(console, font, save, false) {
                return false;
            }
            if d.is_choosing() || d.current_text != before {
                return true;
            }
        }
    }

    // --- text-flow + portrait carry-over (a message with no `#pic`/`#flip`
    // carries whatever the box was already showing across the page break —
    // see `TextContent::Clear` and `lower_messages`) ---

    /// A message that never mentions `#pic` doesn't clear the portrait at the
    /// page break — it carries over from the message before it.
    #[test]
    fn portrait_carries_across_a_keep_message() {
        let messages = dialogue_from("#dialogue d\n    #pic y_normal\n    Hi.\n\n    Bye.", "d");
        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        d.set_messages(&mut console, &font, &mut save, &messages);
        assert_eq!(d.current_text.as_deref(), Some("Hi."));
        assert!(d.portrait.is_some(), "the first message's #pic shows a portrait");
        let first_portrait = d.portrait.clone();

        advance(&mut d, &mut console, &font, &mut save);
        assert_eq!(d.current_text.as_deref(), Some("Bye."));
        assert_eq!(
            d.portrait, first_portrait,
            "the second message mentions no #pic, so the portrait carries over"
        );
    }

    /// `#pic none` explicitly clears the carried portrait back to narration —
    /// distinct from a message that simply never mentions `#pic` (which would
    /// carry it, per the test above).
    #[test]
    fn pic_none_clears_the_carried_portrait() {
        let messages =
            dialogue_from("#dialogue d\n    #pic y_normal\n    Hi.\n\n    #pic none\n    Bye.", "d");
        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        d.set_messages(&mut console, &font, &mut save, &messages);
        assert!(d.portrait.is_some());

        advance(&mut d, &mut console, &font, &mut save);
        assert_eq!(d.current_text.as_deref(), Some("Bye."));
        assert!(d.portrait.is_none(), "#pic none explicitly clears back to narration");
    }

    /// The portrait's flip side carries over the same way the portrait itself
    /// does: a message that mentions neither `#pic` nor `#flip` keeps
    /// whatever side was already showing.
    #[test]
    fn flip_side_carries_across_a_keep_message() {
        let messages = dialogue_from(
            "#dialogue d\n    #pic y_normal\n    #flip true\n    Hi.\n\n    Bye.",
            "d",
        );
        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        d.set_messages(&mut console, &font, &mut save, &messages);
        assert!(d.flip_portrait, "the first message's #flip true takes effect");

        advance(&mut d, &mut console, &font, &mut save);
        assert_eq!(d.current_text.as_deref(), Some("Bye."));
        assert!(
            d.flip_portrait,
            "the second message mentions neither #pic nor #flip, so the side carries over"
        );
    }

    /// A message that re-declares the *same* portrait (no new `#flip`) is a
    /// no-op transition, not a flicker back to narration and a fresh side —
    /// portrait and flip are independent axes, so redeclaring one doesn't
    /// reset the other.
    #[test]
    fn redeclaring_the_same_portrait_does_not_reset_the_flip_side() {
        let messages = dialogue_from(
            "#dialogue d\n    #pic y_normal\n    #flip true\n    Hi.\n\n    #pic y_normal\n    Bye.",
            "d",
        );
        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        d.set_messages(&mut console, &font, &mut save, &messages);
        assert!(d.flip_portrait);
        let first_portrait = d.portrait.clone();

        advance(&mut d, &mut console, &font, &mut save);
        assert_eq!(d.current_text.as_deref(), Some("Bye."));
        assert_eq!(
            d.portrait, first_portrait,
            "still the same portrait — no flicker to narration in between"
        );
        assert!(d.flip_portrait, "flip side survives a redundant #pic");
    }

    // --- `#speed N` typewriter pacing ---

    /// `#speed N` holds `N` frames between each revealed character — slower
    /// than the default pace — but a manual skip still completes the reveal
    /// instantly regardless, same as it always has for `#delay`.
    #[test]
    fn speed_paces_reveal_and_skip_still_completes_instantly() {
        let messages = dialogue_from("#dialogue d\n    #speed 5\n    Hi.", "d");
        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();

        d.set_messages(&mut console, &font, &mut save, &messages);
        assert_eq!(d.speed, 5, "the #speed directive set the widget's pace");
        assert_eq!(d.characters, 0);

        // 3 (of the 5 held) frames isn't enough to reveal the second
        // character yet.
        for _ in 0..3 {
            d.tick(&mut console, &font, &mut save, 1);
        }
        assert_eq!(d.characters, 1, "still holding on the first character");

        d.skip(&mut console, &font, &mut save);
        assert!(d.is_line_done());
        assert_eq!(d.characters, 2, "skip reveals the rest of the line instantly");
    }

    /// The motivating bug this whole runtime-`#if` change fixes (mirrors
    /// `debug_portrait2` in `assets/script/en.eggtext`): a `#choice` sets a
    /// flag, and later in the *same* conversation an `#if` branches on it.
    /// Before this change, `#if` was flattened once when the conversation was
    /// fetched — before the choice had been made — so it always saw the flag's
    /// value from *before* this conversation started. Now it's picked live, at
    /// playback time, so it sees whatever the player just chose.
    #[test]
    fn a_choice_earlier_in_a_conversation_steers_a_later_if_in_it() {
        let src = "#flag flag_set\n\
                    #dialogue conv\n\
                    \x20   Hi!\n\
                    \x20   #choice\n\
                    \x20   #option Large\n\
                    \x20   #set flag_set true\n\
                    \x20   #option Small\n\
                    \x20   #set flag_set false\n\
                    \n\
                    \x20   Hmm...\n\
                    \n\
                    \x20   #if flag_set\n\
                    \x20   Big branch.\n\
                    \x20   #else\n\
                    \x20   Small branch.\n\
                    \x20   #end";
        let messages = dialogue_from(src, "conv");

        // Play the whole conversation once per picked option, asserting which
        // branch text the box ends up showing.
        let play = |pick: usize| -> String {
            let mut console = NullConsole::new();
            let font = Font::blank();
            let mut save = SaveData::default();
            let mut d = Dialogue::default();

            d.set_messages(&mut console, &font, &mut save, &messages);
            assert_eq!(d.current_text.as_deref(), Some("Hi!"));

            advance(&mut d, &mut console, &font, &mut save);
            assert!(d.is_choosing(), "the choice opens next");
            d.choice.as_mut().unwrap().selected = pick;
            d.confirm_choice(&mut console, &font, &mut save);

            advance(&mut d, &mut console, &font, &mut save);
            assert_eq!(d.current_text.as_deref(), Some("Hmm..."), "the page between the choice and the #if");

            advance(&mut d, &mut console, &font, &mut save);
            d.current_text.clone().unwrap_or_default()
        };

        assert_eq!(play(0), "Big branch.", "picking option 0 sets flag_set true");
        assert_eq!(play(1), "Small branch.", "picking option 1 sets flag_set false");
    }

    /// An `#if` whose chosen branch is empty, sitting mid-conversation, must
    /// not cost an extra press: the branch contributes nothing, so the very
    /// next `next_text` call after the one that eats the preceding pause lands
    /// straight on the following message — not a further no-op call.
    #[test]
    fn an_if_with_an_empty_branch_flows_into_the_next_message_without_an_extra_press() {
        let src = "#flag unset_flag\n\
                    #dialogue conv\n\
                    \x20   First.\n\
                    \n\
                    \x20   #if unset_flag\n\
                    \x20   Never shown.\n\
                    \x20   #end\n\
                    \n\
                    \x20   Second.";
        let messages = dialogue_from(src, "conv");

        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();
        d.set_messages(&mut console, &font, &mut save, &messages);
        assert_eq!(d.current_text.as_deref(), Some("First."));

        // `finish_line` before each call mirrors the real driver only calling
        // `next_text` once `is_line_done()` — otherwise `add_text` would just
        // requeue the next `Text` item instead of showing it.
        //
        // One press eats the pause after "First." — no visible change yet.
        d.finish_line();
        assert!(d.next_text(&mut console, &font, &mut save, false));
        assert_eq!(d.current_text.as_deref(), Some("First."));

        // The very next press resolves the (empty) branch and lands straight
        // on "Second." — proof there's no second, spurious pause in between.
        d.finish_line();
        assert!(d.next_text(&mut console, &font, &mut save, false));
        assert_eq!(d.current_text.as_deref(), Some("Second."));
    }

    /// A chosen (non-empty) branch followed by more conversation gets exactly
    /// one `Pause` between its last page and the following page — the same
    /// single silent press any ordinary paused message transition costs, no
    /// more.
    #[test]
    fn a_chosen_branch_gets_exactly_one_pause_before_the_following_message() {
        let src = "#flag set_flag\n\
                    #dialogue conv\n\
                    \x20   #if set_flag\n\
                    \x20   Branch page.\n\
                    \x20   #end\n\
                    \n\
                    \x20   Outro.";
        let messages = dialogue_from(src, "conv");

        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        save.set_flag("set_flag", true);
        let mut d = Dialogue::default();
        d.set_messages(&mut console, &font, &mut save, &messages);
        assert_eq!(d.current_text.as_deref(), Some("Branch page."));

        // The one pause after the branch's last page — no visible change.
        d.finish_line();
        assert!(d.next_text(&mut console, &font, &mut save, false));
        assert_eq!(d.current_text.as_deref(), Some("Branch page."));
        // Immediately followed by "Outro." — not a second no-op call.
        d.finish_line();
        assert!(d.next_text(&mut console, &font, &mut save, false));
        assert_eq!(d.current_text.as_deref(), Some("Outro."));
        // Nothing left.
        d.finish_line();
        assert!(!d.next_text(&mut console, &font, &mut save, false));
    }

    /// An `#if`/`#elif`/`#else` chain still resolves to one carrier and picks
    /// the first matching branch live — proof the nested-carrier resolution
    /// (`SegmentDef::resolve` in `egg_world`) plays back with no runtime
    /// changes: the `#if` wins even when a later `#elif` would also match.
    #[test]
    fn an_elif_chain_picks_the_first_matching_branch_live() {
        let src = "#flag a\n#flag b\n\
                    #dialogue conv\n\
                    \x20   #if a\n\
                    \x20   A branch.\n\
                    \x20   #elif b\n\
                    \x20   B branch.\n\
                    \x20   #else\n\
                    \x20   Neither.\n\
                    \x20   #end";
        let messages = dialogue_from(src, "conv");

        let play = |a: bool, b: bool| -> String {
            let mut console = NullConsole::new();
            let font = Font::blank();
            let mut save = SaveData::default();
            save.set_flag("a", a);
            save.set_flag("b", b);
            let mut d = Dialogue::default();
            d.set_messages(&mut console, &font, &mut save, &messages);
            d.current_text.clone().unwrap_or_default()
        };

        assert_eq!(play(true, false), "A branch.");
        assert_eq!(play(true, true), "A branch.", "the #if wins even when the #elif also matches");
        assert_eq!(play(false, true), "B branch.");
        assert_eq!(play(false, false), "Neither.");
    }

    /// A `#set` (not just a `#choice`) earlier in a conversation also drives a
    /// later `#if` in that same conversation — the `#set`/`#if` pairing goes
    /// through the same live-`save` mechanism as the choice case, just without
    /// the interactive menu.
    #[test]
    fn a_set_earlier_in_a_conversation_drives_a_later_if_in_it() {
        let src = "#flag flag_x\n\
                    #dialogue conv\n\
                    \x20   #set flag_x true\n\
                    \x20   Intro.\n\
                    \n\
                    \x20   #if flag_x\n\
                    \x20   Yes.\n\
                    \x20   #else\n\
                    \x20   No.\n\
                    \x20   #end";
        let messages = dialogue_from(src, "conv");

        let mut console = NullConsole::new();
        let font = Font::blank();
        let mut save = SaveData::default();
        let mut d = Dialogue::default();
        d.set_messages(&mut console, &font, &mut save, &messages);
        assert_eq!(d.current_text.as_deref(), Some("Intro."));
        assert!(save.flag("flag_x"), "the #set fired immediately, during its own message");

        advance(&mut d, &mut console, &font, &mut save);
        assert_eq!(d.current_text.as_deref(), Some("Yes."));
    }
}
