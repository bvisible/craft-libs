// //// Neoffice — added file (no upstream equivalent): end-to-end tests of the Nora conversation state machine.

use craft_nora::{
    AppKind, CommandPolicy, DocActionKind, DocumentCall, DocumentInfo, HttpRequest, HttpResponse, Input, NoraError, NoraSession, Output, PageContext,
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

/// Like [`started`], with the server time of the send in the answer, as the real server gives it.
fn started_at(sent_at: f64) -> NoraSession {
    let mut nora = session();
    let sent = nora.handle(Input::Send("Bonjour".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    nora.handle(ok(&start, json!({"thread_id": "t1", "sent_at": sent_at})));
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

/// Like [`poll`], with the action queue answered as `queue` (its `actions` and its `timestamp`).
fn poll_queue(nora: &mut NoraSession, now: u64, queue: Value) -> Vec<Output> {
    let outputs = nora.handle(Input::Tick { now_ms: now });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let actions = request_to(&outputs, protocol::GET_ACTIONS).clone();
    nora.handle(ok(&messages, json!({"success": true, "messages": [], "cards": []})));
    nora.handle(ok(&actions, queue))
}

/// The error Nora was told for the observation of `callback`, among `outputs`.
fn refusal_of<'a>(outputs: &'a [Output], callback: &str) -> &'a Value {
    let observation = requests(outputs)
        .into_iter()
        .find(|r| r.path == protocol::RECEIVE_OBSERVATION && r.body["callback_id"] == callback)
        .unwrap_or_else(|| panic!("no observation for {callback} in {outputs:?}"));
    &observation.body["observation"]["error"]
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
fn replies_arrive_by_polling_and_the_turn_ends_once_the_final_reply_has_settled() {
    let mut nora = started();
    assert_eq!(nora.thread_id(), Some("t1"));
    let reply = json!({"role": "assistant", "text": "Voici une version plus courte."});
    poll(&mut nora, 1_000, json!([reply]), json!([]));
    assert_eq!(nora.transcript().len(), 2);
    assert_eq!(nora.transcript()[1].text, "Voici une version plus courte.");
    // The final reply is in, but the actions queued with it get a moment to arrive.
    assert!(nora.is_busy());
    // The first seconds are read every 400 ms: not due yet, then due; a repeated reply is not added twice.
    assert!(requests(&nora.handle(Input::Tick { now_ms: 1_200 })).is_empty());
    poll(&mut nora, 1_500, json!([reply]), json!([]));
    assert_eq!(nora.transcript().len(), 2);
    // Settled: the turn ends.
    let done = nora.handle(Input::Tick { now_ms: 2_600 });
    assert!(!nora.is_busy());
    assert!(done.contains(&Output::Changed));
    assert_eq!(nora.next_wake_ms(), None);
}

#[test]
fn an_acknowledgement_is_shown_but_does_not_end_the_wait_for_the_real_answer() {
    let mut nora = started();
    let ack = json!({"role": "assistant", "text": "Je transmets votre demande à votre pôle Comptabilité, je reviens vers vous."});
    poll(&mut nora, 500, json!([ack]), json!([]));
    assert_eq!(nora.transcript().len(), 2, "the acknowledgement is shown");
    // The specialist takes its time: still waiting after 20 s and after 40 s.
    poll(&mut nora, 20_000, json!([ack]), json!([]));
    poll(&mut nora, 25_000, json!([ack]), json!([]));
    assert!(nora.is_busy());
    let answer = json!({"role": "assistant", "text": "253 devis ouverts."});
    poll(&mut nora, 40_000, json!([ack, answer]), json!([]));
    assert_eq!(nora.transcript().len(), 3);
    assert_eq!(nora.transcript()[2].text, "253 devis ouverts.");
    assert!(nora.is_busy(), "the answer settles first");
    nora.handle(Input::Tick { now_ms: 42_000 });
    assert!(!nora.is_busy());
}

#[test]
fn the_acknowledgements_of_every_language_are_recognised_and_a_real_answer_is_not() {
    for text in ["I'm passing this to your Sales desk", "Ich leite Ihre Anfrage weiter", "Inoltro la sua richiesta", "Je confie ça à votre pôle"] {
        let mut nora = started();
        poll(&mut nora, 500, json!([{"role": "assistant", "text": text}]), json!([]));
        nora.handle(Input::Tick { now_ms: 5_000 });
        assert!(nora.is_busy(), "{text:?} is an acknowledgement");
    }
    let mut nora = started();
    poll(&mut nora, 500, json!([{"role": "assistant", "text": "Le chiffre d'affaires du mois en cours est de 517 CHF."}]), json!([]));
    nora.handle(Input::Tick { now_ms: 5_000 });
    assert!(!nora.is_busy(), "a real answer that mentions the current month ends the turn");
}

#[test]
fn a_host_can_pass_the_servers_live_list_of_acknowledgements() {
    let config = SessionConfig { ack_anchors: vec!["un instant".into()], ..SessionConfig::default() };
    let mut nora = NoraSession::new(config, context());
    let sent = nora.handle(Input::Send("Bonjour".into()));
    nora.handle(ok(request_to(&sent, protocol::START_OR_RESUME_THREAD), json!({"thread_id": "t1"})));
    poll(&mut nora, 500, json!([{"role": "assistant", "text": "Un instant, je regarde."}]), json!([]));
    nora.handle(Input::Tick { now_ms: 5_000 });
    assert!(nora.is_busy());
}

#[test]
fn silence_ends_the_wait_with_an_error_after_eight_minutes() {
    let mut nora = started();
    poll(&mut nora, 100, json!([]), json!([]));
    nora.handle(Input::Tick { now_ms: 479_000 });
    assert!(nora.is_busy());
    // Even with a poll the host never answered, the wait does end.
    let outputs = nora.handle(Input::Tick { now_ms: 480_100 });
    assert!(!nora.is_busy());
    assert_eq!(nora.last_error(), Some("Nora did not answer in time"));
    assert_eq!(nora.failure(), Some(&NoraError::Timeout));
    assert_eq!(nora.last_failure(), Some(&NoraError::Timeout), "for the host to say it in its own words");
    assert!(outputs.contains(&Output::Changed));
}

/// A failure while Nora is awaited keeps its kind for the host's words, and the next read that works clears it.
#[test]
fn a_failed_read_keeps_its_kind_for_the_host_s_words() {
    let mut nora = started();
    let outputs = nora.handle(Input::Tick { now_ms: 100 });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    nora.handle(Input::Http(HttpResponse { id: messages.id, status: 502, body: String::new() }));
    assert_eq!(nora.last_failure(), Some(&NoraError::Http(502)));
    assert_eq!(nora.last_error(), Some("the server answered HTTP 502"));
    assert!(nora.is_busy(), "a failed read is not the end of the turn");
    // The queue of that same round answered, the next round reads again.
    nora.handle(Input::Http(HttpResponse { id: queue.id, status: 502, body: String::new() }));
    poll(&mut nora, 10_000, json!([]), json!([]));
    assert_eq!((nora.last_failure(), nora.last_error()), (None, None));
}

#[test]
fn replies_that_say_nothing_are_not_shown_and_do_not_end_the_wait() {
    let mut nora = started();
    poll(&mut nora, 500, json!([{"role": "assistant", "text": "NO_REPLY"}, {"role": "assistant", "content": ""}]), json!([]));
    assert_eq!(nora.transcript().len(), 1, "only the user's own message");
    nora.handle(Input::Tick { now_ms: 5_000 });
    assert!(nora.is_busy());
}

#[test]
fn a_machine_block_never_reaches_the_transcript() {
    let mut nora = started();
    let reply = "Voici le total.\n```nora\n{\"followups\": [\"Et le mois dernier ?\"]}\n```";
    poll(&mut nora, 500, json!([{"role": "assistant", "text": reply}]), json!([]));
    assert_eq!(nora.transcript()[1].text, "Voici le total.");
    assert_eq!(nora.transcript()[1].followups, vec!["Et le mois dernier ?"]);
}

#[test]
fn a_buffer_that_briefly_shrinks_does_not_repeat_replies() {
    let mut nora = started();
    let (one, two) = (json!({"role": "assistant", "text": "Un"}), json!({"role": "assistant", "text": "Deux"}));
    poll(&mut nora, 100, json!([one, two]), json!([]));
    assert_eq!(nora.transcript().len(), 3);
    // The cache lost a reply for a moment, then got it back.
    poll(&mut nora, 600, json!([one]), json!([]));
    poll(&mut nora, 1_100, json!([one, two]), json!([]));
    assert_eq!(nora.transcript().len(), 3);
}

#[test]
fn a_follow_up_waits_for_its_own_answer() {
    let mut nora = started();
    let first = json!({"role": "assistant", "text": "Première réponse."});
    poll(&mut nora, 100, json!([first]), json!([]));
    nora.handle(Input::Tick { now_ms: 2_000 });
    assert!(!nora.is_busy());
    let sent = nora.handle(Input::Send("Et ensuite ?".into()));
    nora.handle(ok(request_to(&sent, protocol::POST_MESSAGE), json!({"thread_id": "t1"})));
    // The buffer still holds only the first answer: the second turn goes on waiting.
    poll(&mut nora, 2_100, json!([first]), json!([]));
    poll(&mut nora, 6_000, json!([first]), json!([]));
    assert!(nora.is_busy());
    let second = json!({"role": "assistant", "text": "Seconde réponse."});
    poll(&mut nora, 7_100, json!([first, second]), json!([]));
    nora.handle(Input::Tick { now_ms: 9_000 });
    assert!(!nora.is_busy());
    assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("Seconde réponse."));
}

#[test]
fn with_the_send_time_only_replies_stamped_after_it_are_new() {
    let mut nora = started_at(1_000.0);
    let old = json!({"role": "assistant", "text": "Ancienne réponse", "ts": 900.0});
    let new = json!({"role": "assistant", "text": "Nouvelle réponse", "ts": 1_001.0});
    poll(&mut nora, 100, json!([old, new]), json!([]));
    assert_eq!(nora.transcript().len(), 2, "the user's message and the new reply");
    assert_eq!(nora.transcript()[1].text, "Nouvelle réponse");
    // Read again: nothing is added twice.
    poll(&mut nora, 600, json!([old, new]), json!([]));
    assert_eq!(nora.transcript().len(), 2);
}

#[test]
fn a_flushed_buffer_does_not_hide_the_answer() {
    // The replies live in a cache that a clear can empty. Counting positions would miss an answer that
    // lands as the only message left; its time says it is new.
    let mut nora = started_at(1_000.0);
    let ack = json!({"role": "assistant", "text": "Je transmets votre demande à votre pôle Ventes.", "ts": 1_000.5});
    let answer = json!({"role": "assistant", "text": "253 devis ouverts.", "ts": 1_030.0});
    poll(&mut nora, 500, json!([ack]), json!([]));
    poll(&mut nora, 4_000, json!([]), json!([]));
    poll(&mut nora, 31_000, json!([answer]), json!([]));
    let texts: Vec<&str> = nora.transcript().iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, vec!["Bonjour", "Je transmets votre demande à votre pôle Ventes.", "253 devis ouverts."]);
    nora.handle(Input::Tick { now_ms: 33_000 });
    assert!(!nora.is_busy());
}

#[test]
fn each_send_has_its_own_send_time() {
    let mut nora = started_at(1_000.0);
    let first = json!({"role": "assistant", "text": "Première réponse.", "ts": 1_002.0});
    poll(&mut nora, 100, json!([first]), json!([]));
    nora.handle(Input::Tick { now_ms: 2_000 });
    assert!(!nora.is_busy());
    let sent = nora.handle(Input::Send("Et ensuite ?".into()));
    nora.handle(ok(request_to(&sent, protocol::POST_MESSAGE), json!({"thread_id": "t1", "sent_at": 2_000.0})));
    // The first answer is older than this send, so it does not end the second turn.
    poll(&mut nora, 2_100, json!([first]), json!([]));
    poll(&mut nora, 6_000, json!([first]), json!([]));
    assert!(nora.is_busy());
    let second = json!({"role": "assistant", "text": "Seconde réponse.", "ts": 2_006.0});
    poll(&mut nora, 7_000, json!([first, second]), json!([]));
    nora.handle(Input::Tick { now_ms: 9_000 });
    assert!(!nora.is_busy());
    assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("Seconde réponse."));
}

