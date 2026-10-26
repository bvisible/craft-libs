// //// Neoffice — added file (no upstream equivalent): what brings Nora's pane back once it is closed.

//! Nora's pane is open by default; once the person closes it with its cross, a narrow rail stays at the right edge of
//! the window, beside the document and never over it, with Nora's sphere on it: the mark NeoMail shows her by, in the
//! clay of Neoffice. A click opens the pane again, on the same conversation (the session is the app's, untouched). Its
//! words, in the app's language, show in a bubble on hover ([`LauncherWords`]).
//!
//! Whether the pane is open is remembered per editor in the browser ([`PaneMemory`]), and open when nothing is: a
//! browser that refuses its storage (a private window, cookies off) opens it, as before.
//!
//! The app labels the button for screen readers itself: egui 0.36 and 0.37 name a widget's role differently
//! (`WidgetType`, then accesskit's `Role`), and this crate builds on both.

use egui::{Color32, Frame, Painter, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

/// The width of the rail at the right edge of the window, in points: the sphere's button and its margins.
pub const RAIL_WIDTH: f32 = 40.0;

/// The side of the button, in points.
const BUTTON: f32 = 30.0;

/// Nora's colour: the clay of Neoffice, darker on a light theme and lighter on a dark one, as NeoMail draws her.
pub fn clay(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(0xD6, 0x8A, 0x59) } else { Color32::from_rgb(0xA1, 0x5A, 0x2E) }
}

/// What the button is filled with under the pointer.
fn hover_fill(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(0x46, 0x33, 0x2A) } else { Color32::from_rgb(0xF0, 0xDC, 0xCE) }
}

/// Nora's sphere in `rect`, the Nora app's orb as NeoMail draws it: seven rings of latitude, the near half of each
/// strong and the far half faint.
pub fn paint_orb(painter: &Painter, rect: Rect, color: Color32) {
    use std::f32::consts::PI;
    const RINGS: usize = 7;
    const STEPS: usize = 18;
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.46;
    if !r.is_finite() || r <= 0.0 {
        return;
    }
    let width = (r * 0.11).max(0.9);
    for k in 0..RINGS {
        let latitude = ((k as f32 + 0.5) / RINGS as f32 - 0.5) * PI * 0.9;
        let (sin, cos) = latitude.sin_cos();
        let (y, rx) = (c.y + r * sin, r * cos);
        let ry = rx * 0.342;
        for (near, alpha) in [(true, 0.95), (false, 0.35)] {
            let points: Vec<egui::Pos2> = (0..=STEPS)
                .map(|i| {
                    let a = PI * i as f32 / STEPS as f32;
                    let a = if near { a } else { -a };
                    pos2(c.x + rx * a.cos(), y + ry * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(width, color.gamma_multiply(alpha))));
        }
    }
}

/// What the button says in its bubble, in the app's language. English by default: an app sets its own words here.
#[derive(Debug, Clone)]
pub struct LauncherWords {
    pub open: String,
}

impl Default for LauncherWords {
    fn default() -> Self {
        LauncherWords { open: "Open Nora".into() }
    }
}

/// Nora's button: her sphere, lit under the pointer or with the keyboard's focus, its words in a bubble on hover. True
/// in `.clicked()` when the person asks for the pane. The app labels it for screen readers (see the module).
pub fn button(ui: &mut Ui, words: &LauncherWords) -> Response {
    let dark = ui.visuals().dark_mode;
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() || response.has_focus() || response.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, 7.0, hover_fill(dark));
        }
        paint_orb(ui.painter(), rect.shrink(6.0), clay(dark));
    }
    response.on_hover_text(words.open.as_str())
}

/// The rail at the right edge of the window, outermost, with Nora's button at its top: drawn where the pane would be,
/// while it is closed. `frame` is the app's frame for its panes (its fill and border). The button's response: the app
/// opens the pane on its click.
pub fn rail(ui: &mut Ui, frame: Frame, words: &LauncherWords) -> Response {
    egui::Panel::right("nora_rail")
        .resizable(false)
        .exact_size(RAIL_WIDTH)
        .frame(frame)
        .show(ui, |ui| ui.vertical_centered(|ui| button(ui, words)).inner)
        .inner
}

