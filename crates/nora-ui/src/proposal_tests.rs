// //// Neoffice — added file (no upstream equivalent): tests of the card of a proposal of Nora's.

use craft_nora::proposal::{Field, FieldKind, Piece, Proposal, Reply};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::*;

struct State {
    card: ProposalCard,
    proposal: Proposal,
    actions: Vec<ProposalAction>,
}

fn proposal() -> Proposal {
    let text = |t: &str| Piece::Text(t.to_string());
    Proposal {
        language: "en".into(),
        draft: Reply {
            label: String::new(),
            pieces: vec![text("Dear Claire,\n\nIt goes live on "), Piece::Field(0), text(", for "), Piece::Field(1), text(".\n\nBest regards")],
        },
        short: vec![Reply { label: "Confirm the date".into(), pieces: vec![text("Confirmed for "), Piece::Field(0), text(".")] }],
        fields: vec![Field { label: "Delivery date".into(), kind: FieldKind::Date }, Field { label: "Amount".into(), kind: FieldKind::Amount }],
    }
}

fn harness(step: f32) -> Harness<'static, State> {
    Harness::builder().with_size(egui::vec2(720.0, 620.0)).with_step_dt(step).build_ui_state(
        |ui, state: &mut State| {
            let words = ProposalWords::default();
            let tone = ProposalTone::from_visuals(ui.visuals());
            let choices = |kind: FieldKind| {
                if kind == FieldKind::Date { vec![Choice { shown: "Tomorrow".into(), value: "Friday 30 October".into() }] } else { Vec::new() }
            };
            let context = Context { to: "claire@example.com", choices: &choices, learning: false };
            let actions = state.card.show(ui, &state.proposal, &words, &tone, &context);
            state.actions.extend(actions);
        },
        State { card: ProposalCard::default(), proposal: proposal(), actions: Vec::new() },
    )
}

#[test]
fn send_stays_off_while_a_field_is_empty() {
    let mut h = harness(0.05);
    h.run();
    assert!(h.get_by_label("Send").accesskit_node().is_disabled());
    assert!(h.query_by_label("2 fields to fill in").is_some());
    assert!(h.query_by_label("Delivery date").is_some(), "an empty field says what goes there");
}

#[test]
fn a_pill_opens_its_field_a_shortcut_fills_it_and_the_next_one_opens() {
    let mut h = harness(0.05);
    h.run();
    h.get_by_label("Delivery date").click();
    h.run();
    h.get_by_label("Tomorrow").click();
    h.run();
    assert_eq!(h.state().card.values[0], "Friday 30 October");
    assert_eq!(h.state().card.open, Some(1), "the next empty field of the reply");
    assert!(h.query_by_label("1 field to fill in").is_some());
}

#[test]
fn an_amount_typed_is_kept_the_swiss_way_and_send_comes_on() {
    let mut h = harness(0.05);
    h.state_mut().card.values = vec!["Friday 30 October".into(), String::new()];
    h.state_mut().card.open_field(1);
    h.run();
    h.get_by_role(egui::accesskit::Role::TextInput).type_text("1250");
    h.run();
    h.get_by_label("OK").click();
    h.run();
    assert_eq!(h.state().card.values[1], "1'250.00 CHF");
    assert_eq!(h.state().card.open, None, "no field left to fill");
    assert!(!h.get_by_label("Send").accesskit_node().is_disabled());
}

#[test]
fn edit_hands_the_text_over_with_a_marker_for_an_empty_field() {
    let mut h = harness(0.05);
    h.state_mut().card.values = vec!["Friday 30 October".into(), String::new()];
    h.run();
    h.get_by_label("Edit").click();
    h.run();
    let edits: Vec<_> = h.state().actions.iter().filter_map(|a| if let ProposalAction::Edit(t) = a { Some(t.clone()) } else { None }).collect();
    assert_eq!(edits.len(), 1);
    assert!(edits[0].contains("It goes live on Friday 30 October, for ⟦Amount⟧."));
}

#[test]
fn a_send_waits_five_seconds_and_can_be_taken_back() {
    let mut h = harness(0.5);
    h.state_mut().card.values = vec!["Friday 30 October".into(), "1'250.00 CHF".into()];
    h.run();
    h.get_by_label("Send").click();
    h.run_steps(4);
    assert!(h.state().card.sending() && h.state().actions.is_empty(), "nothing goes at once");
    h.get_by_label("Cancel").click();
    h.run_steps(16);
    assert!(!h.state().card.sending() && h.state().actions.is_empty(), "taken back: nothing goes");

    h.get_by_label("Send").click();
    h.run_steps(16);
    let sent: Vec<_> = h.state().actions.iter().filter_map(|a| if let ProposalAction::Send(t) = a { Some(t.clone()) } else { None }).collect();
    assert_eq!(sent.len(), 1, "once");
    assert_eq!(sent[0], "Dear Claire,\n\nIt goes live on Friday 30 October, for 1'250.00 CHF.\n\nBest regards");
}

#[test]
fn a_short_reply_takes_the_draft_s_place_and_shares_its_fields() {
    let mut h = harness(0.05);
    h.state_mut().card.values = vec!["Friday 30 October".into(), String::new()];
    h.run();
    h.get_by_label("« Confirm the date »").click();
    h.run();
    assert_eq!(h.state().card.chosen, Some(0));
    assert!(h.query_by_label_contains("Confirmed for").is_some());
    assert!(!h.get_by_label("Send").accesskit_node().is_disabled(), "its one field is filled already");
}

#[test]
fn lines_split_at_line_breaks_and_keep_fields_in_place() {
    let reply = proposal().draft;
    let lines = lines(&reply);
    assert_eq!(lines.len(), 5, "« Dear Claire, », an empty line, the sentence, an empty line, « Best regards »");
    assert!(lines[1].is_empty() && lines[3].is_empty());
    assert_eq!(lines[2].iter().filter(|p| matches!(p, Piece::Field(_))).count(), 2);
}

#[test]
fn the_consent_card_says_yes_or_not_now() {
    let mut h = Harness::builder().with_size(egui::vec2(560.0, 260.0)).build_ui_state(
        |ui, answer: &mut Option<bool>| {
            let tone = ProposalTone::from_visuals(ui.visuals());
            if let Some(said) = consent(ui, &ConsentWords::default(), &tone) {
                *answer = Some(said);
            }
        },
        None,
    );
    h.run();
    h.get_by_label("Not now").click();
    h.run();
    assert_eq!(*h.state(), Some(false));
    h.get_by_label("Turn on").click();
    h.run();
    assert_eq!(*h.state(), Some(true));
}
