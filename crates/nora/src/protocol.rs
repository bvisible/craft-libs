// //// Neoffice — added file (no upstream equivalent): the wire protocol between an editor and Nora.

use serde_json::{Value, json};

use crate::context::PageContext;
use crate::error::NoraError;

/// Starts a conversation, or resumes one, and posts its first message.
pub const START_OR_RESUME_THREAD: &str = "/api/method/nora.api.chat.start_or_resume_thread";
/// Posts a follow-up message in an existing conversation.
pub const POST_MESSAGE: &str = "/api/method/nora.api.chat.post_message";
/// Reads the replies buffered for a conversation.
pub const GET_MESSAGES: &str = "/api/method/nora.api.chat.get_messages";
/// Pops the actions Nora queued for the client of a conversation (one-shot, kept five minutes).
pub const GET_ACTIONS: &str = "/api/method/nora.api.chat.get_browser_actions";
/// Answers an observation Nora asked for, by its `callback_id` (kept one minute).
pub const RECEIVE_OBSERVATION: &str = "/api/method/nora.api.page_observation.receive_observation";

/// The body of the call that opens a conversation. `block: 0` because an editor polls; the default
/// would hold the request until Nora answers.
pub fn start_thread_body(message: &str, context: &PageContext) -> Value {
    json!({ "message": message, "context": context.to_json_string(), "tts_active": 0, "block": 0 })
}

/// The body of a follow-up message.
pub fn post_message_body(thread_id: &str, message: &str, context: &PageContext) -> Value {
    json!({ "thread_id": thread_id, "message": message, "context": context.to_json_string(), "tts_active": 0, "block": 0 })
}

/// The body of `get_messages` and `get_browser_actions`.
pub fn thread_body(thread_id: &str) -> Value {
    json!({ "thread_id": thread_id })
}

/// The body of `receive_observation`.
pub fn observation_body(callback_id: &str, observation: &Value) -> Value {
    json!({ "callback_id": callback_id, "observation": observation })
}

/// What an editor answers when Nora asked for something and the action went well.
pub fn observation_ok(result: Value) -> Value {
    json!({ "kind": "document", "ok": true, "result": result })
}

/// What an editor answers when it could not or would not do what Nora asked.
pub fn observation_error(message: &str) -> Value {
    json!({ "kind": "document", "ok": false, "error": message })
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
    Other,
}

/// One message of the conversation, as shown in the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub text: String,
    /// The message as Nora sent it, for whatever the panel learns to show later.
    pub raw: Value,
}

impl ChatMessage {
    /// A message the user just typed.
    pub fn user(text: impl Into<String>) -> Self {
        let text = text.into();
        ChatMessage { role: Role::User, raw: json!({ "role": "user", "text": text }), text }
    }

    /// Reads one buffered message. Nora's replies carry `role`, `author`, `content` and `text`;
    /// the text is `text`, else `content`.
    pub fn from_json(value: &Value) -> Self {
        let role = match value.get("role").and_then(Value::as_str) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            Some("system") => Role::System,
            _ => Role::Other,
        };
        let text = ["text", "content"].iter().find_map(|key| value.get(*key).and_then(Value::as_str)).unwrap_or_default().to_string();
        ChatMessage { role, text, raw: value.clone() }
    }
}

/// The conversation id in the answer of `start_or_resume_thread`.
pub fn parse_thread_id(message: &Value) -> Result<String, NoraError> {
    thread_id_of(message).ok_or_else(|| NoraError::Malformed("no `thread_id` in the answer".into()))
}

/// The conversation id of any answer that carries one (`post_message` may change it).
pub fn thread_id_of(message: &Value) -> Option<String> {
    message.get("thread_id").and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_string)
}

/// The replies and the action cards in the answer of `get_messages`.
pub fn parse_messages(message: &Value) -> (Vec<ChatMessage>, Vec<Value>) {
    let replies = message.get("messages").and_then(Value::as_array).map(|list| list.iter().map(ChatMessage::from_json).collect()).unwrap_or_default();
    let cards = message.get("cards").and_then(Value::as_array).cloned().unwrap_or_default();
    (replies, cards)
}

/// The actions in the answer of `get_browser_actions`.
pub fn parse_actions(message: &Value) -> Vec<RemoteAction> {
    message.get("actions").and_then(Value::as_array).map(|list| list.iter().map(RemoteAction::from_json).collect()).unwrap_or_default()
}

/// One command of a `doc_batch`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandCall {
    pub command: String,
    pub params: Value,
}

/// What Nora asks an editor to do in the open document. The names follow the tools of the
/// editors' own MCP servers (`execute`, `batch`, `select_text`, `type_text`, `get_text` and
/// `inspect_document`, `render_page`, `save_document`), so the same vocabulary drives a document
/// headless on a server and one open in a window.
#[derive(Debug, Clone, PartialEq)]
pub enum DocActionKind {
    /// Read the document: text, structure, selection.
    Observe,
    /// Run one editor command with its parameters.
    Execute { command: String, params: Value },
    /// Run several commands in order.
    Batch { commands: Vec<CommandCall> },
    /// Select the n-th occurrence of some text.
    SelectText { text: String, occurrence: u32 },
    /// Type at the caret, replacing the selection.
    TypeText { text: String, paragraphs: bool },
    /// Render a page to a picture, to look at the result.
    RenderPage { page: u32, scale: f64 },
    /// Save the document. Always confirmed by the user first.
    Save,
    /// Undo the last change. Never sent by Nora: the panel's Undo button asks for it.
    Undo,
}

