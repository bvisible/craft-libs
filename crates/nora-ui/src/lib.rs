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
//! **How it looks** is the Neoffice design system's, the same in every editor and in NeoMail, light and dark ([`look`]):
//! the warm paper for the canvas, ink for the person's words and for every action, Nora's replies on white cards, the
//! signal as the tint of what Nora is doing, Karla for the words and Forum for her name, Lucide's icons, and her own
//! sphere ([`orb`]). It follows the egui theme only to know whether it is light or dark. Everything the user reads is in
//! [`NoraLabels`] and the other word lists (English by default); an app passes its own translations.

mod icons;
pub mod launcher;
mod look;
mod markdown;
pub mod orb;
mod orb_rings;
pub mod proposal;
pub mod voice;

pub use icons::{Icon, paint as paint_icon};
pub use launcher::{LauncherWords, PaneMemory};
pub use proposal::Context as ProposalContext;
pub use proposal::{Choice, ConsentWords, ProposalAction, ProposalCard, ProposalTone, ProposalWords};
pub use voice::VoiceWords;

use std::time::Duration;

use craft_nora::{
    Activity, ChatMessage, Confirmation, Correction, DocActionKind, Input, NoraError, NoraSession, Output, Role, Segment, ShownCard, segments,
};
use egui::{
    Align, Button, CentralPanel, Color32, Context, CornerRadius, CursorIcon, Event, FontFamily, FontId, Frame, Id, Key, KeyboardShortcut, Label,
    Layout, Margin, Modifiers, Panel, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Visuals, pos2, vec2,
};

use look::{Face, Fonts, Palette, RADIUS_LG, RADIUS_MD, RADIUS_XL};

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
    /// The user dictated a request: what was recorded, to be turned into words by the server (the session asks for it,
    /// [`Input::Transcribe`]); the words come back as [`NoraSession::dictation`].
    Dictate { audio_base64: String, mime: String },
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
            PanelAction::Dictate { audio_base64, mime } => Input::Transcribe { audio_base64, mime },
        }
    }
}

/// What an app draws in the panel while the conversation is empty ([`NoraPanel::show_with`]): it gets the `Ui` and the
/// hint's words, and answers the request the person picked, if any.
pub type EmptyState<'a> = &'a mut dyn FnMut(&mut Ui, &str) -> Option<String>;

/// The words of the panel. English by default; an app fills in its own language.
#[derive(Debug, Clone)]
pub struct NoraLabels {
    /// Her name, at the head of the panel beside her sphere; leave it empty when the app draws its own title.
    pub title: String,
    pub new_conversation: String,
    pub undo: String,
    pub undo_hint: String,
    pub placeholder: String,
    pub send: String,
    /// In place of Send while Nora works.
    pub stop: String,
    /// Nora is working, in one line. The pane now says what she is doing ([`ActivityWords`]); kept for apps that set it.
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

/// The other words of the pane: its close button, its greeting, the requests it offers on an empty page. A field of its
/// own, so that an app built on [`NoraLabels`] keeps building.
#[derive(Debug, Clone)]
pub struct PaneWords {
    pub close: String,
    /// The button under what Nora changed, which takes the change back: « Undo this change ».
    pub undo_change: String,
    /// Above the hint, while the conversation is empty (set in Forum, as the design system sets a title).
    pub greeting: String,
    /// Ready requests, one chip each, while the conversation is empty and the app draws nothing of its own: a click
    /// sends it as if it had been typed.
    pub suggestions: Vec<String>,
}

impl Default for PaneWords {
    fn default() -> Self {
        PaneWords { close: "Close".into(), undo_change: "Undo this change".into(), greeting: "How can I help?".into(), suggestions: Vec::new() }
    }
}

/// What the line under the conversation says while Nora works, by what she is doing ([`NoraSession::activity`]). English
/// by default; an app sets its own words here.
#[derive(Debug, Clone)]
pub struct ActivityWords {
    pub thinking: String,
    pub reading: String,
    pub writing: String,
    pub handing: String,
    /// After the seconds a turn has taken: « s ».
    pub seconds: String,
}

impl Default for ActivityWords {
    fn default() -> Self {
        ActivityWords {
            thinking: "Nora is thinking…".into(),
            reading: "Nora is reading the document…".into(),
            writing: "Nora is editing the document…".into(),
            handing: "Nora is finishing up…".into(),
            seconds: "s".into(),
        }
    }
}

/// Colours an app may still set; one left at `None` is the design system's.
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
    /// draws them in the pane's own Karla Bold (or, where that is not there yet, twice in the regular one, a pixel
    /// apart). Only a family the app binds: egui stops on one it does not know.
    pub bold_family: Option<&'static str>,
}

impl Default for NoraStyle {
    fn default() -> Self {
        NoraStyle {
            user_fill: None,
            user_text: None,
            nora_fill: None,
            nora_text: None,
            corner_radius: RADIUS_XL,
            bubble_padding: 10,
            max_bubble_share: 0.88,
            bold_family: None,
        }
    }
}

/// What the pane is drawn with this frame: the design system's palette for the theme, the fonts, and the app's own
/// colours where it set some.
struct Skin {
    pal: Palette,
    fonts: Fonts,
    user_fill: Color32,
    user_text: Color32,
    nora_fill: Color32,
    nora_text: Color32,
    corner_radius: u8,
    bubble_padding: i8,
    bold_family: Option<&'static str>,
}

impl Skin {
    fn new(style: &NoraStyle, ctx: &Context, dark: bool) -> Skin {
        let pal = Palette::of(dark);
        let fonts = look::install_fonts(ctx);
        Skin {
            user_fill: style.user_fill.unwrap_or(pal.primary),
            user_text: style.user_text.unwrap_or(pal.primary_fg),
            nora_fill: style.nora_fill.unwrap_or(pal.card),
            nora_text: style.nora_text.unwrap_or(pal.text),
            corner_radius: style.corner_radius,
            bubble_padding: style.bubble_padding,
            bold_family: style.bold_family,
            pal,
            fonts,
        }
    }

    fn font(&self, face: Face, size: f32) -> FontId {
        self.fonts.font(face, size)
    }

    fn text(&self, words: impl Into<String>, face: Face, size: f32, color: Color32) -> RichText {
        RichText::new(words).font(self.font(face, size)).color(color)
    }
}

/// How strong the panel's weak words are (the greeting, the text box's hint, the notes under a turn), and its buttons that
/// cannot be used yet (« Undo », « New conversation », « Send » with nothing typed): egui's 60 % and 50 % of the text's
/// colour fell under 4.5:1 on a dark pane.
const WEAK_TEXT: f32 = 0.75;
const DISABLED_ALPHA: f32 = 0.7;

