// //// Neoffice — added file (no upstream equivalent): tests of the proposals Nora prepares.

use serde_json::json;

use super::*;

fn answer() -> Value {
    json!({
        "success": true,
        "language": "fr",
        "draft": {"segments": [
            {"t": "Bonjour Claire,\n\nLa mise en service est prévue le "}, {"f": 0},
            {"t": ". Je vous transmets le devis d'ici "}, {"f": 1}, {"t": ".\n\nMeilleures salutations"}
        ]},
        "short_replies": [
            {"label": "Confirmer la date", "segments": [{"t": "C'est prévu le "}, {"f": 0}, {"t": "."}]},
            {"label": "Proposer un appel", "segments": [{"t": "Pouvons-nous en parler "}, {"f": 2}, {"t": " ?"}]},
            {"label": "Une troisième", "segments": [{"t": "Jamais montrée."}]}
        ],
        "fields": [
            {"label": "Date de mise en service", "kind": "date"},
            {"label": "Date d'envoi du devis", "kind": "date"},
            {"label": "Moment de l'appel", "kind": "text"}
        ],
        "style": {"learnt": true, "learning": false}
    })
}

fn proposal() -> Proposal {
    match parse_ready(&answer()) {
        Ok((Ready::Proposal(p), _)) => p,
        other => panic!("not a proposal: {other:?}"),
    }
}

#[test]
fn an_answer_is_read_into_a_proposal() {
    let (ready, style) = parse_ready(&answer()).unwrap();
    assert_eq!(style, StyleState { learnt: true, learning: false });
    let Ready::Proposal(p) = ready else { panic!("not a proposal") };
    assert_eq!(p.language, "fr");
    assert_eq!(p.short.len(), MAX_SHORT, "two short replies at most");
    assert_eq!(p.short[1].label, "Proposer un appel");
    assert_eq!(p.fields[0], Field { label: "Date de mise en service".into(), kind: FieldKind::Date });
    assert_eq!(p.draft.fields(), vec![0, 1]);
}

#[test]
fn a_field_filled_once_is_filled_everywhere_and_an_empty_one_is_a_marker() {
    let p = proposal();
    let values = vec!["vendredi 6 novembre".to_string(), String::new(), String::new()];
    assert_eq!(p.missing(&p.draft, &values), vec![1]);
    assert!(p.missing(&p.short[0], &values).is_empty());
    let text = p.text(&p.draft, &values);
    assert!(text.contains("prévue le vendredi 6 novembre."));
    assert!(text.contains("d'ici ⟦Date d'envoi du devis⟧."));
    assert_eq!(p.text(&p.short[0], &values), "C'est prévu le vendredi 6 novembre.");
    assert_eq!(unfilled(&text), vec!["Date d'envoi du devis".to_string()]);
}

#[test]
fn the_reasons_for_no_proposal() {
    let read = |v: Value| parse_ready(&v).unwrap().0;
    assert_eq!(read(json!({"success": true, "consent_needed": true})), Ready::ConsentNeeded);
    assert_eq!(read(json!({"success": true, "declined": true})), Ready::Declined);
    assert_eq!(read(json!({"success": true, "disabled": true})), Ready::Disabled);
    assert_eq!(read(json!({"success": true, "skip": "newsletter"})), Ready::Skip("newsletter".into()));
    assert_eq!(read(json!({"success": true, "blocked": true, "message": "Nora refused."})), Ready::Blocked("Nora refused.".into()));
    assert!(parse_ready(&json!({"success": true, "draft": {"segments": []}})).is_err(), "no draft is no proposal");
    assert!(parse_ready(&json!("text")).is_err());
}

#[test]
fn what_the_server_says_is_read_warily() {
    let mut a = answer();
    a["fields"][0]["label"] = json!("Date\u{202E}gnol ⟦x⟧ ".repeat(20));
    a["draft"]["segments"] = json!([{"t": "Bonjour\u{200B} ⟦Fake⟧"}, {"f": 7}, {"f": "0"}, {"t": "\u{0007}fin"}]);
    let Ok((Ready::Proposal(p), _)) = parse_ready(&a) else { panic!("not a proposal") };
    assert!(p.fields[0].label.chars().count() <= MAX_LABEL_CHARS);
    assert!(!p.fields[0].label.contains('\u{202E}') && !p.fields[0].label.contains(OPEN));
    assert_eq!(p.draft.pieces, vec![Piece::Text("Bonjour \"Fake\"".into()), Piece::Text("fin".into())]);
    assert!(unfilled(&p.text(&p.draft, &[])).is_empty(), "the server cannot write a marker of its own");
}

#[test]
fn unfilled_finds_only_whole_markers() {
    assert_eq!(unfilled("a ⟦Montant⟧ b ⟦ Date ⟧"), vec!["Montant".to_string(), "Date".to_string()]);
    assert!(unfilled("a ⟦ open\nnext⟧ line").is_empty());
    assert_eq!(unfilled("⟦a ⟦b⟧"), vec!["b".to_string()]);
    assert!(unfilled("no field here").is_empty());
}

#[test]
fn a_date_in_words_in_the_reply_s_language() {
    assert_eq!(date_words("fr", 4, 30, 10), "vendredi 30 octobre");
    assert_eq!(date_words("fr", 6, 1, 11), "dimanche 1er novembre");
    assert_eq!(date_words("de", 4, 30, 10), "Freitag, 30. Oktober");
    assert_eq!(date_words("it", 4, 30, 10), "venerdì 30 ottobre");
    assert_eq!(date_words("en", 4, 30, 10), "Friday 30 October");
    assert_eq!(date_words("", 0, 2, 13), "lundi 2 décembre", "French, and a month past December held");
}

#[test]
fn an_amount_the_swiss_way() {
    assert_eq!(amount_words("1250").as_deref(), Some("1'250.00 CHF"));
    assert_eq!(amount_words("1'250.5").as_deref(), Some("1'250.50 CHF"));
    assert_eq!(amount_words("1250,50").as_deref(), Some("1'250.50 CHF"));
    assert_eq!(amount_words("1.250").as_deref(), Some("1'250.00 CHF"));
    assert_eq!(amount_words("EUR 980").as_deref(), Some("980.00 EUR"));
    assert_eq!(amount_words("1234567.891").as_deref(), Some("1'234'567'891.00 CHF"));
    assert_eq!(amount_words("à voir"), None);
}