impl DocActionKind {
    /// Whether it can change the document (and so deserves an Undo).
    pub fn modifies(&self) -> bool {
        matches!(self, DocActionKind::Execute { .. } | DocActionKind::Batch { .. } | DocActionKind::TypeText { .. })
    }

    /// A short sentence for the panel ("Run `format.bold`").
    pub fn summary(&self) -> String {
        match self {
            DocActionKind::Observe => "Read the document".into(),
            DocActionKind::Execute { command, .. } => format!("Run `{command}`"),
            DocActionKind::Batch { commands } => format!("Run {} commands", commands.len()),
            DocActionKind::SelectText { text, .. } => format!("Select \"{}\"", shorten(text)),
            DocActionKind::TypeText { text, .. } => format!("Type \"{}\"", shorten(text)),
            DocActionKind::RenderPage { page, .. } => format!("Look at page {page}"),
            DocActionKind::Save => "Save the document".into(),
            DocActionKind::Undo => "Undo the last change".into(),
        }
    }
}

fn shorten(text: &str) -> String {
    let mut short: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        short.push('…');
    }
    short
}

/// A document action and the observation id Nora is waiting on, if it is.
#[derive(Debug, Clone, PartialEq)]
pub struct DocAction {
    pub callback_id: Option<String>,
    pub kind: DocActionKind,
}

/// One entry of the queue Nora fills for the client of a conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteAction {
    /// A `doc_*` action.
    Doc(DocAction),
    /// Anything else, the desk's page actions (`dom_click`, `navigate`…) included: an editor is one
    /// canvas and has no elements to click. If Nora waits on an answer it gets an error.
    Unsupported { action: String, callback_id: Option<String>, reason: String },
}

