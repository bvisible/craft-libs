// //// Neoffice — added file (no upstream equivalent): the page context an editor sends to Nora.

use serde::{Deserialize, Serialize};

/// Which editor the conversation happens in. The vocabulary of document commands is the editor's
/// own (Word has hundreds of them, the spreadsheet has `set_cell`), so Nora needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Word,
    Sheet,
    Deck,
    Pdf,
    /// The mail program (NeoMail): its "document" is the message or the conversation being read and
    /// the message being written, and nothing Nora does there is sent.
    Mail,
}

/// The selected text, when the editor has one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub text: String,
}

/// The open document, as far as Nora needs to know it to answer. Never the whole content: Nora
/// reads that on demand with `doc_observe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub name: String,
    /// The Drive file id, when the document comes from the Drive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
}

/// The longest file id a host may name ([`DocumentInfo::file_id`]).
pub const MAX_FILE_ID_CHARS: usize = 140;

impl DocumentInfo {
    /// A document known by its name only.
    pub fn named(name: impl Into<String>) -> Self {
        DocumentInfo { name: name.into(), id: None, pages: None, selection: None }
    }

    /// The id of the file a document is, as the page that hosts the editor names it (the Drive sends its
    /// file's id with the file): kept when it reads as one, 1 to [`MAX_FILE_ID_CHARS`] ASCII letters, digits,
    /// `-`, `_` or `.` once trimmed; `None` for anything else, which the editor then does not pass on. The
    /// server never believes an id as it comes: it reads it again under the person's own rights.
    pub fn file_id(raw: &str) -> Option<String> {
        let id = raw.trim();
        let well_formed =
            !id.is_empty() && id.len() <= MAX_FILE_ID_CHARS && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        well_formed.then(|| id.to_string())
    }
}

/// What Nora is told about where the user is, sent with every message. Nora stores it for the
/// turn and reads it through its `get_page_context` tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageContext {
    /// Always `"editor"` here, to tell an editor from a desk page.
    pub surface: String,
    pub app: AppKind,
    pub document: DocumentInfo,
}

impl PageContext {
    /// The context of an editor showing `document`.
    pub fn editor(app: AppKind, document: DocumentInfo) -> Self {
        PageContext { surface: "editor".into(), app, document }
    }

    /// The JSON text Nora expects in the `context` parameter (a string, not an object).
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn context_has_the_shape_nora_reads() {
        let mut document = DocumentInfo::named("devis.docx");
        document.pages = Some(2);
        document.selection = Some(Selection { text: "Total".into() });
        let ctx = PageContext::editor(AppKind::Word, document);
        let value: Value = serde_json::from_str(&ctx.to_json_string()).unwrap();
        assert_eq!(
            value,
            json!({"surface": "editor", "app": "word", "document": {"name": "devis.docx", "pages": 2, "selection": {"text": "Total"}}})
        );
    }

    #[test]
    fn empty_optional_fields_are_left_out() {
        let ctx = PageContext::editor(AppKind::Pdf, DocumentInfo::named("a.pdf"));
        let value: Value = serde_json::from_str(&ctx.to_json_string()).unwrap();
        assert_eq!(value, json!({"surface": "editor", "app": "pdf", "document": {"name": "a.pdf"}}));
    }

    #[test]
    fn a_file_id_is_kept_only_when_it_reads_as_one() {
        assert_eq!(DocumentInfo::file_id("3fcaa7893c"), Some("3fcaa7893c".to_string()));
        assert_eq!(DocumentInfo::file_id("  a1-b_2.c  "), Some("a1-b_2.c".to_string()));
        let too_long = "a".repeat(MAX_FILE_ID_CHARS + 1);
        for junk in ["", "   ", "a b", "../x", "a/b", "x\u{0}", "é1", "<script>", too_long.as_str()] {
            assert_eq!(DocumentInfo::file_id(junk), None, "{junk:?}");
        }
        assert!(DocumentInfo::file_id(&"a".repeat(MAX_FILE_ID_CHARS)).is_some());
        let mut document = DocumentInfo::named("devis.xlsx");
        document.id = DocumentInfo::file_id("3fcaa7893c");
        let value: Value = serde_json::from_str(&PageContext::editor(AppKind::Sheet, document).to_json_string()).unwrap();
        assert_eq!(value["document"], json!({"name": "devis.xlsx", "id": "3fcaa7893c"}));
    }

    #[test]
    fn every_app_name_round_trips() {
        for (kind, name) in
            [(AppKind::Word, "word"), (AppKind::Sheet, "sheet"), (AppKind::Deck, "deck"), (AppKind::Pdf, "pdf"), (AppKind::Mail, "mail")]
        {
            let text = serde_json::to_string(&kind).unwrap();
            assert_eq!(text, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<AppKind>(&text).unwrap(), kind);
        }
    }
}
