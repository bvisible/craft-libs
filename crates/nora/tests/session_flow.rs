// //// Neoffice — added file (no upstream equivalent): end-to-end tests of the Nora conversation state machine.

use craft_nora::{
    AppKind, CommandPolicy, Correction, DocActionKind, DocumentCall, DocumentInfo, HttpRequest, HttpResponse, Input, NoraError, NoraSession, Output,
    PageContext, SessionConfig, protocol,
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
    // The read still on its way left before the deadline: it says nothing of the silence after it...
    nora.handle(Input::Tick { now_ms: 480_100 });
    assert!(nora.is_busy());
    // ...but even with a read the host never answers, the wait does end (the tick back from a gap without frames
    // judges nothing: the one after it does).
    nora.handle(Input::Tick { now_ms: 720_100 });
    assert!(nora.is_busy());
    let outputs = nora.handle(Input::Tick { now_ms: 720_150 });
    assert!(!nora.is_busy());
    assert_eq!(nora.last_error(), Some("Nora did not answer in time"));
    assert_eq!(nora.failure(), Some(&NoraError::Timeout));
    assert_eq!(nora.last_failure(), Some(&NoraError::Timeout), "for the host to say it in its own words");
    assert!(outputs.contains(&Output::Changed));
}

/// Gives up on the turn of `nora` (started at 0) for silence: a read sent after the deadline brings nothing.
fn given_up(nora: &mut NoraSession) {
    poll(nora, 100, json!([]), json!([]));
    poll(nora, 480_100, json!([]), json!([]));
    assert!(nora.is_busy(), "the read after the deadline is what says it");
    nora.handle(Input::Tick { now_ms: 480_200 });
    assert!(!nora.is_busy());
    assert_eq!(nora.failure(), Some(&NoraError::Timeout));
}

#[test]
fn a_read_made_after_the_deadline_that_brings_nothing_ends_the_wait() {
    let mut nora = started();
    given_up(&mut nora);
    assert_eq!(nora.last_error(), Some("Nora did not answer in time"));
    assert!(nora.reads_late(), "the server may still finish the turn");
}

/// A tab in the background draws no frame, nor does a machine too busy to draw: the next tick comes minutes later, and
/// the answer waits on the server. The wait does not end on the clock: the server is read first.
#[test]
fn a_clock_that_jumps_reads_the_server_before_giving_up() {
    let mut nora = started_at(1_000.0);
    poll(&mut nora, 100, json!([]), json!([]));
    let answer = json!({"role": "assistant", "text": "Le rendez-vous est prêt.", "ts": 1_500.0});
    poll(&mut nora, 900_000, json!([answer]), json!([]));
    assert!(nora.is_busy());
    assert_eq!((nora.failure(), nora.last_error()), (None, None));
    assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("Le rendez-vous est prêt."));
    nora.handle(Input::Tick { now_ms: 902_000 });
    assert!(!nora.is_busy());
    assert_eq!((nora.failure(), nora.last_error()), (None, None));
    // The same jump with nothing on the server: one read, then the wait ends.
    let mut quiet = started();
    poll(&mut quiet, 100, json!([]), json!([]));
    poll(&mut quiet, 900_000, json!([]), json!([]));
    assert!(quiet.is_busy());
    quiet.handle(Input::Tick { now_ms: 900_100 });
    assert_eq!(quiet.failure(), Some(&NoraError::Timeout));
}

/// The turn given up on is read on, every fifteen seconds: its late answer and its card are shown as they come, and the
/// failure is taken back. The next message is not needed for it.
#[test]
fn a_late_answer_and_its_card_are_shown_as_they_come() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    assert_eq!(nora.next_wake_ms(), Some(495_200), "the host is woken for the next late read");
    assert!(requests(&nora.handle(Input::Tick { now_ms: 490_000 })).is_empty());
    // A late read that brings nothing leaves the failure shown.
    poll(&mut nora, 495_200, json!([]), json!([]));
    assert_eq!(nora.last_error(), Some("Nora did not answer in time"));
    assert!(nora.reads_late() && !nora.is_busy());
    let answer = json!({"role": "assistant", "text": "Voici la proposition de rendez-vous.", "ts": 1_600.0});
    let card = json!({"token": "c-event", "kind": "Event", "status": "Pending", "title": "Rendez-vous", "conversation_id": "t1"});
    let outputs = nora.handle(Input::Tick { now_ms: 510_200 });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    let shown = nora.handle(ok(&messages, json!({"success": true, "messages": [answer], "cards": [card]})));
    nora.handle(ok(&queue, json!({"success": true, "actions": []})));
    assert!(shown.contains(&Output::Changed));
    assert!(!nora.is_busy(), "shown, not waited for: the person may write again");
    assert_eq!((nora.last_failure(), nora.last_error()), (None, None), "the failure is taken back");
    assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("Voici la proposition de rendez-vous."));
    assert_eq!(nora.cards().len(), 1);
    assert_eq!(nora.cards()[0].after, Some(nora.transcript().len()), "under the late answer");
    // The answer is in: one more read for what comes with it, then the late reading ends.
    assert_eq!(nora.next_wake_ms(), Some(511_700));
    poll(&mut nora, 511_700, json!([answer]), json!([]));
    assert_eq!(nora.transcript().len(), 2, "a reply is shown once");
    assert!(nora.reads_late());
    assert!(requests(&nora.handle(Input::Tick { now_ms: 526_700 })).is_empty());
    assert!(!nora.reads_late());
    assert_eq!(nora.next_wake_ms(), None);
    assert_eq!(nora.last_error(), None);
}

