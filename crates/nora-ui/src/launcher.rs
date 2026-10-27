// //// Neoffice — added file (no upstream equivalent): Nora's button in an editor's top bar, which opens and closes her pane.

//! Nora's button, in the bar at the top of an editor's window, whatever tab of the ribbon is in view: the « Ask Nora »
//! pill NeoMail shows, her sphere and its words in the clay of Neoffice, or her sphere alone, its words in a bubble,
//! when the bar is short of room. A click opens her pane, or closes it, on the same conversation (the session is the
//! app's, untouched); the pill is lit while the pane is open. Closed, the pane leaves the whole width to the document.
//!
//! Whether the pane is open is remembered per editor in the browser ([`PaneMemory`]), and open when nothing is: a
//! browser that refuses its storage (a private window, cookies off) opens it, as before.
//!
//! The app places the button and labels it for screen readers itself: egui 0.36 and 0.37 name a widget's role
//! differently (`WidgetType`, then accesskit's `Role`), and this crate builds on both.

use egui::{Color32, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// The height of the pill, in points: NeoMail's.
pub const PILL_HEIGHT: f32 = 24.0;

/// Nora's colours in a light or dark theme, as NeoMail draws her (its `nora::look::tone`).
#[derive(Debug, Clone, Copy)]
struct Tone {
    orb: Color32,
    text: Color32,
    fill: Color32,
    hover: Color32,
    border: Color32,
}

fn tone(dark: bool) -> Tone {
    if dark {
        Tone {
            orb: Color32::from_rgb(0xD6, 0x8A, 0x59),
            text: Color32::from_rgb(0xE7, 0xB4, 0x8F),
            fill: Color32::from_rgb(0x3A, 0x2A, 0x21),
            hover: Color32::from_rgb(0x46, 0x33, 0x2A),
            border: Color32::from_rgb(0x7A, 0x4A, 0x2C),
        }
    } else {
        Tone {
            orb: Color32::from_rgb(0xA1, 0x5A, 0x2E),
            text: Color32::from_rgb(0x8A, 0x4A, 0x22),
            fill: Color32::from_rgb(0xF7, 0xEB, 0xE3),
            hover: Color32::from_rgb(0xF0, 0xDC, 0xCE),
            border: Color32::from_rgb(0xE2, 0xB5, 0x97),
        }
    }
}

/// Nora's colour: the clay of Neoffice, darker on a light theme and lighter on a dark one, as NeoMail draws her.
pub fn clay(dark: bool) -> Color32 {
    tone(dark).orb
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

/// What the button says, in the app's language: its words, and what its bubble says of Nora. English by default, as
/// NeoMail says them: an app sets its own words here.
#[derive(Debug, Clone)]
pub struct LauncherWords {
    pub label: String,
    pub tip: String,
}

impl Default for LauncherWords {
    fn default() -> Self {
        LauncherWords { label: "Ask Nora".into(), tip: "Nora, the Neoffice assistant".into() }
    }
}

/// The width of the pill with its words, in points.
fn full_width(ui: &Ui, words: &LauncherWords) -> f32 {
    let label = ui.painter().layout_no_wrap(words.label.clone(), FontId::proportional(12.0), Color32::PLACEHOLDER);
    10.0 + (PILL_HEIGHT - 10.0) + 6.0 + label.size().x + 10.0
}

/// Nora's button, where the app places it (the right of its top bar): the pill with her sphere and its words, or her
/// sphere alone when the space left in the row is short (less than three pills), its words then in the bubble. Lit
/// while her pane is `open`. True in `.clicked()` when the person asks to open or close the pane. The app labels it for
/// screen readers (see the module).
pub fn button(ui: &mut Ui, words: &LauncherWords, open: bool) -> Response {
    let full = full_width(ui, words);
    let compact = ui.available_width() < full * 3.0;
    let dark = ui.visuals().dark_mode;
    let tone = tone(dark);
    let width = if compact { PILL_HEIGHT } else { full };
    let (rect, response) = ui.allocate_exact_size(vec2(width, PILL_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let lit = open || response.hovered() || response.has_focus() || response.is_pointer_button_down_on();
        let border = if open { Stroke::new(1.5, tone.orb) } else { Stroke::new(1.0, tone.border) };
        let painter = ui.painter();
        painter.rect(rect, PILL_HEIGHT / 2.0, if lit { tone.hover } else { tone.fill }, border, StrokeKind::Inside);
        let side = PILL_HEIGHT - 10.0;
        let at = if compact { rect.center().x - side / 2.0 } else { rect.min.x + 10.0 };
        let mark = Rect::from_min_size(pos2(at, rect.min.y + 5.0), vec2(side, side));
        paint_orb(painter, mark, tone.orb);
        if !compact {
            let label = painter.layout_no_wrap(words.label.clone(), FontId::proportional(12.0), tone.text);
            let at = pos2(mark.max.x + 6.0, rect.center().y - label.size().y / 2.0);
            painter.galley(at, label, tone.text);
        }
    }
    response.on_hover_text(if compact { words.label.as_str() } else { words.tip.as_str() })
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

    struct State {
        words: LauncherWords,
        open: bool,
        rect: Option<Rect>,
    }

    /// A top bar `width` points wide, as an app has it: the button at its right, labelled for screen readers, and a
    /// click that opens or closes the pane.
    fn bar(width: f32) -> Harness<'static, State> {
        Harness::builder().with_size(egui::vec2(width, 60.0)).build_ui_state(
            |ui, state: &mut State| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = button(ui, &state.words, state.open);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, state.open, &state.words.label));
                    state.rect = Some(response.rect);
                    if response.clicked() {
                        state.open = !state.open;
                    }
                });
            },
            State { words: LauncherWords::default(), open: true, rect: None },
        )
    }

    /// The button opens and closes the pane: the pill with its words where the bar has room, the sphere alone where it
    /// hasn't.
    #[test]
    fn the_button_opens_and_closes_the_pane() {
        let mut h = bar(900.0);
        h.run();
        let wide = h.state().rect.expect("drawn");
        assert!(wide.width() > PILL_HEIGHT * 2.0, "the pill with its words: {wide:?}");
        assert_eq!(wide.height(), PILL_HEIGHT);
        h.get_by_label("Ask Nora").click();
        h.run();
        assert!(!h.state().open, "closed");
        h.get_by_label("Ask Nora").click();
        h.run();
        assert!(h.state().open, "open again");
        let mut narrow = bar(120.0);
        narrow.run();
        let short = narrow.state().rect.expect("drawn");
        assert_eq!(short.width(), PILL_HEIGHT, "the sphere alone: {short:?}");
    }

    #[test]
    fn the_orb_is_drawn_in_any_rect_and_never_in_none() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint_orb(ui.painter(), Rect::from_min_size(pos2(0.0, 0.0), vec2(18.0, 18.0)), clay(false));
            paint_orb(ui.painter(), Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0)), clay(true));
            paint_orb(ui.painter(), Rect::NOTHING, clay(true));
        });
        output.textures_delta.clear();
        assert_ne!(clay(false), clay(true));
        assert_eq!(LauncherWords::default().label, "Ask Nora");
        assert_eq!(LauncherWords::default().tip, "Nora, the Neoffice assistant");
    }
}