/// The panel's words in the app's text colour (`NoraStyle::nora_text`), its weak words and its buttons that cannot be used
/// yet at [`WEAK_TEXT`] and [`DISABLED_ALPHA`] of it. An app that leaves egui's text colours to egui for a theme (its own
/// set for the other, the page then following the system into dark) had the whole panel in egui's grey: some 2:1 on its
/// dark pane.
fn legible(visuals: &mut Visuals, text: Color32) {
    visuals.override_text_color = Some(text);
    visuals.weak_text_color = Some(text.gamma_multiply(WEAK_TEXT));
    visuals.disabled_alpha = visuals.disabled_alpha.max(DISABLED_ALPHA);
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
    /// The close button, the greeting, the ready requests.
    pub pane_words: PaneWords,
    /// What the line under the conversation says of what Nora is doing.
    pub activity_words: ActivityWords,
    /// The words of the microphone.
    pub voice_words: VoiceWords,
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
    /// The app's pane has no close button of its own: the panel draws one in its head, and says when it was clicked
    /// ([`NoraPanel::take_close_request`]).
    pub closable: bool,
    /// Whether the person may dictate (the microphone), where the browser has one: an app whose server cannot turn
    /// speech into words turns it off. On by default.
    pub dictation: bool,
    close_requested: bool,
    draft: String,
    focus_input: bool,
    scroll_to_end: bool,
    /// Where the transcript's end was when it was last drawn.
    end_offset: f32,
    /// The host's words for a failure of the connection to Nora; English (the error's own sentence) without them.
    error_words: Option<fn(&NoraError) -> String>,
    /// When the turn under way began, by the frame clock, to say how long it has taken.
    busy_since: Option<f64>,
    voice: voice::Voice,
    /// The last words of a dictation put in the box, so that each is put once.
    dictation_seen: u64,
    /// The dictation, once transcribed, is sent without the person's click (they stopped it with the send button).
    dictate_then_send: bool,
    /// What the microphone has to say (refused, nothing heard…), and when it began to be said.
    voice_notice: Option<(voice::Recorded, f64)>,
}