#[test]
fn a_late_acknowledgement_is_shown_and_the_late_reading_goes_on() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    let ack = json!({"role": "assistant", "text": "Je transmets votre demande à votre pôle Ventes.", "ts": 1_100.0});
    poll(&mut nora, 495_200, json!([ack]), json!([]));
    assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("Je transmets votre demande à votre pôle Ventes."));
    assert_eq!(nora.last_error(), Some("Nora did not answer in time"), "nothing else may come: the failure stays");
    assert!(nora.reads_late());
    assert_eq!(nora.next_wake_ms(), Some(510_200));
    // A read on its way that the host has not answered yet does not wake the window every frame.
    nora.handle(Input::Tick { now_ms: 510_200 });
    nora.handle(Input::Tick { now_ms: 530_000 });
    assert_eq!(nora.next_wake_ms(), Some(545_000));
}

#[test]
fn a_new_card_alone_is_news_too() {
    let mut nora = started();
    given_up(&mut nora);
    let card = json!({"token": "c-doc", "kind": "Document", "status": "Executed", "title": "Lettre", "conversation_id": "t1"});
    poll_with_cards(&mut nora, 495_200, json!([]), json!([card]));
    assert!(!nora.is_busy() && nora.last_error().is_none() && nora.reads_late());
    assert_eq!(nora.cards().len(), 1);
    // While the turn runs, a new card moves the deadline as a reply does.
    let mut busy = started();
    poll(&mut busy, 100, json!([]), json!([]));
    let card = json!({"token": "c-doc", "kind": "Document", "status": "Executed", "title": "Lettre", "conversation_id": "t1"});
    poll_with_cards(&mut busy, 400_000, json!([]), json!([card]));
    poll(&mut busy, 480_100, json!([]), json!([]));
    busy.handle(Input::Tick { now_ms: 480_200 });
    assert!(busy.is_busy());
}

/// The person was told Nora did not answer in time, and may have closed the pane: what she asks of the document then is
/// refused, and she is told why.
#[test]
fn a_late_action_is_refused_and_never_runs() {
    let mut nora = started();
    given_up(&mut nora);
    let bold = json!({"action": "doc_execute", "command": "format.bold", "params": {}, "callback_id": "c2"});
    let outputs = poll(&mut nora, 495_200, json!([]), json!([bold, {"action": "doc_observe", "callback_id": "c3"}]));
    assert!(documents(&outputs).is_empty());
    for callback in ["c2", "c3"] {
        assert!(refusal_of(&outputs, callback).as_str().is_some_and(|why| why.contains("stopped waiting")), "{callback}");
    }
    assert!(!nora.is_busy() && !nora.can_undo());
}

#[test]
fn a_stop_ends_the_late_reading() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    nora.handle(Input::Cancel);
    assert!(!nora.reads_late());
    assert!(requests(&nora.handle(Input::Tick { now_ms: 500_000 })).is_empty());
    assert_eq!(nora.next_wake_ms(), None);
    // What the stopped turn still queued is refused when the next message reads it; what this one queues runs.
    let sent = nora.handle(Input::Send("Autre chose".into()));
    nora.handle(ok(request_to(&sent, protocol::POST_MESSAGE), json!({"thread_id": "t1", "sent_at": 1_500.0})));
    let old = poll_queue(&mut nora, 501_000, json!({"actions": [{"action": "doc_observe", "callback_id": "o1"}], "timestamp": 1_200.0}));
    assert!(documents(&old).is_empty() && refusal_of(&old, "o1").as_str().is_some());
    let new = poll_queue(&mut nora, 502_000, json!({"actions": [{"action": "doc_observe", "callback_id": "n1"}], "timestamp": 1_600.0}));
    assert_eq!(documents(&new).len(), 1);
}

/// A read on its way when the late reading ends brings an action: the turn was given up on, it is refused all the same.
#[test]
fn an_action_answered_after_the_late_reading_ended_is_refused() {
    let mut nora = started();
    given_up(&mut nora);
    let outputs = nora.handle(Input::Tick { now_ms: 1_799_000 });
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    nora.handle(Input::Tick { now_ms: 1_800_000 });
    assert!(!nora.reads_late());
    let refused = nora.handle(ok(&queue, json!({"success": true, "actions": [{"action": "doc_observe", "callback_id": "c9"}]})));
    assert!(documents(&refused).is_empty() && refusal_of(&refused, "c9").as_str().is_some());
}

#[test]
fn the_late_reading_stops_thirty_minutes_after_the_turn_started() {
    let mut nora = started();
    given_up(&mut nora);
    poll(&mut nora, 1_000_000, json!([]), json!([]));
    assert!(nora.reads_late());
    assert!(requests(&nora.handle(Input::Tick { now_ms: 1_800_000 })).is_empty());
    assert!(!nora.reads_late());
    assert_eq!(nora.next_wake_ms(), None);
    assert_eq!(nora.failure(), Some(&NoraError::Timeout), "the failure stays shown");
}

#[test]
fn a_new_conversation_ends_the_late_reading() {
    let mut nora = started();
    given_up(&mut nora);
    nora.handle(Input::Reset);
    assert!(!nora.reads_late());
    assert!(requests(&nora.handle(Input::Tick { now_ms: 500_000 })).is_empty());
    assert_eq!(nora.next_wake_ms(), None);
}

