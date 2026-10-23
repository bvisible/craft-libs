// //// Neoffice — added file (no upstream equivalent): the little Markdown Nora writes, drawn in her bubbles.

//! Nora writes her replies in a little Markdown (`**gras**`, lists, a heading now and then); shown as it
//! is, the pane printed the stars. This reads that little and nothing more:
//!
//! - `**bold**` and `__bold__`; `*italic*` and `_italic_` (an underscore only at the edge of a word, so that
//!   `nora_doc_observe` stays a name); `` `code` `` (its words as written, on the code background);
//! - a line that starts with `-`, `*` or `+` and a space is a point (`•`), `1.` or `1)` a numbered one,
//!   its indent kept; a heading (`#` to `######`) is a line in bold; a quote (`>`) is its words;
//! - a fence (```` ``` ````) opens and closes lines of code, shown as written (a fence left open makes code of
//!   the rest of the reply);
//! - a backslash before a marker keeps the marker as text.
//!
//! Nothing else is read: no link (a link's words reach here as words, the panel lays out the links that
//! lead to the instance apart), no picture, no HTML (shown as text), no table. A marker of emphasis or
//! code that is not closed stays as written.

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontId};

/// How a piece of a reply is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Look {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

/// A piece of a reply: its words and how they look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    pub text: String,
    pub look: Look,
}

/// The reply as pieces, in order: its lines (joined by line breaks) read as the module says.
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut fenced = false;
    let mut first = true;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !first {
            push(&mut out, "\n", Look::default());
        }
        first = false;
        if fenced {
            push(&mut out, line, Look { code: true, ..Look::default() });
            continue;
        }
        let indent = line.len() - trimmed.len();
        if let Some(heading) = heading(trimmed) {
            inline(&mut out, heading, Look { bold: true, ..Look::default() });
        } else if let Some((marker, rest)) = point(trimmed) {
            push(&mut out, &format!("{}{marker} ", "  ".repeat(indent / 2)), Look::default());
            inline(&mut out, rest, Look::default());
        } else if let Some(quoted) = trimmed.strip_prefix('>') {
            inline(&mut out, quoted.trim_start(), Look::default());
        } else {
            inline(&mut out, line, Look::default());
        }
    }
    out
}

/// The words of `text` as they are shown, its markers drawn: what a link's label reads.
pub(crate) fn plain(text: &str) -> String {
    pieces(text).into_iter().map(|piece| piece.text).collect()
}

/// How bold words are drawn: in the app's bold family, or, for an app whose interface has no bold face (a web
/// build embeds none in NeoGrille and NeoPrés: their bold families draw the regular one), twice, the second time
/// a pixel to the right, on the same layout ([`bold_overlay`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Bold {
    Font(FontId),
    Doubled,
}

/// The pieces laid out for egui: `base` for the words, `bold` for the bold ones, `code_bg` the background of code.
pub(crate) fn job(text: &str, base: &TextFormat, bold: &Bold, code_bg: Color32) -> LayoutJob {
    laid_out(&pieces(text), base, bold, code_bg, false)
}

/// The layout of [`job`] with [`Bold::Doubled`], every word but the bold ones transparent and no background: drawn
/// over the reply a pixel to the right, it thickens the bold words alone. `None` when nothing is bold.
pub(crate) fn bold_overlay(text: &str, base: &TextFormat) -> Option<LayoutJob> {
    let pieces = pieces(text);
    pieces.iter().any(|piece| piece.look.bold).then(|| laid_out(&pieces, base, &Bold::Doubled, Color32::TRANSPARENT, true))
}

/// One section per piece, never merged with the next as `LayoutJob::append` merges two of the same format: egui
/// shapes each section on its own, so the reply and its overlay, cut at the same places, are laid out alike.
fn laid_out(pieces: &[Piece], base: &TextFormat, bold: &Bold, code_bg: Color32, only_bold: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for piece in pieces.iter().filter(|piece| !piece.text.is_empty()) {
        let mut format = base.clone();
        if piece.look.bold
            && let Bold::Font(font) = bold
        {
            format.font_id = font.clone();
        }
        format.italics = piece.look.italic;
        if piece.look.code {
            format.background = code_bg;
        }
        if only_bold && !piece.look.bold {
            format.color = Color32::TRANSPARENT;
        }
        let start = job.text.len();
        job.text.push_str(&piece.text);
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(start)..ByteIndex(job.text.len()), format });
    }
    job
}

/// The words of a heading line (`# Title`), when it is one.
fn heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    rest.starts_with(' ').then(|| rest.trim())
}