impl NoraPanel {
    pub fn new() -> Self {
        NoraPanel { dictation: true, ..Self::default() }
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

    /// Whether the person clicked the panel's close button since the last call ([`NoraPanel::closable`]).
    pub fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Whether the microphone is on offer: the app allows it and the browser has one.
    pub fn can_dictate(&self) -> bool {
        self.dictation && self.voice.available()
    }

    /// Draws the panel and returns what the user did this frame.
    pub fn show(&mut self, ui: &mut Ui, session: &NoraSession) -> Vec<PanelAction> {
        self.show_with(ui, session, None)
    }

    /// Draws the panel like [`show`](Self::show), with what the app draws while the conversation is empty: `empty` is
    /// given the `Ui` of the conversation's area, under the header where the hint goes, and the hint's
    /// words ([`NoraLabels::empty`]) to place where it wants. A request it returns is sent as if the person had typed
    /// it (`PanelAction::Send`). Once there is a message to read, or Nora works, it is not drawn.
    pub fn show_with(&mut self, ui: &mut Ui, session: &NoraSession, empty: Option<EmptyState<'_>>) -> Vec<PanelAction> {
        let mut actions = Vec::new();
        let dark = ui.visuals().dark_mode;
        let skin = Skin::new(&self.style, ui.ctx(), dark);
        let now = ui.input(|input| input.time);
        self.track(session, now);
        self.take_dictation(session);
        self.poll_voice(now, &mut actions);
        // The panel's own look, for the length of the panel: the app's visuals and style are as they were after.
        let kept_visuals = ui.visuals().clone();
        let kept_style = ui.style().clone();
        paint_over(ui, &skin);
        let area = ui.max_rect();
        ui.painter().rect_filled(area, 0.0, skin.pal.bg);
        // (`make_persistent_id`: the same id as `ui.id().with(…)` on egui 0.36, and not deprecated on 0.37.)
        let id = ui.make_persistent_id("nora");
        self.header(ui, session, &skin, now, &mut actions);
        // The text box is a bottom panel, so that its height follows what is typed in it; the
        // transcript takes what is left above it.
        Panel::bottom(id.with("input-panel"))
            .frame(Frame::new().inner_margin(Margin { left: 12, right: 12, top: 8, bottom: 12 }))
            .show_separator_line(false)
            .show(ui, |ui| self.composer(ui, id.with("input"), session, &skin, now, &mut actions));
        CentralPanel::no_frame().show(ui, |ui| self.transcript(ui, session, &skin, now, &mut actions, empty));
        ui.set_style(kept_style);
        *ui.visuals_mut() = kept_visuals;
        actions
    }

    /// Notes when the turn began, to say how long it has taken.
    fn track(&mut self, session: &NoraSession, now: f64) {
        if session.is_busy() {
            self.busy_since.get_or_insert(now);
        } else {
            self.busy_since = None;
        }
    }

    /// What the microphone did since the last frame: a recording that is over goes to the server as a dictation; one that
    /// came to nothing, or could not be made, is said once, under the composer.
    fn poll_voice(&mut self, now: f64, actions: &mut Vec<PanelAction>) {
        if let Some(recorded) = self.voice.poll(now)
            && let voice::Recorded::Audio { bytes, mime } = recorded
        {
            actions.push(PanelAction::Dictate { audio_base64: craft_nora::protocol::base64(&bytes), mime });
        }
        if let Some(said) = self.voice.take_notice() {
            self.voice_notice = Some((said, now));
        }
        if self.voice_notice.as_ref().is_some_and(|(_, since)| now - since > 8.0) {
            self.voice_notice = None;
        }
    }

    /// Puts the words of a dictation in the box, once (and sends them when the person stopped with the send button).
    fn take_dictation(&mut self, session: &NoraSession) {
        let Some(done) = session.dictation() else { return };
        if done.id <= self.dictation_seen {
            return;
        }
        self.dictation_seen = done.id;
        self.voice.finished();
        if let Some(words) = done.text.as_deref().map(str::trim).filter(|words| !words.is_empty()) {
            if !self.draft.trim().is_empty() && !self.draft.ends_with(char::is_whitespace) {
                self.draft.push(' ');
            }
            self.draft.push_str(words);
            self.focus_input = true;
        } else {
            self.voice.say_nothing_heard();
        }
    }

    fn header(&mut self, ui: &mut Ui, session: &NoraSession, skin: &Skin, now: f64, actions: &mut Vec<PanelAction>) {
        let pal = &skin.pal;
        let busy = session.is_busy();
        Frame::new().inner_margin(Margin { left: 14, right: 8, top: 10, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (mark, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                orb::paint(ui.painter(), mark, pal.orb, busy.then_some(now as f32));
                if !self.labels.title.is_empty() {
                    ui.add(Label::new(skin.text(self.labels.title.as_str(), Face::Title, 21.0, pal.text)).selectable(false));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if self.closable && ghost_icon(ui, skin, Icon::Close, &self.pane_words.close, true).clicked() {
                        self.close_requested = true;
                    }
                    let started = session.thread_id().is_some() || !session.transcript().is_empty();
                    if ghost_icon(ui, skin, Icon::NewChat, &self.labels.new_conversation, started).clicked() {
                        actions.push(PanelAction::NewConversation);
                    }
                });
            });
        });
        let line = ui.cursor().min.y;
        ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, pal.border));
        ui.add_space(1.0);
    }

    fn transcript(
        &mut self,
        ui: &mut Ui,
        session: &NoraSession,
        skin: &Skin,
        now: f64,
        actions: &mut Vec<PanelAction>,
        empty: Option<EmptyState<'_>>,
    ) {
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
        let pal = &skin.pal;
        let shown = area.show_viewport(ui, |ui, viewport| {
            at = viewport.min.y;
            ui.spacing_mut().item_spacing.y = 10.0;
            Frame::new().inner_margin(Margin { left: 14, right: 14, top: 12, bottom: 12 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if session.transcript().is_empty() && !session.is_busy() {
                    self.greeting(ui, skin, actions, empty);
                }
                // The cards of the conversation, each under the message it came after; those of the turn that runs, at the end.
                let cards = session.cards();
                let transcript = session.transcript();
                for shown in cards.iter().filter(|shown| shown.after == Some(0)) {
                    self.card(ui, session, shown, skin, actions);
                }
                for (index, message) in transcript.iter().enumerate() {
                    // A reply written before Nora knew that what she asked of the document was refused: never shown as done.
                    match session.correction(index) {
                        Some(Correction::NothingDone) => self.notice(ui, &self.correction_words.nothing_done, skin, false),
                        Some(Correction::PartlyDone) => {
                            self.bubble(ui, session, message, skin, actions);
                            self.notice(ui, &self.correction_words.partly_done, skin, false);
                        }
                        None => self.bubble(ui, session, message, skin, actions),
                    }
                    for shown in cards.iter().filter(|shown| shown.after == Some(index + 1)) {
                        self.card(ui, session, shown, skin, actions);
                    }
                }
                for shown in cards.iter().filter(|shown| shown.after.is_none_or(|after| after > transcript.len())) {
                    self.card(ui, session, shown, skin, actions);
                }
                let confirmations = session.confirmations();
                for confirmation in &confirmations {
                    self.confirmation(ui, confirmation, skin, actions);
                }
                // Nora waiting for the user's yes or no is not Nora working.
                if session.is_busy() && confirmations.is_empty() {
                    self.working(ui, session, skin, now);
                }
                if session.stopped() {
                    ui.add(Label::new(skin.text(self.labels.stopped.as_str(), Face::Body, 12.5, pal.weak)));
                }
                if let Some(error) = session.last_error() {
                    let said = match (session.last_failure(), self.error_words) {
                        (Some(failure), Some(words)) => words(failure),
                        _ => error.to_string(),
                    };
                    self.notice(ui, &said, skin, true);
                }
                // A turn lost to a page that ran nothing: the same request again, on the person's click only (a new turn).
                if let Some(request) = session.resumable() {
                    self.notice(ui, &self.resume_words.said, skin, false);
                    if pill(ui, skin, &self.resume_words.resume, Weight::Quiet, true).clicked() {
                        actions.push(PanelAction::Send(request.to_string()));
                    }
                }
                // What Nora changed can be taken back, right under what she said of it.
                if session.can_undo() && !session.is_busy() {
                    let response = pill_with_icon(ui, skin, Icon::Undo, &self.pane_words.undo_change);
                    if response.on_hover_text(self.labels.undo_hint.as_str()).clicked() {
                        actions.push(PanelAction::Undo);
                    }
                }
            });
        });
        let end = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
        // Asked again next frame while the view is short of the end (something came in the same frame).
        self.scroll_to_end &= at < end - 0.5;
        self.end_offset = end;
    }

    /// The empty page: her sphere, a greeting in Forum, the hint, and the requests on offer.
    fn greeting(&mut self, ui: &mut Ui, skin: &Skin, actions: &mut Vec<PanelAction>, empty: Option<EmptyState<'_>>) {
        let pal = &skin.pal;
        ui.add_space(ui.spacing().item_spacing.y * 2.0);
        ui.vertical_centered(|ui| {
            let (mark, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
            orb::paint(ui.painter(), mark, pal.orb, None);
            ui.add_space(4.0);
            ui.add(Label::new(skin.text(self.pane_words.greeting.as_str(), Face::Title, 23.0, pal.text)).selectable(false));
        });
        match empty {
            Some(draw) => {
                ui.add_space(ui.spacing().item_spacing.y);
                if let Some(request) = draw(ui, self.labels.empty.as_str()) {
                    actions.push(PanelAction::Send(request));
                }
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add(Label::new(skin.text(self.labels.empty.as_str(), Face::Body, 13.5, pal.weak)).wrap().selectable(false));
                });
                if !self.pane_words.suggestions.is_empty() {
                    ui.add_space(ui.spacing().item_spacing.y);
                    for suggestion in &self.pane_words.suggestions {
                        if chip(ui, skin, suggestion).clicked() {
                            actions.push(PanelAction::Send(suggestion.clone()));
                        }
                    }
                }
            }
        }
    }

    /// What Nora is doing while she works: her sphere turning, in a pill of the signal's tint, and how long it has taken.
    fn working(&self, ui: &mut Ui, session: &NoraSession, skin: &Skin, now: f64) {
        let pal = &skin.pal;
        let words = match session.activity() {
            Activity::Thinking => &self.activity_words.thinking,
            Activity::Reading => &self.activity_words.reading,
            Activity::Writing => &self.activity_words.writing,
            Activity::Handing => &self.activity_words.handing,
        };
        let seconds = self.busy_since.map_or(0, |since| (now - since).max(0.0) as u64);
        Frame::new()
            .fill(pal.signal_soft)
            .stroke(Stroke::new(1.0, pal.signal_border))
            .corner_radius(CornerRadius::same(255))
            .inner_margin(Margin { left: 8, right: 14, top: 6, bottom: 6 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (mark, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                    orb::paint(ui.painter(), mark, pal.orb, Some(now as f32));
                    ui.add(Label::new(skin.text(words, Face::Medium, 13.0, pal.text)).selectable(false));
                    if seconds >= 3 {
                        ui.add(
                            Label::new(skin.text(format!("{seconds} {}", self.activity_words.seconds), Face::Body, 12.5, pal.weak)).selectable(false),
                        );
                    }
                });
            });
        // The sphere turns: the frames keep coming, about thirty a second.
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }

    /// A word of the panel itself, in the warning colour (or the error's).
    fn notice(&self, ui: &mut Ui, text: &str, skin: &Skin, error: bool) {
        let pal = &skin.pal;
        let (fill, ink) = if error { (pal.error_fill, pal.error_text) } else { (pal.warn_fill, pal.warn_text) };
        Frame::new().fill(fill).corner_radius(CornerRadius::same(RADIUS_MD)).inner_margin(Margin::symmetric(12, 9)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(Label::new(skin.text(text, Face::Body, 13.0, ink)).wrap());
        });
    }

    fn bubble(&self, ui: &mut Ui, session: &NoraSession, message: &ChatMessage, skin: &Skin, actions: &mut Vec<PanelAction>) {
        if message.text.trim().is_empty() {
            return;
        }
        let pal = &skin.pal;
        if message.role == Role::System {
            ui.vertical_centered(|ui| ui.add(Label::new(skin.text(message.text.as_str(), Face::Body, 12.5, pal.weak)).wrap()));
            return;
        }
        let from_user = message.role == Role::User;
        let max_width = (ui.available_width() * self.style.max_bubble_share).max(80.0);
        let side = if from_user { Align::Max } else { Align::Min };
        ui.with_layout(Layout::top_down(side), |ui| {
            if from_user {
                // The person's words: ink on the right, its corner at the speaker squared off.
                let radius = CornerRadius { nw: skin.corner_radius, ne: skin.corner_radius, sw: skin.corner_radius, se: 4 };
                Frame::new().fill(skin.user_fill).corner_radius(radius).inner_margin(Margin::symmetric(13, skin.bubble_padding.min(9))).show(
                    ui,
                    |ui| {
                        ui.set_max_width(max_width);
                        ui.add(Label::new(skin.text(message.text.as_str(), Face::Body, 14.0, skin.user_text)).wrap());
                    },
                );
                return;
            }
            // Nora's reply: a white card of the pane, its corner at her squared off.
            let radius = CornerRadius { nw: 4, ne: RADIUS_LG, sw: RADIUS_LG, se: RADIUS_LG };
            Frame::new()
                .fill(skin.nora_fill)
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(radius)
                .inner_margin(Margin::symmetric(13, 11))
                .show(ui, |ui| {
                    ui.set_max_width(max_width);
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
                    let body = skin.font(Face::Body, 14.0);
                    let base = egui::text::TextFormat { line_height: Some(20.0), ..egui::text::TextFormat::simple(body.clone(), skin.nora_text) };
                    let bold = match (skin.bold_family, skin.fonts.bold_family()) {
                        (Some(family), _) | (None, Some(family)) => markdown::Bold::Font(FontId::new(body.size, FontFamily::Name(family.into()))),
                        (None, None) => markdown::Bold::Doubled,
                    };
                    // Laid out here, at the width a wrapping label takes, so that the bold words drawn again fall on it.
                    let width = ui.available_width();
                    let mut job = markdown::job(&words, &base, &bold, pal.quiet);
                    job.wrap.max_width = width;
                    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
                    let shown = ui.add(Label::new(galley));
                    // No bold face: the bold words a second time, one pixel to the right (markdown::bold_overlay).
                    if bold == markdown::Bold::Doubled
                        && let Some(mut overlay) = markdown::bold_overlay(&words, &base)
                    {
                        overlay.wrap.max_width = width;
                        let overlay = ui.ctx().fonts_mut(|fonts| fonts.layout_job(overlay));
                        let pixel = vec2(1.0 / ui.pixels_per_point(), 0.0);
                        ui.painter().galley(shown.rect.left_top() + pixel, overlay, skin.nora_text);
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
    }

    /// A card of the conversation: what it is, what it says, its Open when it leads to a page of the instance, and the
    /// person's answer when it waits for them: yes or no (a letter to make the company's template), or one of its
    /// choices or no (a mailbox to write a draft from).
    fn card(&self, ui: &mut Ui, session: &NoraSession, shown: &ShownCard, skin: &Skin, actions: &mut Vec<PanelAction>) {
        let pal = &skin.pal;
        let card = &shown.card;
        let waits = card.awaits_the_person();
        let (fill, stroke) = if waits { (pal.signal_soft, pal.signal_border) } else { (pal.card, pal.border) };
        Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(CornerRadius::same(RADIUS_LG))
            .inner_margin(Margin::symmetric(13, 11))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                if !card.title.is_empty() {
                    ui.add(Label::new(skin.text(card.title.as_str(), Face::Bold, 14.0, pal.text)).wrap());
                }
                if let Some(summary) = &card.summary {
                    ui.add(Label::new(skin.text(summary.as_str(), Face::Body, 13.5, pal.text_2)).wrap());
                }
                if let Some(result) = card.result.as_deref().filter(|_| !waits) {
                    ui.add(Label::new(skin.text(result, Face::Body, 12.5, pal.weak)).wrap());
                }
                if waits && let Some(why) = &self.cards_unanswered {
                    ui.add(Label::new(skin.text(why.as_str(), Face::Body, 12.5, pal.weak)).wrap());
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    if waits && self.cards_unanswered.is_none() {
                        let refuse = card.refuse_label.as_deref().unwrap_or(self.labels.refuse.as_str());
                        if card.choices.is_empty() {
                            let confirm = card.confirm_label.as_deref().unwrap_or(self.labels.allow.as_str());
                            if pill(ui, skin, confirm, Weight::Primary, !shown.answering).clicked() {
                                actions.push(PanelAction::AnswerCard { token: card.token.clone(), approve: true });
                            }
                        }
                        for choice in &card.choices {
                            if pill(ui, skin, choice.label.as_str(), Weight::Primary, !shown.answering).clicked() {
                                actions.push(PanelAction::ChooseOnCard { token: card.token.clone(), choice: choice.id.clone() });
                            }
                        }
                        if pill(ui, skin, refuse, Weight::Quiet, !shown.answering).clicked() {
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
                        if pill(ui, skin, words, Weight::Quiet, true).on_hover_text(url).clicked() {
                            actions.push(PanelAction::OpenLink(url.to_string()));
                        }
                    }
                });
            });
    }

    fn confirmation(&self, ui: &mut Ui, confirmation: &Confirmation, skin: &Skin, actions: &mut Vec<PanelAction>) {
        let pal = &skin.pal;
        Frame::new()
            .fill(pal.signal_soft)
            .stroke(Stroke::new(1.0, pal.signal_border))
            .corner_radius(CornerRadius::same(RADIUS_LG))
            .inner_margin(Margin::symmetric(13, 11))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.add(Label::new(skin.text(self.labels.confirm_heading.as_str(), Face::Body, 12.5, pal.weak)));
                ui.add(Label::new(skin.text((self.labels.describe)(&confirmation.kind), Face::Medium, 14.0, pal.text)).wrap());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if pill(ui, skin, &self.labels.allow, Weight::Primary, true).clicked() {
                        actions.push(PanelAction::Confirm { id: confirmation.id, accepted: true });
                    }
                    if pill(ui, skin, &self.labels.refuse, Weight::Quiet, true).clicked() {
                        actions.push(PanelAction::Confirm { id: confirmation.id, accepted: false });
                    }
                });
            });
    }

    /// The composer: a white card with the text box, and under it the microphone and the send button. While Nora works
    /// (`busy`) the message waits in the box, as in the desk's chat, which locks its field until the answer is in, and the
    /// button stops Nora.
    fn composer(&mut self, ui: &mut Ui, input_id: Id, session: &NoraSession, skin: &Skin, now: f64, actions: &mut Vec<PanelAction>) {
        let pal = &skin.pal;
        let busy = session.is_busy();
        if let Some(reason) = self.offline.as_deref() {
            self.focus_input = false;
            ui.add(Label::new(skin.text(reason, Face::Body, 13.0, pal.weak)).wrap());
            return;
        }
        if std::mem::take(&mut self.focus_input) {
            ui.memory_mut(|memory| memory.request_focus(input_id));
        }
        if let Some((said, _)) = &self.voice_notice {
            let words = match said {
                voice::Recorded::Denied => &self.voice_words.denied,
                voice::Recorded::Missing => &self.voice_words.no_microphone,
                voice::Recorded::Nothing => &self.voice_words.nothing_heard,
                _ => &self.voice_words.failed,
            };
            self.notice(ui, words, skin, false);
        }
        let focused = ui.memory(|memory| memory.has_focus(input_id));
        let (border, width) = if focused { (pal.signal, 1.5) } else { (pal.border_strong, 1.0) };
        let mut send_clicked = false;
        let mut typing = false;
        let listening = self.voice.listening();
        let frame = Frame::new()
            .fill(pal.card)
            .stroke(Stroke::new(width, border))
            .corner_radius(CornerRadius::same(RADIUS_XL))
            .inner_margin(Margin { left: 14, right: 8, top: 10, bottom: 8 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                if listening || self.voice.transcribing() {
                    self.listening_row(ui, skin, now);
                } else {
                    // Plain Enter is handled below: only Shift+Enter is a new line.
                    let hint = skin.text(self.labels.placeholder.as_str(), Face::Body, 14.0, pal.weak);
                    let output = ScrollArea::vertical()
                        .id_salt("nora-draft")
                        .max_height(132.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            TextEdit::multiline(&mut self.draft)
                                .id(input_id)
                                .font(skin.font(Face::Body, 14.0))
                                .text_color(pal.text)
                                .hint_text(hint)
                                .frame(Frame::NONE)
                                .margin(Margin::ZERO)
                                .desired_rows(1)
                                .desired_width(f32::INFINITY)
                                .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                                .show(ui)
                        })
                        .inner;
                    typing = output.response.has_focus();
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let sending = self.voice.listening();
                        if busy {
                            if solid_icon(ui, skin, Icon::Stop, &self.labels.stop, true, true).clicked() {
                                actions.push(PanelAction::Stop);
                            }
                        } else if sending {
                            // Stopped by the send arrow: what was said goes to Nora as soon as it is words.
                            if solid_icon(ui, skin, Icon::ArrowUp, &self.labels.send, true, false).clicked() {
                                self.dictate_then_send = true;
                                self.voice.stop();
                            }
                        } else {
                            let can_send = !self.draft.trim().is_empty() && !self.voice.transcribing();
                            send_clicked = solid_icon(ui, skin, Icon::ArrowUp, &self.labels.send, can_send, false).clicked();
                        }
                        if self.can_dictate() && !busy {
                            self.mic_button(ui, skin);
                        }
                    });
                });
            });
        if focused {
            // The design system's focus ring: the signal at a third of its strength, three points out.
            ui.painter().rect_stroke(
                frame.response.rect.expand(1.5),
                CornerRadius::same(RADIUS_XL + 2),
                Stroke::new(3.0, pal.signal.gamma_multiply(0.3)),
                StrokeKind::Outside,
            );
        }
        // After the text box, so that text typed in the same frame is already in the draft.
        let enter = typing && take_enter(ui);
        if (send_clicked || enter) && !busy && !self.draft.trim().is_empty() {
            let text = std::mem::take(&mut self.draft);
            actions.push(PanelAction::Send(text.trim().to_string()));
            self.scroll_to_end = true;
        }
        // A dictation that was stopped with the send arrow goes as soon as its words are in the box.
        if self.dictate_then_send && !self.voice.transcribing() && !self.voice.listening() && self.voice.settled() {
            self.dictate_then_send = false;
            if !busy && !self.draft.trim().is_empty() {
                let text = std::mem::take(&mut self.draft);
                actions.push(PanelAction::Send(text.trim().to_string()));
                self.scroll_to_end = true;
            }
        }
    }

    /// The microphone button: one click to speak, one to stop (what was said goes into the box).
    fn mic_button(&mut self, ui: &mut Ui, skin: &Skin) {
        if self.voice.listening() {
            if solid_icon_quiet(ui, skin, Icon::Stop, &self.voice_words.stop, true).clicked() {
                self.voice.stop();
            }
        } else if ghost_icon(ui, skin, Icon::Mic, &self.voice_words.dictate, !self.voice.transcribing()).clicked() {
            self.voice.start(ui.ctx());
        }
    }

    /// What the composer shows while the microphone is open (the level of the voice, the time) or while the words come.
    fn listening_row(&mut self, ui: &mut Ui, skin: &Skin, now: f64) {
        let pal = &skin.pal;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if self.voice.transcribing() {
                let (mark, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                orb::paint(ui.painter(), mark, pal.orb, Some(now as f32));
                ui.add(Label::new(skin.text(self.voice_words.transcribing.as_str(), Face::Medium, 13.5, pal.text_2)).selectable(false));
            } else {
                if ghost_icon(ui, skin, Icon::Close, &self.voice_words.cancel, true).clicked() {
                    self.voice.cancel();
                }
                let level = self.voice.level();
                let (bars, _) = ui.allocate_exact_size(vec2((ui.available_width() - 58.0).max(40.0), 26.0), Sense::hover());
                paint_level(ui, skin, bars, now as f32, level);
                let seconds = self.voice.seconds(now);
                ui.add(Label::new(skin.text(format!("{}:{:02}", seconds / 60, seconds % 60), Face::Medium, 13.0, pal.text_2)).selectable(false));
            }
        });
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

