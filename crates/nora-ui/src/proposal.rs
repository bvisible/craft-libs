// //// Neoffice — added file (no upstream equivalent): the card of a proposal of Nora's, with its fields to fill.

//! The card of a [`Proposal`]: NeoMail's ready replies under a message being read, and any app's text that Nora
//! prepared for the person to send.
//!
//! The card only draws and keeps what the person did on it (which reply, the values of the fields, a send that waits
//! its five seconds). It hands back what they decided as [`ProposalAction`]s; the app writes the reply, opens it, or
//! sends it, its own way. Nothing here sends anything.
//!
//! What the card shows: a band (Nora's mark, the heading, one chip per short reply, a link to the person's style), and
//! in it the reply chosen (the draft by default), addressed, with its buttons: Dismiss, Edit, Send. A field Nora could
//! not fill is a pill that says what goes there; a click (or Tab) opens a strip under the text to fill it: shortcuts
//! the app gives for its kind (dates), a box for anything else, Enter to keep it and go to the next one, Escape to
//! close. Send stays off while a field of the reply is empty, and once clicked waits [`SEND_DELAY_S`] seconds, with a
//! way back, before the card hands the text over.
//!
//! Everything the person reads is in [`ProposalWords`], the colours in [`ProposalTone`]: the app passes its own.

use std::time::Duration;

use craft_nora::proposal::{self, FieldKind, Piece, Proposal, Reply};
use egui::{Align, Button, Color32, CornerRadius, Frame, Key, Label, Layout, Margin, Painter, Rect, RichText, Sense, Stroke, TextEdit, Ui, vec2};

/// How long a send waits for the person to change their mind, in seconds.
pub const SEND_DELAY_S: f64 = 5.0;

/// What the card says. The app fills in its own language.
#[derive(Debug, Clone)]
pub struct ProposalWords {
    /// At the head of the band: « Ready replies ».
    pub heading: String,
    /// Over the reply: « Nora's draft ».
    pub draft: String,
    /// Before the address the reply goes to: « to ».
    pub to: String,
    /// How many fields of the reply are still empty.
    pub fields_left: fn(usize) -> String,
    pub send: String,
    pub edit: String,
    pub dismiss: String,
    /// While a send waits: « Sending in {n} s… ».
    pub sending: fn(u32) -> String,
    pub cancel: String,
    /// Under the reply: the signature and the quoted message are added on sending.
    pub footer: String,
    /// The box of an open field, when its shortcuts are not enough.
    pub other_value: String,
    pub keep: String,
    /// Under an open field: what Enter, Tab and Escape do.
    pub keys: String,
    /// The link to the person's style.
    pub style_link: String,
    /// Said while Nora learns the person's style (the first replies are in a plain style).
    pub learning: String,
}

impl Default for ProposalWords {
    fn default() -> Self {
        ProposalWords {
            heading: "Ready replies".into(),
            draft: "NORA'S DRAFT".into(),
            to: "to".into(),
            fields_left: |n| if n == 1 { "1 field to fill in".into() } else { format!("{n} fields to fill in") },
            send: "Send".into(),
            edit: "Edit".into(),
            dismiss: "Dismiss".into(),
            sending: |n| format!("Sending in {n} s…"),
            cancel: "Cancel".into(),
            footer: "Your signature and the quoted message are added when it is sent.".into(),
            other_value: "Something else".into(),
            keep: "OK".into(),
            keys: "Enter: keep · Tab: next field · Escape: close".into(),
            style_link: "In your style · My style".into(),
            learning: "Nora is learning your style from your sent emails.".into(),
        }
    }
}