/// The marker to show and the words of a point (`- x`, `* x`, `+ x`, `1. x`, `1) x`), when the line is one.
fn point(line: &str) -> Option<(String, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some(("•".to_string(), rest));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let rest = &line[digits..];
        for close in [". ", ") "] {
            if let Some(words) = rest.strip_prefix(close) {
                return Some((format!("{}.", &line[..digits]), words));
            }
        }
    }
    None
}

/// Adds `text` in `look`, joined to the last piece when it looks the same.
fn push(out: &mut Vec<Piece>, text: &str, look: Look) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.look == look => last.text.push_str(text),
        _ => out.push(Piece { text: text.to_string(), look }),
    }
}

/// One line's words, its emphasis and its code read, on top of `outer` (a heading's bold).
fn inline(out: &mut Vec<Piece>, text: &str, outer: Look) {
    let chars: Vec<char> = text.chars().collect();
    let mut plain = String::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        if c == '\\' && chars.get(at + 1).is_some_and(|next| "*_`\\#>-+".contains(*next)) {
            plain.extend(chars.get(at + 1));
            at += 2;
            continue;
        }
        if c == '`'
            && let Some(end) = find(&chars, at + 1, "`")
        {
            flush(out, &mut plain, outer);
            push(out, &chars[at + 1..end].iter().collect::<String>(), Look { code: true, ..outer });
            at = end + 1;
            continue;
        }
        let double = chars.get(at + 1) == Some(&c);
        if (c == '*' || c == '_') && double && opens(&chars, at, 2) {
            let marker: String = [c, c].iter().collect();
            if let Some(end) = find(&chars, at + 2, &marker).filter(|end| closes(&chars, *end, 2)) {
                flush(out, &mut plain, outer);
                let words: String = chars[at + 2..end].iter().collect();
                inline(out, &words, Look { bold: true, ..outer });
                at = end + 2;
                continue;
            }
        }
        if (c == '*' || c == '_') && !double && opens(&chars, at, 1) {
            let marker = c.to_string();
            if let Some(end) = find(&chars, at + 1, &marker).filter(|end| closes(&chars, *end, 1)) {
                flush(out, &mut plain, outer);
                let words: String = chars[at + 1..end].iter().collect();
                inline(out, &words, Look { italic: true, ..outer });
                at = end + 1;
                continue;
            }
        }
        plain.push(c);
        at += 1;
    }
    flush(out, &mut plain, outer);
}

fn flush(out: &mut Vec<Piece>, plain: &mut String, look: Look) {
    push(out, plain, look);
    plain.clear();
}

/// Where `marker` is next found from `from`, not right where it starts (an empty emphasis is none).
fn find(chars: &[char], from: usize, marker: &str) -> Option<usize> {
    let marker: Vec<char> = marker.chars().collect();
    (from + 1..=chars.len().saturating_sub(marker.len())).find(|at| chars.get(*at..*at + marker.len()) == Some(marker.as_slice()))
}

/// Whether the marker of `width` characters at `at` opens an emphasis: something follows it that is no space,
/// and an underscore stands at the start of a word.
fn opens(chars: &[char], at: usize, width: usize) -> bool {
    let next = chars.get(at + width);
    let starts_a_word = at == 0 || chars.get(at - 1).is_some_and(|before| !before.is_alphanumeric());
    next.is_some_and(|next| !next.is_whitespace()) && (chars.get(at) != Some(&'_') || starts_a_word)
}

