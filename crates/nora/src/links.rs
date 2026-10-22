// //// Neoffice — added file (no upstream equivalent): where a link Nora wrote may lead, one rule for every editor's pane.

//! Where a link that Nora wrote in a document may lead.
//!
//! The editors check a link only when it is clicked, and the file they save keeps it as it was written:
//! Word, Excel, PowerPoint and the PDF readers open a link to a file (`C:\…`, `\\server\…`, `file:`,
//! `Book.xlsx`) or to a program. The Neoffice server refuses such a link in what Nora asks for, but a
//! command can compose one itself (a formula that builds its target, a replace that turns one text into
//! another). So each pane reads what a command of Nora's really wrote, and takes the command back when it
//! left a link that goes elsewhere than a web page, an e-mail address or a place of the document itself.
//! The place of the document (a cell, a slide, a bookmark) is each pane's to tell, in its own terms; the
//! rest is this module, the same rule as the server's (`neoffice_theme.editor_catalogue._web_or_mail`).

/// The parameters of a `mailto:` link that make some mail programs attach a file.
const ATTACHING: &[&str] = &["attach", "attachment", "attachments", "insert"];

/// Whether `target` is a web page or an e-mail address: `http://…` or `https://…` with something after
/// the `//`, or `mailto:…` that names no file to attach (`attach=`, `attachment=`, `attachments=` or
/// `insert=`, read decoded as a mail program reads them), with no control character and no character
/// that hides or reorders text ([`hides_or_controls`]). The spaces, tabs and line breaks around it are not
/// part of it.
pub fn web_or_mail(target: &str) -> bool {
    let text = target.trim_matches([' ', '\t', '\n', '\r']);
    if text.is_empty() || hides_or_controls(text) {
        return false;
    }
    // The control characters are refused above: only a space can follow the scheme and be no address.
    if let Some(rest) = after_scheme(text, "https://").or_else(|| after_scheme(text, "http://")) {
        return rest.chars().next().is_some_and(|c| c != ' ');
    }
    let Some(address) = after_scheme(text, "mailto:") else { return false };
    if !address.chars().next().is_some_and(|c| c != ' ') {
        return false;
    }
    let decoded = percent_decoded(address);
    if hides_or_controls(&decoded) {
        return false;
    }
    let query = decoded.split_once('?').map_or("", |(_, query)| query);
    !query.split(['&', ';']).any(|pair| {
        let key = pair.split_once('=').map_or(pair, |(key, _)| key).trim().to_lowercase();
        ATTACHING.contains(&key.as_str())
    })
}

/// Whether `text` holds a control character (Unicode's Cc) or a character that hides or reorders text
/// (Unicode's Cf: the soft hyphen, the zero-width characters, the direction marks and overrides, the
/// tags…): a target that shows one thing and leads to another.
pub fn hides_or_controls(text: &str) -> bool {
    text.chars().any(|c| c.is_control() || is_format(c))
}

/// Unicode's Cf (format) characters, as of Unicode 15.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// What follows `scheme` at the start of `text`, the scheme read in any case.
fn after_scheme<'a>(text: &'a str, scheme: &str) -> Option<&'a str> {
    if text.get(..scheme.len())?.eq_ignore_ascii_case(scheme) { text.get(scheme.len()..) } else { None }
}

/// `text` with its `%XX` escapes decoded, as UTF-8 (a byte that is not is shown as U+FFFD), as a mail
/// program reads an address.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match escape_at(bytes, at) {
            Some(decoded) => {
                out.push(decoded);
                at += 3;
            }
            None => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The byte a `%XX` escape at `at` stands for, when there is one.
fn escape_at(bytes: &[u8], at: usize) -> Option<u8> {
    if *bytes.get(at)? != b'%' {
        return None;
    }
    Some((hex(*bytes.get(at + 1)?)? << 4) | hex(*bytes.get(at + 2)?)?)
}

fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_pages_and_e_mail_addresses_are_kept() {
        for target in [
            "https://example.com",
            "HTTP://example.com/a?b=c#d",
            "  https://example.com/tarifs  ",
            "\thttps://example.com\n",
            "mailto:info@example.com",
            "MailTo:info@example.com?subject=Offre&body=Bonjour",
            "mailto:a@example.com?cc=b@example.com;subject=x",
        ] {
            assert!(web_or_mail(target), "{target:?}");
        }
    }

    #[test]
    fn files_programs_and_other_schemes_are_not() {
        for target in [
            "",
            "   ",
            "C:\\Tarifs\\catalogue.pdf",
            "\\\\serveur\\partage\\x.xlsx",
            "//serveur/partage",
            "file:///C:/x.pdf",
            "Book.xlsx",
            "Book.xlsx#Sheet1!A1",
            "ms-excel:ofe|u|https://example.com",
            "javascript:alert(1)",
            "ftp://example.com",
            "https://",
            "https:// example.com",
            "http:example.com",
            "mailto:",
            "mailto: a@example.com",
            "#Sheet2!A1",
            "Sheet2!A1",
        ] {
            assert!(!web_or_mail(target), "{target:?}");
        }
    }

    #[test]
    fn a_mail_link_that_attaches_a_file_is_not_an_address() {
        for target in [
            "mailto:a@example.com?attach=C:\\x.pdf",
            "mailto:a@example.com?subject=x&Attachment=/etc/passwd",
            "mailto:a@example.com?subject=x; attachments =x",
            "mailto:a@example.com?insert=x",
            "mailto:a@example.com?%61ttach=x",
            "mailto:a@example.com%3Fattach%3Dx",
        ] {
            assert!(!web_or_mail(target), "{target:?}");
        }
        assert!(web_or_mail("mailto:a@example.com?subject=attach"), "a word of the subject is no parameter");
    }

    #[test]
    fn a_target_that_hides_or_reorders_text_is_refused() {
        for target in [
            "https://example.com/\u{202e}fdp.exe",
            "https://exa\u{200b}mple.com",
            "https://example.com/\u{0}x",
            "https://example.com\u{1c}",
            "mailto:a@example.com%E2%80%AE",
            "mailto:a@example.com%00",
            "\u{feff}https://example.com",
        ] {
            assert!(!web_or_mail(target), "{target:?}");
        }
        assert!(hides_or_controls("a\u{ad}b"));
        assert!(hides_or_controls("\u{e0041}"));
        assert!(!hides_or_controls("Zürich — 12 %"));
    }

    #[test]
    fn escapes_decode_as_utf8() {
        assert_eq!(percent_decoded("a%20b%C3%A9%zz%4"), "a bé%zz%4");
        assert_eq!(percent_decoded("%ff"), "\u{fffd}");
        assert_eq!(percent_decoded("plain"), "plain");
    }
}