/// The card's colours: Nora's (the band, the pills) and the window's (the reply, its Send button).
#[derive(Debug, Clone, Copy)]
pub struct ProposalTone {
    /// Nora's strong colour: her mark, a pill's border, a filled value's underline.
    pub accent: Color32,
    /// Nora's words.
    pub text: Color32,
    pub fill: Color32,
    pub hover: Color32,
    pub border: Color32,
    /// The reply's own frame and words.
    pub card: Color32,
    pub card_border: Color32,
    pub body: Color32,
    pub dim: Color32,
    /// The window's Send button.
    pub send_fill: Color32,
    pub send_text: Color32,
    /// Draws Nora's mark in a rectangle; none: a dot of the accent.
    pub mark: Option<fn(&Painter, Rect, Color32)>,
}

impl ProposalTone {
    /// Colours from the egui theme, for an app that has none of its own.
    pub fn from_visuals(visuals: &egui::Visuals) -> Self {
        ProposalTone {
            accent: visuals.hyperlink_color,
            text: visuals.text_color(),
            fill: visuals.faint_bg_color,
            hover: visuals.widgets.hovered.weak_bg_fill,
            border: visuals.widgets.noninteractive.bg_stroke.color,
            card: visuals.panel_fill,
            card_border: visuals.widgets.noninteractive.bg_stroke.color,
            body: visuals.text_color(),
            dim: visuals.weak_text_color(),
            send_fill: visuals.selection.bg_fill,
            send_text: visuals.selection.stroke.color,
            mark: None,
        }
    }
}

/// What the person decided on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalAction {
    /// Send this text (every field of the reply filled), after the wait.
    Send(String),
    /// Open it to write on: an empty field is a marker in it ([`proposal::marker`]).
    Edit(String),
    /// Leave it.
    Dismiss,
    /// Show the person's style.
    OpenStyle,
}

/// A shortcut for a field: what its button says, and what goes into the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub shown: String,
    pub value: String,
}

/// What the card shows besides the proposal.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    /// The address the reply goes to.
    pub to: &'a str,
    /// The shortcuts of a field of a kind (the dates the app offers).
    pub choices: &'a dyn Fn(FieldKind) -> Vec<Choice>,
    /// Nora is learning the person's style: the first replies are in a plain style.
    pub learning: bool,
}

/// What the person did on one proposal. A new proposal (another message) starts from [`ProposalCard::default`].
#[derive(Debug, Default, Clone)]
pub struct ProposalCard {
    /// The short reply shown in place of the draft.
    pub chosen: Option<usize>,
    /// The value of each field, by its number; empty: not filled.
    pub values: Vec<String>,
    /// The field whose strip is open.
    pub open: Option<usize>,
    typed: String,
    focus: bool,
    /// When the send that waits goes (egui time, seconds).
    send_at: Option<f64>,
}

