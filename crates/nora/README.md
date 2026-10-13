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
