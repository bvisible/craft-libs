// //// Neoffice — added file (no upstream equivalent): the conversation state machine of the Nora client core.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::context::PageContext;
use crate::error::NoraError;
use crate::http::{HttpRequest, HttpResponse, RequestId, frappe_message};
use crate::policy::{CommandPolicy, Decision};
use crate::protocol::{
    self, Card, ChatMessage, DocAction, DocActionKind, RemoteAction, Role, card_answer_body, card_choice_body, card_refusal_body, observation_body,
    observation_error, observation_ok, parse_actions, parse_messages, post_message_body, queued_at_of, sent_at_of, start_thread_body, thread_body,
    thread_id_of,
};
use crate::reply::instance_path;

/// What Nora is told about an action that will not run because the user stopped it.
const STOPPED: &str = "the user stopped Nora";

/// What Nora is told about an action that comes for a turn the pane gave up on (it told the person she did not answer in
/// time): nothing is done in the document the person may no longer watch, or even have open (a pane closed since).
const LATE: &str = "the pane had stopped waiting for this turn and told the person you did not answer in time: nothing was \
done in the document. Say so in your reply; they will ask again";

/// How long past the silence a read the host never answers is waited for, before the turn is given up on anyway: more
/// than the hosts' own limit on a call (180 s in the Drive's page, 200 s in NeoMail).
const UNANSWERED_READ_MS: u64 = 240_000;

/// How a session behaves. The defaults suit an editor talking to a Neoffice instance.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// What Nora may play in the document.
    pub policy: CommandPolicy,
    /// Time between two reads of the replies and of the action queue in the first three seconds of
    /// a turn: a greeting or an acknowledgement is there within a second.
    pub poll_fast_ms: u64,
    /// The same up to fifteen seconds into the turn.
    pub poll_mid_ms: u64,
    /// The same after that. A business desk answers 10 to 70 seconds after its acknowledgement.
    pub poll_slow_ms: u64,
    /// How long without a reply, an action or a new card before Nora is given up on. Eight minutes, as the
    /// desk's chat waits: a worker retried after a crash answered after three and a half. Time alone never ends a
    /// turn whose conversation can be read: a read made after that long must bring nothing (a clock that jumps, a
    /// tab in the background that draws no frame or a machine too busy to draw, would otherwise end a turn whose
    /// answer waits on the server).
    pub max_silence_ms: u64,
    /// How long after the start of a turn given up on (silence, or a message whose send got no answer) the session
    /// still reads the conversation, for an answer that comes late: its replies and cards are shown as they come, and
    /// the failure is taken back; an action is refused (the person was told to ask again, and may have closed the
    /// pane). Thirty minutes, as long as Nora's gateway lets a turn run without news.
    pub late_read_ms: u64,
    /// Time between two reads while the session reads late.
    pub poll_late_ms: u64,
    /// How long the turn stays open after the final reply, for the actions queued with it.
    pub settle_ms: u64,
    /// Lowercase phrases that mark an acknowledgement rather than an answer. Defaults to
    /// [`protocol::ACK_ANCHORS`]; a host that has the server's live list passes it here.
    pub ack_anchors: Vec<String>,
    /// The origin the page that frames the editor shares with the person's instance (the Drive's), when it does: an
    /// address of it in a reply is a page of the instance too ([`crate::reply::instance_path`]). `None` where the
    /// page is not the instance's (Neodesk): only a path is.
    pub instance_origin: Option<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            policy: CommandPolicy::default(),
            poll_fast_ms: 400,
            poll_mid_ms: 1_000,
            poll_slow_ms: 3_000,
            max_silence_ms: 480_000,
            late_read_ms: 1_800_000,
            poll_late_ms: 15_000,
            settle_ms: 1_500,
            ack_anchors: protocol::ACK_ANCHORS.iter().map(|anchor| anchor.to_string()).collect(),
            instance_origin: None,
        }
    }
}

/// What the host and the user feed into a session.
#[derive(Debug, Clone)]
pub enum Input {
    /// The user sent a message.
    Send(String),
    /// The open document changed (another selection, another page count…).
    SetContext(PageContext),
    /// Time passed. The host sends one regularly (every frame is fine); it is the session's only
    /// clock.
    Tick { now_ms: u64 },
    /// The answer to an [`Output::Http`].
    Http(HttpResponse),
    /// The result of an [`Output::Document`] the editor ran. An `Ok` object with `"changed": false` says the action
    /// changed nothing (a command that only reads): it leaves nothing to undo.
    DocumentResult { id: u64, result: Result<Value, String> },
    /// The user clicked yes or no on a [`Confirmation`].
    Confirm { id: u64, accepted: bool },
    /// The user stopped Nora (the Stop button). Nothing to stop does nothing; see
    /// [`NoraSession::stopped`].
    Cancel,
    /// The user clicked Undo.
    UndoLast,
    /// The user started a new conversation.
    Reset,
    /// The user clicked a link of a reply or a card's Open: a page of the instance is opened ([`Output::Open`]),
    /// anything else is not.
    OpenLink(String),
    /// The user answered a card that waits for them ([`Card::awaits_the_person`]): yes or no.
    AnswerCard { token: String, approve: bool },
    /// The user picked one of the choices of a card that waits for them (a mailbox to write a draft from), by its id.
    ChooseOnCard { token: String, choice: String },
}