impl ProposalCard {
    /// The reply shown: the short reply chosen, else the draft.
    pub fn reply<'a>(&self, proposal: &'a Proposal) -> &'a Reply {
        self.chosen.and_then(|i| proposal.short.get(i)).unwrap_or(&proposal.draft)
    }

    /// Whether a send waits its delay.
    pub fn sending(&self) -> bool {
        self.send_at.is_some()
    }

    /// Opens `field` to be filled, its current value in the box.
    pub fn open_field(&mut self, field: usize) {
        self.open = Some(field);
        self.typed = self.values.get(field).cloned().unwrap_or_default();
        self.focus = true;
    }

    /// Keeps `value` for the open field, and opens the next empty one of the reply (or closes).
    fn keep(&mut self, proposal: &Proposal, value: String) {
        let Some(field) = self.open else { return };
        let kind = proposal.fields.get(field).map_or(FieldKind::Text, |f| f.kind);
        let value: String = value.trim().chars().take(proposal::MAX_VALUE_CHARS).collect();
        let value = match kind {
            FieldKind::Amount => proposal::amount_words(&value).unwrap_or(value),
            _ => value,
        };
        if let Some(slot) = self.values.get_mut(field) {
            *slot = value;
        }
        self.next(proposal, field);
    }

    /// Opens the first empty field of the reply after `after` (then before it), or closes.
    fn next(&mut self, proposal: &Proposal, after: usize) {
        let reply = self.reply(proposal);
        let fields = reply.fields();
        let missing = proposal.missing(reply, &self.values);
        let at = fields.iter().position(|f| *f == after).unwrap_or(0);
        let next = fields.iter().skip(at + 1).chain(fields.iter().take(at)).find(|f| missing.contains(f)).copied();
        match next {
            Some(field) => self.open_field(field),
            None => {
                self.open = None;
                self.typed.clear();
            }
        }
    }

    /// Draws the card for `proposal`, as `context` says (to whom, the shortcuts of a field, whether Nora is learning the
    /// person's style). Returns what the person decided this frame.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        proposal: &Proposal,
        words: &ProposalWords,
        tone: &ProposalTone,
        context: &Context<'_>,
    ) -> Vec<ProposalAction> {
        let Context { to, choices, learning } = *context;
        let mut actions = Vec::new();
        self.values.resize(proposal.fields.len(), String::new());
        if self.chosen.is_some_and(|i| i >= proposal.short.len()) {
            self.chosen = None;
        }
        let id = ui.make_persistent_id("nora-proposal");
        Frame::new()
            .fill(tone.fill)
            .stroke(Stroke::new(1.0, tone.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                self.band(ui, proposal, words, tone, &mut actions);
                if learning {
                    ui.add(Label::new(RichText::new(words.learning.as_str()).size(12.0).color(tone.text)).wrap());
                }
                Frame::new()
                    .fill(tone.card)
                    .stroke(Stroke::new(1.0, tone.card_border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                        self.head(ui, proposal, words, tone, to, &mut actions);
                        self.body(ui, proposal, tone);
                        if self.open.is_some() {
                            self.strip(ui, id, proposal, words, tone, choices);
                        }
                        ui.add(Label::new(RichText::new(words.footer.as_str()).size(12.0).color(tone.dim)).wrap());
                    });
            });
        // A send that waits goes when its time is up, unless the person took it back meanwhile.
        if let Some(at) = self.send_at {
            let now = ui.input(|i| i.time);
            let reply = self.reply(proposal);
            if !proposal.missing(reply, &self.values).is_empty() {
                self.send_at = None;
            } else if now >= at {
                self.send_at = None;
                actions.push(ProposalAction::Send(proposal.text(reply, &self.values)));
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(((at - now).fract() * 1000.0) as u64 + 30));
            }
        }
        actions
    }

    /// The band's head: Nora's mark and the heading, a chip per short reply, the link to the person's style.
    fn band(&mut self, ui: &mut Ui, proposal: &Proposal, words: &ProposalWords, tone: &ProposalTone, actions: &mut Vec<ProposalAction>) {
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            match tone.mark {
                Some(mark) => mark(ui.painter(), rect, tone.accent),
                None => {
                    ui.painter().circle_filled(rect.center(), 5.0, tone.accent);
                }
            }
            ui.label(RichText::new(words.heading.as_str()).strong().color(tone.text));
            for (index, short) in proposal.short.iter().enumerate() {
                let chosen = self.chosen == Some(index);
                let label = if chosen { format!("✓ « {} »", short.label) } else { format!("« {} »", short.label) };
                let chip = Button::new(RichText::new(label).color(tone.text))
                    .fill(if chosen { tone.hover } else { Color32::TRANSPARENT })
                    .stroke(Stroke::new(1.0, if chosen { tone.accent } else { tone.border }))
                    .corner_radius(CornerRadius::same(12));
                if ui.add_enabled(!self.sending(), chip).clicked() {
                    self.chosen = if chosen { None } else { Some(index) };
                    self.open = None;
                }
            }
            if ui.link(RichText::new(words.style_link.as_str()).size(12.0).color(tone.text)).clicked() {
                actions.push(ProposalAction::OpenStyle);
            }
        });
    }

    /// The reply's head: whose draft, to whom, and its buttons (or the send that waits, and its way back).
    fn head(&mut self, ui: &mut Ui, proposal: &Proposal, words: &ProposalWords, tone: &ProposalTone, to: &str, actions: &mut Vec<ProposalAction>) {
        let reply = self.reply(proposal);
        let missing = proposal.missing(reply, &self.values).len();
        let text = proposal.text(reply, &self.values);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(words.draft.as_str()).size(11.5).strong().color(tone.text));
            if !to.is_empty() {
                ui.label(RichText::new(format!("{} {to}", words.to)).size(13.0).color(tone.dim));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(at) = self.send_at {
                    let left = (at - ui.input(|i| i.time)).max(0.0).ceil() as u32;
                    if ui.button(words.cancel.as_str()).clicked() {
                        self.send_at = None;
                    }
                    ui.label(RichText::new((words.sending)(left.max(1))).color(tone.body));
                    return;
                }
                let send = Button::new(RichText::new(words.send.as_str()).strong().color(tone.send_text)).fill(tone.send_fill);
                if ui.add_enabled(missing == 0, send).clicked() {
                    self.open = None;
                    self.send_at = Some(ui.input(|i| i.time) + SEND_DELAY_S);
                    ui.ctx().request_repaint_after(Duration::from_millis(200));
                }
                if ui.button(words.edit.as_str()).clicked() {
                    actions.push(ProposalAction::Edit(text.clone()));
                }
                if ui.add(Button::new(words.dismiss.as_str()).frame(false)).clicked() {
                    actions.push(ProposalAction::Dismiss);
                }
                if missing > 0 {
                    ui.label(RichText::new((words.fields_left)(missing)).size(12.5).color(tone.text));
                }
            });
        });
    }

    /// The reply's text, line by line: its words, each empty field a pill, each filled one its value underlined; a
    /// click on either opens the field.
    fn body(&mut self, ui: &mut Ui, proposal: &Proposal, tone: &ProposalTone) {
        let reply = self.reply(proposal).clone();
        let mut clicked = None;
        for line in lines(&reply) {
            if line.is_empty() {
                ui.add_space(2.0);
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                for piece in &line {
                    match piece {
                        Piece::Text(text) => {
                            ui.add(Label::new(RichText::new(text.as_str()).size(14.5).color(tone.body)).wrap());
                        }
                        Piece::Field(index) => {
                            let label = proposal.fields.get(*index).map_or("", |f| f.label.as_str());
                            let value = self.values.get(*index).map(|v| v.trim()).filter(|v| !v.is_empty());
                            let open = self.open == Some(*index);
                            let response = match value {
                                Some(value) => ui.add(Label::new(RichText::new(value).size(14.5).color(tone.body).underline()).sense(Sense::click())),
                                None => ui.add(
                                    Button::new(RichText::new(label).size(14.0).color(tone.text))
                                        .fill(if open { tone.hover } else { tone.fill })
                                        .stroke(Stroke::new(if open { 1.5 } else { 1.0 }, tone.accent))
                                        .corner_radius(CornerRadius::same(6)),
                                ),
                            };
                            if response.clicked() {
                                clicked = Some(*index);
                            }
                        }
                    }
                }
            });
        }
        if let Some(field) = clicked
            && !self.sending()
        {
            self.open_field(field);
        }
    }

    /// The strip under the text where the open field is filled: its name, the app's shortcuts for its kind, a box.
    fn strip(
        &mut self,
        ui: &mut Ui,
        id: egui::Id,
        proposal: &Proposal,
        words: &ProposalWords,
        tone: &ProposalTone,
        choices: &dyn Fn(FieldKind) -> Vec<Choice>,
    ) {
        let Some(field) = self.open else { return };
        let Some(spec) = proposal.fields.get(field) else {
            self.open = None;
            return;
        };
        let mut kept = None;
        let mut close = false;
        Frame::new().stroke(Stroke::new(1.0, tone.accent)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(RichText::new(spec.label.as_str()).strong().color(tone.body));
            let shortcuts = choices(spec.kind);
            if !shortcuts.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for choice in &shortcuts {
                        if ui.add(Button::new(choice.shown.as_str()).corner_radius(CornerRadius::same(12))).clicked() {
                            kept = Some(choice.value.clone());
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                let box_id = id.with(("field", field));
                let output = TextEdit::singleline(&mut self.typed).id(box_id).hint_text(words.other_value.as_str()).desired_width(260.0).show(ui);
                if std::mem::take(&mut self.focus) {
                    ui.memory_mut(|m| m.request_focus(box_id));
                }
                let left = output.response.lost_focus();
                let (enter, tab, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Tab), i.key_pressed(Key::Escape)));
                if left && escape {
                    close = true;
                } else if left && (enter || tab) {
                    kept = Some(self.typed.clone());
                }
                if ui.button(words.keep.as_str()).clicked() {
                    kept = Some(self.typed.clone());
                }
            });
            ui.label(RichText::new(words.keys.as_str()).size(11.5).color(tone.dim));
        });
        if close {
            self.open = None;
            self.typed.clear();
        } else if let Some(value) = kept {
            if value.trim().is_empty() {
                self.next(proposal, field);
            } else {
                self.keep(proposal, value);
            }
        }
    }
}