/// The person asks again while the turn given up on is read late: its late answer is shown, and it is not taken for
/// the answer to the new message, which the turn waits for.
#[test]
fn a_late_answer_that_comes_after_the_next_message_is_shown_without_ending_it() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    let sent = nora.handle(Input::Send("Alors ?".into()));
    assert!(!nora.reads_late());
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    let late = json!({"role": "assistant", "text": "Réponse au premier message.", "ts": 1_400.0});
    // Before the server has said when it took the new message, the first one's answer comes.
    poll(&mut nora, 495_300, json!([late]), json!([]));
    assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("Réponse au premier message."));
    nora.handle(ok(&post, json!({"thread_id": "t1", "sent_at": 1_500.0})));
    poll(&mut nora, 497_000, json!([late]), json!([]));
    nora.handle(Input::Tick { now_ms: 499_000 });
    assert!(nora.is_busy(), "the late answer is the first message's");
    let answer = json!({"role": "assistant", "text": "Réponse au second.", "ts": 1_600.0});
    poll(&mut nora, 500_000, json!([late, answer]), json!([]));
    nora.handle(Input::Tick { now_ms: 502_000 });
    assert!(!nora.is_busy());
    let texts: Vec<&str> = nora.transcript().iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, vec!["Bonjour", "Alors ?", "Réponse au premier message.", "Réponse au second."]);
    // A reply already taken before the server said when it took the message counts once it has said it.
    let mut quick = started_at(1_000.0);
    given_up(&mut quick);
    let sent = quick.handle(Input::Send("Alors ?".into()));
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    poll(&mut quick, 495_300, json!([{"role": "assistant", "text": "Réponse au second.", "ts": 1_600.0}]), json!([]));
    quick.handle(ok(&post, json!({"thread_id": "t1", "sent_at": 1_500.0})));
    poll(&mut quick, 496_000, json!([]), json!([]));
    quick.handle(Input::Tick { now_ms: 498_000 });
    assert!(!quick.is_busy());
}

/// A send that got no answer (the host gave up, the connection broke), or a gateway's in front of the instance, may
/// have reached Nora: the conversation is read late for her answer. A refusal of the server is not.
#[test]
fn a_send_that_got_no_answer_is_read_late_and_a_refusal_is_not() {
    let finished = || {
        let mut nora = started_at(1_000.0);
        poll(&mut nora, 100, json!([{"role": "assistant", "text": "Bonjour !", "ts": 1_001.0}]), json!([]));
        nora.handle(Input::Tick { now_ms: 2_000 });
        assert!(!nora.is_busy());
        nora
    };
    for (status, body) in [(0, json!({"exception": "it took too long"})), (504, json!({}))] {
        let mut nora = finished();
        let sent = nora.handle(Input::Send("Prends rendez-vous mardi".into()));
        let post = request_to(&sent, protocol::POST_MESSAGE).clone();
        nora.handle(Input::Http(HttpResponse { id: post.id, status, body: body.to_string() }));
        assert!(!nora.is_busy() && nora.reads_late(), "{status}");
        let answer = json!({"role": "assistant", "text": "C'est noté pour mardi.", "ts": 1_100.0});
        poll(&mut nora, 20_000, json!([answer]), json!([]));
        assert_eq!(nora.transcript().last().map(|m| m.text.as_str()), Some("C'est noté pour mardi."), "{status}");
        assert!(!nora.is_busy() && nora.last_error().is_none(), "{status}");
    }
    let mut refused = finished();
    let sent = refused.handle(Input::Send("Prends rendez-vous mardi".into()));
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    let inner = serde_json::to_string(&json!({"message": "Not permitted"})).unwrap();
    let body = json!({"_server_messages": serde_json::to_string(&vec![inner]).unwrap()}).to_string();
    refused.handle(Input::Http(HttpResponse { id: post.id, status: 403, body }));
    assert!(!refused.is_busy() && !refused.reads_late());
    // A send the host never answers at all: no read goes out meanwhile, the clock ends the wait.
    let mut lost = finished();
    lost.handle(Input::Send("Prends rendez-vous mardi".into()));
    lost.handle(Input::Tick { now_ms: 481_000 });
    assert!(lost.is_busy());
    lost.handle(Input::Tick { now_ms: 482_000 });
    assert_eq!(lost.failure(), Some(&NoraError::Timeout));
    assert!(lost.reads_late());
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

// //// Neoffice — an appointment taken from an e-mail is put in the calendar on the person's yes (23.10.2026).
fn event_card(status: &str) -> Value {
    json!({"token": "c-event", "kind": "Event", "status": status, "title": "Rendez-vous", "conversation_id": "t1",
    "payload": {"summary": "Mardi 27 octobre, 14 h, avec Ada", "confirm_label": "Ajouter à l'agenda", "refuse_label": "Annuler"}})
}

#[test]
fn an_event_card_is_answered_yes_or_no_once_through_the_pane_s_door() {
    let mut nora = started();
    poll_with_cards(&mut nora, 100, json!([{"role": "assistant", "text": "Ajoutez-le sur la carte."}]), json!([event_card("Pending")]));
    nora.handle(Input::Tick { now_ms: 5_000 });
    assert!(nora.cards()[0].card.awaits_the_person());
    let yes = nora.handle(Input::AnswerCard { token: "c-event".into(), approve: true });
    let request = request_to(&yes, protocol::ANSWER_IN_PANE).clone();
    assert_eq!(request.body, json!({"token": "c-event", "action": "approve"}));
    assert!(requests(&nora.handle(Input::AnswerCard { token: "c-event".into(), approve: false })).is_empty(), "one answer");
    let mut other = started();
    poll_with_cards(&mut other, 100, json!([]), json!([event_card("Pending")]));
    let no = other.handle(Input::AnswerCard { token: "c-event".into(), approve: false });
    assert_eq!(request_to(&no, protocol::ANSWER_IN_PANE).body, json!({"token": "c-event", "action": "reject"}));
    let mut later = started();
    poll_with_cards(&mut later, 100, json!([]), json!([event_card("Executed")]));
    assert!(requests(&later.handle(Input::AnswerCard { token: "c-event".into(), approve: true })).is_empty(), "done");
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

// A page that runs nothing (a tab in the background, a computer asleep): what Nora asked meanwhile is refused, the rest
// of the turn too, and once the turn is over the person is offered to send the same request again (24.10.2026).

fn type_text(callback: &str) -> Value {
    json!({"action": "doc_type_text", "text": "42", "callback_id": callback})
}

/// The refusal Nora was told for `callback`, as a string.
fn refused_with<'a>(outputs: &'a [Output], callback: &str) -> &'a str {
    refusal_of(outputs, callback).as_str().unwrap_or_default()
}

