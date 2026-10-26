#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// //// Neoffice — added file (no upstream equivalent): the egui panel of the Nora client core.

//! The egui side of Nora in a Craft editor: a chat panel, and the glue that keeps a
//! [`NoraSession`] alive from the frame loop.
//!
//! The panel only draws. It reads a [`NoraSession`] and hands back what the user did as
//! [`PanelAction`]s; the host feeds them to the session and performs the HTTP and document calls the
//! session asks for (see [`craft_nora`]). Nothing here touches the network or the document, so the
//! same panel runs natively and in the browser.
//!
//! ```
//! use craft_nora::{NoraSession, Output};
//! use craft_nora_ui::{NoraPanel, drive};
//!
//! // One frame. `outputs` is what the host has to do: make the `Http` calls, run the `Document`
//! // calls in the editor, and feed their answers back to the session as `Input`s.
//! fn frame(ui: &mut egui::Ui, nora: &mut NoraSession, panel: &mut NoraPanel) -> Vec<Output> {
//!     let mut outputs = drive(ui.ctx(), nora);
//!     for action in panel.show(ui, nora) {
//!         outputs.extend(nora.handle(action.into_input()));
//!     }
//!     outputs
//! }
//! ```
//!
//! While the text box has the keyboard, Enter sends and Shift+Enter starts a new line. The panel
//! removes the Enter it used from the frame's input, but an editor that reads keys before it draws
//! the panel should look at `ctx.wants_keyboard_input()` first, as it does for any text field.
//!
//! Everything the user reads is in [`NoraLabels`] (English by default); an app passes its own
//! translations. The colors follow the egui theme unless [`NoraStyle`] says otherwise.

pub mod launcher;
mod markdown;

pub use launcher::{LauncherWords, PaneMemory};

use std::time::Duration;

use craft_nora::{ChatMessage, Confirmation, Correction, DocActionKind, Input, NoraError, NoraSession, Output, Role, Segment, ShownCard, segments};
use egui::{
    Align, Button, CentralPanel, Color32, Context, CornerRadius, Event, FontFamily, FontId, Frame, Id, Key, KeyboardShortcut, Label, Layout, Margin,
    Modifiers, Panel, RichText, ScrollArea, Stroke, TextEdit, TextStyle, Ui, Visuals,
};

/// What the user did in the panel. The host turns it into a session input with
/// [`PanelAction::into_input`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelAction {
    /// The user sent a message (already trimmed, never empty).
    Send(String),
    /// The user answered a confirmation Nora asked for.
    Confirm { id: u64, accepted: bool },
    /// The user stopped Nora while it worked.
    Stop,
    /// The user asked to undo Nora's last change.
    Undo,
    /// The user started a new conversation.
    NewConversation,
    /// The user clicked a link of a reply or a card's Open (the session lets a page of the instance through only).
    OpenLink(String),
    /// The user answered a card that waits for them.
    AnswerCard { token: String, approve: bool },
    /// The user picked one of the choices of a card that waits for them (a mailbox to write a draft from).
    ChooseOnCard { token: String, choice: String },
}

impl PanelAction {
    /// The session input this action stands for.
    pub fn into_input(self) -> Input {
        match self {
            PanelAction::Send(text) => Input::Send(text),
            PanelAction::Confirm { id, accepted } => Input::Confirm { id, accepted },
            PanelAction::Stop => Input::Cancel,
            PanelAction::Undo => Input::UndoLast,
            PanelAction::NewConversation => Input::Reset,
            PanelAction::OpenLink(url) => Input::OpenLink(url),
            PanelAction::AnswerCard { token, approve } => Input::AnswerCard { token, approve },
            PanelAction::ChooseOnCard { token, choice } => Input::ChooseOnCard { token, choice },
        }
    }
}

/// The words of the panel. English by default; an app fills in its own language.
#[derive(Debug, Clone)]
pub struct NoraLabels {
    /// Left of the header buttons; leave it empty when the app draws its own title.
    pub title: String,
    pub new_conversation: String,
    pub undo: String,
    pub undo_hint: String,
    pub placeholder: String,
    pub send: String,
    /// In place of Send while Nora works.
    pub stop: String,
    pub working: String,
    /// Under the conversation once the user stopped Nora, until the next message.
    pub stopped: String,
    /// Shown while the conversation is empty.
    pub empty: String,
    /// Above the description of an action Nora wants to run.
    pub confirm_heading: String,
    pub allow: String,
    pub refuse: String,
    /// A card's Open, when the card does not say its own words (« Ouvrir dans NeoDoc »).
    pub open: String,
    /// One sentence for an action waiting for the user's yes or no. The default is the English
    /// summary of the core; an app that shows another language describes the action itself.
    pub describe: fn(&DocActionKind) -> String,
}

impl Default for NoraLabels {
    fn default() -> Self {
        NoraLabels {
            title: "Nora".into(),
            new_conversation: "New conversation".into(),
            undo: "Undo".into(),
            undo_hint: "Undo the last change Nora made to the document".into(),
            placeholder: "Ask Nora…".into(),
            send: "Send".into(),
            stop: "Stop".into(),
            working: "Nora is working…".into(),
            stopped: "Stopped.".into(),
            empty: "Ask Nora to write, rewrite, translate or format your document.".into(),
            confirm_heading: "Nora wants to:".into(),
            allow: "Allow".into(),
            refuse: "Refuse".into(),
            open: "Open".into(),
            describe: |kind| kind.summary(),
        }
    }
}

/// How the panel looks. A color left at `None` is taken from the egui theme, so the panel follows
/// light and dark mode on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoraStyle {
    pub user_fill: Option<Color32>,
    pub user_text: Option<Color32>,
    pub nora_fill: Option<Color32>,
    pub nora_text: Option<Color32>,
    pub corner_radius: u8,
    pub bubble_padding: i8,
    /// The widest a bubble gets, as a share of the panel width.
    pub max_bubble_share: f32,
    /// The app's bold font family (`FontFamily::Name`), for the bold words and headings of Nora's replies; `None`
    /// draws them twice in the regular one, a pixel apart, for an app with no bold face (a family that only names the
    /// regular face, as NeoGrille's and NeoPrés's do in a web build, shows no bold: leave it `None`). Only a family
    /// the app binds: egui stops on one it does not know.
    pub bold_family: Option<&'static str>,
}

impl Default for NoraStyle {
    fn default() -> Self {
        NoraStyle {
            user_fill: None,
            user_text: None,
            nora_fill: None,
            nora_text: None,
            corner_radius: 10,
            bubble_padding: 8,
            max_bubble_share: 0.9,
            bold_family: None,
        }
    }
}

struct Colors {
    user_fill: Color32,
    user_text: Color32,
    nora_fill: Color32,
    nora_text: Color32,
    warn: Color32,
    error: Color32,
}