/// A reply's pieces, line by line (a line break ends a line; an empty line is an empty list).
fn lines(reply: &Reply) -> Vec<Vec<Piece>> {
    let mut out = vec![Vec::new()];
    for piece in &reply.pieces {
        match piece {
            Piece::Field(_) => {
                if let Some(line) = out.last_mut() {
                    line.push(piece.clone());
                }
            }
            Piece::Text(text) => {
                for (i, part) in text.split('\n').enumerate() {
                    if i > 0 {
                        out.push(Vec::new());
                    }
                    if !part.is_empty()
                        && let Some(line) = out.last_mut()
                    {
                        line.push(Piece::Text(part.to_string()));
                    }
                }
            }
        }
    }
    out
}

/// The words of the card that asks the person whether Nora may prepare replies for them.
#[derive(Debug, Clone)]
pub struct ConsentWords {
    pub heading: String,
    pub text: String,
    pub yes: String,
    pub not_now: String,
    pub later: String,
}

impl Default for ConsentWords {
    fn default() -> Self {
        ConsentWords {
            heading: "Ready replies".into(),
            text: "Nora can propose replies in your own style. To do so she reads the message you open and learns from the \
                   emails you sent. Everything stays on your Neoffice server, for you alone; nothing is sent without your click."
                .into(),
            yes: "Turn on".into(),
            not_now: "Not now".into(),
            later: "You can change this in My style.".into(),
        }
    }
}

