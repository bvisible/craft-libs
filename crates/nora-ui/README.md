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

- **The look of the Neoffice design system**, the same in NeoDoc, NeoGrille, NeoPrés, NeoPDF and NeoMail, light and dark:
  the warm paper for the canvas, ink for the person's words and every action, Nora's replies on white cards, the signal
  (`#F55D1A`) as the tint of what Nora is doing, **Karla** for the words and **Forum** for her name, **Lucide**'s icons
  (`icons.rs`), and **her own sphere** (`orb.rs`, the thirteen rings of dust of `assets/icons/nora-orb.svg`, which turns while
  she works). Nothing is the editor's own colour: `look.rs` holds the values, each with the token it comes from.
- A greeting in Forum with ready requests on an empty page (`PaneWords::suggestions`), a transcript with the person's
  words in ink on the right and Nora's on white cards on the left, and **what she is doing** while she works, in words
  (`ActivityWords`: thinking, reading the document, editing it) with the seconds it has taken.
- A composer: **Enter sends, Shift+Enter starts a new line** (an Enter that confirms an input-method composition is left
  alone); while Nora works the message stays in the box, as in the desk's chat, and the round button stops her.
- **Dictation**: a microphone in the composer where the page can record (`voice.rs`: `getUserMedia`, `MediaRecorder`, the level of
  the voice). One click to speak, one to stop (the words go into the box) or the arrow (they go to Nora). The recording leaves the
  page as a `PanelAction::Dictate`, which the session turns into `nora.api.stt.transcribe` through the host (`Input::Transcribe`,
  `NoraSession::dictation`); a transcript of silence (« Sous-titrage Société Radio-Canada ») is never put in the box.
- A card with **Allow** and **Refuse** when Nora asks to do something that needs a yes (saving, a
  command the policy flags), and **Undo this change** right under what she changed.
- When the host has no way to reach Nora, `NoraPanel::offline` says why in place of the text box.

## For the app

- **Words**: everything the user reads is in `NoraLabels`, `PaneWords`, `ActivityWords`, `VoiceWords`, `ResumeWords` and
  `CorrectionWords` (English by default). An app passes its own translations, including `describe`, the function that words
  an action waiting for a yes (the default gives the English summary of the core).
- **Look**: the design system's, chosen by whether the egui theme is light or dark. `NoraStyle` can still override the bubbles'
  colours (NeoMail's clay did). The panel paints its own canvas over the `Ui` it is given: give it a frame with no margin and no
  fill. `NoraPanel::closable` draws a close button, `take_close_request` says when it was clicked.
- **Fonts**: the panel adds its own families (Karla, Forum) to the app's fonts, each followed by the app's own, so that a
  language Karla lacks is still drawn. They are added when the panel first draws and again if the app installs its fonts anew.
- **Voice**: `NoraPanel::dictation` turns the microphone on (the default), and it is offered only where the browser has a recorder
  (`can_dictate`). The host must carry `nora.api.stt.transcribe` with the recording as base64 in a JSON body
  (`protocol::transcribe_body`) and make the multipart form itself, with the person's credentials; an app whose host cannot sets
  `dictation = false`.
- **Clock**: `drive` reads the page's own clock in a browser (`clock_ms`), so that an app can call it from its `logic`, which
  runs when the window is hidden and no frame is drawn, and Nora goes on reading her actions.
- **Keyboard**: the panel takes the Enter it used out of the frame's input. An editor that reads
  keys before it draws the panel should look at `ctx.wants_keyboard_input()` first, as for any
  text field. `NoraPanel::focus_input` puts the keyboard in the field when the panel opens.

## Looking at it

`cargo test -p craft-nora-ui --features preview -- --ignored` draws the panel in five states, light and dark, into the
folder `NORA_PREVIEW_DIR` as PNG files (it builds wgpu, so it is off by default).

## Status

The tests drive the real panel with `egui_kittest` (typing, Enter and Shift+Enter, buttons, confirmations,
translated labels, the microphone's states, the contrast of every word on its canvas).

```
cargo test -p craft-nora-ui
cargo clippy -p craft-nora-ui --all-targets -- -D warnings
```