/// Whether the marker of `width` characters at `at` closes an emphasis: no space before it, and an underscore
/// stands at the end of a word.
fn closes(chars: &[char], at: usize, width: usize) -> bool {
    let before = at.checked_sub(1).and_then(|before| chars.get(before));
    let ends_a_word = chars.get(at + width).is_none_or(|after| !after.is_alphanumeric());
    before.is_some_and(|before| !before.is_whitespace()) && (chars.get(at) != Some(&'_') || ends_a_word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(text: &str) -> Vec<(String, Look)> {
        pieces(text).into_iter().map(|piece| (piece.text, piece.look)).collect()
    }

    const PLAIN: Look = Look { bold: false, italic: false, code: false };
    const BOLD: Look = Look { bold: true, italic: false, code: false };
    const ITALIC: Look = Look { bold: false, italic: true, code: false };
    const CODE: Look = Look { bold: false, italic: false, code: true };

    #[test]
    fn bold_italic_and_code_lose_their_markers() {
        assert_eq!(
            shown("Elle est adressée à **Boiseries Démo Sàrl**, *brouillon* et `Supplier`."),
            vec![
                ("Elle est adressée à ".to_string(), PLAIN),
                ("Boiseries Démo Sàrl".to_string(), BOLD),
                (", ".to_string(), PLAIN),
                ("brouillon".to_string(), ITALIC),
                (" et ".to_string(), PLAIN),
                ("Supplier".to_string(), CODE),
                (".".to_string(), PLAIN),
            ]
        );
        assert_eq!(shown("__gras__ et _penché_"), vec![("gras".into(), BOLD), (" et ".into(), PLAIN), ("penché".into(), ITALIC)]);
        assert_eq!(shown("**gras *et penché* fin**")[1], ("et penché".to_string(), Look { bold: true, italic: true, code: false }));
    }

    #[test]
    fn what_is_no_emphasis_stays_as_written() {
        for text in ["nora_doc_observe et range_set_values", "2 * 3 * 4", "**pas fermé", "un * seul", "** **", "a**b", "`pas fermé"] {
            let words: String = pieces(text).into_iter().map(|piece| piece.text).collect();
            assert_eq!(words, text, "{text}");
        }
        assert_eq!(shown("\\*pas penché\\*"), vec![("*pas penché*".to_string(), PLAIN)]);
        assert_eq!(shown("`**code**`"), vec![("**code**".to_string(), CODE)], "code is its words");
    }

    #[test]
    fn lists_headings_quotes_and_fences() {
        let text = "## Les services\n- **Chauffage** : contrôle\n  * détail\n2) deuxième\n> cité\n```\nlet x = **1**;\n```\nfin";
        let words: String = pieces(text).into_iter().map(|piece| piece.text).collect();
        assert_eq!(words, "Les services\n• Chauffage : contrôle\n  • détail\n2. deuxième\ncité\nlet x = **1**;\nfin");
        let shown = shown(text);
        assert_eq!(shown[0], ("Les services".to_string(), BOLD), "a heading is bold");
        assert!(shown.contains(&("Chauffage".to_string(), BOLD)));
        assert!(shown.contains(&("let x = **1**;".to_string(), CODE)));
        assert_eq!(pieces("#hashtag").into_iter().map(|p| p.text).collect::<String>(), "#hashtag", "no space: no heading");
    }

    #[test]
    fn a_link_and_html_stay_words() {
        let words: String = pieces("Voir [le site](https://example.com) et <b>ceci</b>").into_iter().map(|piece| piece.text).collect();
        assert_eq!(words, "Voir [le site](https://example.com) et <b>ceci</b>", "nothing here makes a link");
    }

    #[test]
    fn the_layout_keeps_every_word_once() {
        let base = TextFormat::simple(FontId::proportional(14.0), Color32::BLACK);
        let laid = job("**Total** : 42\n- un", &base, &Bold::Font(FontId::new(14.0, egui::FontFamily::Name("bold".into()))), Color32::GRAY);
        assert_eq!(laid.text, "Total : 42\n• un");
        let bold: Vec<&str> = laid
            .sections
            .iter()
            .filter(|section| section.format.font_id.family == egui::FontFamily::Name("bold".into()))
            .map(|section| &laid.text[section.byte_range.start.0..section.byte_range.end.0])
            .collect();
        assert_eq!(bold, ["Total"], "the bold word alone in the bold family");
        let laid = job("**Total**", &base, &Bold::Doubled, Color32::GRAY);
        assert_eq!(laid.sections[0].format, base, "no bold family: the regular one, drawn twice by the pane");
        assert_eq!(plain("[**Ouvrir**]"), "[Ouvrir]");
    }

    #[test]
    fn the_overlay_is_the_same_layout_with_only_the_bold_words_seen() {
        let base = TextFormat::simple(FontId::proportional(14.0), Color32::BLACK);
        let text = "Reliée à **Régie Exemple Sàrl** (*fournisseur*), `LET-1`\n## Suite";
        let shown = job(text, &base, &Bold::Doubled, Color32::GRAY);
        let overlay = bold_overlay(text, &base).expect("there are bold words");
        assert_eq!(overlay.text, shown.text, "the same words");
        assert_eq!(overlay.sections.len(), shown.sections.len());
        for (over, under) in overlay.sections.iter().zip(&shown.sections) {
            assert_eq!(over.byte_range, under.byte_range);
            assert_eq!((&over.format.font_id, over.format.italics), (&under.format.font_id, under.format.italics), "laid out alike");
            assert_eq!(over.format.background, Color32::TRANSPARENT, "the code background is not drawn twice");
        }
        let seen: Vec<&str> = overlay
            .sections
            .iter()
            .filter(|s| s.format.color != Color32::TRANSPARENT)
            .map(|s| &overlay.text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(seen, ["Régie Exemple Sàrl", "Suite"], "only the bold words and the heading");
        assert!(bold_overlay("rien de gras, *penché*", &base).is_none());
    }
}