/// What a session asks of the host.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Make this call and feed the answer back as [`Input::Http`].
    Http(HttpRequest),
    /// Run this in the open document and feed the result back as [`Input::DocumentResult`].
    Document(DocumentCall),
    /// The transcript, the busy state, the error or the confirmations changed: repaint.
    Changed,
    /// Open this page of the person's instance (a path: `/drive/f/…`), on the person's click: the host knows the
    /// instance (the Drive's page is on it, Neodesk signs in to it) and opens it its own way.
    Open(String),
}

/// An action for the editor to run through its own control channel.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentCall {
    pub id: u64,
    pub kind: DocActionKind,
}

/// A card of the conversation, as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownCard {
    pub card: Card,
    /// Where it goes in the transcript: after that many messages. `None` while the turn that made it runs: it is
    /// shown at the end, under the replies of the turn, and stays there once the turn ends.
    pub after: Option<usize>,
    /// The person's answer to it is on its way to the server.
    pub answering: bool,
}

/// An action waiting for the user's yes or no.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirmation {
    pub id: u64,
    /// An English sentence for the panel. A host that shows another language describes `kind` itself.
    pub summary: String,
    /// What Nora asked for.
    pub kind: DocActionKind,
}

enum Pending {
    Send,
    Messages,
    Actions,
    /// A read of the action queue made before the user stopped: what it took out of the queue is
    /// refused, not run.
    StoppedActions,
    Observation,
    /// The person's answer to the card of this token.
    CardAnswer(String),
}

/// How the last message's turn ended.
enum TurnEnd {
    Answered,
    Failed(NoraError),
    Stopped,
}

struct Running {
    callback_id: Option<String>,
    modifies: bool,
    summary: String,
    /// The user's Undo of Nora's last change.
    undo: bool,
}

struct Waiting {
    id: u64,
    action: DocAction,
}

/// One conversation between a user and Nora, inside one open document.
pub struct NoraSession {
    cfg: SessionConfig,
    context: PageContext,
    thread_id: Option<String>,
    transcript: Vec<ChatMessage>,
    /// Replies counted so far: the fallback when the server gives no send time.
    known_replies: usize,
    /// The server time of the last send. A reply stamped after it is new.
    sent_at: Option<f64>,
    /// The replies already taken, by time and start, so a poll never adds one twice.
    seen: HashSet<String>,
    busy: bool,
    last_error: Option<String>,
    /// The failure behind `last_error`, when it is one between the editor and Nora (not the editor's own words).
    last_failure: Option<NoraError>,
    now_ms: u64,
    last_activity_ms: u64,
    turn_started_ms: u64,
    /// When the first reply that is an answer, not an acknowledgement, came in.
    final_seen_ms: Option<u64>,
    next_poll_ms: Option<u64>,
    next_id: u64,
    inflight: HashMap<RequestId, Pending>,
    running: HashMap<u64, Running>,
    waiting: Vec<Waiting>,
    changes: Vec<String>,
    /// How the last turn ended: `None` while it runs and before the first message.
    outcome: Option<TurnEnd>,
    /// Set by a Stop until the conversation is reset. The chat has no stop on the server: Nora's agent
    /// may go on with the stopped message and queue actions for it, in the queue the next messages
    /// read. A queue stamped before the current message holds that work only, and is refused.
    refuse_older_actions: bool,
    /// The cards of this conversation, in the order they came.
    cards: Vec<ShownCard>,
    /// When the read of the replies in flight was sent.
    read_sent_ms: Option<u64>,
    /// When the last read of the replies that came back with nothing new (or failed) was sent: made after the
    /// deadline, it is what says Nora is silent ([`SessionConfig::max_silence_ms`]).
    quiet_read_ms: Option<u64>,
    /// Until when the session reads late, for the turn it gave up on ([`SessionConfig::late_read_ms`]).
    late_until_ms: Option<u64>,
    /// The server time of the send of a turn given up on while it was read late, once another message is sent: that
    /// turn's replies still to come are new too, and they are not the answer to the message sent after it.
    late_since: Option<f64>,
    /// Whether the server took the last message (its send was answered): `sent_at` is then that message's.
    send_answered: bool,
    /// Where the last message is in the transcript.
    turn_index: usize,
}

impl NoraSession {
    pub fn new(cfg: SessionConfig, context: PageContext) -> Self {
        NoraSession {
            cfg,
            context,
            thread_id: None,
            transcript: Vec::new(),
            known_replies: 0,
            sent_at: None,
            seen: HashSet::new(),
            busy: false,
            last_error: None,
            last_failure: None,
            now_ms: 0,
            last_activity_ms: 0,
            turn_started_ms: 0,
            final_seen_ms: None,
            next_poll_ms: None,
            next_id: 0,
            inflight: HashMap::new(),
            running: HashMap::new(),
            waiting: Vec::new(),
            changes: Vec::new(),
            outcome: None,
            refuse_older_actions: false,
            cards: Vec::new(),
            read_sent_ms: None,
            quiet_read_ms: None,
            late_until_ms: None,
            late_since: None,
            send_answered: false,
            turn_index: 0,
        }
    }