/// The card that asks: `Some(true)` when the person said yes, `Some(false)` for « not now ».
pub fn consent(ui: &mut Ui, words: &ConsentWords, tone: &ProposalTone) -> Option<bool> {
    let mut answer = None;
    Frame::new()
        .fill(tone.fill)
        .stroke(Stroke::new(1.0, tone.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                match tone.mark {
                    Some(mark) => mark(ui.painter(), rect, tone.accent),
                    None => {
                        ui.painter().circle_filled(rect.center(), 5.0, tone.accent);
                    }
                }
                ui.label(RichText::new(words.heading.as_str()).strong().color(tone.text));
            });
            ui.add(Label::new(RichText::new(words.text.as_str()).color(tone.body)).wrap());
            ui.horizontal_wrapped(|ui| {
                let yes = Button::new(RichText::new(words.yes.as_str()).strong().color(tone.text))
                    .fill(tone.hover)
                    .stroke(Stroke::new(1.0, tone.accent))
                    .corner_radius(CornerRadius::same(12));
                if ui.add(yes).clicked() {
                    answer = Some(true);
                }
                if ui.add(Button::new(words.not_now.as_str()).corner_radius(CornerRadius::same(12))).clicked() {
                    answer = Some(false);
                }
            });
            ui.label(RichText::new(words.later.as_str()).size(12.0).color(tone.dim));
        });
    answer
}

#[cfg(test)]
#[path = "proposal_tests.rs"]
mod tests;