/// The bars that move with the voice: the design system's signal for the voice that is heard, the paper's line for the rest.
fn paint_level(ui: &Ui, skin: &Skin, rect: egui::Rect, now: f32, level: f32) {
    let pal = &skin.pal;
    let count = (rect.width() / 5.0).floor().max(1.0) as usize;
    let mid = rect.center().y;
    for index in 0..count {
        // Newer bars at the right: a ripple that follows the voice, from a slow wave of the clock.
        let phase = index as f32 / count as f32;
        let wave = ((now * 6.0 - phase * 14.0).sin() * 0.5 + 0.5).powf(1.4);
        let height = (3.0 + (rect.height() - 3.0) * (0.12 + 0.88 * level.clamp(0.0, 1.0) * wave) * phase.max(0.2)).min(rect.height());
        let x = rect.min.x + 2.0 + index as f32 * 5.0;
        let color = if level > 0.04 { pal.signal } else { pal.border_strong };
        ui.painter().rect_filled(egui::Rect::from_center_size(pos2(x, mid), vec2(2.5, height)), CornerRadius::same(2), color);
    }
}

/// The weight of a button: the primary action is ink; the rest are quiet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Weight {
    Primary,
    Quiet,
}

/// The pane's text button, from the design system's: radius 10, Karla Medium, ink for the primary action.
fn pill(ui: &mut Ui, skin: &Skin, label: &str, weight: Weight, enabled: bool) -> Response {
    let pal = &skin.pal;
    ui.scope(|ui| {
        let (fill, hover, ink, stroke) = match weight {
            Weight::Primary => (pal.primary, pal.primary_hover, pal.primary_fg, Stroke::NONE),
            Weight::Quiet => (Color32::TRANSPARENT, pal.quiet, pal.text, Stroke::new(1.0, pal.border_strong)),
        };
        let visuals = ui.visuals_mut();
        for (widget, fill) in [(&mut visuals.widgets.inactive, fill), (&mut visuals.widgets.hovered, hover), (&mut visuals.widgets.active, hover)] {
            widget.weak_bg_fill = fill;
            widget.bg_fill = fill;
            widget.bg_stroke = stroke;
            widget.fg_stroke = Stroke::new(1.0, ink);
            widget.corner_radius = CornerRadius::same(RADIUS_MD);
            widget.expansion = 0.0;
        }
        ui.spacing_mut().button_padding = vec2(14.0, 6.0);
        ui.add_enabled(enabled, Button::new(skin.text(label, Face::Medium, 13.0, ink)).min_size(vec2(0.0, 30.0)))
            .on_hover_cursor(CursorIcon::PointingHand)
    })
    .inner
}