    /// Feeds one input and returns what the host has to do about it.
    pub fn handle(&mut self, input: Input) -> Vec<Output> {
        let mut out = Vec::new();
        match input {
            Input::Send(text) => self.on_send(text, &mut out),
            Input::SetContext(context) => self.context = context,
            Input::Tick { now_ms } => self.on_tick(now_ms, &mut out),
            Input::Http(response) => self.on_http(response, &mut out),
            Input::DocumentResult { id, result } => self.on_document_result(id, result, &mut out),
            Input::Confirm { id, accepted } => self.on_confirm(id, accepted, &mut out),
            Input::Cancel => self.on_cancel(&mut out),
            Input::UndoLast => self.on_undo(&mut out),
            Input::Reset => self.on_reset(&mut out),
            Input::OpenLink(url) => self.on_open(&url, &mut out),
            Input::AnswerCard { token, approve } => self.on_answer_card(&token, approve, &mut out),
            Input::ChooseOnCard { token, choice } => self.on_choose_on_card(&token, &choice, &mut out),
        }
        out
    }

    /// The cards of this conversation (a document made, an e-mail draft, a proposal to confirm), in the order they
    /// came, each with its place in the transcript.
    pub fn cards(&self) -> &[ShownCard] {
        &self.cards
    }

    /// The path of the instance's page a link leads to, when the panel may open it ([`crate::reply::instance_path`]
    /// with this session's instance origin).
    pub fn page_of(&self, url: &str) -> Option<String> {
        instance_path(url, self.cfg.instance_origin.as_deref())
    }

    /// The conversation so far, oldest first.
    pub fn transcript(&self) -> &[ChatMessage] {
        &self.transcript
    }

    /// Whether Nora is working on the last message.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Whether the session still reads the conversation for the turn it gave up on, for an answer that comes late
    /// ([`SessionConfig::late_read_ms`]): its replies and cards are shown as they come, and the failure is taken back.
    /// The session is not busy meanwhile: the person may write again.
    pub fn reads_late(&self) -> bool {
        self.late_until_ms.is_some()
    }

    /// The last failure, to show in the panel. A successful read clears it.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// The failure behind [`Self::last_error`] when it came from the connection to Nora (no answer, a refusal of
    /// the server, a timeout, an answer out of protocol), for a host that says it in its own words; `None` when the
    /// error shown is the editor's own sentence (a refused action) or there is none.
    pub fn last_failure(&self) -> Option<&NoraError> {
        self.last_failure.as_ref()
    }

    /// Whether the user stopped the last message. The next message clears it.
    pub fn stopped(&self) -> bool {
        matches!(self.outcome, Some(TurnEnd::Stopped))
    }

    /// Why the last message failed, when it did: a host checks [`NoraError::is_auth`] to offer to
    /// sign in again. The next message clears it.
    pub fn failure(&self) -> Option<&NoraError> {
        match &self.outcome {
            Some(TurnEnd::Failed(error)) => Some(error),
            _ => None,
        }
    }

    /// The conversation id, once Nora has given one.
    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    /// The actions waiting for the user's yes or no.
    pub fn confirmations(&self) -> Vec<Confirmation> {
        self.waiting.iter().map(|w| Confirmation { id: w.id, summary: w.action.kind.summary(), kind: w.action.kind.clone() }).collect()
    }

    /// Whether a change Nora made can be undone.
    pub fn can_undo(&self) -> bool {
        !self.changes.is_empty()
    }