impl Colors {
    fn resolve(style: &NoraStyle, visuals: &Visuals) -> Self {
        Colors {
            user_fill: style.user_fill.unwrap_or(visuals.selection.bg_fill),
            user_text: style.user_text.unwrap_or(visuals.selection.stroke.color),
            nora_fill: style.nora_fill.unwrap_or(visuals.widgets.inactive.weak_bg_fill),
            nora_text: style.nora_text.unwrap_or_else(|| visuals.text_color()),
            warn: visuals.warn_fg_color,
            error: visuals.error_fg_color,
        }
    }
}

/// What the panel says under a turn that lost an action to a page that ran nothing (a tab in the background, a computer
/// asleep: `NoraSession::resumable`), and its button, which sends the same request again. English by default: an app
/// sets its own words here (a field of its own, so that an app built on [`NoraLabels`] keeps building).
#[derive(Debug, Clone)]
pub struct ResumeWords {
    pub said: String,
    pub resume: String,
}

impl Default for ResumeWords {
    fn default() -> Self {
        ResumeWords { said: "This tab was in the background: Nora could not act in the document.".into(), resume: "Resume".into() }
    }
}

/// What the panel says of a reply of Nora's whose turn could not act in the document in time (`NoraSession::correction`):
/// in place of a reply when nothing was done (it may say it was), under it when part was. English by default: an app
/// sets its own words here.
#[derive(Debug, Clone)]
pub struct CorrectionWords {
    pub nothing_done: String,
    pub partly_done: String,
}

impl Default for CorrectionWords {
    fn default() -> Self {
        CorrectionWords {
            nothing_done: "Nothing was done in the document: Nora could not act in it in time. Ask again if you still need it.".into(),
            partly_done: "Not all of this was done: part of it could not run in the document in time.".into(),
        }
    }
}

/// The conversation with Nora, drawn into the `Ui` it is given (a side panel, a window…).
#[derive(Debug, Default)]
pub struct NoraPanel {
    pub labels: NoraLabels,
    /// Under a turn lost to a page that ran nothing: what is said, and the button that sends the request again.
    pub resume_words: ResumeWords,
    /// What is said of a reply whose turn could not act in the document in time.
    pub correction_words: CorrectionWords,
    /// Set when the app does not carry the person's answer to a card (it drops [`PanelAction::AnswerCard`] and
    /// [`PanelAction::ChooseOnCard`]): why, in one line, shown on a card that waits for the person in place of its
    /// buttons, which would do nothing. `None` (the default): the buttons.
    pub cards_unanswered: Option<String>,
    pub style: NoraStyle,
    /// Set when the host has no way to reach Nora: the panel then says why instead of offering a
    /// text box that would swallow the message.
    pub offline: Option<String>,
    /// Whether the host opens a page of the instance ([`Output::Open`]): a link of a reply, a card's Open. Without
    /// it a link is shown as its words, never as something to click that does nothing.
    pub open_links: bool,
    /// Only the cards' Open lead to a page: a reply's link to the instance is its words too, as any other. For an app
    /// whose window shows what others wrote (mail), where a link Nora repeats may come from a message rather than from
    /// her: the cards are the server's, their links its own.
    pub links_on_cards_only: bool,
    draft: String,
    focus_input: bool,
    scroll_to_end: bool,
    /// Where the transcript's end was when it was last drawn.
    end_offset: f32,
    /// The host's words for a failure of the connection to Nora; English (the error's own sentence) without them.
    error_words: Option<fn(&NoraError) -> String>,
}