/// A quiet text button with an icon before its words (the undo under what Nora changed).
fn pill_with_icon(ui: &mut Ui, skin: &Skin, icon: Icon, label: &str) -> Response {
    let pal = &skin.pal;
    let galley = ui.painter().layout_no_wrap(label.to_string(), skin.font(Face::Medium, 13.0), pal.text);
    let size = vec2(12.0 + 16.0 + 6.0 + galley.size().x + 14.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() || response.has_focus();
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(RADIUS_MD),
            if hovered { pal.quiet } else { Color32::TRANSPARENT },
            Stroke::new(1.0, pal.border_strong),
            StrokeKind::Inside,
        );
        paint_icon(painter, icon, egui::Rect::from_min_size(pos2(rect.min.x + 12.0, rect.center().y - 8.0), vec2(16.0, 16.0)), pal.text_2, false);
        painter.galley(pos2(rect.min.x + 12.0 + 16.0 + 6.0, rect.center().y - galley.size().y / 2.0), galley, pal.text);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A ready request on the empty page: a pill that sends its words as they are.
fn chip(ui: &mut Ui, skin: &Skin, label: &str) -> Response {
    let pal = &skin.pal;
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        for (widget, fill) in
            [(&mut visuals.widgets.inactive, pal.card), (&mut visuals.widgets.hovered, pal.quiet), (&mut visuals.widgets.active, pal.quiet)]
        {
            widget.weak_bg_fill = fill;
            widget.bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0, pal.border_strong);
            widget.corner_radius = CornerRadius::same(255);
            widget.expansion = 0.0;
        }
        ui.spacing_mut().button_padding = vec2(14.0, 7.0);
        let width = ui.available_width();
        ui.add(Button::new(skin.text(label, Face::Body, 13.5, pal.text_2)).min_size(vec2(width, 0.0))).on_hover_cursor(CursorIcon::PointingHand)
    })
    .inner
}

