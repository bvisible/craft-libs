// //// Neoffice — added file (no upstream equivalent): the pane's colours, fonts and sizes, from the Neoffice design system.

//! How the pane looks: the Neoffice design system's tokens (`tokens/colors.css`, `typography.css`), light and dark, so
//! that Nora's pane is the same in NeoDoc, NeoGrille, NeoPrés, NeoPDF and NeoMail, and nothing in it is invented: ink
//! `#141414` carries every action, the warm paper is the canvas, the signal `#F55D1A` is a field (a tint, a well, a
//! focus ring) and never a button or the colour under light text; Karla sets the words and Forum the titles.
//!
//! Only the values the pane draws are here, each with the token it comes from.

use egui::{Color32, Context, FontData, FontFamily, FontId};
use std::sync::Arc;

/// The colours of the pane in one theme.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub dark: bool,
    /// The pane's canvas: `--color-bg` (paper cream), dark `--surface-subtle`.
    pub bg: Color32,
    /// What sits on the canvas, Nora's replies and the composer: `--surface-card`.
    pub card: Color32,
    /// A hovered or quiet fill: `--sand-100`, dark `--color-primary-soft`.
    pub quiet: Color32,
    /// `--border-subtle` and `--border-default`.
    pub border: Color32,
    pub border_strong: Color32,
    /// `--text-primary`, `--text-secondary`, a weak word (`--sand-500`, dark `--sand-400`: 4.5:1 on the pane) and a
    /// thing that cannot be used yet (`--text-disabled`).
    pub text: Color32,
    pub text_2: Color32,
    pub weak: Color32,
    pub disabled: Color32,
    /// The primary action, ink (dark: bright paper), and what is drawn on it: `--color-primary`, `--color-primary-fg`.
    pub primary: Color32,
    pub primary_hover: Color32,
    pub primary_fg: Color32,
    /// The signal as a field: `--color-accent-soft` and its border, the focus ring `--signal-400`, and the signal as
    /// text on paper `--signal-600` (dark `--signal-200`).
    pub signal_soft: Color32,
    pub signal_border: Color32,
    pub signal: Color32,
    pub signal_text: Color32,
    /// The warning and the error: `--amber-*` and `--red-*`, fill and text.
    pub warn_fill: Color32,
    pub warn_text: Color32,
    pub error_fill: Color32,
    pub error_text: Color32,
    /// The orb's colour: the clay `--clay-600` on paper, `--clay-400` on dark (the SVG's own).
    pub orb: Color32,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Palette {
    pub fn of(dark: bool) -> Palette {
        if dark {
            Palette {
                dark,
                bg: rgb(0x161413),
                card: rgb(0x221C16),
                quiet: Color32::from_rgba_unmultiplied(0xFF, 0xFB, 0xF5, 26),
                border: Color32::from_rgba_unmultiplied(0xFF, 0xFB, 0xF5, 20),
                border_strong: Color32::from_rgba_unmultiplied(0xFF, 0xFB, 0xF5, 61),
                text: rgb(0xFFFDF8),
                text_2: rgb(0xBFB5A4),
                weak: rgb(0x968C7C),
                disabled: rgb(0x524B41),
                primary: rgb(0xFFFDF8),
                primary_hover: rgb(0xECE7DE),
                primary_fg: rgb(0x141414),
                signal_soft: Color32::from_rgba_unmultiplied(0xF5, 0x5D, 0x1A, 41),
                signal_border: Color32::from_rgba_unmultiplied(0xF5, 0x5D, 0x1A, 77),
                signal: rgb(0xF55D1A),
                signal_text: rgb(0xFFB599),
                warn_fill: Color32::from_rgba_unmultiplied(0xE0, 0x98, 0x0F, 36),
                warn_text: rgb(0xF8E2B4),
                error_fill: Color32::from_rgba_unmultiplied(0xDC, 0x4A, 0x3D, 41),
                error_text: rgb(0xF6D2C9),
                orb: rgb(0xD68A59),
            }
        } else {
            Palette {
                dark,
                bg: rgb(0xFAF3EA),
                card: rgb(0xFFFFFF),
                quiet: rgb(0xECE7DE),
                border: rgb(0xECE7DE),
                border_strong: rgb(0xDCD4C7),
                text: rgb(0x141414),
                text_2: rgb(0x524B41),
                weak: rgb(0x6F665A),
                disabled: rgb(0xBFB5A4),
                primary: rgb(0x141414),
                primary_hover: rgb(0x232020),
                primary_fg: rgb(0xFFFDF8),
                signal_soft: rgb(0xFFEAE2),
                signal_border: rgb(0xFFD1C1),
                signal: rgb(0xF55D1A),
                signal_text: rgb(0xAA3700),
                warn_fill: rgb(0xFCF2DD),
                warn_text: rgb(0x92610C),
                error_fill: rgb(0xFBEBE7),
                error_text: rgb(0x9C2920),
                orb: rgb(0xA15A2E),
            }
        }
    }
}

