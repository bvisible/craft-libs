// //// Neoffice — added file (no upstream equivalent): end-to-end tests of the Nora conversation state machine.

use craft_nora::{
    AppKind, CommandPolicy, DocActionKind, DocumentCall, DocumentInfo, HttpRequest, HttpResponse, Input, NoraSession, Output, PageContext,
    SessionConfig, protocol,
};
use serde_json::{Value, json};

fn context() -> PageContext {
    PageContext::editor(AppKind::Word, DocumentInfo::named("devis.docx"))
}

fn session() -> NoraSession {
    NoraSession::new(SessionConfig::default(), context())
}

fn ok(request: &HttpRequest, message: Value) -> Input {
    Input::Http(HttpResponse { id: request.id, status: 200, body: json!({ "message": message }).to_string() })
}

fn requests(outputs: &[Output]) -> Vec<&HttpRequest> {
    outputs.iter().filter_map(|o| if let Output::Http(request) = o { Some(request) } else { None }).collect()
}

fn request_to<'a>(outputs: &'a [Output], path: &str) -> &'a HttpRequest {
    requests(outputs).into_iter().find(|r| r.path == path).unwrap_or_else(|| panic!("no request to {path} in {outputs:?}"))
}

fn documents(outputs: &[Output]) -> Vec<&DocumentCall> {
    outputs.iter().filter_map(|o| if let Output::Document(call) = o { Some(call) } else { None }).collect()
}

