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

impl DocumentInfo {
    /// A document known by its name only.
    pub fn named(name: impl Into<String>) -> Self {
        DocumentInfo { name: name.into(), id: None, pages: None, selection: None }
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
    fn every_app_name_round_trips() {
        for (kind, name) in [(AppKind::Word, "word"), (AppKind::Sheet, "sheet"), (AppKind::Deck, "deck"), (AppKind::Pdf, "pdf")] {
            let text = serde_json::to_string(&kind).unwrap();
            assert_eq!(text, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<AppKind>(&text).unwrap(), kind);
        }
    }
}