/// A square icon button with no frame until the pointer is on it (28 points): the header's and the composer's.
fn ghost_icon(ui: &mut Ui, skin: &Skin, icon: Icon, label: &str, enabled: bool) -> Response {
    let pal = &skin.pal;
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let lit = enabled && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on());
        let painter = ui.painter();
        if lit {
            painter.rect_filled(rect, CornerRadius::same(RADIUS_MD), pal.quiet);
        }
        if enabled && response.has_focus() {
            painter.rect_stroke(rect, CornerRadius::same(RADIUS_MD), Stroke::new(1.5, pal.signal), StrokeKind::Inside);
        }
        let ink = if !enabled {
            pal.disabled
        } else if lit {
            pal.text
        } else {
            pal.text_2
        };
        paint_icon(painter, icon, egui::Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), ink, false);
    }
    let response = response.on_hover_text(label);
    if enabled { response.on_hover_cursor(CursorIcon::PointingHand) } else { response }
}

/// A round button of ink with its icon in paper (32 points): send, and stop while Nora works. `filled` draws the icon
/// filled (the stop's square).
fn solid_icon(ui: &mut Ui, skin: &Skin, icon: Icon, label: &str, enabled: bool, filled: bool) -> Response {
    let pal = &skin.pal;
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let lit = enabled && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on());
        let fill = if !enabled {
            pal.quiet
        } else if lit {
            pal.primary_hover
        } else {
            pal.primary
        };
        let ink = if enabled { pal.primary_fg } else { pal.disabled };
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 16.0, fill);
        let size = if filled { 14.0 } else { 18.0 };
        paint_icon(painter, icon, egui::Rect::from_center_size(rect.center(), vec2(size, size)), ink, filled);
    }
    let response = response.on_hover_text(label);
    if enabled { response.on_hover_cursor(CursorIcon::PointingHand) } else { response }
}

/// The round button that stops the microphone: the signal's tint with ink on it (the signal is a field, never a button).
fn solid_icon_quiet(ui: &mut Ui, skin: &Skin, icon: Icon, label: &str, enabled: bool) -> Response {
    let pal = &skin.pal;
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let lit = response.hovered() || response.has_focus();
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 16.0, pal.signal_soft);
        painter.circle_stroke(rect.center(), 15.5, Stroke::new(if lit { 1.5 } else { 1.0 }, pal.signal));
        paint_icon(painter, icon, egui::Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), pal.text, true);
    }
    response.on_hover_text(label).on_hover_cursor(CursorIcon::PointingHand)
}