#[test]
fn an_action_that_waited_for_a_page_that_ran_nothing_is_refused_and_the_request_can_be_resumed() {
    let mut nora = started_at(1_000.0);
    // The page comes back: the action was queued eight minutes before this read, by the server's clock.
    let old = poll_queue(&mut nora, 100, json!({"actions": [type_text("c1")], "queued_at": [1_002.0], "now": 1_482.0}));
    assert!(documents(&old).is_empty(), "never played");
    assert!(refused_with(&old, "c1").contains("background"));
    assert!(nora.is_busy() && nora.resumable().is_none(), "offered once the turn is over");
    // Nora tries again in the same turn: refused too.
    let again = poll_queue(&mut nora, 600, json!({"actions": [type_text("c2")], "queued_at": [1_483.0], "now": 1_483.5}));
    assert!(documents(&again).is_empty() && refused_with(&again, "c2").contains("background"));
    poll(&mut nora, 1_100, json!([{"role": "assistant", "text": "Rien n'a été modifié.", "ts": 1_484.0}]), json!([]));
    nora.handle(Input::Tick { now_ms: 3_000 });
    assert!(!nora.is_busy());
    assert_eq!(nora.resumable(), Some("Bonjour"));
    // The person's click sends the same request again: a new turn, whose actions run.
    let sent = nora.handle(Input::Send(nora.resumable().unwrap().to_string()));
    assert_eq!(nora.resumable(), None);
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    assert_eq!(post.body["message"], "Bonjour");
    nora.handle(ok(&post, json!({"thread_id": "t1", "sent_at": 1_600.0})));
    let fresh = poll_queue(&mut nora, 3_500, json!({"actions": [type_text("c3")], "queued_at": [1_601.0], "now": 1_601.5}));
    assert_eq!(documents(&fresh).len(), 1);
}

#[test]
fn an_action_read_at_once_runs_as_before() {
    let mut nora = started_at(1_000.0);
    let outputs = poll_queue(&mut nora, 100, json!({"actions": [type_text("c1")], "queued_at": [1_001.0], "now": 1_002.0}));
    let played = documents(&outputs);
    assert_eq!(played.len(), 1);
    nora.handle(Input::DocumentResult { id: played[0].id, result: Ok(json!({"ok": true})) });
    poll(&mut nora, 600, json!([{"role": "assistant", "text": "C'est fait.", "ts": 1_003.0}]), json!([]));
    nora.handle(Input::Tick { now_ms: 3_000 });
    assert!(!nora.is_busy() && nora.resumable().is_none());
}

/// The queue expired before the page read it (five minutes): the server says so once, and the turn is lost.
#[test]
fn a_queue_that_expired_unread_loses_the_turn() {
    let mut nora = started_at(1_000.0);
    poll_queue(&mut nora, 100, json!({"actions": [], "missed": 1_002.0, "now": 1_490.0}));
    let late = poll_queue(&mut nora, 600, json!({"actions": [type_text("c2")], "queued_at": [1_490.5], "now": 1_491.0}));
    assert!(documents(&late).is_empty() && refused_with(&late, "c2").contains("background"));
    poll(&mut nora, 1_100, json!([{"role": "assistant", "text": "Je n'ai pas pu écrire.", "ts": 1_492.0}]), json!([]));
    nora.handle(Input::Tick { now_ms: 3_000 });
    assert_eq!(nora.resumable(), Some("Bonjour"));
    // One that expired before this turn's send is another turn's: this one is not lost.
    let mut other = started_at(1_000.0);
    poll_queue(&mut other, 100, json!({"actions": [], "missed": 900.0, "now": 1_001.0}));
    poll(&mut other, 600, json!([{"role": "assistant", "text": "C'est fait.", "ts": 1_002.0}]), json!([]));
    other.handle(Input::Tick { now_ms: 3_000 });
    assert_eq!(other.resumable(), None);
}

/// An action that waited since a turn before this one is refused, never played; this turn is not lost for it.
#[test]
fn an_old_action_of_a_turn_before_is_refused_without_losing_this_one() {
    let mut nora = started_at(1_000.0);
    let outputs = poll_queue(&mut nora, 100, json!({"actions": [type_text("c0"), type_text("c1")], "queued_at": [800.0, 1_001.0], "now": 1_002.0}));
    let played = documents(&outputs);
    assert_eq!(played.len(), 1, "this turn's runs");
    assert!(refused_with(&outputs, "c0").contains("background"));
    poll(&mut nora, 600, json!([{"role": "assistant", "text": "C'est fait.", "ts": 1_003.0}]), json!([]));
    nora.handle(Input::DocumentResult { id: played[0].id, result: Ok(json!({"ok": true})) });
    nora.handle(Input::Tick { now_ms: 3_000 });
    assert_eq!(nora.resumable(), None);
}

/// A server that says no times: a page that drew no frame for a minute while Nora worked ran nothing meanwhile; the
/// first read after it is refused.
#[test]
fn without_the_server_s_times_a_gap_without_frames_says_it() {
    let mut nora = started();
    poll(&mut nora, 100, json!([]), json!([]));
    let outputs = poll_queue(&mut nora, 480_000, json!({"actions": [type_text("c1")]}));
    assert!(documents(&outputs).is_empty() && refused_with(&outputs, "c1").contains("background"));
    // A short gap is no freeze.
    let mut fine = started();
    poll(&mut fine, 100, json!([]), json!([]));
    let outputs = poll_queue(&mut fine, 30_000, json!({"actions": [type_text("c1")]}));
    assert_eq!(documents(&outputs).len(), 1);
}