impl NoraPanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// A panel that says a failure of the connection to Nora (no answer, a refusal of the server, a timeout) in the
    /// app's language. The editor's own sentences (a refused action) are shown as they come.
    pub fn with_error_words(mut self, words: fn(&NoraError) -> String) -> Self {
        self.error_words = Some(words);
        self
    }

    /// A panel with the app's own words.
    pub fn with_labels(mut self, labels: NoraLabels) -> Self {
        self.labels = labels;
        self
    }

    /// A panel with the app's own colors.
    pub fn with_style(mut self, style: NoraStyle) -> Self {
        self.style = style;
        self
    }

    /// Puts the keyboard in the text box at the next frame (when the user opens the panel).
    pub fn focus_input(&mut self) {
        self.focus_input = true;
    }

    /// The message being typed.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Replaces the message being typed.
    pub fn set_draft(&mut self, text: impl Into<String>) {
        self.draft = text.into();
    }

    /// Draws the panel and returns what the user did this frame.
    pub fn show(&mut self, ui: &mut Ui, session: &NoraSession) -> Vec<PanelAction> {
        let mut actions = Vec::new();
        let colors = Colors::resolve(&self.style, ui.visuals());
        // (`make_persistent_id`: the same id as `ui.id().with(…)` on egui 0.36, and not deprecated on 0.37.)
        let id = ui.make_persistent_id("nora");
        self.header(ui, session, &mut actions);
        ui.separator();
        // The text box is a bottom panel, so that its height follows what is typed in it; the
        // transcript takes what is left above it.
        Panel::bottom(id.with("input-panel"))
            .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
            .show(ui, |ui| self.input_row(ui, id.with("input"), session.is_busy(), &mut actions));
        CentralPanel::no_frame().show(ui, |ui| self.transcript(ui, session, &colors, &mut actions));
        actions
    }

    fn header(&self, ui: &mut Ui, session: &NoraSession, actions: &mut Vec<PanelAction>) {
        ui.horizontal(|ui| {
            if !self.labels.title.is_empty() {
                ui.strong(self.labels.title.as_str());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let started = session.thread_id().is_some() || !session.transcript().is_empty();
                if ui.add_enabled(started, Button::new(self.labels.new_conversation.as_str()).small()).clicked() {
                    actions.push(PanelAction::NewConversation);
                }
                let undo = ui.add_enabled(session.can_undo(), Button::new(self.labels.undo.as_str()).small());
                if undo.on_hover_text(self.labels.undo_hint.as_str()).clicked() {
                    actions.push(PanelAction::Undo);
                }
            });
        });
    }

    fn transcript(&mut self, ui: &mut Ui, session: &NoraSession, colors: &Colors, actions: &mut Vec<PanelAction>) {
        let mut area = ScrollArea::vertical().id_salt("nora-transcript").auto_shrink([false, false]).stick_to_bottom(true);
        // The user just sent something: show it, even when they had scrolled up. The view is put exactly at the end as
        // it was last drawn, where egui counts it as stuck to the end, so that `stick_to_bottom` follows what comes
        // next (the message being sent, Nora's replies). A scroll to the cursor would not do: it lands one item spacing
        // past the end, which egui does not count as the end, and every reply after it stayed below the edge.
        if self.scroll_to_end {
            area = area.vertical_scroll_offset(self.end_offset);
        }
        // Where the view is, as the viewport says it (egui 0.37 keeps the scroll state's offset to itself; this reads the
        // same on 0.36 and 0.37).
        let mut at = 0.0;
        let shown = area.show_viewport(ui, |ui, viewport| {
            at = viewport.min.y;
            if session.transcript().is_empty() && !session.is_busy() {
                ui.add_space(ui.spacing().item_spacing.y * 4.0);
                ui.vertical_centered(|ui| ui.weak(self.labels.empty.as_str()));
            }
            // The cards of the conversation, each under the message it came after; those of the turn that runs, at the end.
            let cards = session.cards();
            let transcript = session.transcript();
            for shown in cards.iter().filter(|shown| shown.after == Some(0)) {
                self.card(ui, session, shown, colors, actions);
            }
            for (index, message) in transcript.iter().enumerate() {
                // A reply written before Nora knew that what she asked of the document was refused: never shown as done.
                match session.correction(index) {
                    Some(Correction::NothingDone) => self.notice(ui, &self.correction_words.nothing_done, colors),
                    Some(Correction::PartlyDone) => {
                        self.bubble(ui, session, message, colors, actions);
                        self.notice(ui, &self.correction_words.partly_done, colors);
                    }
                    None => self.bubble(ui, session, message, colors, actions),
                }
                for shown in cards.iter().filter(|shown| shown.after == Some(index + 1)) {
                    self.card(ui, session, shown, colors, actions);
                }
            }
            for shown in cards.iter().filter(|shown| shown.after.is_none_or(|after| after > transcript.len())) {
                self.card(ui, session, shown, colors, actions);
            }
            let confirmations = session.confirmations();
            for confirmation in &confirmations {
                self.confirmation(ui, confirmation, colors, actions);
            }
            // Nora waiting for the user's yes or no is not Nora working.
            if session.is_busy() && confirmations.is_empty() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(self.labels.working.as_str());
                });
            }
            if session.stopped() {
                ui.weak(self.labels.stopped.as_str());
            }
            if let Some(error) = session.last_error() {
                let said = match (session.last_failure(), self.error_words) {
                    (Some(failure), Some(words)) => words(failure),
                    _ => error.to_string(),
                };
                ui.colored_label(colors.error, said);
            }
            // A turn lost to a page that ran nothing: the same request again, on the person's click only (a new turn).
            if let Some(request) = session.resumable() {
                ui.add_space(ui.spacing().item_spacing.y);
                ui.add(Label::new(RichText::new(self.resume_words.said.as_str()).color(colors.warn)).wrap());
                if ui.button(self.resume_words.resume.as_str()).clicked() {
                    actions.push(PanelAction::Send(request.to_string()));
                }
            }
        });
        let end = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
        // Asked again next frame while the view is short of the end (something came in the same frame).
        self.scroll_to_end &= at < end - 0.5;
        self.end_offset = end;
    }

    /// A word of the panel itself, in the warning colour.
    fn notice(&self, ui: &mut Ui, text: &str, colors: &Colors) {
        ui.add(Label::new(RichText::new(text).color(colors.warn)).wrap());
    }

    fn bubble(&self, ui: &mut Ui, session: &NoraSession, message: &ChatMessage, colors: &Colors, actions: &mut Vec<PanelAction>) {
        if message.text.trim().is_empty() {
            return;
        }
        if message.role == Role::System {
            ui.vertical_centered(|ui| ui.weak(message.text.as_str()));
            return;
        }
        let from_user = message.role == Role::User;
        let (fill, text) = if from_user { (colors.user_fill, colors.user_text) } else { (colors.nora_fill, colors.nora_text) };
        let max_width = (ui.available_width() * self.style.max_bubble_share).max(80.0);
        let side = if from_user { Align::Max } else { Align::Min };
        ui.with_layout(Layout::top_down(side), |ui| {
            Frame::new()
                .fill(fill)
                .corner_radius(CornerRadius::same(self.style.corner_radius))
                .inner_margin(Margin::same(self.style.bubble_padding))
                .show(ui, |ui| {
                    ui.set_max_width(max_width);
                    if from_user {
                        ui.add(Label::new(RichText::new(message.text.as_str()).color(text)).wrap());
                        return;
                    }
                    // A reply with links: its words as one text (a link's own words in it), then, on a row of their own,
                    // the links that lead to a page of the instance. Laid out apart, a link is never under another
                    // widget's words (an egui text that wraps in a row takes the row's width from its start).
                    let pieces = segments(&message.text);
                    let words: String = pieces
                        .iter()
                        .map(|piece| match piece {
                            Segment::Text(words) => words.as_str(),
                            Segment::Link { label, .. } => label.as_str(),
                        })
                        .collect();
                    // Her little Markdown drawn, not printed (markdown.rs): bold, italic, code, lists, headings in bold.
                    let body = TextStyle::Body.resolve(ui.style());
                    let base = egui::text::TextFormat::simple(body.clone(), text);
                    let bold = match self.style.bold_family {
                        Some(family) => markdown::Bold::Font(FontId::new(body.size, FontFamily::Name(family.into()))),
                        None => markdown::Bold::Doubled,
                    };
                    // Laid out here, at the width a wrapping label takes, so that the bold words drawn again fall on it.
                    let width = ui.available_width();
                    let mut job = markdown::job(&words, &base, &bold, ui.visuals().code_bg_color);
                    job.wrap.max_width = width;
                    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
                    let shown = ui.add(Label::new(galley));
                    // No bold face: the bold words a second time, one pixel to the right (markdown::bold_overlay).
                    if bold == markdown::Bold::Doubled
                        && let Some(mut overlay) = markdown::bold_overlay(&words, &base)
                    {
                        overlay.wrap.max_width = width;
                        let overlay = ui.ctx().fonts_mut(|fonts| fonts.layout_job(overlay));
                        let pixel = egui::vec2(1.0 / ui.pixels_per_point(), 0.0);
                        ui.painter().galley(shown.rect.left_top() + pixel, overlay, text);
                    }
                    let pages: Vec<(String, String)> = pieces
                        .into_iter()
                        .filter_map(|piece| match piece {
                            Segment::Link { label, url } if self.open_links && !self.links_on_cards_only && session.page_of(&url).is_some() => {
                                Some((label, url))
                            }
                            _ => None,
                        })
                        .collect();
                    if !pages.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (label, url) in pages {
                                // Its words as the reply shows them, the Markdown of a label drawn away.
                                if ui.link(markdown::plain(&label)).on_hover_text(url.as_str()).clicked() {
                                    actions.push(PanelAction::OpenLink(url));
                                }
                            }
                        });
                    }
                });
        });
        ui.add_space(ui.spacing().item_spacing.y);
    }

    /// A card of the conversation: what it is, what it says, its Open when it leads to a page of the instance, and the
    /// person's answer when it waits for them: yes or no (a letter to make the company's template), or one of its
    /// choices or no (a mailbox to write a draft from).
    fn card(&self, ui: &mut Ui, session: &NoraSession, shown: &ShownCard, colors: &Colors, actions: &mut Vec<PanelAction>) {
        let card = &shown.card;
        let waits = card.awaits_the_person();
        let stroke = if waits { colors.warn } else { ui.visuals().widgets.noninteractive.bg_stroke.color };
        Frame::new()
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(CornerRadius::same(self.style.corner_radius))
            .inner_margin(Margin::same(self.style.bubble_padding))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !card.title.is_empty() {
                    ui.add(Label::new(RichText::new(card.title.as_str()).strong()).wrap());
                }
                if let Some(summary) = &card.summary {
                    ui.add(Label::new(summary.as_str()).wrap());
                }
                if let Some(result) = card.result.as_deref().filter(|_| !waits) {
                    ui.add(Label::new(RichText::new(result).weak()).wrap());
                }
                if waits && let Some(why) = &self.cards_unanswered {
                    ui.add(Label::new(RichText::new(why.as_str()).weak()).wrap());
                }
                ui.horizontal_wrapped(|ui| {
                    if waits && self.cards_unanswered.is_none() {
                        let refuse = card.refuse_label.as_deref().unwrap_or(self.labels.refuse.as_str());
                        if card.choices.is_empty() {
                            let confirm = card.confirm_label.as_deref().unwrap_or(self.labels.allow.as_str());
                            if ui.add_enabled(!shown.answering, Button::new(confirm)).clicked() {
                                actions.push(PanelAction::AnswerCard { token: card.token.clone(), approve: true });
                            }
                        }
                        for choice in &card.choices {
                            if ui.add_enabled(!shown.answering, Button::new(choice.label.as_str())).clicked() {
                                actions.push(PanelAction::ChooseOnCard { token: card.token.clone(), choice: choice.id.clone() });
                            }
                        }
                        if ui.add_enabled(!shown.answering, Button::new(refuse)).clicked() {
                            actions.push(PanelAction::AnswerCard { token: card.token.clone(), approve: false });
                        }
                        if shown.answering {
                            ui.spinner();
                        }
                    }
                    if self.open_links
                        && let Some(url) = card.open_url.as_deref()
                        && session.page_of(url).is_some()
                    {
                        let words = card.open_label.as_deref().unwrap_or(self.labels.open.as_str());
                        if ui.button(words).on_hover_text(url).clicked() {
                            actions.push(PanelAction::OpenLink(url.to_string()));
                        }
                    }
                });
            });
        ui.add_space(ui.spacing().item_spacing.y);
    }

    fn confirmation(&self, ui: &mut Ui, confirmation: &Confirmation, colors: &Colors, actions: &mut Vec<PanelAction>) {
        Frame::new()
            .stroke(Stroke::new(1.0, colors.warn))
            .corner_radius(CornerRadius::same(self.style.corner_radius))
            .inner_margin(Margin::same(self.style.bubble_padding))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.weak(self.labels.confirm_heading.as_str());
                ui.label((self.labels.describe)(&confirmation.kind));
                ui.horizontal(|ui| {
                    if ui.button(self.labels.allow.as_str()).clicked() {
                        actions.push(PanelAction::Confirm { id: confirmation.id, accepted: true });
                    }
                    if ui.button(self.labels.refuse.as_str()).clicked() {
                        actions.push(PanelAction::Confirm { id: confirmation.id, accepted: false });
                    }
                });
            });
        ui.add_space(ui.spacing().item_spacing.y);
    }

    /// The text box and the Send button. While Nora works (`busy`) the message waits in the box, as in
    /// the desk's chat, which locks its field until the answer is in, and the button stops Nora.
    fn input_row(&mut self, ui: &mut Ui, input_id: Id, busy: bool, actions: &mut Vec<PanelAction>) {
        if let Some(reason) = self.offline.as_deref() {
            self.focus_input = false;
            ui.add(Label::new(RichText::new(reason).weak()).wrap());
            return;
        }
        if std::mem::take(&mut self.focus_input) {
            ui.memory_mut(|memory| memory.request_focus(input_id));
        }
        let mut send_clicked = false;
        let mut typing = false;
        ui.horizontal(|ui| {
            // Right to left so that the button takes its own width and the text box the rest.
            ui.with_layout(Layout::right_to_left(Align::BOTTOM), |ui| {
                if busy {
                    if ui.button(self.labels.stop.as_str()).clicked() {
                        actions.push(PanelAction::Stop);
                    }
                } else {
                    let can_send = !self.draft.trim().is_empty();
                    send_clicked = ui.add_enabled(can_send, Button::new(self.labels.send.as_str())).clicked();
                }
                // Plain Enter is handled below: only Shift+Enter is a new line.
                let output = TextEdit::multiline(&mut self.draft)
                    .id(input_id)
                    .hint_text(self.labels.placeholder.as_str())
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                    .show(ui);
                typing = output.response.has_focus();
            });
        });
        // After the text box, so that text typed in the same frame is already in the draft.
        let enter = typing && take_enter(ui);
        if (send_clicked || enter) && !busy && !self.draft.trim().is_empty() {
            let text = std::mem::take(&mut self.draft);
            actions.push(PanelAction::Send(text.trim().to_string()));
            self.scroll_to_end = true;
        }
    }
}