/// Whether Nora's pane is open, remembered per editor in the browser's `localStorage`. Open when nothing is
/// remembered, or when the browser refuses its storage; nothing is kept outside a browser (a desktop app keeps its own
/// setting).
#[derive(Debug, Clone)]
pub struct PaneMemory {
    key: String,
}

impl PaneMemory {
    /// The memory of the pane of `app` ("wordcraft", "gridcraft", "deckcraft", "pdfcraft"…).
    pub fn new(app: &str) -> Self {
        PaneMemory { key: format!("neoffice.nora.pane.{app}") }
    }

    /// The storage key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Whether the pane was left open: open unless it was closed.
    pub fn open(&self) -> bool {
        Self::read(self.stored().as_deref())
    }

    /// Keeps whether the pane is open, if the browser lets it.
    pub fn remember(&self, open: bool) {
        self.store(if open { "open" } else { "closed" });
    }

    /// What a stored value says: only « closed » closes the pane; nothing, or anything else, leaves it open.
    pub fn read(stored: Option<&str>) -> bool {
        stored.map(str::trim) != Some("closed")
    }

    #[cfg(target_arch = "wasm32")]
    fn stored(&self) -> Option<String> {
        // Each step may be refused (a private window, cookies off, a sandboxed frame): then nothing is remembered.
        web_sys::window()?.local_storage().ok()??.get_item(&self.key).ok()?
    }

    #[cfg(target_arch = "wasm32")]
    fn store(&self, value: &str) {
        if let Some(Ok(Some(storage))) = web_sys::window().map(|window| window.local_storage()) {
            let _ = storage.set_item(&self.key, value);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn stored(&self) -> Option<String> {
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn store(&self, _value: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn only_a_closed_pane_is_remembered_closed() {
        assert!(PaneMemory::read(None), "nothing remembered: open, as before");
        assert!(!PaneMemory::read(Some("closed")));
        assert!(!PaneMemory::read(Some(" closed ")));
        assert!(PaneMemory::read(Some("open")));
        assert!(PaneMemory::read(Some("")), "a value that says nothing leaves it open");
        assert!(PaneMemory::read(Some("{\"weird\":1}")));
        assert_eq!(PaneMemory::new("gridcraft").key(), "neoffice.nora.pane.gridcraft");
        // Outside a browser nothing is kept: the desktop app keeps its own setting.
        let memory = PaneMemory::new("pdfcraft");
        memory.remember(false);
        assert!(memory.open());
    }

    #[derive(Default)]
    struct State {
        words: LauncherWords,
        clicked: usize,
        document: Rect,
    }

    fn harness() -> Harness<'static, State> {
        Harness::builder().with_size(egui::vec2(480.0, 320.0)).build_ui_state(
            |ui, state: &mut State| {
                // As an app does: the rail, its button labelled for screen readers.
                let response = rail(ui, Frame::NONE, &state.words);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Nora"));
                if response.clicked() {
                    state.clicked += 1;
                }
                state.document = egui::CentralPanel::default().show(ui, |ui| ui.max_rect()).inner;
            },
            State::default(),
        )
    }

    /// The pane closed, the rail stays at the right edge beside the document, and its button asks for the pane.
    #[test]
    fn the_rail_stays_at_the_right_edge_and_asks_for_the_pane() {
        let mut h = harness();
        h.run();
        let button = h.get_by_label("Nora");
        let at = button.rect();
        assert!(at.max.x <= 480.0 && at.min.x >= 480.0 - RAIL_WIDTH, "at the right edge, in the rail: {at:?}");
        let document = h.state().document;
        assert!(document.max.x <= at.min.x, "the document is beside it, not under it: {document:?}, {at:?}");
        h.get_by_label("Nora").click();
        h.run();
        assert_eq!(h.state().clicked, 1);
    }

    #[test]
    fn the_orb_is_drawn_in_any_rect_and_never_in_none() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_orb(ui.painter(), Rect::from_min_size(pos2(0.0, 0.0), vec2(18.0, 18.0)), clay(false));
            paint_orb(ui.painter(), Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0)), clay(true));
            paint_orb(ui.painter(), Rect::NOTHING, clay(true));
        });
        assert_ne!(clay(false), clay(true));
        assert_eq!(LauncherWords::default().open, "Open Nora");
    }
}
