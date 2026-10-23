// //// Neoffice — added file (no upstream equivalent): the links of Nora's replies, and where the pane lets them lead.

//! The links in what Nora says, and where a pane lets them lead.
//!
//! Nora's replies carry links the server wrote (`[Ouvrir dans NeoDoc](/drive/f/…)`, the draft of an e-mail), and so do
//! the cards its tools leave. A pane shows such a link as a link only when it leads to a page of the person's own
//! instance ([`instance_path`]): a document of the Drive, a page of the desk, NeoMail. Anything else (another site, a
//! file, a script, an address a reply was made to carry) stays plain text: the reply's words came through a model,
//! which may have read them in a document. The host that frames the editor opens the page, on the person's click.

use crate::links::hides_or_controls;

/// The pages of the instance a link of the pane may open, by the start of their path.
pub const OPEN_PREFIXES: &[&str] = &["/drive/", "/app/", "/neomail"];

/// The longest address a link of the pane leads to.
pub const MAX_LINK_CHARS: usize = 2048;

/// A piece of a reply: text, or a Markdown link `[label](address)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Link { label: String, url: String },
}

/// The reply as text and links, in order. Only the Markdown form `[label](address)` is a link, its label without
/// brackets and its address without a space or a parenthesis; anything else is text, as written.
pub fn segments(text: &str) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find(']').and_then(|close| {
            let label = &after[..close];
            let tail = after[close + 1..].strip_prefix('(')?;
            let end = tail.find(')')?;
            let url = &tail[..end];
            let well_formed = !label.is_empty()
                && !label.contains('[')
                && !label.contains('\n')
                && !url.is_empty()
                && !url.contains(char::is_whitespace)
                && !url.contains('(');
            // How much of `rest` the link takes: `[`, the label, `](`, the address, `)`.
            well_formed.then(|| (label.to_string(), url.to_string(), open + 1 + close + 2 + end + 1))
        });
        match link {
            Some((label, url, taken)) => {
                plain.push_str(&rest[..open]);
                if !plain.is_empty() {
                    out.push(Segment::Text(std::mem::take(&mut plain)));
                }
                out.push(Segment::Link { label, url });
                rest = &rest[taken..];
            }
            None => {
                plain.push_str(&rest[..open + 1]);
                rest = &rest[open + 1..];
            }
        }
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        out.push(Segment::Text(plain));
    }
    out
}

/// The path of the instance's page a link leads to, when it is one a pane may open; `None` otherwise.
///
/// A path of the instance (`/drive/f/…`), or an address of `instance_origin` (the origin the page that frames the
/// editor shares with the instance; `None` where it does not, in Neodesk), under [`OPEN_PREFIXES`]. Never another
/// site, a path a browser reads as another host (`//…`, `/\…`), a path that climbs out of its prefix (`..`, its
/// percent-encoded forms), a space, a control character or one that hides text. The path keeps its query.
pub fn instance_path(url: &str, instance_origin: Option<&str>) -> Option<String> {
    if url.is_empty() || url.len() > MAX_LINK_CHARS || hides_or_controls(url) || url.contains(char::is_whitespace) || url.contains('\\') {
        return None;
    }
    let path = if url.starts_with('/') {
        url
    } else {
        let origin = instance_origin.map(|o| o.trim_end_matches('/')).filter(|o| !o.is_empty())?;
        url.strip_prefix(origin).filter(|rest| rest.starts_with('/'))?
    };
    if path.starts_with("//") {
        return None;
    }
    let route = path.split(['?', '#']).next().unwrap_or_default();
    let lowered = route.to_ascii_lowercase();
    if lowered.contains("%2e") || lowered.contains("%2f") || lowered.contains("%5c") || route.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    let allowed = OPEN_PREFIXES
        .iter()
        .any(|prefix| if prefix.ends_with('/') { route.starts_with(prefix) } else { route == *prefix || route.starts_with(&format!("{prefix}/")) });
    allowed.then(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(label: &str, url: &str) -> Segment {
        Segment::Link { label: label.into(), url: url.into() }
    }

    fn text(t: &str) -> Segment {
        Segment::Text(t.into())
    }

    #[test]
    fn a_reply_is_its_text_and_its_links_in_order() {
        assert_eq!(
            segments("La lettre est prête : [Ouvrir dans NeoDoc](/drive/f/abc). Rien n'est envoyé."),
            vec![text("La lettre est prête : "), link("Ouvrir dans NeoDoc", "/drive/f/abc"), text(". Rien n'est envoyé.")]
        );
        assert_eq!(segments("[a](/x)[b](/y)"), vec![link("a", "/x"), link("b", "/y")]);
        assert_eq!(segments(""), Vec::<Segment>::new());
        assert_eq!(segments("Sans lien."), vec![text("Sans lien.")]);
    }

    #[test]
    fn what_is_not_a_well_formed_link_stays_text() {
        for written in ["[a] (b)", "[](x)", "[a]()", "[a](b c)", "[a](b", "[a [b](c)", "a]b(c)", "[ligne\nsuite](x)", "["] {
            let joined: String = segments(written)
                .into_iter()
                .map(|s| match s {
                    Segment::Text(t) => t,
                    Segment::Link { label, url } => format!("<{label}|{url}>"),
                })
                .collect();
            assert!(!joined.contains('<') || written == "[a [b](c)", "{written:?} gave {joined:?}");
        }
        // The last bracket before a well-formed link opens it.
        assert_eq!(segments("[a [b](/c)"), vec![text("[a "), link("b", "/c")]);
    }

    #[test]
    fn a_page_of_the_instance_is_opened_and_nothing_else() {
        let origin = Some("https://erp.example.test");
        assert_eq!(instance_path("/drive/f/abc", None).as_deref(), Some("/drive/f/abc"));
        assert_eq!(instance_path("https://erp.example.test/drive/f/abc", origin).as_deref(), Some("/drive/f/abc"));
        assert_eq!(
            instance_path("/app/webmail?account=Bo%C3%AEte&folder=Drafts&uid=41", None).as_deref(),
            Some("/app/webmail?account=Bo%C3%AEte&folder=Drafts&uid=41")
        );
        assert_eq!(instance_path("/neomail", None).as_deref(), Some("/neomail"));
        for refused in [
            "https://erp.example.test/drive/f/abc",
            "https://elsewhere.example/drive/f/abc",
            "//elsewhere.example/drive/f/abc",
            "/\\elsewhere.example/drive",
            "javascript:alert(1)",
            "drive/f/abc",
            "/api/method/frappe.client.delete",
            "/files/secret.pdf",
            "/neomailbox",
            "/drive/../api/method/x",
            "/drive/%2e%2e/api/method/x",
            "/drive/f/a b",
            "/drive/f/\u{202e}abc",
            "",
        ] {
            assert_eq!(instance_path(refused, None), None, "{refused:?}");
        }
        assert_eq!(instance_path("https://erp.example.test.elsewhere.example/drive/f/x", origin), None);
        assert_eq!(instance_path("https://erp.example.test//elsewhere.example/x", origin), None);
        assert_eq!(instance_path(&format!("/drive/{}", "a".repeat(MAX_LINK_CHARS)), None), None);
    }
}