/// The panel's look for the length of its drawing: selection and cursor in the design system's colours, the widgets'
/// own text in Karla, and the words readable on the pane ([`legible`]).
fn paint_over(ui: &mut Ui, skin: &Skin) {
    let pal = &skin.pal;
    legible(ui.visuals_mut(), pal.text);
    let visuals = ui.visuals_mut();
    visuals.weak_text_color = Some(pal.weak);
    visuals.selection.bg_fill = pal.signal.gamma_multiply(if pal.dark { 0.45 } else { 0.3 });
    visuals.selection.stroke = Stroke::new(1.0, pal.text);
    visuals.text_cursor.stroke = Stroke::new(1.5, pal.text);
    visuals.hyperlink_color = pal.signal_text;
    visuals.code_bg_color = pal.quiet;
    let style = ui.style_mut();
    style.spacing.item_spacing = vec2(8.0, 8.0);
    for (text_style, face, size) in [
        (TextStyle::Body, Face::Body, 14.0),
        (TextStyle::Button, Face::Medium, 13.0),
        (TextStyle::Small, Face::Body, 12.0),
        (TextStyle::Heading, Face::Title, 20.0),
    ] {
        style.text_styles.insert(text_style, skin.font(face, size));
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

/// The session's clock, in milliseconds. In a browser the page's own monotonic clock, which keeps going when the page is
/// hidden (egui's `input.time` is that of the last frame it drew: an editor whose window sits behind another runs no
/// frame, and Nora's pump, called from the app's `logic` then, would read a time that stands still); elsewhere the frame's.
pub fn clock_ms(ctx: &Context) -> u64 {
    #[cfg(target_arch = "wasm32")]
    if let Some(now) = web_sys::window().and_then(|window| window.performance()).map(|performance| performance.now()) {
        return now.max(0.0) as u64;
    }
    (ctx.input(|input| input.time) * 1000.0).max(0.0) as u64
}

/// Feeds the session the frame clock and keeps egui repainting while the session waits for
/// something (a reply, an action). Call it once per frame; the returned outputs are the calls the
/// session wants made now, to be handled like those of [`NoraSession::handle`].
pub fn drive(ctx: &Context, session: &mut NoraSession) -> Vec<Output> {
    let now_ms = clock_ms(ctx);
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

    /// `color`, translucent (premultiplied, as egui keeps it), drawn over `under`.
    fn over(color: Color32, under: Color32) -> Color32 {
        let mix = |c: u8, u: u8| (c as f32 + u as f32 * (1.0 - color.a() as f32 / 255.0)).round().min(255.0) as u8;
        Color32::from_rgb(mix(color.r(), under.r()), mix(color.g(), under.g()), mix(color.b(), under.b()))
    }

    /// The WCAG contrast of two colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let luminance = |c: Color32| {
            let channel = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        };
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// On a dark pane whose app left egui's text colours to egui (egui's grey text), and on the light and dark panes of the
    /// editors, the panel's words are in the app's colour and its weak words and its buttons that cannot be used yet stay
    /// above 4.5:1; the app's own visuals are as they were once the panel is drawn.
    #[test]
    fn the_panel_s_words_stay_readable_on_light_and_dark_panes() {
        let panes = [
            (Visuals::dark(), Color32::from_rgb(0x2D, 0x2D, 0x2D), Color32::from_rgb(0xEE, 0xEE, 0xEE)),
            (Visuals::dark(), Color32::from_rgb(0x22, 0x22, 0x22), Color32::from_rgb(0xEB, 0xEB, 0xEB)),
            (Visuals::light(), Color32::WHITE, Color32::from_rgb(0x24, 0x24, 0x24)),
        ];
        for (mut visuals, pane, text) in panes {
            assert!(contrast(over(visuals.weak_text_color(), pane), pane) < 4.5 || !visuals.dark_mode, "egui's own weak text");
            legible(&mut visuals, text);
            assert_eq!(visuals.text_color(), text);
            let weak = contrast(over(visuals.weak_text_color(), pane), pane);
            let disabled = contrast(over(visuals.disable(text), pane), pane);
            assert!(weak >= 4.5 && disabled >= 4.5, "{pane:?}: weak {weak:.2}, a button not usable yet {disabled:.2}");
            assert!(disabled < contrast(text, pane), "a button not usable yet still reads as such");
        }
        // The app's visuals are left as they were.
        let mut h = Harness::builder().with_size(egui::vec2(360.0, 520.0)).build_ui_state(
            |ui, state: &mut (NoraPanel, NoraSession, Option<(Option<Color32>, f32)>)| {
                ui.visuals_mut().override_text_color = None;
                state.0.show(ui, &state.1);
                state.2 = Some((ui.visuals().override_text_color, ui.visuals().disabled_alpha));
            },
            (NoraPanel::new(), session(), None),
        );
        h.run();
        assert_eq!(h.state().2, Some((None, Visuals::light().disabled_alpha)));
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

    /// A panel whose app draws a request of its own while the conversation is empty (`NoraPanel::show_with`).
    struct Slot {
        panel: NoraPanel,
        session: NoraSession,
        actions: Vec<PanelAction>,
        hint: Option<String>,
    }

    fn slot_harness(session: NoraSession) -> Harness<'static, Slot> {
        Harness::builder().with_size(egui::vec2(360.0, 520.0)).build_ui_state(
            |ui, state: &mut Slot| {
                let mut hint = None;
                let actions = {
                    let mut draw = |ui: &mut Ui, words: &str| {
                        hint = Some(words.to_string());
                        ui.button("A ready-made request").clicked().then(|| "Do the ready thing".to_string())
                    };
                    state.panel.show_with(ui, &state.session, Some(&mut draw))
                };
                state.hint = state.hint.take().or(hint);
                state.actions.extend(actions);
            },
            Slot { panel: NoraPanel::new(), session, actions: Vec::new(), hint: None },
        )
    }

    #[test]
    fn what_the_app_draws_while_the_conversation_is_empty_sits_under_the_header_and_replaces_the_hint() {
        let mut h = slot_harness(session());
        h.run();
        let header = h.get_by_label("New conversation").rect();
        let slot = h.get_by_label("A ready-made request").rect();
        assert!(slot.top() >= header.bottom(), "under the header's buttons: {header:?} then {slot:?}");
        // The app is given the hint's words, and the panel does not draw them itself.
        let hint = NoraLabels::default().empty;
        assert_eq!(h.state().hint.as_deref(), Some(hint.as_str()));
        assert!(h.query_by_label(&hint).is_none());
        assert!(h.query_by_label("Ask Nora…").is_some() || h.query_by_role(egui::accesskit::Role::MultilineTextInput).is_some());
    }

    #[test]
    fn a_request_the_app_returns_is_sent_as_if_typed() {
        let mut h = slot_harness(session());
        h.run();
        h.get_by_label("A ready-made request").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Send("Do the ready thing".into())]);
    }

    #[test]
    fn what_the_app_draws_is_gone_once_the_conversation_has_begun() {
        let mut h = slot_harness(conversation());
        h.run();
        assert!(h.query_by_label("A ready-made request").is_none());
        assert_eq!(h.state().hint, None);
        // And without an app slot the panel keeps drawing its own hint.
        let mut plain = harness(session());
        plain.run();
        assert!(plain.query_by_label(&NoraLabels::default().empty).is_some());
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

    /// A session in which Nora typed `text` in the document and the editor did it: there is a change to take back.
    fn changed() -> NoraSession {
        let mut nora = session();
        let sent = nora.handle(Input::Send("Write 42 in B2".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        let polls = nora.handle(Input::Tick { now_ms: 10 });
        answer(&mut nora, &polls[0], json!({ "messages": [{ "role": "assistant", "text": "Done." }] }));
        let action = json!({ "action": "doc_type_text", "text": "42", "callback_id": "c1" });
        let played = answer(&mut nora, &polls[1], json!({ "actions": [action] }));
        let id = played.iter().find_map(|output| if let Output::Document(call) = output { Some(call.id) } else { None }).expect("the action runs");
        nora.handle(Input::DocumentResult { id, result: Ok(json!({ "ok": true })) });
        nora.handle(Input::Tick { now_ms: 20_000 });
        assert!(nora.can_undo() && !nora.is_busy());
        nora
    }

    #[test]
    fn undo_and_new_conversation_are_offered_when_they_make_sense() {
        // Nothing said, nothing to take back: no undo, and a new conversation cannot be started.
        let mut h = harness(session());
        h.run();
        assert!(h.query_by_label("Undo this change").is_none());
        assert!(h.get_by_label("New conversation").accesskit_node().is_disabled());

        let mut h = harness(conversation());
        h.run();
        assert!(h.query_by_label("Undo this change").is_none(), "Nora changed nothing: nothing to undo");
        assert!(!h.get_by_label("New conversation").accesskit_node().is_disabled());
        h.get_by_label("New conversation").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::NewConversation]);

        // What Nora changed can be taken back, right under what she said.
        let mut h = harness(changed());
        h.run();
        h.get_by_label("Undo this change").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Undo]);
    }

    #[test]
    fn a_failure_and_the_working_state_are_shown() {
        let mut nora = session();
        let outputs = nora.handle(Input::Send("Hello".into()));
        let mut h = harness(nora);
        h.run_steps(2);
        assert!(h.query_by_label("Nora is thinking…").is_some(), "what she does, in words: she is away from the document");

        let Output::Http(request) = &outputs[0] else {
            return;
        };
        h.state_mut().session.handle(Input::Http(HttpResponse { id: request.id, status: 500, body: "{}".into() }));
        h.run_steps(2);
        assert!(h.query_by_label("Nora is thinking…").is_none());
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

    // //// Neoffice — the new look of the pane (design system): its sphere, its greeting, what Nora is doing, the microphone.

    #[test]
    fn an_empty_pane_greets_in_forum_and_offers_ready_requests() {
        let mut h = harness(session());
        h.state_mut().panel.pane_words = PaneWords { suggestions: vec!["Fix the spelling".into(), "Summarise".into()], ..PaneWords::default() };
        h.run_steps(3);
        assert!(h.query_by_label("How can I help?").is_some());
        h.get_by_label("Summarise").click();
        h.run();
        assert_eq!(h.state().actions, vec![PanelAction::Send("Summarise".into())], "a chip sends its words as typed");
        // Once the conversation has begun the greeting and the chips are gone.
        let mut begun = harness(conversation());
        begun.state_mut().panel.pane_words.suggestions = vec!["Summarise".into()];
        begun.run_steps(3);
        assert!(begun.query_by_label("How can I help?").is_none() && begun.query_by_label("Summarise").is_none());
    }

    #[test]
    fn the_line_under_the_conversation_says_what_nora_is_doing() {
        // Away from the document: thinking; the editor running a read: reading; a change: editing.
        let mut nora = session();
        let sent = nora.handle(Input::Send("Improve the text".into()));
        answer(&mut nora, &sent[0], json!({ "thread_id": "t1" }));
        assert_eq!(nora.activity(), Activity::Thinking);
        let polls = nora.handle(Input::Tick { now_ms: 10 });
        answer(&mut nora, &polls[0], json!({ "messages": [] }));
        let played = answer(&mut nora, &polls[1], json!({ "actions": [{ "action": "doc_observe", "callback_id": "c1" }] }));
        assert!(played.iter().any(|output| matches!(output, Output::Document(_))));
        assert_eq!(nora.activity(), Activity::Reading);
        let mut h = harness(nora);
        h.run_steps(3);
        assert!(h.query_by_label("Nora is reading the document…").is_some());
        let mut french = harness(session());
        french.state_mut().panel.activity_words.thinking = "Nora réfléchit…".into();
        french.state_mut().session.handle(Input::Send("Bonjour".into()));
        french.run_steps(3);
        assert!(french.query_by_label("Nora réfléchit…").is_some());
    }

    #[test]
    fn the_close_button_is_drawn_only_where_the_app_has_none_and_says_when_it_was_clicked() {
        let mut h = harness(session());
        h.run_steps(2);
        assert!(h.query_by_label("Close").is_none());
        h.state_mut().panel.closable = true;
        h.run_steps(2);
        h.get_by_label("Close").click();
        h.run_steps(2);
        assert!(h.state_mut().panel.take_close_request());
        assert!(!h.state_mut().panel.take_close_request(), "said once");
    }

    #[test]
    fn the_microphone_dictates_into_the_box_and_the_send_arrow_sends_what_was_said() {
        let mut h = harness(session());
        h.state_mut().panel.voice.pretend_available();
        h.run_steps(3);
        // One click to speak.
        h.get_by_label("Dictate").click();
        h.run_steps(2);
        assert!(h.state().panel.voice.listening());
        assert!(h.query_by_label("Dictate").is_none(), "the microphone is now the button that stops it");
        // The recording is over: it goes to the server as a dictation, and the pane waits for its words.
        h.state_mut().panel.voice.begin_for_test(0.0);
        h.get_by_label("Stop and keep what you said").click();
        h.run_steps(2);
        h.state_mut().panel.voice.inject(voice::Recorded::Audio { bytes: vec![1, 2, 3, 4], mime: "audio/webm".into() });
        h.run_steps(2);
        let dictate =
            h.state().actions.iter().find_map(|action| if let PanelAction::Dictate { mime, .. } = action { Some(mime.clone()) } else { None });
        assert_eq!(dictate.as_deref(), Some("audio/webm"));
        assert!(h.state().panel.voice.transcribing());
        assert!(h.query_by_label("Writing down what you said…").is_some());
    }

    /// The words of a dictation go into the box once, after what is already typed.
    #[test]
    fn the_words_of_a_dictation_are_put_in_the_box_once() {
        let mut nora = session();
        let sent = nora.handle(Input::Transcribe { audio_base64: "AAAA".into(), mime: "audio/webm".into() });
        assert!(nora.dictating());
        let request = sent.iter().find_map(|output| if let Output::Http(request) = output { Some(request.clone()) } else { None }).expect("a call");
        assert_eq!(request.path, craft_nora::protocol::STT_TRANSCRIBE);
        assert_eq!(request.body["language"], "fr");
        let mut h = harness(nora);
        h.state_mut().panel.set_draft("Please");
        h.run_steps(2);
        assert_eq!(h.state().panel.draft(), "Please", "nothing is put in the box before the words are in");
        let body = json!({ "message": { "success": true, "text": " corrige le texte " } }).to_string();
        h.state_mut().session.handle(Input::Http(HttpResponse { id: request.id, status: 200, body }));
        h.run_steps(3);
        assert_eq!(h.state().panel.draft(), "Please corrige le texte");
        h.state_mut().panel.set_draft("");
        h.run_steps(3);
        assert_eq!(h.state().panel.draft(), "", "the same dictation is not put twice");
    }

    #[test]
    fn a_transcript_of_silence_is_no_dictation() {
        for phantom in ["Sous-titrage Société Radio-Canada", "Merci d'avoir regardé cette vidéo.", "Thank you for watching!", "  "] {
            let mut nora = session();
            let sent = nora.handle(Input::Transcribe { audio_base64: "AAAA".into(), mime: "audio/webm".into() });
            let Output::Http(request) = &sent[0] else { panic!("a call") };
            nora.handle(Input::Http(HttpResponse {
                id: request.id,
                status: 200,
                body: json!({ "message": { "success": true, "text": phantom } }).to_string(),
            }));
            assert_eq!(nora.dictation().map(|done| done.text.clone()), Some(None), "{phantom:?}");
        }
        let mut nora = session();
        let sent = nora.handle(Input::Transcribe { audio_base64: "AAAA".into(), mime: "audio/webm".into() });
        let Output::Http(request) = &sent[0] else { panic!("a call") };
        nora.handle(Input::Http(HttpResponse {
            id: request.id,
            status: 200,
            body: json!({ "message": { "success": false, "error": "Empty audio" } }).to_string(),
        }));
        let done = nora.dictation().expect("answered");
        assert_eq!((done.text.as_deref(), done.error.as_deref()), (None, Some("Empty audio")));
    }

    #[test]
    fn the_sphere_and_the_icons_are_the_design_system_s_and_the_theme_picks_the_palette() {
        let (light, dark) = (Palette::of(false), Palette::of(true));
        // The primary action is ink on paper and bright paper on espresso: never a colour of the app's own.
        assert_eq!(light.primary, Color32::from_rgb(0x14, 0x14, 0x14));
        assert_eq!(dark.primary_fg, Color32::from_rgb(0x14, 0x14, 0x14));
        assert_ne!(light.bg, dark.bg);
        // Every word the pane sets reads at 4.5:1 on its canvas and on its cards, in both themes.
        for pal in [light, dark] {
            for (name, ink) in [("text", pal.text), ("text_2", pal.text_2), ("weak", pal.weak)] {
                for under in [pal.bg, pal.card] {
                    let seen = contrast(over(ink, over(under, pal.bg)), over(under, pal.bg));
                    assert!(seen >= 4.5, "{name} on {under:?} in {}: {seen:.2}", if pal.dark { "dark" } else { "light" });
                }
            }
            let button = contrast(pal.primary_fg, pal.primary);
            assert!(button >= 7.0, "the words of an ink button: {button:.2}");
            // Ink on the signal's tint, never light text under the signal.
            let tint = over(pal.signal_soft, pal.bg);
            assert!(contrast(pal.text, tint) >= 7.0, "the words of what Nora is doing");
            assert!(contrast(pal.warn_text, over(pal.warn_fill, pal.bg)) >= 4.5 && contrast(pal.error_text, over(pal.error_fill, pal.bg)) >= 4.5);
        }
    }

    #[cfg(feature = "preview")]
    mod preview {
        use super::*;

        /// A scene: its name, the conversation it shows and how the panel is set for it.
        type Scene = (&'static str, fn() -> NoraSession, fn(&mut NoraPanel));

        /// Draws the pane in a few states, light and dark, into NORA_PREVIEW_DIR as PNG files.
        #[test]
        #[ignore = "draws files: cargo test -p craft-nora-ui --features preview -- --ignored"]
        fn draw_the_pane() {
            let dir = std::path::PathBuf::from(std::env::var("NORA_PREVIEW_DIR").expect("NORA_PREVIEW_DIR names where to draw"));
            std::fs::create_dir_all(&dir).expect("a folder to draw in");
            let scenes: Vec<Scene> = vec![
                ("empty", session, |panel| {
                    panel.pane_words.suggestions =
                        vec!["Corriger l'orthographe".into(), "Améliorer le texte".into(), "Résumer en trois phrases".into()];
                    panel.pane_words.greeting = "Comment puis-je vous aider ?".into();
                    panel.labels.empty = "Demandez à Nora d'écrire, de réécrire, de traduire ou de mettre en forme votre document.".into();
                    panel.labels.placeholder = "Demander à Nora…".into();
                }),
                ("reply", changed, |panel| {
                    panel.pane_words.undo_change = "Annuler la modification".into();
                    panel.labels.placeholder = "Demander à Nora…".into();
                }),
                (
                    "working",
                    || {
                        let mut nora = session();
                        nora.handle(Input::Send("Améliore le texte".into()));
                        nora
                    },
                    |panel| panel.labels.placeholder = "Demander à Nora…".into(),
                ),
                ("confirm", || asking(json!({ "action": "doc_save", "callback_id": "c1" })), |_| {}),
                ("listening", session, |panel| {
                    panel.voice.pretend_available();
                    panel.voice.begin_for_test(0.0);
                }),
            ];
            for dark in [false, true] {
                for (name, make, set) in &scenes {
                    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
                    let mut panel = NoraPanel::new();
                    panel.closable = true;
                    panel.voice.pretend_available();
                    set(&mut panel);
                    let mut h =
                        Harness::builder().with_size(egui::vec2(340.0, 620.0)).with_pixels_per_point(2.0).with_theme(theme).wgpu().build_ui_state(
                            |ui, state: &mut State| {
                                let actions = state.panel.show(ui, &state.session);
                                state.actions.extend(actions);
                            },
                            State { panel, session: make(), actions: Vec::new() },
                        );
                    h.run_steps(6);
                    if *name == "working" {
                        h.run_steps(260);
                    }
                    let image = h.render().expect("the pane is drawn");
                    image.save(dir.join(format!("{name}-{}.png", if dark { "dark" } else { "light" }))).expect("a file");
                }
            }
        }
    }
}