/// Whether Enter without Shift was pressed this frame, and takes it out of the frame's input so
/// that nothing drawn after the panel sees it. An Enter that confirms an input-method composition
/// is left alone.
fn take_enter(ui: &Ui) -> bool {
    ui.input_mut(|input| {
        if input.events.iter().any(|event| matches!(event, Event::Ime(_))) {
            return false;
        }
        let before = input.events.len();
        input
            .events
            .retain(|event| !matches!(event, Event::Key { key: Key::Enter, pressed: true, repeat: false, modifiers, .. } if !modifiers.shift));
        input.events.len() != before
    })
}

/// Feeds the session the frame clock and keeps egui repainting while the session waits for
/// something (a reply, an action). Call it once per frame; the returned outputs are the calls the
/// session wants made now, to be handled like those of [`NoraSession::handle`].
pub fn drive(ctx: &Context, session: &mut NoraSession) -> Vec<Output> {
    let now_ms = (ctx.input(|input| input.time) * 1000.0).max(0.0) as u64;
    let outputs = session.handle(Input::Tick { now_ms });
    if let Some(wake_ms) = session.next_wake_ms() {
        ctx.request_repaint_after(Duration::from_millis(wake_ms.saturating_sub(now_ms).max(50)));
    }
    outputs
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_nora::{AppKind, DocumentInfo, HttpResponse, PageContext, SessionConfig};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::{Value, json};

    struct State {
        panel: NoraPanel,
        session: NoraSession,
        actions: Vec<PanelAction>,
    }

    fn session() -> NoraSession {
        NoraSession::new(SessionConfig::default(), PageContext::editor(AppKind::Word, DocumentInfo::named("devis.docx")))
    }

    fn answer(session: &mut NoraSession, output: &Output, message: Value) -> Vec<Output> {
        let Output::Http(request) = output else {
            return Vec::new();
        };
        session.handle(Input::Http(HttpResponse { id: request.id, status: 200, body: json!({ "message": message }).to_string() }))
    }

    /// A finished conversation: the user said "Hello", Nora replied "Hi there" and went quiet.
    fn conversation() -> NoraSession {
        let mut nora = session();
        let sent = nora.handle(Input::Send("Hello".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        let polls = nora.handle(Input::Tick { now_ms: 10 });
        answer(&mut nora, &polls[0], json!({ "messages": [{ "role": "assistant", "text": "Hi there" }] }));
        answer(&mut nora, &polls[1], json!({ "actions": [] }));
        nora.handle(Input::Tick { now_ms: 20_000 });
        assert!(!nora.is_busy());
        nora
    }

    /// A session in which Nora asked to run `action` and waits for the user.
    fn asking(action: Value) -> NoraSession {
        let mut nora = session();
        let sent = nora.handle(Input::Send("Do it".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        let polls = nora.handle(Input::Tick { now_ms: 100 });
        answer(&mut nora, &polls[1], json!({ "actions": [action] }));
        assert_eq!(nora.confirmations().len(), 1);
        nora
    }

    /// Puts the keyboard in the text box and types, the way a user does.
    fn type_into_the_box(h: &mut Harness<'_, State>, text: &str) {
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
        h.run_steps(3);
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(text);
        h.run_steps(3);
    }

    fn harness(session: NoraSession) -> Harness<'static, State> {
        Harness::builder().with_size(egui::vec2(360.0, 520.0)).build_ui_state(
            |ui, state: &mut State| {
                let actions = state.panel.show(ui, &state.session);
                state.actions.extend(actions);
            },
            State { panel: NoraPanel::new(), session, actions: Vec::new() },
        )
    }

    #[test]
    fn actions_map_to_session_inputs() {
        assert!(matches!(PanelAction::Send("a".into()).into_input(), Input::Send(text) if text == "a"));
        assert!(matches!(PanelAction::Confirm { id: 3, accepted: false }.into_input(), Input::Confirm { id: 3, accepted: false }));
        assert!(matches!(PanelAction::Stop.into_input(), Input::Cancel));
        assert!(matches!(PanelAction::Undo.into_input(), Input::UndoLast));
        assert!(matches!(PanelAction::NewConversation.into_input(), Input::Reset));
    }

    #[test]
    fn an_empty_conversation_shows_the_hint_and_the_input() {
        let mut h = harness(session());
        h.run();
        assert!(h.query_by_label("Nora").is_some());
        assert!(h.query_by_label_contains("Ask Nora to write").is_some());
        assert!(h.get_by_label("Send").accesskit_node().is_disabled());
        assert!(h.state().actions.is_empty());
    }

    #[test]
    fn the_transcript_is_shown_in_order() {
        let mut h = harness(conversation());
        h.run();
        assert!(h.query_by_label("Hello").is_some());
        assert!(h.query_by_label("Hi there").is_some());
        assert!(h.query_by_label_contains("Ask Nora to write").is_none());
    }

    #[test]
    fn enter_sends_the_trimmed_message_and_empties_the_box() {
        let mut h = harness(session());
        h.run();
        type_into_the_box(&mut h, "  Rewrite the intro  ");
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Send("Rewrite the intro".into())]);
        assert_eq!(h.state().panel.draft(), "");
    }

    #[test]
    fn shift_enter_starts_a_new_line_instead_of_sending() {
        let mut h = harness(session());
        h.run();
        type_into_the_box(&mut h, "line one");
        h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
        h.run();
        assert!(h.state().actions.is_empty());
        assert_eq!(h.state().panel.draft(), "line one\n");
    }

    /// The panel as a host runs it: each of its actions goes to the session in the same frame, and the session's
    /// requests wait for the test to answer them.
    struct Hosted {
        panel: NoraPanel,
        session: NoraSession,
        requests: Vec<Output>,
    }

    /// A conversation taller than the panel: once the person sends, each of Nora's replies comes into view, even one
    /// that lands while the view is still moving to the end (it used to stop one message short, for good).
    #[test]
    fn the_newest_reply_comes_into_view_after_a_send() {
        let mut listed: Vec<Value> =
            (0..12).map(|n| json!({ "role": "assistant", "text": format!("Earlier reply {n}, long enough to wrap in a narrow panel.") })).collect();
        let mut nora = session();
        let sent = nora.handle(Input::Send("Hello".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        let polls = nora.handle(Input::Tick { now_ms: 10 });
        answer(&mut nora, &polls[0], json!({ "messages": listed }));
        answer(&mut nora, &polls[1], json!({ "actions": [] }));
        nora.handle(Input::Tick { now_ms: 20_000 });
        let height = 300.0;
        let mut h = Harness::builder().with_size(egui::vec2(360.0, height)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, state: &mut Hosted| {
                for action in state.panel.show(ui, &state.session) {
                    let out = state.session.handle(action.into_input());
                    state.requests.extend(out);
                }
            },
            Hosted { panel: NoraPanel::new(), session: nora, requests: Vec::new() },
        );
        h.run();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
        h.run_steps(3);
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("One more");
        h.run_steps(3);
        h.key_press(Key::Enter);
        h.run_steps(1);
        let state = h.state_mut();
        let send = state.requests.drain(..).find(|output| matches!(output, Output::Http(_)));
        answer(&mut state.session, &send.expect("the message went to the server"), json!({ "thread_id": "t1" }));
        // Nora's acknowledgement lands two frames after the send, while the view is still on its way down.
        h.run_steps(2);
        let mut now = 20_000;
        let mut reply = |h: &mut Harness<'_, Hosted>, text: &str| {
            listed.push(json!({ "role": "assistant", "text": text }));
            let state = h.state_mut();
            loop {
                now += 250;
                let polls = state.session.handle(Input::Tick { now_ms: now });
                if let [messages, actions] = polls.as_slice() {
                    answer(&mut state.session, messages, json!({ "messages": listed }));
                    answer(&mut state.session, actions, json!({ "actions": [] }));
                    break;
                }
                assert!(now < 60_000, "the session never asked for Nora's replies");
            }
        };
        reply(&mut h, "Passing this on, back in a moment.");
        h.run_steps(60);
        reply(&mut h, "The newest reply");
        // Nora is still at work (her spinner keeps drawing): a second of frames, the view's time to settle.
        h.run_steps(60);
        let newest = h.get_by_label("The newest reply").rect();
        assert!(newest.min.y >= 0.0 && newest.max.y <= height, "the reply is in view, not below the edge: {newest:?}");
    }

    /// A turn whose action waited too long for a page that ran nothing (a tab in the background): the panel says so and
    /// offers the same request again, sent on the click only; in the app's words when it gives them.
    #[test]
    fn a_turn_lost_to_a_tab_in_the_background_offers_to_resume() {
        let lost = || {
            let mut nora = session();
            let sent = nora.handle(Input::Send("Write 42 in B2".into()));
            answer(&mut nora, &sent[0], json!({ "thread_id": "t1", "sent_at": 1_000.0 }));
            let polls = nora.handle(Input::Tick { now_ms: 10 });
            answer(&mut nora, &polls[0], json!({ "messages": [] }));
            let action = json!({ "action": "doc_type_text", "text": "42", "callback_id": "c1" });
            answer(&mut nora, &polls[1], json!({ "actions": [action], "queued_at": [1_002.0], "now": 1_500.0 }));
            let polls = nora.handle(Input::Tick { now_ms: 500 });
            let reply = json!({ "role": "assistant", "text": "Nothing was changed.", "ts": 1_501.0 });
            answer(&mut nora, &polls[0], json!({ "messages": [reply] }));
            answer(&mut nora, &polls[1], json!({ "actions": [] }));
            nora.handle(Input::Tick { now_ms: 5_000 });
            assert_eq!(nora.resumable(), Some("Write 42 in B2"));
            nora
        };
        let mut h = harness(lost());
        h.run();
        assert!(h.state().actions.is_empty(), "never sent by itself");
        assert!(h.query_by_label("This tab was in the background: Nora could not act in the document.").is_some());
        h.get_by_label("Resume").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Send("Write 42 in B2".into())]);
        let mut french = harness(lost());
        french.state_mut().panel.resume_words =
            ResumeWords { said: "L'onglet était en arrière-plan : Nora n'a pas pu agir dans le document.".into(), resume: "Reprendre".into() };
        french.run();
        assert!(french.query_by_label("Reprendre").is_some() && french.query_by_label("Resume").is_none());
        // Nothing to resume in a turn that went well.
        let mut fine = harness(conversation());
        fine.run();
        assert!(fine.query_by_label("Resume").is_none());
    }

    /// A turn whose write was refused when the tab came back (or expired unread): Nora's reply, written before she knew,
    /// says it is done. The panel says what is true in its place; a turn that did part of it keeps the reply, with a
    /// word under it (24.10, NeoMail).
    #[test]
    fn a_reply_that_says_done_after_its_write_was_refused_is_not_shown_as_done() {
        let refused = |ran_first: bool| {
            let mut nora = session();
            let sent = nora.handle(Input::Send("Write 42 in B2".into()));
            answer(&mut nora, &sent[0], json!({ "thread_id": "t1", "sent_at": 1_000.0 }));
            let polls = nora.handle(Input::Tick { now_ms: 10 });
            answer(&mut nora, &polls[0], json!({ "messages": [] }));
            if ran_first {
                let action = json!({ "action": "doc_type_text", "text": "41", "callback_id": "c0" });
                let played = answer(&mut nora, &polls[1], json!({ "actions": [action], "queued_at": [1_001.0], "now": 1_001.5 }));
                let id = played.iter().find_map(|output| if let Output::Document(call) = output { Some(call.id) } else { None }).unwrap();
                nora.handle(Input::DocumentResult { id, result: Ok(json!({ "ok": true })) });
                let polls = nora.handle(Input::Tick { now_ms: 500 });
                answer(&mut nora, &polls[0], json!({ "messages": [] }));
                let action = json!({ "action": "doc_type_text", "text": "42", "callback_id": "c1" });
                answer(&mut nora, &polls[1], json!({ "actions": [action], "queued_at": [1_002.0], "now": 1_500.0 }));
            } else {
                let action = json!({ "action": "doc_type_text", "text": "42", "callback_id": "c1" });
                answer(&mut nora, &polls[1], json!({ "actions": [action], "queued_at": [1_002.0], "now": 1_500.0 }));
            }
            let polls = nora.handle(Input::Tick { now_ms: 1_000 });
            let reply = json!({ "role": "assistant", "text": "Done: B2 holds 42.", "ts": 1_501.0 });
            answer(&mut nora, &polls[0], json!({ "messages": [reply] }));
            answer(&mut nora, &polls[1], json!({ "actions": [] }));
            nora.handle(Input::Tick { now_ms: 5_000 });
            nora
        };
        let mut nothing = harness(refused(false));
        nothing.run();
        assert!(nothing.query_by_label("Done: B2 holds 42.").is_none(), "never shown as done");
        assert!(nothing.query_by_label(CorrectionWords::default().nothing_done.as_str()).is_some());
        let mut partly = harness(refused(true));
        partly.run();
        assert!(partly.query_by_label("Done: B2 holds 42.").is_some());
        assert!(partly.query_by_label(CorrectionWords::default().partly_done.as_str()).is_some());
        let mut french = harness(refused(false));
        french.state_mut().panel.correction_words.nothing_done = "Rien n'a été fait dans le document.".into();
        french.run();
        assert!(french.query_by_label("Rien n'a été fait dans le document.").is_some());
        // A turn that went well says nothing of the kind.
        let mut fine = harness(conversation());
        fine.run();
        assert!(fine.query_by_label(CorrectionWords::default().nothing_done.as_str()).is_none());
    }

    #[test]
    fn the_send_button_sends() {
        let mut h = harness(session());
        h.run();
        h.state_mut().panel.set_draft("Translate this");
        h.run();
        h.get_by_label("Send").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Send("Translate this".into())]);
    }

    /// A failure of the connection is said in the app's words when it gives them, in English otherwise.
    #[test]
    fn a_failure_is_said_in_the_app_s_words() {
        let failed = || {
            let mut nora = session();
            nora.handle(Input::Send("Hello".into()));
            nora.handle(Input::Tick { now_ms: 10 });
            nora.handle(Input::Tick { now_ms: 500_000 });
            nora.handle(Input::Tick { now_ms: 500_050 });
            assert!(nora.last_failure().is_some(), "nothing came: the wait ended");
            nora
        };
        let mut english = harness(failed());
        english.run();
        assert!(english.query_by_label("Nora did not answer in time").is_some());
        let mut french = Harness::builder().with_size(egui::vec2(360.0, 520.0)).build_ui_state(
            |ui, state: &mut State| {
                let actions = state.panel.show(ui, &state.session);
                state.actions.extend(actions);
            },
            State {
                panel: NoraPanel::new().with_error_words(|failure| match failure {
                    NoraError::Timeout => "Nora n'a pas répondu à temps".into(),
                    other => other.to_string(),
                }),
                session: failed(),
                actions: Vec::new(),
            },
        );
        french.run();
        assert!(french.query_by_label("Nora n'a pas répondu à temps").is_some());
        assert!(french.query_by_label("Nora did not answer in time").is_none());
    }

    #[test]
    fn an_offline_panel_says_why_and_offers_no_text_box() {
        let mut h = harness(session());
        h.state_mut().panel.offline = Some("Nora isn't available here".into());
        h.run();
        assert!(h.query_by_label("Nora isn't available here").is_some());
        assert!(h.query_by_role(egui::accesskit::Role::MultilineTextInput).is_none());
        assert!(h.query_by_label("Send").is_none());
        assert!(h.state().actions.is_empty());
    }

    #[test]
    fn while_nora_works_the_message_waits_in_the_box() {
        let mut nora = session();
        nora.handle(Input::Send("First".into()));
        assert!(nora.is_busy());
        let mut h = harness(nora);
        h.run_steps(2);
        type_into_the_box(&mut h, "Second");
        h.key_press(Key::Enter);
        h.run_steps(3);
        assert!(h.state().actions.is_empty(), "Enter does not send while Nora works");
        assert_eq!(h.state().panel.draft(), "Second", "and the message is still there");
        assert!(h.query_by_label("Send").is_none(), "the button stops Nora instead");
        assert!(h.query_by_label("Stop").is_some());
    }

    #[test]
    fn stop_stops_nora_and_says_so_until_the_next_message() {
        let mut nora = session();
        nora.handle(Input::Send("Hello".into()));
        let mut h = harness(nora);
        h.run_steps(2);
        h.get_by_label("Stop").click();
        h.run_steps(2);
        assert_eq!(h.state().actions, vec![PanelAction::Stop]);

        let state = h.state_mut();
        let stop = state.actions.remove(0).into_input();
        state.session.handle(stop);
        state.panel.set_draft("Something else");
        h.run_steps(2);
        assert!(h.query_by_label("Stopped.").is_some());
        assert!(h.query_by_label("Nora is working…").is_none());
        assert!(!h.get_by_label("Send").accesskit_node().is_disabled(), "the next message can go");
        h.state_mut().session.handle(Input::Send("Something else".into()));
        h.run_steps(2);
        assert!(h.query_by_label("Stopped.").is_none());
    }

    #[test]
    fn a_blank_message_is_never_sent() {
        let mut h = harness(session());
        h.run();
        type_into_the_box(&mut h, "   ");
        h.key_press(Key::Enter);
        h.run();
        assert!(h.state().actions.is_empty());
    }

    #[test]
    fn a_confirmation_is_described_and_answered() {
        // Nora asks to save: always confirmed by the user first.
        let nora = asking(json!({ "action": "doc_save", "callback_id": "c1" }));
        let id = nora.confirmations()[0].id;

        let mut h = harness(nora);
        h.run();
        assert!(h.query_by_label("Nora wants to:").is_some());
        assert!(h.query_by_label("Save the document").is_some());
        h.get_by_label("Allow").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Confirm { id, accepted: true }]);
        h.get_by_label("Refuse").click();
        h.run();
        assert_eq!(h.state().actions.last(), Some(&PanelAction::Confirm { id, accepted: false }));
    }

    #[test]
    fn an_app_can_describe_actions_in_its_own_language() {
        let nora = asking(json!({ "action": "doc_save" }));
        let labels = NoraLabels {
            describe: |kind| match kind {
                DocActionKind::Save => "Enregistrer le document".into(),
                other => other.summary(),
            },
            allow: "Autoriser".into(),
            ..NoraLabels::default()
        };
        let mut h = harness(nora);
        h.state_mut().panel = NoraPanel::new().with_labels(labels);
        h.run();
        assert!(h.query_by_label("Enregistrer le document").is_some());
        assert!(h.query_by_label("Autoriser").is_some());
        assert!(h.query_by_label("Allow").is_none());
    }

    #[test]
    fn undo_and_new_conversation_are_offered_when_they_make_sense() {
        let mut h = harness(session());
        h.run();
        assert!(h.get_by_label("Undo").accesskit_node().is_disabled());
        assert!(h.get_by_label("New conversation").accesskit_node().is_disabled());

        let mut h = harness(conversation());
        h.run();
        assert!(!h.get_by_label("New conversation").accesskit_node().is_disabled());
        h.get_by_label("New conversation").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::NewConversation]);
    }

    #[test]
    fn a_failure_and_the_working_state_are_shown() {
        let mut nora = session();
        let outputs = nora.handle(Input::Send("Hello".into()));
        let mut h = harness(nora);
        h.run_steps(2);
        assert!(h.query_by_label("Nora is working…").is_some());

        let Output::Http(request) = &outputs[0] else {
            return;
        };
        h.state_mut().session.handle(Input::Http(HttpResponse { id: request.id, status: 500, body: "{}".into() }));
        h.run_steps(2);
        assert!(h.query_by_label("Nora is working…").is_none());
        assert!(h.query_by_label_contains("500").is_some());
    }

    #[test]
    fn drive_ticks_the_session_with_the_frame_clock() {
        let mut nora = session();
        let sent = nora.handle(Input::Send("Hello".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        let ctx = Context::default();
        let mut polls = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput { time: Some(5.0), ..Default::default() }, |ui| polls = drive(ui.ctx(), &mut nora));
        output.textures_delta.clear();
        // Five seconds after the conversation opened, the replies and the actions are read.
        assert_eq!(polls.len(), 2);
        assert!(matches!(&polls[0], Output::Http(request) if request.path.ends_with("get_messages")));
    }

    // //// Neoffice — the links of Nora's replies and the cards of the conversation (step 7 of « Nora partout »).

    /// A finished conversation whose reply is `text` and whose answer carried `cards`.
    fn replied_with(text: &str, cards: Value) -> NoraSession {
        let mut nora = session();
        let sent = nora.handle(Input::Send("Imprime-la".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        let polls = nora.handle(Input::Tick { now_ms: 10 });
        answer(&mut nora, &polls[0], json!({ "messages": [{ "role": "assistant", "text": text }], "cards": cards }));
        answer(&mut nora, &polls[1], json!({ "actions": [] }));
        nora.handle(Input::Tick { now_ms: 20_000 });
        nora
    }

    fn opening(session: NoraSession) -> Harness<'static, State> {
        let mut h = harness(session);
        h.state_mut().panel.open_links = true;
        h.run();
        h
    }

    #[test]
    fn a_link_to_a_page_of_the_instance_is_a_link_and_any_other_is_its_words() {
        let reply = "Le brouillon est prêt : [Ouvrir le brouillon](/app/webmail?uid=4). Voir aussi [ce site](https://elsewhere.example/x).";
        let mut h = opening(replied_with(reply, json!([])));
        assert!(h.query_by_label("Le brouillon est prêt : Ouvrir le brouillon. Voir aussi ce site.").is_some(), "the words, no Markdown");
        assert!(h.query_by_label("ce site").is_none(), "a link elsewhere is only its words");
        h.get_by_label("Ouvrir le brouillon").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::OpenLink("/app/webmail?uid=4".into())]);
    }

    // //// Neoffice — Nora's little Markdown drawn, not printed (markdown.rs).
    #[test]
    fn a_reply_s_markdown_is_drawn_not_printed() {
        let reply = "Cette lettre est adressée à **Régie Exemple Sàrl** :\n- *brouillon*\n- objet `Livraison`\n## Suite\nVoir [ce site](https://elsewhere.example/x).";
        let h = opening(replied_with(reply, json!([])));
        assert!(
            h.query_by_label("Cette lettre est adressée à Régie Exemple Sàrl :\n• brouillon\n• objet Livraison\nSuite\nVoir ce site.").is_some(),
            "the words, their markers drawn"
        );
        assert!(h.query_by_label_contains("**").is_none(), "no star is shown");
        assert!(h.query_by_label("ce site").is_none(), "a link from the Markdown is only its words");
    }

    #[test]
    fn where_only_the_cards_lead_to_pages_a_reply_s_link_to_the_instance_is_its_words() {
        let reply = "Le brouillon est prêt : [Ouvrir le brouillon](/app/webmail?uid=4). Voir aussi [ce site](https://elsewhere.example/x).";
        let cards = json!([{ "token": "c1", "kind": "Document", "status": "Executed", "title": "Brouillon · Fuite d'eau",
            "conversation_id": "t1", "payload": { "editor_url": "/app/webmail?uid=9", "open_label": "Ouvrir la carte" } }]);
        let mut h = harness(replied_with(reply, cards));
        h.state_mut().panel.open_links = true;
        h.state_mut().panel.links_on_cards_only = true;
        h.run();
        assert!(h.query_by_label("Le brouillon est prêt : Ouvrir le brouillon. Voir aussi ce site.").is_some(), "the words");
        assert!(h.query_by_label("Ouvrir le brouillon").is_none() && h.query_by_label("ce site").is_none(), "no link of the reply");
        h.get_by_label("Ouvrir la carte").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::OpenLink("/app/webmail?uid=9".into())]);
    }

    #[test]
    fn without_a_host_that_opens_pages_no_link_is_offered() {
        let mut h = harness(replied_with("[Ouvrir dans NeoDoc](/drive/f/abc)", json!([])));
        h.run();
        h.get_by_label("Ouvrir dans NeoDoc").click();
        h.run();
        assert!(h.state().actions.is_empty());
    }

    #[test]
    fn a_document_card_opens_its_document() {
        let cards = json!([{ "token": "c1", "kind": "Document", "status": "Executed", "title": "Brouillon · Fuite d'eau",
            "conversation_id": "t1", "result": "shared",
            "payload": { "editor_url": "/app/webmail?uid=4", "open_label": "Ouvrir le brouillon" } }]);
        let mut h = opening(replied_with("C'est prêt.", cards));
        assert!(h.query_by_label("Brouillon · Fuite d'eau").is_some());
        assert!(h.query_by_label("shared").is_none());
        h.get_by_label("Ouvrir le brouillon").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::OpenLink("/app/webmail?uid=4".into())]);
    }

    #[test]
    fn a_template_card_is_confirmed_or_refused_with_its_own_words() {
        let cards = json!([{ "token": "c2", "kind": "Template", "status": "Pending", "title": "Modèle de lettre",
            "conversation_id": "t1",
            "payload": { "summary": "« Lettre.docx » devient le modèle de lettre.", "confirm_label": "Confirmer",
                         "refuse_label": "Annuler" } }]);
        let mut h = opening(replied_with("Confirmez sur la carte.", cards));
        assert!(h.query_by_label("« Lettre.docx » devient le modèle de lettre.").is_some());
        h.get_by_label("Confirmer").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::AnswerCard { token: "c2".into(), approve: true }]);
        h.get_by_label("Annuler").click();
        h.run();
        assert_eq!(h.state().actions.last(), Some(&PanelAction::AnswerCard { token: "c2".into(), approve: false }));
        assert!(matches!(
            PanelAction::AnswerCard { token: "c2".into(), approve: true }.into_input(),
            Input::AnswerCard { token, approve: true } if token == "c2"
        ));
        assert!(matches!(PanelAction::OpenLink("/x".into()).into_input(), Input::OpenLink(url) if url == "/x"));
    }

    /// An app that does not carry a card's answer (NeoPDF) says why in one line instead of buttons that do nothing; the
    /// card's words and its Open stay.
    #[test]
    fn where_the_app_takes_no_answer_a_waiting_card_says_why_and_has_no_buttons() {
        let cards = json!([{ "token": "c2", "kind": "Template", "status": "Pending", "title": "Modèle de lettre",
            "conversation_id": "t1",
            "payload": { "summary": "« Lettre.docx » devient le modèle de lettre.", "confirm_label": "Confirmer",
                         "refuse_label": "Annuler", "choices": [{ "id": "Bureau", "label": "Bureau" }] } }]);
        let mut h = harness(replied_with("Confirmez sur la carte.", cards));
        h.state_mut().panel.cards_unanswered = Some("Answer this card in Nora's chat: this editor does not take it.".into());
        h.run();
        assert!(h.query_by_label("« Lettre.docx » devient le modèle de lettre.").is_some(), "the card is read");
        assert!(h.query_by_label("Answer this card in Nora's chat: this editor does not take it.").is_some());
        for button in ["Confirmer", "Annuler", "Bureau"] {
            assert!(h.query_by_label(button).is_none(), "{button}");
        }
        assert!(h.state().actions.is_empty());
        // A card that waits for nobody shows no such line.
        let done = json!([{ "token": "c1", "kind": "Document", "status": "Executed", "title": "Brouillon",
            "conversation_id": "t1", "payload": {} }]);
        let mut h = harness(replied_with("C'est prêt.", done));
        h.state_mut().panel.cards_unanswered = Some("Answer this card in Nora's chat: this editor does not take it.".into());
        h.run();
        assert!(h.query_by_label("Answer this card in Nora's chat: this editor does not take it.").is_none());
    }

    #[test]
    fn a_mailbox_card_offers_its_choices_and_its_no_but_no_bare_yes() {
        let cards = json!([{ "token": "c3", "kind": "Mailbox", "status": "Pending", "title": "Brouillon d'e-mail",
            "conversation_id": "t1",
            "payload": { "summary": "Vous n'avez pas de boîte à vous : depuis laquelle écrire le brouillon ?",
                         "confirm_label": "Confirmer", "refuse_label": "Annuler",
                         "choices": [{ "id": "Bureau", "label": "Bureau (bureau@exemple.ch)" }] } }]);
        let mut h = opening(replied_with("Choisissez la boîte sur la carte.", cards));
        assert!(h.query_by_label("Confirmer").is_none(), "a card that asks for a choice has no bare yes");
        h.get_by_label("Bureau (bureau@exemple.ch)").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::ChooseOnCard { token: "c3".into(), choice: "Bureau".into() }]);
        h.get_by_label("Annuler").click();
        h.run();
        assert_eq!(h.state().actions.last(), Some(&PanelAction::AnswerCard { token: "c3".into(), approve: false }));
        assert!(matches!(
            PanelAction::ChooseOnCard { token: "c3".into(), choice: "Bureau".into() }.into_input(),
            Input::ChooseOnCard { token, choice } if token == "c3" && choice == "Bureau"
        ));
    }
}