/// A read that left before the page went quiet and whose answer waited in the page: its actions are as old as the gap,
/// whatever the server's times said when it answered.
#[test]
fn a_read_answered_while_the_page_ran_nothing_is_refused() {
    let mut nora = started_at(1_000.0);
    let outputs = nora.handle(Input::Tick { now_ms: 100 });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    // Eight minutes without a frame; the host ticks before it hands the answers over.
    nora.handle(Input::Tick { now_ms: 480_100 });
    nora.handle(ok(&messages, json!({"success": true, "messages": [], "cards": []})));
    let refused = nora.handle(ok(&queue, json!({"success": true, "actions": [type_text("c1")], "queued_at": [1_000.5], "now": 1_000.6})));
    assert!(documents(&refused).is_empty() && refused_with(&refused, "c1").contains("background"));
    // Back from a gap the pane saw itself, an action of the same turn is refused even when the server says it was
    // queued after it (24.10: the pane keeps the gap until the turn ends); the request can be resumed.
    let mut other = started_at(1_000.0);
    poll(&mut other, 100, json!([]), json!([]));
    let after = poll_queue(&mut other, 70_100, json!({"actions": [type_text("c2")], "queued_at": [1_070.0], "now": 1_070.2}));
    assert!(documents(&after).is_empty() && refused_with(&after, "c2").contains("background"));
    poll(&mut other, 73_200, json!([{"role": "assistant", "text": "Rien n'a été modifié.", "ts": 1_073.0}]), json!([]));
    other.handle(Input::Tick { now_ms: 75_000 });
    assert_eq!(other.resumable(), Some("Bonjour"));
}

/// A server that gives no queue times (an older one, a fake one): after a gap the pane saw itself, the first read can
/// come back empty and the late action come with the next. Every action of the turn is refused until it ends; the next
/// turn's run again (24.10, seen by NeoDoc's agent).
#[test]
fn without_the_server_s_times_every_action_after_a_gap_is_refused_until_the_turn_ends() {
    let mut nora = started();
    poll(&mut nora, 100, json!([]), json!([]));
    let first = poll_queue(&mut nora, 70_000, json!({"actions": []}));
    assert!(documents(&first).is_empty());
    let late = poll_queue(&mut nora, 73_100, json!({"actions": [type_text("c1")]}));
    assert!(documents(&late).is_empty() && refused_with(&late, "c1").contains("background"));
    let later = poll_queue(&mut nora, 76_200, json!({"actions": [{"action": "doc_observe", "callback_id": "c2"}]}));
    assert!(documents(&later).is_empty() && refused_with(&later, "c2").contains("background"), "a reading too");
    poll(&mut nora, 79_300, json!([{"role": "assistant", "text": "C'est fait."}]), json!([]));
    nora.handle(Input::Tick { now_ms: 81_000 });
    assert!(!nora.is_busy());
    assert_eq!(nora.resumable(), Some("Bonjour"));
    assert_eq!(nora.correction(1), Some(Correction::NothingDone), "« C'est fait » is not shown as it is");
    // The next turn starts without the gap.
    let sent = nora.handle(Input::Send("Bonjour".into()));
    nora.handle(ok(request_to(&sent, protocol::POST_MESSAGE), json!({"thread_id": "t1"})));
    let fresh = poll_queue(&mut nora, 81_100, json!({"actions": [type_text("c3")]}));
    assert_eq!(documents(&fresh).len(), 1);
}

#[test]
fn a_stop_or_a_new_conversation_forgets_the_lost_turn() {
    let lost = || {
        let mut nora = started_at(1_000.0);
        poll_queue(&mut nora, 100, json!({"actions": [], "missed": 1_002.0, "now": 1_490.0}));
        nora
    };
    let mut stopped = lost();
    stopped.handle(Input::Cancel);
    assert_eq!(stopped.resumable(), None);
    let mut reset = lost();
    reset.handle(Input::Reset);
    assert_eq!(reset.resumable(), None);
    // A Stop sent by a host while the session reads late, too.
    let mut late = started_at(1_000.0);
    given_up(&mut late);
    poll_queue(&mut late, 495_200, json!({"actions": [], "missed": 1_010.0, "now": 1_495.0}));
    assert!(late.resumable().is_some());
    late.handle(Input::Cancel);
    assert_eq!(late.resumable(), None);
}

/// A turn given up on and read late: a queue that expired unread there too offers to resume it.
#[test]
fn a_turn_read_late_that_lost_an_action_offers_to_resume_it() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    poll_queue(&mut nora, 495_200, json!({"actions": [], "missed": 1_010.0, "now": 1_495.0}));
    assert_eq!(nora.resumable(), Some("Bonjour"));
}

/// An action asked for a turn the pane gave up on never plays in the next one: not while the server has not yet said it
/// took the new message, nor once it has, when the action was queued before it (24.10.2026).
#[test]
fn an_action_of_a_turn_given_up_on_never_plays_in_the_next_one() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    let sent = nora.handle(Input::Send("Encore".into()));
    let post = request_to(&sent, protocol::POST_MESSAGE).clone();
    // The late reading's read is due: it brings the given-up turn's action while the new message is on its way.
    let early = poll_queue(&mut nora, 495_300, json!({"actions": [type_text("a1")], "queued_at": [1_490.0], "now": 1_495.0}));
    assert!(documents(&early).is_empty() && refused_with(&early, "a1").contains("another message"));
    nora.handle(ok(&post, json!({"thread_id": "t1", "sent_at": 1_500.0})));
    let older = poll_queue(&mut nora, 496_000, json!({"actions": [type_text("a2")], "queued_at": [1_499.0], "now": 1_500.5}));
    assert!(documents(&older).is_empty() && refused_with(&older, "a2").contains("another message"));
    // This turn's own action runs, and the turn is not lost for the others.
    let own = poll_queue(&mut nora, 499_000, json!({"actions": [type_text("b1")], "queued_at": [1_501.0], "now": 1_501.5}));
    assert_eq!(documents(&own).len(), 1);
    assert_eq!(nora.resumable(), None);
    // A server that keeps no time per action: the queue's last write says it.
    let mut old = started_at(1_000.0);
    given_up(&mut old);
    let sent = old.handle(Input::Send("Encore".into()));
    old.handle(ok(request_to(&sent, protocol::POST_MESSAGE), json!({"thread_id": "t1", "sent_at": 1_500.0})));
    let stale = poll_queue(&mut old, 495_300, json!({"actions": [type_text("a3")], "timestamp": 1_499.0}));
    assert!(documents(&stale).is_empty() && refused_with(&stale, "a3").contains("another message"));
}