/// A session whose first message got its thread id. Time is at 0.
fn started() -> NoraSession {
    let mut nora = session();
    let sent = nora.handle(Input::Send("Bonjour".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    nora.handle(ok(&start, json!({"thread_id": "t1"})));
    nora
}

/// Runs one poll at `now`, answering the replies request with `replies` and the action queue with `actions`.
/// Returns what the answers to the action queue produced.
fn poll(nora: &mut NoraSession, now: u64, replies: Value, actions: Value) -> Vec<Output> {
    let outputs = nora.handle(Input::Tick { now_ms: now });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    nora.handle(ok(&messages, json!({"success": true, "messages": replies, "cards": []})));
    nora.handle(ok(&queue, json!({"success": true, "actions": actions})))
}

#[test]
fn the_first_message_opens_a_thread_with_the_page_context() {
    let mut nora = session();
    let sent = nora.handle(Input::Send("  Réécris le premier paragraphe  ".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD);
    assert_eq!(start.body["message"], "Réécris le premier paragraphe");
    assert_eq!(start.body["block"], 0);
    let sent_context: Value = serde_json::from_str(start.body["context"].as_str().unwrap()).unwrap();
    assert_eq!(sent_context["surface"], "editor");
    assert_eq!(sent_context["document"]["name"], "devis.docx");
    assert!(nora.is_busy());
    assert_eq!(nora.transcript().len(), 1);
    assert!(sent.contains(&Output::Changed));
}

#[test]
fn blank_messages_are_not_sent() {
    let mut nora = session();
    assert!(nora.handle(Input::Send("   \n ".into())).is_empty());
    assert!(!nora.is_busy());
}

#[test]
fn replies_arrive_by_polling_and_the_turn_ends_when_nora_goes_quiet() {
    let mut nora = started();
    assert_eq!(nora.thread_id(), Some("t1"));
    poll(&mut nora, 1_000, json!([{"role": "assistant", "text": "Voici une version plus courte."}]), json!([]));
    assert_eq!(nora.transcript().len(), 2);
    assert_eq!(nora.transcript()[1].text, "Voici une version plus courte.");
    assert!(nora.is_busy());
    // Not due yet, then due again; a repeated reply is not added twice.
    assert!(requests(&nora.handle(Input::Tick { now_ms: 1_500 })).is_empty());
    poll(
        &mut nora,
        2_600,
        json!([{"role": "assistant", "text": "Voici une version plus courte."}, {"role": "assistant", "text": "Autre chose ?"}]),
        json!([]),
    );
    assert_eq!(nora.transcript().len(), 3);
    // Quiet for ten seconds after the last reply.
    let done = nora.handle(Input::Tick { now_ms: 13_000 });
    assert!(!nora.is_busy());
    assert!(done.contains(&Output::Changed));
    assert_eq!(nora.next_wake_ms(), None);
}

#[test]
fn polls_do_not_pile_up_while_the_previous_ones_are_unanswered() {
    let mut nora = started();
    assert_eq!(requests(&nora.handle(Input::Tick { now_ms: 100 })).len(), 2);
    assert!(requests(&nora.handle(Input::Tick { now_ms: 5_000 })).is_empty());
}

#[test]
fn a_follow_up_goes_to_the_same_thread() {
    let mut nora = started();
    let sent = nora.handle(Input::Send("Et le deuxième ?".into()));
    let post = request_to(&sent, protocol::POST_MESSAGE);
    assert_eq!(post.body["thread_id"], "t1");
    assert_eq!(nora.transcript().len(), 2);
}

#[test]
fn the_context_follows_the_selection() {
    let mut nora = started();
    let mut document = DocumentInfo::named("devis.docx");
    document.selection = Some(craft_nora::Selection { text: "Total".into() });
    nora.handle(Input::SetContext(PageContext::editor(AppKind::Word, document)));
    let sent = nora.handle(Input::Send("Traduis".into()));
    let body = &request_to(&sent, protocol::POST_MESSAGE).body;
    assert!(body["context"].as_str().unwrap().contains("\"selection\":{\"text\":\"Total\"}"));
}

#[test]
fn a_server_refusal_is_shown_and_ends_the_turn() {
    let mut nora = session();
    let sent = nora.handle(Input::Send("Bonjour".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    let inner = serde_json::to_string(&json!({"message": "Not permitted"})).unwrap();
    let body = json!({"_server_messages": serde_json::to_string(&vec![inner]).unwrap()}).to_string();
    nora.handle(Input::Http(HttpResponse { id: start.id, status: 403, body }));
    assert_eq!(nora.last_error(), Some("Not permitted"));
    assert!(!nora.is_busy());
}

#[test]
fn an_answer_without_a_thread_id_is_an_error() {
    let mut nora = session();
    let sent = nora.handle(Input::Send("Bonjour".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    nora.handle(ok(&start, json!({"success": true})));
    assert!(nora.last_error().is_some_and(|e| e.contains("thread_id")));
    assert!(!nora.is_busy());
}

#[test]
fn nora_reads_the_document_and_gets_the_answer_by_callback() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_observe", "callback_id": "c1"}]));
    let calls = documents(&outputs);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].kind, DocActionKind::Observe);
    let done = nora.handle(Input::DocumentResult { id: calls[0].id, result: Ok(json!({"text": "Devis n° 1"})) });
    let observation = request_to(&done, protocol::RECEIVE_OBSERVATION);
    assert_eq!(observation.body["callback_id"], "c1");
    assert_eq!(observation.body["observation"], json!({"kind": "document", "ok": true, "result": {"text": "Devis n° 1"}}));
    assert!(!nora.can_undo());
}

#[test]
fn a_change_can_be_undone_once() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_execute", "command": "format.bold", "params": {}, "callback_id": "c2"}]));
    let call = documents(&outputs)[0].clone();
    assert_eq!(call.kind, DocActionKind::Execute { command: "format.bold".into(), params: json!({}) });
    nora.handle(Input::DocumentResult { id: call.id, result: Ok(json!({})) });
    assert!(nora.can_undo());
    let undone = nora.handle(Input::UndoLast);
    assert_eq!(documents(&undone)[0].kind, DocActionKind::Undo);
    assert!(!nora.can_undo());
    assert!(nora.handle(Input::UndoLast).is_empty());
}

#[test]
fn a_failed_action_is_reported_to_nora_and_to_the_user() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_type_text", "text": "x", "callback_id": "c3"}]));
    let call = documents(&outputs)[0].clone();
    let done = nora.handle(Input::DocumentResult { id: call.id, result: Err("no caret".into()) });
    assert_eq!(request_to(&done, protocol::RECEIVE_OBSERVATION).body["observation"], json!({"kind": "document", "ok": false, "error": "no caret"}));
    assert_eq!(nora.last_error(), Some("no caret"));
    assert!(!nora.can_undo());
}

#[test]
fn refused_commands_never_reach_the_editor() {
    let mut nora = started();
    let outputs = poll(
        &mut nora,
        100,
        json!([]),
        json!([{"action": "doc_execute", "command": "file.saveAs", "params": {"path": "x.docx"}, "callback_id": "c4"}]),
    );
    assert!(documents(&outputs).is_empty());
    let observation = request_to(&outputs, protocol::RECEIVE_OBSERVATION);
    assert_eq!(observation.body["observation"]["ok"], false);
    assert!(observation.body["observation"]["error"].as_str().unwrap().contains("refused"));
}

#[test]
fn one_refused_command_refuses_the_whole_batch() {
    let mut nora = started();
    let outputs = poll(
        &mut nora,
        100,
        json!([]),
        json!([{"action": "doc_batch", "commands": [{"command": "format.bold"}, {"command": "app.quit"}], "callback_id": "c5"}]),
    );
    assert!(documents(&outputs).is_empty());
    assert_eq!(request_to(&outputs, protocol::RECEIVE_OBSERVATION).body["observation"]["ok"], false);
}

#[test]
fn an_app_can_ask_for_confirmation_before_some_commands() {
    let config = SessionConfig { policy: CommandPolicy::default().confirm_prefix("edit.delete"), ..SessionConfig::default() };
    let mut nora = NoraSession::new(config, context());
    let sent = nora.handle(Input::Send("Bonjour".into()));
    nora.handle(ok(request_to(&sent, protocol::START_OR_RESUME_THREAD), json!({"thread_id": "t1"})));
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_execute", "command": "edit.deleteParagraph", "callback_id": "c6"}]));
    assert!(documents(&outputs).is_empty());
    assert_eq!(nora.confirmations().len(), 1);
    assert_eq!(nora.confirmations()[0].summary, "Run `edit.deleteParagraph`");
}

#[test]
fn saving_always_waits_for_the_user_and_can_be_declined() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_save", "callback_id": "c7"}, {"action": "doc_save", "callback_id": "c8"}]));
    assert!(documents(&outputs).is_empty());
    let waiting = nora.confirmations();
    assert_eq!(waiting.len(), 2);
    assert_eq!(waiting[0].summary, "Save the document");
    // The turn stays open while an answer is pending, however quiet it is.
    nora.handle(Input::Tick { now_ms: 60_000 });
    assert!(nora.is_busy());
    // Yes runs it; no tells Nora.
    let accepted = nora.handle(Input::Confirm { id: waiting[0].id, accepted: true });
    assert_eq!(documents(&accepted)[0].kind, DocActionKind::Save);
    let declined = nora.handle(Input::Confirm { id: waiting[1].id, accepted: false });
    assert!(documents(&declined).is_empty());
    assert_eq!(request_to(&declined, protocol::RECEIVE_OBSERVATION).body["observation"]["error"], "the user declined");
    assert!(nora.confirmations().is_empty());
    // An unknown id does nothing.
    assert!(nora.handle(Input::Confirm { id: 9_999, accepted: true }).is_empty());
}

#[test]
fn desk_page_actions_are_answered_as_unsupported() {
    let mut nora = started();
    let outputs =
        poll(&mut nora, 100, json!([]), json!([{"action": "dom_click", "index": 3, "callback_id": "c9"}, {"action": "navigate", "url": "/app"}]));
    assert!(documents(&outputs).is_empty());
    let observations: Vec<_> = requests(&outputs).into_iter().filter(|r| r.path == protocol::RECEIVE_OBSERVATION).collect();
    assert_eq!(observations.len(), 1, "only the action Nora waits on is answered");
    assert!(observations[0].body["observation"]["error"].as_str().unwrap().contains("not available in the editor"));
}

#[test]
fn a_new_conversation_forgets_everything() {
    let mut nora = started();
    poll(&mut nora, 100, json!([{"role": "assistant", "text": "Salut"}]), json!([]));
    nora.handle(Input::Reset);
    assert!(nora.transcript().is_empty() && nora.thread_id().is_none() && !nora.is_busy() && !nora.can_undo());
    let sent = nora.handle(Input::Send("Encore".into()));
    request_to(&sent, protocol::START_OR_RESUME_THREAD);
}

#[test]
fn stale_answers_are_ignored() {
    let mut nora = started();
    let stale = Input::Http(HttpResponse { id: 424_242, status: 200, body: json!({"message": {}}).to_string() });
    assert!(nora.handle(stale).is_empty());
    assert!(nora.handle(Input::DocumentResult { id: 424_242, result: Ok(json!(1)) }).is_empty());
}