#[test]
fn the_buffer_is_read_often_at_first_then_less() {
    let mut nora = started();
    let mut wakes = Vec::new();
    for now in [0, 3_000, 15_000] {
        poll(&mut nora, now, json!([]), json!([]));
        wakes.push(nora.next_wake_ms().map(|wake| wake - now));
    }
    assert_eq!(wakes, vec![Some(400), Some(1_000), Some(3_000)]);
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
    // The host can tell a refused session from other failures, to offer to sign in again.
    assert!(nora.failure().is_some_and(NoraError::is_auth));
    nora.handle(Input::Send("Encore".into()));
    assert!(nora.failure().is_none());
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

/// An action the editor reports as having changed nothing (a command that only reads) leaves nothing to undo: the
/// Undo button would otherwise stay on for a change that is not there.
#[test]
fn an_action_that_changed_nothing_leaves_nothing_to_undo() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_execute", "command": "sheet.read", "params": {}, "callback_id": "c4"}]));
    let call = documents(&outputs)[0].clone();
    nora.handle(Input::DocumentResult { id: call.id, result: Ok(json!({"changed": false, "values": [[1]]})) });
    assert!(!nora.can_undo());
}

/// An Undo the editor refuses (the person changed the document after Nora) leaves nothing more to undo: Nora's earlier
/// changes lie under the person's edit too, and each click would only be refused again.
#[test]
fn an_undo_the_editor_refuses_leaves_nothing_more_to_undo() {
    let mut nora = started();
    let outputs = poll(
        &mut nora,
        100,
        json!([]),
        json!([
            {"action": "doc_execute", "command": "format.bold", "params": {}, "callback_id": "c5"},
            {"action": "doc_execute", "command": "format.italic", "params": {}, "callback_id": "c6"}
        ]),
    );
    let mut pending: Vec<DocumentCall> = documents(&outputs).into_iter().cloned().collect();
    while let Some(call) = pending.pop() {
        let next = nora.handle(Input::DocumentResult { id: call.id, result: Ok(json!({})) });
        pending.extend(documents(&next).into_iter().cloned());
    }
    assert!(nora.can_undo());
    let undone = nora.handle(Input::UndoLast);
    let undo = documents(&undone)[0].clone();
    nora.handle(Input::DocumentResult { id: undo.id, result: Err("the document changed since".into()) });
    assert!(!nora.can_undo(), "Nora's earlier change lies under the person's edit as well");
}