impl RemoteAction {
    /// Reads one queued action; a malformed one becomes [`RemoteAction::Unsupported`] with the reason.
    pub fn from_json(value: &Value) -> Self {
        let action = value.get("action").and_then(Value::as_str).unwrap_or_default().to_string();
        let callback_id = value.get("callback_id").and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_string);
        let unsupported =
            |reason: &str| RemoteAction::Unsupported { action: action.clone(), callback_id: callback_id.clone(), reason: reason.into() };
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let params = |v: &Value| v.get("params").cloned().filter(|p| !p.is_null()).unwrap_or_else(|| json!({}));
        let kind = match action.as_str() {
            "doc_observe" => DocActionKind::Observe,
            "doc_execute" => match text("command").filter(|c| !c.is_empty()) {
                Some(command) => DocActionKind::Execute { command, params: params(value) },
                None => return unsupported("missing `command`"),
            },
            "doc_batch" => {
                let commands: Vec<CommandCall> = value
                    .get("commands")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(|entry| {
                                let command = entry.get("command").and_then(Value::as_str).filter(|c| !c.is_empty())?;
                                Some(CommandCall { command: command.to_string(), params: params(entry) })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if commands.is_empty() {
                    return unsupported("`commands` is empty");
                }
                DocActionKind::Batch { commands }
            }
            "doc_select_text" => match text("text").filter(|t| !t.is_empty()) {
                Some(text) => DocActionKind::SelectText {
                    text,
                    occurrence: value.get("occurrence").and_then(Value::as_u64).unwrap_or(1).clamp(1, u64::from(u32::MAX)) as u32,
                },
                None => return unsupported("missing `text`"),
            },
            "doc_type_text" => match text("text") {
                Some(text) => DocActionKind::TypeText { text, paragraphs: value.get("paragraphs").and_then(Value::as_bool).unwrap_or(false) },
                None => return unsupported("missing `text`"),
            },
            "doc_render_page" => DocActionKind::RenderPage {
                page: value.get("page").and_then(Value::as_u64).unwrap_or(1).clamp(1, u64::from(u32::MAX)) as u32,
                scale: value.get("scale").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.1, 4.0),
            },
            "doc_save" => DocActionKind::Save,
            _ => return unsupported("not a document action"),
        };
        RemoteAction::Doc(DocAction { callback_id, kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{AppKind, DocumentInfo};

    fn ctx() -> PageContext {
        PageContext::editor(AppKind::Word, DocumentInfo::named("devis.docx"))
    }

    #[test]
    fn request_bodies_have_the_shape_of_the_chat_api() {
        let start = start_thread_body("Bonjour", &ctx());
        assert_eq!(start["message"], "Bonjour");
        assert_eq!(start["block"], 0);
        assert!(start["context"].as_str().is_some_and(|c| c.contains("\"app\":\"word\"")));
        assert!(start.get("thread_id").is_none());
        let post = post_message_body("t1", "Suite", &ctx());
        assert_eq!(post["thread_id"], "t1");
        assert_eq!(thread_body("t1"), json!({"thread_id": "t1"}));
        assert_eq!(
            observation_body("c1", &observation_ok(json!(1))),
            json!({"callback_id": "c1", "observation": {"kind": "document", "ok": true, "result": 1}})
        );
        assert_eq!(observation_error("no")["ok"], false);
    }

    #[test]
    fn reads_replies_with_text_or_content() {
        let message = json!({"messages": [{"role": "assistant", "author": "assistant", "content": "Bonjour", "text": "Bonjour !"}, {"role": "assistant", "content": "Seulement le contenu"}, {"role": "tool"}], "cards": [{"id": 1}]});
        let (replies, cards) = parse_messages(&message);
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0].text, "Bonjour !");
        assert_eq!(replies[1].text, "Seulement le contenu");
        assert_eq!((replies[2].role, replies[2].text.as_str()), (Role::Other, ""));
        assert_eq!(cards.len(), 1);
        assert_eq!(parse_messages(&json!({})), (Vec::new(), Vec::new()));
    }

    #[test]
    fn thread_ids_are_read_from_the_answers() {
        assert_eq!(parse_thread_id(&json!({"thread_id": "t9", "run_id": null})), Ok("t9".to_string()));
        assert!(parse_thread_id(&json!({"success": true})).is_err());
        assert_eq!(thread_id_of(&json!({"thread_id": ""})), None);
    }

    #[test]
    fn reads_every_document_action() {
        let actions = parse_actions(&json!({"actions": [
            {"action": "doc_observe", "callback_id": "c1"},
            {"action": "doc_execute", "command": "format.bold", "params": {"x": 1}},
            {"action": "doc_batch", "commands": [{"command": "a"}, {"command": "b", "params": {"n": 2}}, {"nothing": true}]},
            {"action": "doc_select_text", "text": "Total", "occurrence": 2},
            {"action": "doc_type_text", "text": "Bonjour", "paragraphs": true},
            {"action": "doc_render_page", "page": 3, "scale": 9.0},
            {"action": "doc_save", "callback_id": "c2"}
        ]}));
        let kinds: Vec<&DocActionKind> = actions.iter().filter_map(|a| if let RemoteAction::Doc(d) = a { Some(&d.kind) } else { None }).collect();
        assert_eq!(kinds.len(), 7);
        assert_eq!(kinds[0], &DocActionKind::Observe);
        assert_eq!(kinds[1], &DocActionKind::Execute { command: "format.bold".into(), params: json!({"x": 1}) });
        assert_eq!(
            kinds[2],
            &DocActionKind::Batch {
                commands: vec![CommandCall { command: "a".into(), params: json!({}) }, CommandCall { command: "b".into(), params: json!({"n": 2}) }]
            }
        );
        assert_eq!(kinds[3], &DocActionKind::SelectText { text: "Total".into(), occurrence: 2 });
        assert_eq!(kinds[4], &DocActionKind::TypeText { text: "Bonjour".into(), paragraphs: true });
        assert_eq!(kinds[5], &DocActionKind::RenderPage { page: 3, scale: 4.0 });
        assert_eq!(kinds[6], &DocActionKind::Save);
        assert!(matches!(&actions[0], RemoteAction::Doc(DocAction { callback_id: Some(id), .. }) if id == "c1"));
    }

    #[test]
    fn desk_actions_and_malformed_ones_are_unsupported_with_their_callback() {
        for (raw, reason) in [
            (json!({"action": "dom_click", "index": 3, "callback_id": "c5"}), "not a document action"),
            (json!({"action": "doc_execute", "callback_id": "c5"}), "missing `command`"),
            (json!({"action": "doc_batch", "commands": [], "callback_id": "c5"}), "`commands` is empty"),
            (json!({"action": "doc_select_text", "callback_id": "c5"}), "missing `text`"),
            (json!("nonsense"), "not a document action"),
        ] {
            match RemoteAction::from_json(&raw) {
                RemoteAction::Unsupported { reason: got, callback_id, .. } => {
                    assert_eq!(got, reason);
                    assert_eq!(callback_id.is_some(), raw.get("callback_id").is_some());
                }
                other => panic!("expected an unsupported action, got {other:?}"),
            }
        }
    }

    #[test]
    fn summaries_and_the_modifying_flag() {
        assert!(DocActionKind::TypeText { text: "x".into(), paragraphs: false }.modifies());
        assert!(!DocActionKind::Observe.modifies() && !DocActionKind::Save.modifies() && !DocActionKind::Undo.modifies());
        let long = "é".repeat(60);
        let summary = DocActionKind::TypeText { text: long, paragraphs: false }.summary();
        assert!(summary.ends_with("…\"") || summary.contains('…'));
        assert_eq!(DocActionKind::Execute { command: "format.bold".into(), params: json!({}) }.summary(), "Run `format.bold`");
    }
}
