// //// Neoffice — added file (no upstream equivalent)
//! Replays a measured editor turn through the real session code, to count what the page's reads cost.
//! Measured on osiris, 05.11.2026 (NeoDoc, « améliore le texte », three sentences): the server queued its first action 26.5 s
//! after the message (cold prefix), then each next action 5.1, 5.9 s after the answer to the one before, and the final reply
//! 3.3 s after the answer to the last. The page draws a frame every 20 ms; a call takes 40 ms to answer; an action runs in 0.2 s.
use craft_nora::{AppKind, DocumentInfo, HttpRequest, HttpResponse, Input, NoraSession, Output, PageContext, SessionConfig, protocol};
use serde_json::{Value, json};

fn answer(request: &HttpRequest, message: Value) -> Input {
    Input::Http(HttpResponse { id: request.id, status: 200, body: json!({ "message": message }).to_string() })
}

#[test]
fn replay_a_measured_turn() {
    for (label, first, gaps, reply_gap) in
        [("cold turn (measured)", 26_500u64, [5_100u64, 5_900], 3_300u64), ("warm turn (measured the same day)", 3_000, [3_400, 3_800], 1_900)]
    {
        let ctx = PageContext::editor(AppKind::Word, DocumentInfo::named("devis.docx"));
        let mut nora = NoraSession::new(SessionConfig::default(), ctx);
        let mut now: u64 = 400_000; // the page has been open for a while
        nora.handle(Input::Tick { now_ms: now });
        let start = now;
        let mut pending: Vec<(u64, Input)> = Vec::new(); // (time, input to feed)
        let mut outputs = nora.handle(Input::Send("améliore le texte".into()));
        // The server side of the scenario.
        let mut queued: Vec<u64> = Vec::new(); // when each action was put in the queue
        let mut taken = 0usize; // actions already read
        let mut next_queue_at = Some(start + first);
        let mut reply_at: Option<u64> = None;
        let mut reads: Vec<(u64, u64)> = Vec::new(); // (queued, read)
        let mut running: Vec<(u64, u64, u64)> = Vec::new(); // (done_at, call id, callback)
        let mut ended = None;
        let mut sent_thread = false;
        for step in 0..60_000u64 {
            now = start + step * 20;
            // Inputs that are due.
            let due: Vec<Input> = {
                let mut keep = Vec::new();
                let mut out = Vec::new();
                for (t, input) in pending.drain(..) {
                    if t <= now { out.push(input) } else { keep.push((t, input)) }
                }
                pending = keep;
                out
            };
            for input in due {
                outputs.extend(nora.handle(input));
            }
            // An action finished running in the page: the server hears of it, and plans its next move.
            let finished: Vec<_> = running.iter().filter(|(done, ..)| *done <= now).cloned().collect();
            running.retain(|(done, ..)| *done > now);
            for (_, id, _) in finished {
                outputs.extend(nora.handle(Input::DocumentResult { id, result: Ok(json!({"ok": true})) }));
                if queued.len() < 3 {
                    next_queue_at = Some(now + gaps[queued.len() - 1]);
                } else {
                    reply_at = Some(now + reply_gap);
                }
            }
            if let Some(at) = next_queue_at
                && now >= at
                && queued.len() < 3
            {
                queued.push(now);
                next_queue_at = None;
            }
            outputs.extend(nora.handle(Input::Tick { now_ms: now }));
            for output in std::mem::take(&mut outputs) {
                match output {
                    Output::Http(request) => {
                        let done = now + 40;
                        let input = match request.path.as_str() {
                            p if p == protocol::START_OR_RESUME_THREAD || p == protocol::POST_MESSAGE => {
                                sent_thread = true;
                                answer(&request, json!({"thread_id": "t1", "sent_at": 1000.0}))
                            }
                            p if p == protocol::GET_ACTIONS => {
                                let mut list = Vec::new();
                                while taken < queued.len() {
                                    list.push(json!({"action": "doc_observe", "callback_id": format!("c{taken}")}));
                                    reads.push((queued[taken], done));
                                    taken += 1;
                                }
                                answer(&request, json!({"success": true, "actions": list}))
                            }
                            p if p == protocol::GET_MESSAGES => {
                                let messages = match reply_at {
                                    Some(r) if done >= r => vec![json!({"role": "assistant", "text": "Done.", "ts": 1500.0})],
                                    _ => vec![],
                                };
                                answer(&request, json!({"success": true, "messages": messages, "cards": []}))
                            }
                            _ => answer(&request, json!({})),
                        };
                        pending.push((done, input));
                    }
                    Output::Document(call) => running.push((now + 200, call.id, 0)),
                    _ => {}
                }
            }
            if sent_thread && ended.is_none() && !nora.is_busy() {
                ended = Some(now);
                break;
            }
        }
        let lags: Vec<u64> = reads.iter().map(|(q, r)| r - q).collect();
        let end = ended.map_or(u64::MAX, |e| e - start);
        let reply = reply_at.map_or(0, |r| r - start);
        println!(
            "REPLAY {label}: turn ended at {end} ms; the page read each action {lags:?} ms after the server queued it (sum {} ms); the reply was available at {reply} ms",
            lags.iter().sum::<u64>()
        );
        // After the first action the page reads again soon: no later action waits more than a second for a read, and the turn
        // ends within three seconds of its reply being there (the session settles for 1.5 s).
        assert!(lags.iter().skip(1).all(|lag| *lag <= 1_000), "{label}: {lags:?}");
        assert!(end <= reply + 3_000, "{label}: the turn ended at {end}, the reply was there at {reply}");
    }
}