/// The design system's radii: `--radius-md` for a control, `--radius-lg` for a card, `--radius-xl` for a composer.
pub(crate) const RADIUS_MD: u8 = 10;
pub(crate) const RADIUS_LG: u8 = 14;
pub(crate) const RADIUS_XL: u8 = 18;

/// The families the pane draws with, when the app has them (see [`install_fonts`]); the app's own otherwise.
#[derive(Debug, Clone)]
pub(crate) struct Fonts {
    pub installed: bool,
}

const SANS: &str = "nora-sans";
const MEDIUM: &str = "nora-medium";
const BOLD: &str = "nora-bold";
const TITLE: &str = "nora-title";

/// What the pane's text is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Face {
    Body,
    Medium,
    Bold,
    Title,
}

impl Fonts {
    pub fn font(&self, face: Face, size: f32) -> FontId {
        if !self.installed {
            return FontId::proportional(size);
        }
        let name = match face {
            Face::Body => SANS,
            Face::Medium => MEDIUM,
            Face::Bold => BOLD,
            Face::Title => TITLE,
        };
        FontId::new(size, FontFamily::Name(name.into()))
    }

    /// The name of the bold family, for what draws bold words of its own (Nora's Markdown).
    pub fn bold_family(&self) -> Option<&'static str> {
        self.installed.then_some(BOLD)
    }
}

const KARLA_REGULAR: &[u8] = include_bytes!("../fonts/Karla-Regular.ttf");
const KARLA_MEDIUM: &[u8] = include_bytes!("../fonts/Karla-Medium.ttf");
const KARLA_BOLD: &[u8] = include_bytes!("../fonts/Karla-Bold.ttf");
const FORUM: &[u8] = include_bytes!("../fonts/Forum-Latin.ttf");

/// Makes sure the context has the pane's four families: Karla in three weights and Forum, each followed by the app's
/// own interface fonts so that a character they lack (a language of the person's own) is still drawn. The families are
/// added to the app's definitions, never in place of them; when the app installs its fonts again they are gone and
/// are added again on the next frame (at most a few times: an app that resets its fonts at every frame is left with
/// its own).
pub(crate) fn install_fonts(ctx: &Context) -> Fonts {
    let wanted = FontFamily::Name(SANS.into());
    let proportional = ctx.fonts(|fonts| fonts.definitions().families.get(&FontFamily::Proportional).cloned()).unwrap_or_default();
    let (present, in_step) = ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        let present = definitions.families.contains_key(&wanted);
        // The app's fonts may have grown since (a fallback for a language): the pane's families follow them.
        let tail = definitions.families.get(&wanted).map(|chain| chain.iter().skip(1).cloned().collect::<Vec<_>>());
        (present, tail.is_none_or(|tail| tail == proportional))
    });
    if present && in_step {
        return Fonts { installed: true };
    }
    // An app that sets its fonts at every frame would have this run at every frame: after a few tries, no more.
    let tries = ctx.data_mut(|data| {
        let tries = data.get_temp_mut_or_default::<u32>(egui::Id::unique("nora-ui-font-installs"));
        *tries += 1;
        *tries
    });
    if tries > 4 {
        return Fonts { installed: present };
    }
    let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
    for (name, bytes) in [("Karla-Regular", KARLA_REGULAR), ("Karla-Medium", KARLA_MEDIUM), ("Karla-Bold", KARLA_BOLD), ("Forum-Latin", FORUM)] {
        definitions.font_data.insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }
    let after = |head: &[&str]| head.iter().map(|name| name.to_string()).chain(proportional.iter().cloned()).collect::<Vec<_>>();
    definitions.families.insert(FontFamily::Name(SANS.into()), after(&["Karla-Regular"]));
    definitions.families.insert(FontFamily::Name(MEDIUM.into()), after(&["Karla-Medium"]));
    definitions.families.insert(FontFamily::Name(BOLD.into()), after(&["Karla-Bold"]));
    definitions.families.insert(FontFamily::Name(TITLE.into()), after(&["Forum-Latin", "Karla-Regular"]));
    ctx.set_fonts(definitions);
    // The families exist from the next pass: until then the pane uses the app's own.
    Fonts { installed: false }
}
