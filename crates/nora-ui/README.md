<!-- //// Neoffice — added file (no upstream equivalent): README of the egui panel for Nora. -->
# craft-nora-ui

The egui side of Nora in a Craft editor: a chat panel, and the glue that keeps a
[`craft-nora`](../nora/README.md) session alive from the frame loop.

The panel only draws. It reads a `NoraSession` and hands back what the user did as `PanelAction`s;
the host feeds them to the session and performs the HTTP and document calls the session asks for.
Nothing here touches the network or the document, so the panel runs natively and in the browser.

```rust
use craft_nora::{NoraSession, Output};
use craft_nora_ui::{NoraPanel, drive};

// One frame. `outputs` is what the host has to do: make the `Http` calls, run the `Document`
// calls in the editor, and feed their answers back to the session as `Input`s.
fn frame(ui: &mut egui::Ui, nora: &mut NoraSession, panel: &mut NoraPanel) -> Vec<Output> {
    let mut outputs = drive(ui.ctx(), nora);
    for action in panel.show(ui, nora) {
        outputs.extend(nora.handle(action.into_input()));
    }
    outputs
}
```

`drive` ticks the session with egui's frame clock and asks for a repaint when the session is
waiting for something, so an event-driven host keeps polling Nora without a timer of its own.

## What the user gets

- A transcript with the user's messages on the right and Nora's on the left, a spinner while Nora
  works, the last error, and a field at the bottom: **Enter sends, Shift+Enter starts a new line**
  (an Enter that confirms an input-method composition is left alone).
- A card with **Allow** and **Refuse** when Nora asks to do something that needs a yes (saving, a
  command the policy flags).
- **Undo** for Nora's last change and **New conversation**.

## For the app

- **Words**: everything the user reads is in `NoraLabels` (English by default). An app passes its
  own translations, including `describe`, the function that words an action waiting for a yes
  (the default gives the English summary of the core).
- **Look**: colors follow the egui theme (light and dark); `NoraStyle` overrides them, with the
  corner radius and the bubble padding.
- **Keyboard**: the panel takes the Enter it used out of the frame's input. An editor that reads
  keys before it draws the panel should look at `ctx.wants_keyboard_input()` first, as for any
  text field. `NoraPanel::focus_input` puts the keyboard in the field when the panel opens.

## Status

Skeleton. Replies are shown as plain text (Markdown rendering is a later step). The tests drive
the real panel with `egui_kittest` (typing, Enter and Shift+Enter, buttons, confirmations,
translated labels).

```
cargo test -p craft-nora-ui
cargo clippy -p craft-nora-ui --all-targets -- -D warnings
```