    /// The time at which the host should send the next [`Input::Tick`], when the session is
    /// waiting for something (an answer, or a late one: [`Self::reads_late`]); `None` when it is idle and needs
    /// nothing.
    pub fn next_wake_ms(&self) -> Option<u64> {
        if self.busy {
            Some(self.next_poll_ms.unwrap_or(self.now_ms).max(self.now_ms))
        } else {
            self.late_until_ms.map(|until| {
                let due = self.next_poll_ms.unwrap_or(until);
                // A late read on its way: the host wakes the window when it answers; until then, a look now and then.
                let due = if self.polling() && due <= self.now_ms { self.now_ms.saturating_add(self.cfg.poll_late_ms) } else { due };
                due.min(until).max(self.now_ms)
            })
        }
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn request(&mut self, path: &str, body: Value, kind: Pending) -> Output {
        let id = self.new_id();
        self.inflight.insert(id, kind);
        Output::Http(HttpRequest { id, path: path.to_string(), body })
    }

    fn polling(&self) -> bool {
        self.inflight.values().any(|pending| matches!(pending, Pending::Messages | Pending::Actions))
    }

    fn on_send(&mut self, text: String, out: &mut Vec<Output>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        // A message sent while the turn before is read late: that turn's replies still to come are shown too.
        self.late_since = if self.late_until_ms.take().is_some() { self.sent_at } else { None };
        self.send_answered = false;
        self.quiet_read_ms = None;
        self.turn_index = self.transcript.len();
        self.transcript.push(ChatMessage::user(text.clone()));
        self.busy = true;
        self.outcome = None;
        self.last_error = None;
        self.last_failure = None;
        self.last_activity_ms = self.now_ms;
        self.turn_started_ms = self.now_ms;
        self.final_seen_ms = None;
        let request = match self.thread_id.clone() {
            Some(thread) => self.request(protocol::POST_MESSAGE, post_message_body(&thread, &text, &self.context), Pending::Send),
            None => self.request(protocol::START_OR_RESUME_THREAD, start_thread_body(&text, &self.context), Pending::Send),
        };
        out.push(request);
        out.push(Output::Changed);
    }

    fn on_tick(&mut self, now_ms: u64, out: &mut Vec<Output>) {
        self.now_ms = now_ms;
        if !self.busy {
            self.read_late(now_ms, out);
            return;
        }
        let nothing_running = self.running.is_empty() && self.waiting.is_empty();
        let nothing_pending = nothing_running
            && self.inflight.values().all(|pending| matches!(pending, Pending::Observation | Pending::StoppedActions | Pending::CardAnswer(_)));
        // Done first, so that a finished turn ends without one more poll. The final reply is in and the
        // actions queued with it have had their moment...
        let settled = self.final_seen_ms.is_some_and(|seen| now_ms.saturating_sub(seen) >= self.cfg.settle_ms);
        if nothing_pending && settled {
            self.finish(out);
            return;
        }
        // ...or nothing came for too long (an acknowledgement alone is not an answer), as a read made since says. A
        // call the host never answered holds this back a little only: an action running or waiting for the user does.
        if nothing_running && self.silent(now_ms) {
            self.give_up(NoraError::Timeout, out);
            return;
        }
        if let Some(thread) = self.thread_id.clone()
            && self.next_poll_ms.is_some_and(|due| now_ms >= due)
            && !self.polling()
        {
            self.read(&thread, out);
            self.next_poll_ms = Some(now_ms + self.poll_delay(now_ms));
        }
    }

    /// One read of the replies and of the action queue.
    fn read(&mut self, thread: &str, out: &mut Vec<Output>) {
        let messages = self.request(protocol::GET_MESSAGES, thread_body(thread), Pending::Messages);
        let actions = self.request(protocol::GET_ACTIONS, thread_body(thread), Pending::Actions);
        out.push(messages);
        out.push(actions);
        self.read_sent_ms = Some(self.now_ms);
    }

    /// Whether Nora has said nothing for [`SessionConfig::max_silence_ms`]. Time alone does not say it while the
    /// conversation can be read: a clock that jumps (a tab in the background draws no frame, nor a machine too busy
    /// to draw, and the next frame comes minutes later) would end a turn whose answer waits on the server. A read
    /// sent after the deadline must have come back with nothing; a read the host never answers holds this back by
    /// [`UNANSWERED_READ_MS`] at most.
    fn silent(&self, now_ms: u64) -> bool {
        let deadline = self.last_activity_ms.saturating_add(self.cfg.max_silence_ms);
        if now_ms < deadline {
            return false;
        }
        // With no conversation to read yet, or a message whose send is unanswered (no read goes out meanwhile), time says it.
        let sending = self.inflight.values().any(|pending| matches!(pending, Pending::Send));
        if self.thread_id.is_none() || sending || self.quiet_read_ms.is_some_and(|sent| sent >= deadline) {
            return true;
        }
        self.polling() && self.read_sent_ms.is_some_and(|sent| now_ms >= sent.max(deadline).saturating_add(UNANSWERED_READ_MS))
    }

    /// The turn is given up on: the failure is shown, and while the server may still finish the turn (it did, with a
    /// machine too busy to draw), the conversation is read on, slowly, for an answer that comes late.
    fn give_up(&mut self, error: NoraError, out: &mut Vec<Output>) {
        self.fail(error, out);
        let until = self.turn_started_ms.saturating_add(self.cfg.late_read_ms);
        if self.thread_id.is_some() && self.now_ms < until {
            self.late_until_ms = Some(until);
            self.next_poll_ms = Some(self.now_ms.saturating_add(self.cfg.poll_late_ms));
        }
    }

    /// A read of the turn given up on, when one is due, until [`SessionConfig::late_read_ms`] has passed.
    fn read_late(&mut self, now_ms: u64, out: &mut Vec<Output>) {
        let Some(until) = self.late_until_ms else {
            return;
        };
        if now_ms >= until {
            self.late_until_ms = None;
            self.next_poll_ms = None;
            return;
        }
        if let Some(thread) = self.thread_id.clone()
            && self.next_poll_ms.is_some_and(|due| now_ms >= due)
            && !self.polling()
        {
            self.read(&thread, out);
            self.next_poll_ms = Some(now_ms.saturating_add(self.cfg.poll_late_ms));
        }
    }

    /// A reply or a new card came for the turn given up on: it was not over. Its answer or a new card takes the failure
    /// shown back (the turn's outcome stays a failure: the pane did give up); an acknowledgement alone leaves it, for
    /// nothing else may come. Once its answer is in, one more read for what comes with it, and the late reading ends.
    fn late_news(&mut self, answered: bool, new_card: bool) {
        if answered || new_card {
            self.last_error = None;
            self.last_failure = None;
        }
        if answered {
            let last = self.now_ms.saturating_add(self.cfg.settle_ms);
            self.next_poll_ms = Some(last);
            self.late_until_ms = self.late_until_ms.map(|until| until.min(last.saturating_add(self.cfg.poll_late_ms)));
        }
    }

    /// Time to the next read: often at first, then less, as the desk's chat does.
    fn poll_delay(&self, now_ms: u64) -> u64 {
        match now_ms.saturating_sub(self.turn_started_ms) {
            0..3_000 => self.cfg.poll_fast_ms,
            3_000..15_000 => self.cfg.poll_mid_ms,
            _ => self.cfg.poll_slow_ms,
        }
    }

    fn finish(&mut self, out: &mut Vec<Output>) {
        self.busy = false;
        self.next_poll_ms = None;
        self.outcome = Some(TurnEnd::Answered);
        self.anchor_cards();
        out.push(Output::Changed);
    }

    /// The cards of the turn that ended stay where they were shown: under its replies.
    fn anchor_cards(&mut self) {
        let end = self.transcript.len();
        for shown in self.cards.iter_mut().filter(|shown| shown.after.is_none()) {
            shown.after = Some(end);
        }
    }

    fn on_http(&mut self, response: HttpResponse, out: &mut Vec<Output>) {
        let Some(kind) = self.inflight.remove(&response.id) else {
            return;
        };
        let answer = frappe_message(&response);
        match (kind, answer) {
            (Pending::Send, Ok(message)) => {
                // `start_or_resume_thread` gives the conversation id; `post_message` may repeat or change it.
                match thread_id_of(&message).or_else(|| self.thread_id.clone()) {
                    Some(thread) => {
                        self.thread_id = Some(thread);
                        self.sent_at = sent_at_of(&message);
                        self.send_answered = true;
                        self.next_poll_ms = Some(self.now_ms);
                        self.last_activity_ms = self.now_ms;
                        // While this message was on its way, the turn given up on before it may have sent replies: those
                        // stamped after the server took this one answer it.
                        if self.late_since.is_some()
                            && self.final_seen_ms.is_none()
                            && self.transcript.iter().skip(self.turn_index + 1).any(|reply| self.answers_this_turn(reply))
                        {
                            self.final_seen_ms = Some(self.now_ms);
                        }
                        out.push(Output::Changed);
                    }
                    None => self.fail(NoraError::Malformed("no `thread_id` in the answer".into()), out),
                }
            }
            // A send that got no answer, or a gateway's, may have reached Nora all the same: read late for her answer.
            (Pending::Send, Err(error)) if may_have_reached(&error) => self.give_up(error, out),
            (Pending::Send, Err(error)) => self.fail(error, out),
            (Pending::Messages, Ok(message)) => {
                // A read while the session reads late leaves the failure shown, unless something came.
                if self.busy {
                    self.last_error = None;
                    self.last_failure = None;
                }
                if !self.absorb_replies(&message, out) {
                    self.quiet_read_ms = self.read_sent_ms;
                }
            }
            (Pending::Actions, Ok(message)) => {
                if self.busy {
                    self.last_error = None;
                    self.last_failure = None;
                }
                let stale = self.refuse_older_actions && queued_at_of(&message).zip(self.sent_at).is_some_and(|(queued, sent)| queued < sent);
                // An action for a turn the pane gave up on (read late, or answered after the late reading ended) is never
                // run: the person was told to ask again.
                let given_up = !self.busy;
                for action in parse_actions(&message) {
                    if stale {
                        self.refuse(action, STOPPED, out);
                    } else if given_up {
                        self.refuse(action, LATE, out);
                    } else {
                        self.last_activity_ms = self.now_ms;
                        self.handle_action(action, out);
                    }
                }
            }
            (Pending::StoppedActions, Ok(message)) => {
                for action in parse_actions(&message) {
                    self.refuse(action, STOPPED, out);
                }
            }
            (kind @ (Pending::Messages | Pending::Actions), Err(error)) => {
                // A read that failed came back too: made after the deadline, it ends a turn the server no longer answers.
                if matches!(kind, Pending::Messages) {
                    self.quiet_read_ms = self.read_sent_ms;
                }
                // While the session reads late, the failure shown stays the turn's.
                if self.busy {
                    self.last_error = Some(error.to_string());
                    self.last_failure = Some(error);
                    out.push(Output::Changed);
                }
            }
            (Pending::CardAnswer(token), Ok(message)) => {
                self.last_error = None;
                self.last_failure = None;
                let answered = message.get("card").and_then(Card::from_json);
                if let Some(shown) = self.cards.iter_mut().find(|shown| shown.card.token == token) {
                    shown.answering = false;
                    if let Some(card) = answered.filter(|card| card.token == token) {
                        shown.card = Card { conversation: shown.card.conversation.clone(), ..card };
                    }
                }
                out.push(Output::Changed);
            }
            (Pending::CardAnswer(token), Err(error)) => {
                if let Some(shown) = self.cards.iter_mut().find(|shown| shown.card.token == token) {
                    shown.answering = false;
                }
                self.last_error = Some(error.to_string());
                self.last_failure = Some(error);
                out.push(Output::Changed);
            }
            (Pending::StoppedActions | Pending::Observation, _) => {}
        }
    }

    fn fail(&mut self, error: NoraError, out: &mut Vec<Output>) {
        self.last_error = Some(error.to_string());
        self.last_failure = Some(error.clone());
        self.outcome = Some(TurnEnd::Failed(error));
        self.busy = false;
        self.next_poll_ms = None;
        self.anchor_cards();
        out.push(Output::Changed);
    }

    /// Stops the turn at once. Nora's text chat has no stop on the server and the agent may still
    /// answer, so the session stops reading, refuses what waits for the user and whatever the turn
    /// still sends (see `refuse_older_actions`), and lets the editor finish what it already runs:
    /// that result goes to Nora as it is, and can be undone. A first message whose thread the server
    /// has not named yet leaves it behind; the next message opens another one.
    fn on_cancel(&mut self, out: &mut Vec<Output>) {
        // A Stop while the turn given up on is read late ends that reading, and what that turn still queues is refused
        // when the next message reads it, as after a Stop of a turn that runs.
        if self.late_until_ms.take().is_some() {
            self.next_poll_ms = None;
            self.refuse_older_actions = true;
        }
        if !self.busy {
            return;
        }
        self.busy = false;
        self.next_poll_ms = None;
        self.final_seen_ms = None;
        self.late_until_ms = None;
        self.outcome = Some(TurnEnd::Stopped);
        self.refuse_older_actions = true;
        self.anchor_cards();
        self.inflight.retain(|_, pending| !matches!(pending, Pending::Send | Pending::Messages));
        for pending in self.inflight.values_mut() {
            if matches!(pending, Pending::Actions) {
                *pending = Pending::StoppedActions;
            }
        }
        for waiting in std::mem::take(&mut self.waiting) {
            if let Some(callback) = waiting.action.callback_id {
                self.answer_error(&callback, STOPPED, out);
            }
        }
        out.push(Output::Changed);
    }

    /// Answers the action Nora waits on, if it waits, without running it.
    fn refuse(&mut self, action: RemoteAction, why: &str, out: &mut Vec<Output>) {
        let callback = match action {
            RemoteAction::Doc(doc) => doc.callback_id,
            RemoteAction::Unsupported { callback_id, .. } => callback_id,
        };
        if let Some(callback) = callback {
            self.answer_error(&callback, why, out);
        }
    }

    /// Adds the replies the transcript has not seen. The server list holds Nora's replies only, in the
    /// order they came; the user's own messages are added when sent. An acknowledgement is shown and
    /// the wait goes on; the first reply that is an answer ends it. Whether anything came (a reply, a new card): for a
    /// turn given up on, it is shown and the failure taken back ([`Self::late_news`]).
    fn absorb_replies(&mut self, message: &Value, out: &mut Vec<Output>) -> bool {
        let (replies, cards) = parse_messages(message);
        let replies: Vec<ChatMessage> = replies.into_iter().filter(|reply| reply.role != Role::User).collect();
        let fresh = self.fresh_replies(replies);
        let new_card = self.has_new_card(&cards);
        let news = !fresh.is_empty() || new_card;
        if news {
            self.last_activity_ms = self.now_ms;
        }
        // The session reads late for a turn it gave up on (never while busy): what comes is shown, not waited for.
        let late = self.late_until_ms.is_some();
        let mut answered_late = false;
        let mut changed = !fresh.is_empty();
        for reply in fresh {
            if late {
                answered_late |= !self.is_interim(&reply);
            } else if self.final_seen_ms.is_none() && self.answers_this_turn(&reply) {
                self.final_seen_ms = Some(self.now_ms);
            }
            if !is_hidden(&reply) {
                self.transcript.push(reply);
            }
        }
        // After the replies, so that a card that came with its reply is shown under it.
        changed |= self.absorb_cards(&cards);
        if late && news {
            self.late_news(answered_late, new_card);
            changed = true;
        }
        if changed {
            out.push(Output::Changed);
        }
        news
    }

    /// Whether a reply of this turn is its answer: not an acknowledgement, and, when a turn given up on before it may
    /// still send replies (`late_since`), stamped after the server took this message (until it has said when,
    /// a reply is that turn's).
    fn answers_this_turn(&self, reply: &ChatMessage) -> bool {
        if reply.role == Role::User || self.is_interim(reply) {
            return false;
        }
        if self.late_since.is_none() {
            return true;
        }
        match (self.send_answered, self.sent_at, reply.ts) {
            (false, ..) => false,
            (true, Some(sent), Some(ts)) => ts > sent,
            (true, ..) => true,
        }
    }

    /// Whether the server's list holds a card of this conversation not shown yet (a new card is news; a card shown
    /// that changes its state is not).
    fn has_new_card(&self, cards: &[Value]) -> bool {
        let Some(thread) = self.thread_id.as_deref() else { return false };
        cards
            .iter()
            .filter_map(Card::from_json)
            .any(|card| card.conversation == thread && !self.cards.iter().any(|shown| shown.card.token == card.token))
    }

    /// Takes in the cards of THIS conversation (the server's list also holds the person's pending proposals of every
    /// other one, for the desk); a card already shown gets its new state, unless the person's answer is on its way.
    /// Whether anything changed.
    fn absorb_cards(&mut self, cards: &[Value]) -> bool {
        let Some(thread) = self.thread_id.clone() else { return false };
        let mut changed = false;
        for card in cards.iter().filter_map(Card::from_json).filter(|card| card.conversation == thread) {
            match self.cards.iter_mut().find(|shown| shown.card.token == card.token) {
                Some(shown) => {
                    if !shown.answering && shown.card != card {
                        shown.card = card;
                        changed = true;
                    }
                }
                None => {
                    let after = (!self.busy).then_some(self.transcript.len());
                    self.cards.push(ShownCard { card, after, answering: false });
                    changed = true;
                }
            }
        }
        changed
    }

    /// The replies of the list that are new. With the server time of the send, a reply stamped after
    /// it is new whatever the buffer lost: the replies live in a cache that a clear can empty, and
    /// counting positions then hides an answer that lands as the only message left. Without it
    /// (an older server) the positions are counted, and the count never goes down.
    fn fresh_replies(&mut self, replies: Vec<ChatMessage>) -> Vec<ChatMessage> {
        // A turn given up on before the last message may still send replies: those stamped after its own send are new.
        let since = self.sent_at.map(|sent| self.late_since.map_or(sent, |late| late.min(sent)));
        match since {
            Some(sent_at) => {
                replies.into_iter().filter(|reply| reply.ts.is_none_or(|ts| ts > sent_at) && self.seen.insert(reply_key(reply))).collect()
            }
            None => {
                let fresh = replies.iter().skip(self.known_replies).cloned().collect();
                self.known_replies = self.known_replies.max(replies.len());
                fresh
            }
        }
    }

    /// Whether a reply only says the request is being handled (or adds manual pages, or says nothing)
    /// and the answer is still to come.
    fn is_interim(&self, reply: &ChatMessage) -> bool {
        if reply.sources_only || reply.text.trim().is_empty() || reply.text.contains("NO_REPLY") {
            return true;
        }
        let lowered = reply.text.to_lowercase().replace('\u{2019}', "'");
        self.cfg.ack_anchors.iter().any(|anchor| lowered.contains(anchor.as_str()))
    }

    fn handle_action(&mut self, action: RemoteAction, out: &mut Vec<Output>) {
        match action {
            RemoteAction::Unsupported { action, callback_id, reason } => {
                if let Some(callback) = callback_id {
                    self.answer_error(&callback, &format!("`{action}` is not available in the editor ({reason})"), out);
                }
            }
            RemoteAction::Doc(doc) => {
                let decision = self.decide(&doc.kind);
                match decision {
                    Decision::Deny => {
                        if let Some(callback) = doc.callback_id.clone() {
                            self.answer_error(&callback, &format!("refused by the editor's policy: {}", doc.kind.summary()), out);
                        }
                    }
                    Decision::Confirm => {
                        let id = self.new_id();
                        self.waiting.push(Waiting { id, action: doc });
                        out.push(Output::Changed);
                    }
                    Decision::Allow => self.run_document(doc, out),
                }
            }
        }
    }

    fn decide(&self, kind: &DocActionKind) -> Decision {
        match kind {
            DocActionKind::Execute { command, .. } => self.cfg.policy.check(command),
            DocActionKind::Batch { commands } => commands.iter().map(|call| self.cfg.policy.check(&call.command)).max().unwrap_or(Decision::Allow),
            DocActionKind::Save => Decision::Confirm,
            _ => Decision::Allow,
        }
    }

    fn run_document(&mut self, action: DocAction, out: &mut Vec<Output>) {
        let id = self.new_id();
        let undo = action.kind == DocActionKind::Undo;
        self.running.insert(id, Running { callback_id: action.callback_id, modifies: action.kind.modifies(), summary: action.kind.summary(), undo });
        self.last_activity_ms = self.now_ms;
        out.push(Output::Document(DocumentCall { id, kind: action.kind }));
    }

    fn answer_error(&mut self, callback_id: &str, message: &str, out: &mut Vec<Output>) {
        let request = self.request(protocol::RECEIVE_OBSERVATION, observation_body(callback_id, &observation_error(message)), Pending::Observation);
        out.push(request);
    }

    fn on_document_result(&mut self, id: u64, result: Result<Value, String>, out: &mut Vec<Output>) {
        let Some(run) = self.running.remove(&id) else {
            return;
        };
        self.last_activity_ms = self.now_ms;
        let observation = match result {
            Ok(value) => {
                if run.modifies && value.get("changed") != Some(&Value::Bool(false)) {
                    self.changes.push(run.summary);
                }
                observation_ok(value)
            }
            Err(message) => {
                // An Undo the editor refuses (the person changed the document since): Nora's earlier changes lie
                // under that edit too, and each click would only be refused again.
                if run.undo {
                    self.changes.clear();
                }
                self.last_error = Some(message.clone());
                self.last_failure = None;
                observation_error(&message)
            }
        };
        if let Some(callback) = run.callback_id {
            let request = self.request(protocol::RECEIVE_OBSERVATION, observation_body(&callback, &observation), Pending::Observation);
            out.push(request);
        }
        out.push(Output::Changed);
    }

    fn on_confirm(&mut self, id: u64, accepted: bool, out: &mut Vec<Output>) {
        let Some(position) = self.waiting.iter().position(|waiting| waiting.id == id) else {
            return;
        };
        let waiting = self.waiting.remove(position);
        if accepted {
            self.run_document(waiting.action, out);
        } else if let Some(callback) = waiting.action.callback_id {
            self.answer_error(&callback, "the user declined", out);
        }
        out.push(Output::Changed);
    }

    fn on_undo(&mut self, out: &mut Vec<Output>) {
        if self.changes.pop().is_some() {
            self.run_document(DocAction { callback_id: None, kind: DocActionKind::Undo }, out);
            out.push(Output::Changed);
        }
    }

    fn on_reset(&mut self, out: &mut Vec<Output>) {
        self.thread_id = None;
        self.transcript.clear();
        self.known_replies = 0;
        self.sent_at = None;
        self.seen.clear();
        self.busy = false;
        self.last_error = None;
        self.last_failure = None;
        self.turn_started_ms = 0;
        self.final_seen_ms = None;
        self.next_poll_ms = None;
        self.inflight.clear();
        self.running.clear();
        self.waiting.clear();
        self.changes.clear();
        self.outcome = None;
        self.refuse_older_actions = false;
        self.cards.clear();
        self.read_sent_ms = None;
        self.quiet_read_ms = None;
        self.late_until_ms = None;
        self.late_since = None;
        self.send_answered = false;
        self.turn_index = 0;
        out.push(Output::Changed);
    }

    /// A link the person clicked: a page of the instance goes to the host, anything else nowhere.
    fn on_open(&mut self, url: &str, out: &mut Vec<Output>) {
        if let Some(path) = self.page_of(url) {
            out.push(Output::Open(path));
        }
    }

    /// The person's yes or no on a card that waits for it, once: a second click while the answer travels does nothing.
    /// A card that asks for a choice takes no yes without one ([`Input::ChooseOnCard`]); its no goes as any other.
    fn on_answer_card(&mut self, token: &str, approve: bool, out: &mut Vec<Output>) {
        let Some(index) = self.cards.iter().position(|shown| shown.card.token == token && shown.card.awaits_the_person() && !shown.answering) else {
            return;
        };
        let card = &self.cards[index].card;
        if approve && !card.choices.is_empty() {
            return;
        }
        let path = card.answer_path();
        // //// Neoffice — a yes or a no, through either door: a card with choices never gets here with a yes (its yes is a
        // //// choice, above), and an appointment card's yes is a yes (23.10.2026).
        let body = if approve { card_answer_body(token, true) } else { card_refusal_body(token) };
        self.cards[index].answering = true;
        let request = self.request(path, body, Pending::CardAnswer(token.to_string()));
        out.push(request);
        out.push(Output::Changed);
    }

    /// The person's pick among a card's choices, once, and only one the card offers: the server is told its id.
    fn on_choose_on_card(&mut self, token: &str, choice: &str, out: &mut Vec<Output>) {
        let Some(index) = self.cards.iter().position(|shown| {
            shown.card.token == token && shown.card.awaits_the_person() && !shown.answering && shown.card.choices.iter().any(|c| c.id == choice)
        }) else {
            return;
        };
        self.cards[index].answering = true;
        let request = self.request(protocol::ANSWER_IN_PANE, card_choice_body(token, choice), Pending::CardAnswer(token.to_string()));
        out.push(request);
        out.push(Output::Changed);
    }
}

/// The session in the shared job vocabulary of `craft-ai-core`.
#[cfg(feature = "ai-core")]
impl NoraSession {
    /// The last message's turn as a `craft-ai-core` job, for a host that shows Nora with its other AI
    /// jobs: `None` before the first message, `Queued` until the server has the message, `Running`
    /// while Nora works or waits for the user, then `Succeeded`, `Failed` or `Cancelled` (the Stop
    /// button).
    pub fn turn_state(&self) -> Option<craft_ai_core::JobState> {
        use craft_ai_core::JobState;
        if self.busy {
            let sending = self.inflight.values().any(|pending| matches!(pending, Pending::Send));
            return Some(if sending { JobState::Queued } else { JobState::Running { percent: None } });
        }
        self.outcome.as_ref().map(|end| match end {
            TurnEnd::Answered => JobState::Succeeded,
            TurnEnd::Failed(error) => JobState::Failed { error: error.to_ai_error() },
            TurnEnd::Stopped => JobState::Cancelled,
        })
    }
}

/// Whether a send that failed may have reached Nora all the same: no answer came (the host gave up, the connection
/// broke), or a gateway in front of the instance answered for it (502, 503, 504, 408) while the instance went on.
fn may_have_reached(error: &NoraError) -> bool {
    matches!(error, NoraError::Unreachable(_)) || matches!(error.status(), Some(408 | 502 | 503 | 504))
}

/// A reply with nothing to show: empty, or the marker Nora uses for "nothing to say".
fn is_hidden(reply: &ChatMessage) -> bool {
    let text = reply.text.trim();
    text.is_empty() || text == "NO_REPLY"
}

/// What identifies a reply across polls: its server time and the start of its text.
fn reply_key(reply: &ChatMessage) -> String {
    let start: String = reply.text.chars().take(40).collect();
    format!("{:?}|{start}", reply.ts.map(f64::to_bits))
}