/// The page went quiet right after the person sent a request, before the send's answer was handed to the session: back
/// from the gap, the host ticks first, then hands it over. Nothing is given up on the clock at that tick; the answer is
/// read, the conversation is read, and what waited too long is the turn's loss (24.10.2026, essai on osiris).
#[test]
fn back_from_a_gap_the_answers_that_waited_in_the_page_are_read_before_anything_is_given_up() {
    let mut nora = session();
    nora.handle(Input::Tick { now_ms: 100 });
    let sent = nora.handle(Input::Send("Écris 42 dans B2".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    // No tick between the send and the gap: the person sent, then left the tab.
    nora.handle(Input::Tick { now_ms: 480_200 });
    assert!(nora.is_busy() && nora.failure().is_none(), "the send's answer waits in the page");
    nora.handle(ok(&start, json!({"thread_id": "t1", "sent_at": 1_000.0})));
    poll_queue(&mut nora, 480_250, json!({"actions": [], "missed": 1_002.0, "now": 1_480.0}));
    poll(&mut nora, 483_300, json!([{"role": "assistant", "text": "Rien n'a été modifié.", "ts": 1_481.0}]), json!([]));
    nora.handle(Input::Tick { now_ms: 485_000 });
    assert!(!nora.is_busy() && nora.failure().is_none());
    assert_eq!(nora.resumable(), Some("Écris 42 dans B2"));
    // The same with a read on its way across the gap, and no time at all from the server: nothing given up at the
    // tick back, the read's answer is judged with the gap.
    let mut reading = started();
    let outputs = reading.handle(Input::Tick { now_ms: 100 });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    reading.handle(Input::Tick { now_ms: 900_000 });
    assert!(reading.is_busy() && reading.failure().is_none());
    reading.handle(ok(&messages, json!({"success": true, "messages": [], "cards": []})));
    let refused = reading.handle(ok(&queue, json!({"success": true, "actions": [type_text("c1")]})));
    assert!(documents(&refused).is_empty());
}

/// A session showing a Drive file, whose first message got its thread id `t1`.
fn started_on_file(id: &str) -> NoraSession {
    let mut document = DocumentInfo::named("lettre.docx");
    document.id = DocumentInfo::file_id(id);
    let mut nora = NoraSession::new(SessionConfig::default(), PageContext::editor(AppKind::Word, document));
    let sent = nora.handle(Input::Send("Bonjour".into()));
    let start = request_to(&sent, protocol::START_OR_RESUME_THREAD).clone();
    nora.handle(ok(&start, json!({"thread_id": "t1"})));
    nora
}

fn elsewhere(outputs: &[Output], callback: &str) -> bool {
    refusal_of(outputs, callback).as_str().is_some_and(|why| why.contains("addressed to another document"))
}

/// A person with two editors open (NeoDoc in one tab, NeoGrille in another) holds two conversations at once. On 24.10 a
/// NeoDoc turn read the workbook of the other tab: the server took the person's latest conversation. Whatever queue an
/// action is found in, a pane runs only what is addressed to its own conversation; a write addressed elsewhere is never
/// played in its document.
#[test]
fn an_action_addressed_to_another_pane_is_refused_and_never_runs() {
    let mut nora = started();
    let write = json!({"action": "doc_execute", "command": "format.bold", "params": {}, "callback_id": "w1", "for_thread": "t2"});
    let read = json!({"action": "doc_observe", "callback_id": "r1", "for_thread": "t2"});
    let outputs = poll(&mut nora, 100, json!([]), json!([write, read]));
    assert!(documents(&outputs).is_empty(), "nothing is played in this document");
    assert!(elsewhere(&outputs, "w1") && elsewhere(&outputs, "r1"));
    assert!(!nora.can_undo());
    // The same turn's action addressed to this pane runs.
    let mine = json!({"action": "doc_observe", "callback_id": "r2", "for_thread": "t1"});
    let outputs = poll(&mut nora, 1_000, json!([]), json!([mine]));
    assert_eq!(documents(&outputs).len(), 1);
}

#[test]
fn an_action_for_another_document_is_refused_and_one_for_this_document_runs() {
    let mut nora = started_on_file("20795bd915");
    let other = json!({"action": "doc_execute", "command": "format.bold", "params": {}, "callback_id": "w1",
        "for_thread": "t1", "for_document": "0b0844c613"});
    let outputs = poll(&mut nora, 100, json!([]), json!([other]));
    assert!(documents(&outputs).is_empty() && elsewhere(&outputs, "w1"));
    let mine = json!({"action": "doc_observe", "callback_id": "r1", "for_thread": "t1", "for_document": "20795bd915"});
    let outputs = poll(&mut nora, 1_000, json!([]), json!([mine]));
    assert_eq!(documents(&outputs).len(), 1);
}

/// A pane that shows no Drive file (a document of the computer, in NeoDesk) never runs an action addressed to a file.
#[test]
fn a_pane_without_a_file_refuses_an_action_addressed_to_one() {
    let mut nora = started();
    let read = json!({"action": "doc_observe", "callback_id": "r1", "for_thread": "t1", "for_document": "20795bd915"});
    let outputs = poll(&mut nora, 100, json!([]), json!([read]));
    assert!(documents(&outputs).is_empty() && elsewhere(&outputs, "r1"));
}

/// An action the server did not address (a server older than 24.10) runs as before; the server addresses each one now.
#[test]
fn an_action_without_an_address_runs_as_before() {
    let mut nora = started_on_file("20795bd915");
    let outputs = poll(&mut nora, 100, json!([]), json!([{"action": "doc_observe", "callback_id": "r1"}]));
    assert_eq!(documents(&outputs).len(), 1);
}

/// One read at `now`, the replies answered as `replies` and `cards`, with the server's word on the work still open
/// (`work`, or none), and an empty action queue.
fn poll_work(nora: &mut NoraSession, now: u64, replies: Value, cards: Value, work: Option<Value>) -> Vec<Output> {
    let outputs = nora.handle(Input::Tick { now_ms: now });
    let messages = request_to(&outputs, protocol::GET_MESSAGES).clone();
    let queue = request_to(&outputs, protocol::GET_ACTIONS).clone();
    let mut answer = json!({"success": true, "messages": replies, "cards": cards});
    if let Some(work) = work {
        answer["work"] = work;
    }
    let mut out = nora.handle(ok(&messages, answer));
    out.extend(nora.handle(ok(&queue, json!({"success": true, "actions": []}))));
    out
}

fn appointment_card() -> Value {
    json!({"token": "c-evt", "kind": "Event", "status": "Pending", "title": "Rendez-vous · Parler du plan", "conversation_id": "t1",
           "payload": {"summary": "« Parler du plan » le 28-10-2026 à 10:00", "confirm_label": "Ajouter à mon agenda"}})
}

/// The agent's answer may only hand over to a pole, whose card and result come later (NeoMail's appointment card, 24.10):
/// while the server says work goes on for the turn, the pane reads on, and the turn ends once it says the work is done.
#[test]
fn the_pane_reads_on_while_the_server_says_the_work_goes_on() {
    let mut nora = started_at(1_000.0);
    let answer = json!({"role": "assistant", "text": "Je prépare le rendez-vous.", "ts": 1_002.0});
    poll_work(&mut nora, 100, json!([answer]), json!([]), Some(json!({"open": true, "at": 1_002.0})));
    // Long past the settle time, the turn still reads.
    poll_work(&mut nora, 5_000, json!([answer]), json!([]), Some(json!({"open": true, "at": 1_002.0})));
    assert!(nora.is_busy());
    // The card comes after the answer, with the work done.
    poll_work(&mut nora, 20_000, json!([answer]), json!([appointment_card()]), Some(json!({"open": false, "at": 1_020.0})));
    assert_eq!(nora.cards().len(), 1, "the card made after the answer is shown");
    nora.handle(Input::Tick { now_ms: 21_600 });
    assert!(!nora.is_busy() && nora.last_error().is_none());
}

/// A server that says nothing of the work ends the turn with its answer, as before.
#[test]
fn without_the_server_s_word_the_turn_ends_with_its_answer() {
    let mut nora = started_at(1_000.0);
    let answer = json!({"role": "assistant", "text": "Voici la réponse.", "ts": 1_002.0});
    poll_work(&mut nora, 100, json!([answer]), json!([]), None);
    nora.handle(Input::Tick { now_ms: 1_700 });
    assert!(!nora.is_busy());
}

/// Work the server said open for a delivery made before this message is the turn before's.
#[test]
fn work_said_open_before_the_message_is_not_this_turn_s() {
    let mut nora = started_at(1_000.0);
    let answer = json!({"role": "assistant", "text": "Voici la réponse.", "ts": 1_002.0});
    poll_work(&mut nora, 100, json!([answer]), json!([]), Some(json!({"open": true, "at": 990.0})));
    nora.handle(Input::Tick { now_ms: 1_700 });
    assert!(!nora.is_busy());
}

/// Work announced that never brings anything ends the turn quietly after the silence: the answer is in, no failure.
#[test]
fn work_that_never_comes_ends_the_turn_quietly() {
    let mut nora = started_at(1_000.0);
    let answer = json!({"role": "assistant", "text": "Le pôle Support prépare le rendez-vous.", "ts": 1_002.0});
    let open = Some(json!({"open": true, "at": 1_002.0}));
    poll_work(&mut nora, 100, json!([answer]), json!([]), open.clone());
    poll_work(&mut nora, 479_000, json!([answer]), json!([]), open.clone());
    poll_work(&mut nora, 482_000, json!([answer]), json!([]), open);
    nora.handle(Input::Tick { now_ms: 482_500 });
    assert!(!nora.is_busy(), "ended");
    assert!(nora.last_error().is_none(), "quietly: the answer is in");
    assert!(nora.resumable().is_none());
}

/// Where the reply `text` is in the transcript.
fn index_of(nora: &NoraSession, text: &str) -> usize {
    nora.transcript().iter().position(|message| message.text == text).unwrap_or_else(|| panic!("no « {text} »"))
}

/// The NeoMail case of 24.10: a tab frozen eight minutes, the write refused when it came back, and Nora's reply, written
/// before she knew, says the draft is ready. The person never reads that it was done.
#[test]
fn a_reply_that_says_done_after_its_write_was_refused_is_corrected() {
    let mut nora = started_at(1_000.0);
    let draft = json!({"action": "doc_execute", "command": "draft.new", "params": {}, "callback_id": "w1"});
    let refused = poll_queue(&mut nora, 100, json!({"actions": [draft], "queued_at": [1_002.0], "now": 1_482.0}));
    assert!(documents(&refused).is_empty());
    let ack = "Je m'en occupe.";
    let done = "La réponse est prête dans votre brouillon.";
    poll(&mut nora, 600, json!([{"role": "assistant", "text": ack, "ts": 1_483.0}, {"role": "assistant", "text": done, "ts": 1_484.0}]), json!([]));
    nora.handle(Input::Tick { now_ms: 3_000 });
    assert_eq!(nora.correction(index_of(&nora, done)), Some(Correction::NothingDone));
    assert_eq!(nora.correction(index_of(&nora, ack)), None, "an acknowledgement claims nothing");
    assert_eq!(nora.correction(0), None, "nor does the person's message");
}

#[test]
fn a_turn_that_did_part_of_it_is_said_partly_done() {
    let mut nora = started_at(1_000.0);
    let first = poll_queue(&mut nora, 100, json!({"actions": [type_text("c1")], "queued_at": [1_001.0], "now": 1_001.5}));
    let played = documents(&first);
    assert_eq!(played.len(), 1);
    nora.handle(Input::DocumentResult { id: played[0].id, result: Ok(json!({"ok": true})) });
    let late = poll_queue(&mut nora, 600, json!({"actions": [type_text("c2")], "queued_at": [1_002.0], "now": 1_490.0}));
    assert!(documents(&late).is_empty());
    let done = "J'ai écrit 42 et 43.";
    poll(&mut nora, 1_100, json!([{"role": "assistant", "text": done, "ts": 1_491.0}]), json!([]));
    assert_eq!(nora.correction(index_of(&nora, done)), Some(Correction::PartlyDone));
}

/// A reading refused claims nothing done; a turn that went well needs no word; a queue that expired unread may have
/// held a change.
#[test]
fn only_what_could_have_been_done_is_corrected() {
    let mut reading = started_at(1_000.0);
    poll_queue(&mut reading, 100, json!({"actions": [{"action": "doc_observe", "callback_id": "r1"}], "queued_at": [1_002.0], "now": 1_482.0}));
    poll(&mut reading, 600, json!([{"role": "assistant", "text": "Je n'ai pas pu lire le document.", "ts": 1_483.0}]), json!([]));
    assert_eq!(reading.correction(1), None);
    let mut fine = started_at(1_000.0);
    let played = poll_queue(&mut fine, 100, json!({"actions": [type_text("c1")], "queued_at": [1_001.0], "now": 1_001.5}));
    fine.handle(Input::DocumentResult { id: documents(&played)[0].id, result: Ok(json!({"ok": true})) });
    poll(&mut fine, 600, json!([{"role": "assistant", "text": "C'est fait.", "ts": 1_002.0}]), json!([]));
    assert_eq!(fine.correction(1), None);
    let mut missed = started_at(1_000.0);
    poll_queue(&mut missed, 100, json!({"actions": [], "missed": 1_002.0, "now": 1_490.0}));
    poll(&mut missed, 600, json!([{"role": "assistant", "text": "C'est fait.", "ts": 1_491.0}]), json!([]));
    assert_eq!(missed.correction(1), Some(Correction::NothingDone));
}

/// The pane gave the turn up and told the person; Nora's late answer says it was done, and her write was refused late.
#[test]
fn a_late_answer_whose_write_was_refused_is_corrected() {
    let mut nora = started_at(1_000.0);
    given_up(&mut nora);
    let late = poll_queue(&mut nora, 495_200, json!({"actions": [type_text("c1")], "queued_at": [1_490.0], "now": 1_495.0}));
    assert!(documents(&late).is_empty());
    poll(&mut nora, 510_200, json!([{"role": "assistant", "text": "C'est fait.", "ts": 1_496.0}]), json!([]));
    assert_eq!(nora.correction(1), Some(Correction::NothingDone));
}

/// A long action of Nora's (an OCR, a big file made smaller, a long calculation in a workbook) holds the page 90 s: no
/// frame meanwhile, and the action Nora queued while it ran waited that long. The editor finished it, so the page was
/// alive: nothing is refused, nothing is offered to resume, the turn goes on (24.10, NeoPDF's review). The host may
/// tick before or after it hands the result over.
#[test]
fn a_long_action_is_no_freeze() {
    for result_first in [true, false] {
        let mut nora = started_at(1_000.0);
        let ocr = json!({"action": "doc_execute", "command": "ocr.run", "params": {}, "callback_id": "c1"});
        let started = poll_queue(&mut nora, 100, json!({"actions": [ocr], "queued_at": [1_001.0], "now": 1_001.5}));
        let ocr_id = documents(&started)[0].id;
        // Ninety seconds in the editor's action, no frame meanwhile; Nora queues the next one at once.
        let next = json!({"action": "doc_type_text", "text": "42", "callback_id": "c2"});
        let queue = json!({"success": true, "actions": [next], "queued_at": [1_002.0], "now": 1_092.0});
        let outputs = if result_first {
            nora.handle(Input::DocumentResult { id: ocr_id, result: Ok(json!({"ok": true})) });
            poll_queue(&mut nora, 90_100, queue)
        } else {
            let read = nora.handle(Input::Tick { now_ms: 90_100 });
            nora.handle(Input::DocumentResult { id: ocr_id, result: Ok(json!({"ok": true})) });
            nora.handle(ok(request_to(&read, protocol::GET_MESSAGES), json!({"success": true, "messages": [], "cards": []})));
            nora.handle(ok(request_to(&read, protocol::GET_ACTIONS), queue))
        };
        assert_eq!(documents(&outputs).len(), 1, "the next action runs (result first: {result_first})");
        poll(&mut nora, 93_200, json!([{"role": "assistant", "text": "Le texte est reconnu et 42 est écrit.", "ts": 1_093.0}]), json!([]));
        let played = documents(&outputs)[0].id;
        nora.handle(Input::DocumentResult { id: played, result: Ok(json!({"ok": true})) });
        nora.handle(Input::Tick { now_ms: 96_000 });
        assert!(!nora.is_busy() && nora.last_error().is_none());
        assert_eq!(nora.resumable(), None, "nothing to resume");
        assert_eq!(nora.transcript()[1].text, "Le texte est reconnu et 42 est écrit.");
        assert_eq!(nora.correction(1), None, "the reply is shown as it is");
    }
    // The same silence with no action at work is a freeze, as before.
    let mut idle = started_at(1_000.0);
    poll(&mut idle, 100, json!([]), json!([]));
    let late = poll_queue(&mut idle, 90_100, json!({"actions": [type_text("c3")], "queued_at": [1_002.0], "now": 1_092.0}));
    assert!(documents(&late).is_empty() && refused_with(&late, "c3").contains("background"));
}