#[test]
fn a_failed_action_is_reported_to_nora_and_to_the_user() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_type_text", "text": "x", "callback_id": "c3"}]));
    let call = documents(&outputs)[0].clone();
    let done = nora.handle(Input::DocumentResult { id: call.id, result: Err("no caret".into()) });
    assert_eq!(request_to(&done, protocol::RECEIVE_OBSERVATION).body["observation"], json!({"kind": "document", "ok": false, "error": "no caret"}));
    assert_eq!(nora.last_error(), Some("no caret"));
    assert_eq!(nora.last_failure(), None, "the editor's own words, shown as they are");
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
    // The turn stays open while an answer is pending, however quiet it is (even past the eight minutes).
    nora.handle(Input::Tick { now_ms: 600_000 });
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

#[test]
fn stop_ends_the_turn_at_once_and_nothing_more_is_read() {
    let mut nora = started();
    let polls = nora.handle(Input::Tick { now_ms: 100 });
    let messages = request_to(&polls, protocol::GET_MESSAGES).clone();
    let stopped = nora.handle(Input::Cancel);
    assert!(stopped.contains(&Output::Changed));
    assert!(!nora.is_busy() && nora.stopped() && nora.failure().is_none());
    assert_eq!(nora.next_wake_ms(), None);
    assert!(requests(&nora.handle(Input::Tick { now_ms: 5_000 })).is_empty());
    // The reply that was on its way is not shown.
    nora.handle(ok(&messages, json!({"success": true, "messages": [{"role": "assistant", "text": "Trop tard"}], "cards": []})));
    assert_eq!(nora.transcript().len(), 1);
    // Stopping again does nothing; the next message clears it.
    assert!(nora.handle(Input::Cancel).is_empty());
    let sent = nora.handle(Input::Send("Autre chose".into()));
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    assert!(nora.is_busy() && !nora.stopped());
    // The read of the queue made before the stop never got its answer: it does not hold this turn.
    nora.handle(ok(&post, json!({"thread_id": "t1"})));
    poll(&mut nora, 6_000, json!([{"role": "assistant", "text": "Voici."}]), json!([]));
    nora.handle(Input::Tick { now_ms: 8_000 });
    assert!(!nora.is_busy());
}

