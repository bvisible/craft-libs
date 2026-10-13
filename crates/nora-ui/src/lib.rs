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

use std::time::Duration;

use craft_nora::{ChatMessage, Confirmation, DocActionKind, Input, NoraSession, Output, Role};
use egui::{
    Align, Button, CentralPanel, Color32, Context, CornerRadius, Event, Frame, Id, Key, KeyboardShortcut, Label, Layout, Margin, Modifiers, Panel,
    RichText, ScrollArea, Stroke, TextEdit, Ui, Visuals,
};

/// What the user did in the panel. The host turns it into a session input with
/// [`PanelAction::into_input`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelAction {
    /// The user sent a message (already trimmed, never empty).
    Send(String),
    /// The user answered a confirmation Nora asked for.
    Confirm { id: u64, accepted: bool },
    /// The user asked to undo Nora's last change.
    Undo,
    /// The user started a new conversation.
    NewConversation,
}

impl PanelAction {
    /// The session input this action stands for.
    pub fn into_input(self) -> Input {
        match self {
            PanelAction::Send(text) => Input::Send(text),
            PanelAction::Confirm { id, accepted } => Input::Confirm { id, accepted },
            PanelAction::Undo => Input::UndoLast,
            PanelAction::NewConversation => Input::Reset,
        }
    }
}

/// The words of the panel. English by default; an app fills in its own language.
#[derive(Debug, Clone)]
pub struct NoraLabels {
    pub title: String,
    pub new_conversation: String,
    pub undo: String,
    pub undo_hint: String,
    pub placeholder: String,
    pub send: String,
    pub working: String,
    /// Shown while the conversation is empty.
    pub empty: String,
    /// Above the description of an action Nora wants to run.
    pub confirm_heading: String,
    pub allow: String,
    pub refuse: String,
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
            working: "Nora is working…".into(),
            empty: "Ask Nora to write, rewrite, translate or format your document.".into(),
            confirm_heading: "Nora wants to:".into(),
            allow: "Allow".into(),
            refuse: "Refuse".into(),
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
}

impl Default for NoraStyle {
    fn default() -> Self {
        NoraStyle { user_fill: None, user_text: None, nora_fill: None, nora_text: None, corner_radius: 10, bubble_padding: 8, max_bubble_share: 0.9 }
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

/// The conversation with Nora, drawn into the `Ui` it is given (a side panel, a window…).
#[derive(Debug, Default)]
pub struct NoraPanel {
    pub labels: NoraLabels,
    pub style: NoraStyle,
    draft: String,
    focus_input: bool,
    scroll_to_end: bool,
}

impl NoraPanel {
    pub fn new() -> Self {
        Self::default()
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
        let id = ui.id().with("nora");
        self.header(ui, session, &mut actions);
        ui.separator();
        // The text box is a bottom panel, so that its height follows what is typed in it; the
        // transcript takes what is left above it.
        Panel::bottom(id.with("input-panel"))
            .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
            .show(ui, |ui| self.input_row(ui, id.with("input"), &mut actions));
        CentralPanel::no_frame().show(ui, |ui| self.transcript(ui, session, &colors, &mut actions));
        actions
    }

    fn header(&self, ui: &mut Ui, session: &NoraSession, actions: &mut Vec<PanelAction>) {
        ui.horizontal(|ui| {
            ui.strong(self.labels.title.as_str());
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
        ScrollArea::vertical().id_salt("nora-transcript").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            if session.transcript().is_empty() && !session.is_busy() {
                ui.add_space(ui.spacing().item_spacing.y * 4.0);
                ui.vertical_centered(|ui| ui.weak(self.labels.empty.as_str()));
            }
            for message in session.transcript() {
                self.bubble(ui, message, colors);
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
            if let Some(error) = session.last_error() {
                ui.colored_label(colors.error, error);
            }
            // The user just sent something: show it, even when they had scrolled up.
            if std::mem::take(&mut self.scroll_to_end) {
                ui.scroll_to_cursor(Some(Align::BOTTOM));
            }
        });
    }

    fn bubble(&self, ui: &mut Ui, message: &ChatMessage, colors: &Colors) {
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
                    ui.add(Label::new(RichText::new(message.text.as_str()).color(text)).wrap());
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

    fn input_row(&mut self, ui: &mut Ui, input_id: Id, actions: &mut Vec<PanelAction>) {
        if std::mem::take(&mut self.focus_input) {
            ui.memory_mut(|memory| memory.request_focus(input_id));
        }
        let mut send_clicked = false;
        let mut typing = false;
        ui.horizontal(|ui| {
            // Right to left so that the button takes its own width and the text box the rest.
            ui.with_layout(Layout::right_to_left(Align::BOTTOM), |ui| {
                let can_send = !self.draft.trim().is_empty();
                send_clicked = ui.add_enabled(can_send, Button::new(self.labels.send.as_str())).clicked();
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
        if (send_clicked || enter) && !self.draft.trim().is_empty() {
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
        h.run();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(text);
        h.run();
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
}
