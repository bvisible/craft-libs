<!-- //// Neoffice — added file (no upstream equivalent): README of the Nora client core. -->
# craft-nora

The client core of Nora, the Neoffice assistant, for every Craft app (Word, Excel, PowerPoint, PDF).

Nora itself (the agent, its memory, its tools) stays on the Neoffice server. This crate is what an
editor needs to talk to it: the wire protocol, the state of a conversation, and the actions Nora
asks for inside the open document. It is not a rewrite of Nora and it holds no app code.

It is **sans I/O**: it never opens a socket and never reads a clock. The host (NeoDesk, the Drive
page, a native shell) makes the HTTP calls with its own session and credentials and feeds the
answers back; the app runs document actions through its own control channel. The same code runs in
a browser (wasm) and natively, and is tested without a network. The egui panel is a separate
crate, [`craft-nora-ui`](../nora-ui/README.md).

```rust
use craft_nora::{AppKind, DocumentInfo, Input, NoraSession, Output, PageContext, SessionConfig};

let context = PageContext::editor(AppKind::Word, DocumentInfo::named("quote.docx"));
let mut nora = NoraSession::new(SessionConfig::default(), context);

for output in nora.handle(Input::Send("Rewrite the first paragraph".into())) {
    match output {
        Output::Http(request) => { /* POST request.body to request.path, then Input::Http(answer) */ }
        Output::Document(call) => { /* run call.kind in the editor, then Input::DocumentResult { .. } */ }
        Output::Changed => { /* repaint */ }
    }
}
// Once a frame: nora.handle(Input::Tick { now_ms }). `next_wake_ms()` says when it is needed.
```

## What is in it

| Module | Role |
|---|---|
| `session` | `NoraSession`: the conversation state machine (`Input` in, `Output` out), polling, confirmations, undo |
| `protocol` | the `nora.api.chat.*` and `receive_observation` calls, and the `doc_*` actions Nora can ask for |
| `policy` | `CommandPolicy`: which editor commands Nora may run (`file.` and `app.` are refused by default) |
| `context` | `PageContext`: which editor and document the user is in, sent with every message |
| `http`, `error` | the I/O-free request and answer types, Frappe's `message` envelope, errors a panel can show |

## How a turn goes (read from the desk's own chat and the server)

- The server answers a message at once with an **acknowledgement** in the user's language ("Je
  transmets votre demande à votre pôle…") and the desk's real answer lands 10 to 70 seconds later in
  the same conversation. The session shows the acknowledgement and **keeps waiting**; the first reply
  that is an answer ends the wait, after a moment (`settle_ms`) for the actions queued with it. The
  phrases that mark an acknowledgement are `protocol::ACK_ANCHORS`, a copy of the server's list; a
  host that has the live one (`frappe.boot.nora_ack_anchors`) passes it in `SessionConfig::ack_anchors`.
- **A reply is new when the server stamped it after the send** (`sent_at` in the answer of the send,
  `ts` on the reply), not when it sits past a position in the list: the replies live in a cache that
  a clear can empty, and counting positions then hides an answer that lands as the only message
  left. Without `sent_at` (an older server) the positions are counted.
- The replies are read often at first (400 ms for three seconds), then every second until fifteen
  seconds, then every three. Eight minutes without a reply or an action end the wait with an error.
- A reply may end with a machine block (a ```` ```nora ```` fence or a `<!-- nora: … -->` comment) that
  carries result cards, follow-up questions and manual pages for the desk: the text is shown without
  it, the follow-ups are kept in `ChatMessage::followups`. `NO_REPLY` and empty replies are not shown.

## Rules the core enforces

- Nora edits the document that is open. Commands that open, save, close or quit are refused by the
  default policy, and **saving always asks the user first**.
- Nora asks for the document through `doc_*` actions (observe, execute, batch, select text, type
  text, render a page, save). Desk-page actions (`dom_click`, `navigate`…) get an error answer: an
  editor is one canvas.
- Every change Nora makes can be undone from the panel.

## Status

Skeleton, covered by unit tests and an end-to-end test of a whole conversation
(`tests/session_flow.rs`). The `doc_*` tools on the server side and the wiring into each app are
still to be done.

```
cargo test -p craft-nora
cargo clippy -p craft-nora --all-targets -- -D warnings
```