#[test]
fn nothing_to_stop_does_nothing() {
    let mut nora = session();
    assert!(nora.handle(Input::Cancel).is_empty());
    assert!(!nora.stopped());
}

#[test]
fn a_first_message_stopped_before_the_server_named_its_thread_leaves_it_behind() {
    let mut nora = session();
    let sent = nora.handle(Input::Send("Bonjour".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    nora.handle(Input::Cancel);
    nora.handle(ok(&start, json!({"thread_id": "t1"})));
    assert_eq!(nora.thread_id(), None);
    assert!(!nora.is_busy());
    let next = nora.handle(Input::Send("Autre chose".into()));
    request_to(&next, protocol::START_OR_RESUME_THREAD);
}

#[test]
fn stop_refuses_what_waits_for_the_user() {
    let mut nora = started();
    poll(&mut nora, 100, json!([]), json!([{"action": "doc_save", "callback_id": "c7"}, {"action": "doc_save", "callback_id": "c8"}]));
    assert_eq!(nora.confirmations().len(), 2);
    let stopped = nora.handle(Input::Cancel);
    assert!(nora.confirmations().is_empty());
    assert!(documents(&stopped).is_empty());
    assert_eq!(refusal_of(&stopped, "c7"), "the user stopped Nora");
    assert_eq!(refusal_of(&stopped, "c8"), "the user stopped Nora");
}

#[test]
fn stop_lets_the_running_action_finish_and_report_its_real_result() {
    let mut nora = started();
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_execute", "command": "format.bold", "params": {}, "callback_id": "c2"}]));
    let call = documents(&outputs)[0].clone();
    let stopped = nora.handle(Input::Cancel);
    assert!(requests(&stopped).is_empty(), "nothing is said about it yet: the editor is running it");
    let done = nora.handle(Input::DocumentResult { id: call.id, result: Ok(json!({})) });
    let observation = request_to(&done, protocol::RECEIVE_OBSERVATION);
    assert_eq!(observation.body["callback_id"], "c2");
    assert_eq!(observation.body["observation"]["ok"], true);
    assert!(nora.can_undo(), "what Nora did before the stop can be undone");
}

#[test]
fn what_the_stopped_message_still_queues_is_refused_not_run() {
    let mut nora = started_at(1_000.0);
    let polls = nora.handle(Input::Tick { now_ms: 100 });
    let queue = request_to(&polls, protocol::GET_ACTIONS).clone();
    nora.handle(Input::Cancel);
    // The read on its way took these actions out of the queue: Nora is told, the editor runs nothing.
    let late = nora.handle(ok(
        &queue,
        json!({"success": true, "actions": [{"action": "doc_type_text", "text": "x", "callback_id": "c1"}], "timestamp": 1_001.0}),
    ));
    assert!(documents(&late).is_empty());
    assert_eq!(refusal_of(&late, "c1"), "the user stopped Nora");
    // The next message: a queue filled before it is the stopped work, one filled after it is its own.
    let sent = nora.handle(Input::Send("Autre chose".into()));
    nora.handle(ok(request_to(&sent, protocol::POST_MESSAGE), json!({"thread_id": "t1", "sent_at": 2_000.0})));
    let older = poll_queue(
        &mut nora,
        2_100,
        json!({"success": true, "actions": [{"action": "doc_type_text", "text": "y", "callback_id": "c2"}], "timestamp": 1_500.0}),
    );
    assert!(documents(&older).is_empty());
    assert_eq!(refusal_of(&older, "c2"), "the user stopped Nora");
    let own =
        poll_queue(&mut nora, 2_600, json!({"success": true, "actions": [{"action": "doc_observe", "callback_id": "c3"}], "timestamp": 2_000.5}));
    assert_eq!(documents(&own).len(), 1);
}

#[test]
fn without_a_stop_an_older_queue_still_runs() {
    // Only a Stop makes an older queue suspect; a new conversation forgets it.
    let mut nora = started_at(1_000.0);
    let outputs =
        poll_queue(&mut nora, 100, json!({"success": true, "actions": [{"action": "doc_observe", "callback_id": "c1"}], "timestamp": 900.0}));
    assert_eq!(documents(&outputs).len(), 1);
    nora.handle(Input::Cancel);
    nora.handle(Input::Reset);
    let sent = nora.handle(Input::Send("Bonjour".into()));
    nora.handle(ok(request_to(&sent, protocol::START_OR_RESUME_THREAD), json!({"thread_id": "t2", "sent_at": 3_000.0})));
    let outputs =
        poll_queue(&mut nora, 200, json!({"success": true, "actions": [{"action": "doc_observe", "callback_id": "c2"}], "timestamp": 2_900.0}));
    assert_eq!(documents(&outputs).len(), 1);
}

/// The turns and the failures in the vocabulary of `craft-ai-core`.
#[cfg(feature = "ai-core")]
mod ai_core {
    use super::*;
    use craft_ai_core::{ErrorKind, JobState};

    #[test]
    fn failures_have_the_shared_kinds() {
        let server = |status: u16| NoraError::Server { status, message: "refused".into() };
        let cases = [
            (NoraError::Http(401), ErrorKind::Auth),
            (server(403), ErrorKind::Auth),
            (NoraError::Http(429), ErrorKind::RateLimited),
            (NoraError::Http(502), ErrorKind::Network),
            (server(503), ErrorKind::Network),
            (NoraError::Http(504), ErrorKind::Network),
            (NoraError::Unreachable("No answer from Nora".into()), ErrorKind::Network),
            (server(417), ErrorKind::InvalidRequest),
            (NoraError::Http(404), ErrorKind::InvalidRequest),
            (NoraError::Http(500), ErrorKind::Provider),
            (NoraError::Malformed("no `message`".into()), ErrorKind::Provider),
            (NoraError::Timeout, ErrorKind::Provider),
        ];
        for (error, kind) in cases {
            assert_eq!(error.kind(), kind, "{error:?}");
            let shared = error.to_ai_error();
            assert_eq!((shared.kind, shared.message), (kind, error.to_string()));
        }
        assert!(NoraError::Unreachable("down".into()).to_ai_error().is_retryable());
        assert!(!NoraError::Timeout.to_ai_error().is_retryable());
    }

    /// Records the state after each step and checks that every change is one `craft-ai-core` allows.
    fn lifecycle(nora: &NoraSession, seen: &mut Vec<JobState>) {
        if let Some(state) = nora.turn_state() {
            if let Some(last) = seen.last() {
                assert!(last.can_become(&state), "{last:?} cannot become {state:?}");
            }
            if seen.last() != Some(&state) {
                seen.push(state);
            }
        }
    }

    #[test]
    fn an_answered_turn_is_queued_then_running_then_succeeded() {
        let mut nora = session();
        assert_eq!(nora.turn_state(), None);
        let mut seen = Vec::new();
        let sent = nora.handle(Input::Send("Bonjour".into()));
        lifecycle(&nora, &mut seen);
        nora.handle(ok(request_to(&sent, protocol::START_OR_RESUME_THREAD), json!({"thread_id": "t1"})));
        lifecycle(&nora, &mut seen);
        poll(&mut nora, 100, json!([{"role": "assistant", "text": "Voici."}]), json!([]));
        lifecycle(&nora, &mut seen);
        nora.handle(Input::Tick { now_ms: 2_000 });
        lifecycle(&nora, &mut seen);
        assert_eq!(seen, vec![JobState::Queued, JobState::Running { percent: None }, JobState::Succeeded]);
        nora.handle(Input::Reset);
        assert_eq!(nora.turn_state(), None);
    }

    #[test]
    fn a_stopped_turn_is_cancelled_and_a_refused_one_failed_with_its_kind() {
        let mut nora = started();
        let mut seen = vec![nora.turn_state().unwrap_or(JobState::Queued)];
        nora.handle(Input::Cancel);
        lifecycle(&nora, &mut seen);
        assert_eq!(nora.turn_state(), Some(JobState::Cancelled));

        let mut nora = session();
        let sent = nora.handle(Input::Send("Bonjour".into()));
        let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
        nora.handle(Input::Http(HttpResponse { id: start.id, status: 401, body: String::new() }));
        match nora.turn_state() {
            Some(JobState::Failed { error }) => assert_eq!(error.kind, ErrorKind::Auth),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}

// //// Neoffice — the cards and the links of the pane, print and the PDF (step 7 of « Nora partout », 23.10.2026).

/// One poll at `now` whose replies answer carries `cards` too.
fn poll_with_cards(nora: &mut NoraSession, now: u64, replies: Value, cards: Value) -> Vec<Output> {
    let outputs = nora.handle(Input::Tick { now_ms: now });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    let mut out = nora.handle(ok(&messages, json!({"success": true, "messages": replies, "cards": cards})));
    out.extend(nora.handle(ok(&queue, json!({"success": true, "actions": []}))));
    out
}

fn template_card(status: &str) -> Value {
    json!({"token": "c-tpl", "kind": "Template", "status": status, "title": "Modèle de lettre", "conversation_id": "t1",
           "payload": {"summary": "« Lettre.docx » devient le modèle", "confirm_label": "Confirmer"}})
}

#[test]
fn the_cards_of_this_conversation_are_shown_under_the_reply_that_made_them() {
    let mut nora = started();
    poll_with_cards(
        &mut nora,
        100,
        json!([{"role": "assistant", "text": "La lettre est prête."}]),
        json!([
            {"token": "c-doc", "kind": "Document", "status": "Executed", "title": "Lettre · Régie", "conversation_id": "t1",
             "result": "shared", "payload": {"editor_url": "/drive/f/abc", "open_label": "Ouvrir dans NeoDoc"}},
            {"token": "c-other", "kind": "Email", "status": "Pending", "title": "Ailleurs", "conversation_id": "t9"},
            {"token": "c-old", "kind": "Email", "status": "Pending", "title": "Serveur ancien"}
        ]),
    );
    let cards = nora.cards();
    assert_eq!(cards.len(), 1, "the person's pending cards of other conversations are the desk's");
    assert_eq!(cards[0].card.token, "c-doc");
    assert_eq!(cards[0].after, None, "shown at the end while the turn runs");
    nora.handle(Input::Tick { now_ms: 5_000 });
    assert!(!nora.is_busy());
    assert_eq!(nora.cards()[0].after, Some(2), "under the reply once the turn ended");
    // The same card again changes nothing; a new state is taken.
    let sent = nora.handle(Input::Send("Et ensuite ?".into()));
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    nora.handle(ok(&post, json!({"thread_id": "t1"})));
    poll_with_cards(&mut nora, 6_000, json!([]), json!([template_card("Pending")]));
    assert_eq!(nora.cards().len(), 2);
    assert_eq!(nora.cards()[0].after, Some(2), "a card shown stays where it was");
}

#[test]
fn a_template_card_is_answered_once_through_its_own_door() {
    let mut nora = started();
    poll_with_cards(&mut nora, 100, json!([{"role": "assistant", "text": "Confirmez sur la carte."}]), json!([template_card("Pending")]));
    nora.handle(Input::Tick { now_ms: 5_000 });
    let answer = nora.handle(Input::AnswerCard { token: "c-tpl".into(), approve: true });
    let request = request_to(&answer, protocol::CONFIRM_TEMPLATE).clone();
    assert_eq!(request.body, json!({"token": "c-tpl", "action": "approve"}));
    assert!(nora.cards()[0].answering);
    assert!(requests(&nora.handle(Input::AnswerCard { token: "c-tpl".into(), approve: true })).is_empty(), "one answer");
    nora.handle(ok(
        &request,
        json!({"success": true, "card": {"token": "c-tpl", "kind": "Template", "status": "Executed",
        "title": "Modèle de lettre", "result": "Cette lettre est maintenant le modèle."}}),
    ));
    let shown = &nora.cards()[0];
    assert!(!shown.answering && shown.card.status == "Executed" && !shown.card.awaits_the_person());
    assert_eq!(shown.card.result.as_deref(), Some("Cette lettre est maintenant le modèle."));
    assert_eq!(shown.card.conversation, "t1", "it stays this conversation's");
    assert!(requests(&nora.handle(Input::AnswerCard { token: "c-tpl".into(), approve: false })).is_empty(), "answered");
}

#[test]
fn a_card_that_does_not_wait_for_the_person_is_never_answered_from_the_pane() {
    let mut nora = started();
    poll_with_cards(
        &mut nora,
        100,
        json!([]),
        json!([
            {"token": "c-mail", "kind": "Email", "status": "Pending", "title": "Un e-mail", "conversation_id": "t1"}
        ]),
    );
    assert_eq!(nora.cards().len(), 1);
    assert!(requests(&nora.handle(Input::AnswerCard { token: "c-mail".into(), approve: true })).is_empty());
    assert!(requests(&nora.handle(Input::AnswerCard { token: "nope".into(), approve: true })).is_empty());
}

#[test]
fn only_a_page_of_the_instance_is_opened() {
    let mut nora =
        NoraSession::new(SessionConfig { instance_origin: Some("https://erp.example.test".into()), ..SessionConfig::default() }, context());
    assert_eq!(nora.handle(Input::OpenLink("/drive/f/abc".into())), vec![Output::Open("/drive/f/abc".into())]);
    assert_eq!(nora.handle(Input::OpenLink("https://erp.example.test/app/webmail?uid=4".into())), vec![Output::Open("/app/webmail?uid=4".into())]);
    for refused in ["https://elsewhere.example/drive/f/abc", "//elsewhere.example/x", "javascript:alert(1)", "/api/method/x"] {
        assert!(nora.handle(Input::OpenLink(refused.into())).is_empty(), "{refused}");
    }
    // Without the instance's origin (Neodesk), only a path is a page of it.
    let mut desk = session();
    assert!(desk.handle(Input::OpenLink("https://erp.example.test/drive/f/abc".into())).is_empty());
}

#[test]
fn print_and_the_pdf_run_in_the_editor_without_a_question() {
    let mut nora = started();
    let out = poll(
        &mut nora,
        100,
        json!([]),
        json!([
            {"action": "doc_print", "callback_id": "p1"},
            {"action": "doc_export_pdf", "callback_id": "p2"}
        ]),
    );
    let kinds: Vec<&DocActionKind> = documents(&out).into_iter().map(|call| &call.kind).collect();
    assert_eq!(kinds, vec![&DocActionKind::Print, &DocActionKind::ExportPdf]);
    assert!(nora.confirmations().is_empty(), "the print dialog is the person's yes; the PDF goes to a draft, never sent");
}

fn mailbox_card(status: &str) -> Value {
    json!({"token": "c-box", "kind": "Mailbox", "status": status, "title": "Brouillon d'e-mail", "conversation_id": "t1",
    "payload": {"summary": "Depuis quelle boîte ?", "refuse_label": "Annuler", "choices": [
        {"id": "Bureau", "label": "Bureau (bureau@exemple.ch)"},
        {"id": "Ventes", "label": "Ventes (ventes@exemple.ch)"}
    ]}})
}

#[test]
fn a_mailbox_card_is_answered_by_one_of_its_choices_once_through_the_pane_s_door() {
    let mut nora = started();
    poll_with_cards(&mut nora, 100, json!([{"role": "assistant", "text": "Choisissez la boîte."}]), json!([mailbox_card("Pending")]));
    nora.handle(Input::Tick { now_ms: 5_000 });
    // A bare yes is no choice, and a choice the card does not offer is none either.
    assert!(requests(&nora.handle(Input::AnswerCard { token: "c-box".into(), approve: true })).is_empty());
    assert!(requests(&nora.handle(Input::ChooseOnCard { token: "c-box".into(), choice: "Direction".into() })).is_empty());
    assert!(!nora.cards()[0].answering);
    let chosen = nora.handle(Input::ChooseOnCard { token: "c-box".into(), choice: "Ventes".into() });
    let request = request_to(&chosen, protocol::ANSWER_IN_PANE).clone();
    assert_eq!(request.body, json!({"token": "c-box", "action": "approve", "choice": "Ventes"}));
    assert!(nora.cards()[0].answering);
    assert!(requests(&nora.handle(Input::ChooseOnCard { token: "c-box".into(), choice: "Bureau".into() })).is_empty(), "one answer");
    nora.handle(ok(
        &request,
        json!({"success": true, "card": {"token": "c-box", "kind": "Mailbox", "status": "Executed", "title": "Brouillon d'e-mail",
        "result": "Le brouillon est dans la boîte Ventes.", "payload": {"editor_url": "/app/webmail?account=Ventes&folder=Drafts&uid=7",
        "open_label": "Ouvrir le brouillon"}}}),
    ));
    let shown = &nora.cards()[0];
    assert!(!shown.answering && !shown.card.awaits_the_person());
    assert_eq!(shown.card.open_url.as_deref(), Some("/app/webmail?account=Ventes&folder=Drafts&uid=7"));
    assert!(requests(&nora.handle(Input::ChooseOnCard { token: "c-box".into(), choice: "Bureau".into() })).is_empty(), "answered");
}

#[test]
fn a_mailbox_card_s_no_goes_through_the_pane_s_door() {
    let mut nora = started();
    poll_with_cards(&mut nora, 100, json!([]), json!([mailbox_card("Pending")]));
    let refused = nora.handle(Input::AnswerCard { token: "c-box".into(), approve: false });
    let request = request_to(&refused, protocol::ANSWER_IN_PANE).clone();
    assert_eq!(request.body, json!({"token": "c-box", "action": "reject"}));
    // A card that is no longer pending offers nothing.
    let mut later = started();
    poll_with_cards(&mut later, 100, json!([]), json!([mailbox_card("Expired")]));
    assert!(requests(&later.handle(Input::ChooseOnCard { token: "c-box".into(), choice: "Bureau".into() })).is_empty());
}

#[test]
fn the_document_s_file_is_handed_without_a_question() {
    let mut nora = started();
    let out = poll(&mut nora, 100, json!([]), json!([{"action": "doc_export_file", "callback_id": "f1"}]));
    let kinds: Vec<&DocActionKind> = documents(&out).into_iter().map(|call| &call.kind).collect();
    assert_eq!(kinds, vec![&DocActionKind::ExportFile]);
    assert!(nora.confirmations().is_empty(), "the file goes to a draft, never sent, never saved");
}

#[test]
fn a_new_conversation_forgets_its_cards() {
    let mut nora = started();
    poll_with_cards(&mut nora, 100, json!([]), json!([template_card("Pending")]));
    assert_eq!(nora.cards().len(), 1);
    nora.handle(Input::Reset);
    assert!(nora.cards().is_empty());
}
