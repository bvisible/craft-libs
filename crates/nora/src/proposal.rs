// //// Neoffice — added file (no upstream equivalent): replies Nora prepared, with fields where she did not know.

//! A proposal: replies Nora prepared for the person to send, with fields to fill where she could not know what goes
//! there (a date, an amount, a delay). NeoMail shows the ready replies to an email opened this way (Nora's
//! `nora.api.mail_ready.ready_replies`), and an editor can show a letter to complete the same way.
//!
//! Nothing here sends anything or reads a clock. The server's answer is read as warily as anything from outside: at
//! most [`MAX_SHORT`] short replies and [`MAX_FIELDS`] fields, labels and texts cut to their limits, control and hiding
//! characters dropped, a piece that names a field that does not exist dropped.
//!
//! A field left empty is written into the text as a marker, `⟦Label⟧` ([`marker`]): visible, unlikely in a real
//! message, and found again before sending ([`unfilled`]), so that an email never goes out with a field still in it.
//!
//! In the vocabulary of `craft-ai-core`, a proposal is the output of one job of the app's role
//! [`READY_REPLIES_ROLE`], on the capability `text.complete`, which Nora serves.

use serde_json::Value;

/// The app's role whose job gives a proposal (`craft-ai-core`'s `RoleId`): ready replies to a message.
pub const READY_REPLIES_ROLE: &str = "mail.ready_replies";
/// The capability that role needs (`craft-ai-core`'s `CapabilityId`).
pub const READY_REPLIES_CAPABILITY: &str = "text.complete";
/// The most short replies kept.
pub const MAX_SHORT: usize = 2;
/// The most fields kept; a piece naming another is dropped.
pub const MAX_FIELDS: usize = 12;
/// The longest label of a reply or a field, in characters.
pub const MAX_LABEL_CHARS: usize = 60;
/// The longest text of one reply, in characters.
pub const MAX_TEXT_CHARS: usize = 20_000;
/// The longest value the person types in a field, in characters.
pub const MAX_VALUE_CHARS: usize = 200;
/// What opens and closes an empty field in a text.
pub const OPEN: char = '⟦';
pub const CLOSE: char = '⟧';

/// What a field holds, which says how it is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Date,
    Time,
    Amount,
    Number,
    Text,
}

impl FieldKind {
    /// The kind the server names (`date`, `time`, `amount`, `number`; anything else is text).
    pub fn from_code(code: &str) -> Self {
        match code.trim() {
            "date" => FieldKind::Date,
            "time" => FieldKind::Time,
            "amount" => FieldKind::Amount,
            "number" => FieldKind::Number,
            _ => FieldKind::Text,
        }
    }
}

/// A field to fill: what it is called (in the reply's language) and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: String,
    pub kind: FieldKind,
}

/// A piece of a reply: words, or the field with this number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Field(usize),
}

/// One reply: the draft (no label), or a short reply (its label says what it does).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    pub label: String,
    pub pieces: Vec<Piece>,
}

impl Reply {
    /// The fields this reply holds, in order, each once.
    pub fn fields(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for piece in &self.pieces {
            if let Piece::Field(index) = piece
                && !out.contains(index)
            {
                out.push(*index);
            }
        }
        out
    }
}

/// The replies Nora prepared to one message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Proposal {
    /// The language of the replies (`fr`, `de`, `it`, `en`; empty when the server did not say).
    pub language: String,
    pub draft: Reply,
    pub short: Vec<Reply>,
    /// Shared by the draft and the short replies: a field filled once is filled everywhere.
    pub fields: Vec<Field>,
}

impl Proposal {
    /// The fields of `reply` that `values` leaves empty.
    pub fn missing(&self, reply: &Reply, values: &[String]) -> Vec<usize> {
        reply.fields().into_iter().filter(|index| values.get(*index).is_none_or(|v| v.trim().is_empty())).collect()
    }

    /// `reply` as text, each field its value, or its [`marker`] when empty.
    pub fn text(&self, reply: &Reply, values: &[String]) -> String {
        let mut out = String::new();
        for piece in &reply.pieces {
            match piece {
                Piece::Text(text) => out.push_str(text),
                Piece::Field(index) => match values.get(*index).map(|v| v.trim()).filter(|v| !v.is_empty()) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&marker(self.fields.get(*index).map_or("", |f| f.label.as_str()))),
                },
            }
        }
        out
    }
}

/// The server's answer for one message: a proposal, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    Proposal(Proposal),
    /// The person has not said yes to ready replies yet: the card that asks.
    ConsentNeeded,
    /// They said « not now » lately: nothing.
    Declined,
    /// They turned them off: nothing.
    Disabled,
    /// None wanted for this message (their own, answered, a notification, a newsletter, empty).
    Skip(String),
    /// The guard held the message back: what it said.
    Blocked(String),
}

/// Whether the person's style is learnt, or being learnt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StyleState {
    pub learnt: bool,
    pub learning: bool,
}

/// An empty field as it is written into a text: `⟦Label⟧`.
pub fn marker(label: &str) -> String {
    format!("{OPEN}{label}{CLOSE}")
}

/// The labels of the empty fields still in `text` (each [`marker`] in it), in order.
pub fn unfilled(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(OPEN) {
        let after = &rest[open + OPEN.len_utf8()..];
        // A label is one line, and never holds another opening.
        let Some(close) = after.find([CLOSE, OPEN, '\n']) else { break };
        if after[close..].starts_with(CLOSE) {
            out.push(after[..close].trim().to_string());
            rest = &after[close + CLOSE.len_utf8()..];
        } else {
            rest = &after[close..];
        }
    }
    out
}

/// `text` without control characters (but line breaks and tabs) and without the characters that hide or reorder
/// text, cut to `max` characters.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| matches!(c, '\n' | '\t') || !(c.is_control() || hides(*c)))
        .map(|c| if c == OPEN || c == CLOSE { '"' } else { c })
        .take(max)
        .collect()
}

/// A character that hides text or changes its direction: the zero-width ones, the direction marks, embeddings,
/// overrides and isolates, the byte-order mark, the soft hyphen.
fn hides(c: char) -> bool {
    matches!(u32::from(c), 0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFEFF)
}

fn label(value: Option<&Value>) -> String {
    let text = clean(value.and_then(Value::as_str).unwrap_or(""), MAX_LABEL_CHARS * 2);
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_LABEL_CHARS).collect()
}

fn reply(label_text: String, segments: Option<&Value>, fields: usize) -> Reply {
    let mut pieces = Vec::new();
    let mut used = 0;
    for segment in segments.and_then(Value::as_array).into_iter().flatten() {
        if let Some(text) = segment.get("t").and_then(Value::as_str) {
            let text = clean(text, MAX_TEXT_CHARS.saturating_sub(used));
            used += text.chars().count();
            if !text.is_empty() {
                pieces.push(Piece::Text(text));
            }
        } else if let Some(index) = segment.get("f").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok())
            && index < fields
        {
            pieces.push(Piece::Field(index));
        }
    }
    Reply { label: label_text, pieces }
}

/// The server's answer (`nora.api.mail_ready.ready_replies`, unwrapped of Frappe's `message`), read.
pub fn parse_ready(answer: &Value) -> Result<(Ready, StyleState), String> {
    if !answer.is_object() {
        return Err("Nora's answer is not an object".to_string());
    }
    let style = StyleState {
        learnt: answer.pointer("/style/learnt").and_then(Value::as_bool).unwrap_or(false),
        learning: answer.pointer("/style/learning").and_then(Value::as_bool).unwrap_or(false),
    };
    let yes = |key: &str| answer.get(key).and_then(Value::as_bool).unwrap_or(false);
    if yes("consent_needed") {
        return Ok((Ready::ConsentNeeded, style));
    }
    if yes("declined") {
        return Ok((Ready::Declined, style));
    }
    if yes("disabled") {
        return Ok((Ready::Disabled, style));
    }
    if yes("blocked") {
        let said = clean(answer.get("message").and_then(Value::as_str).unwrap_or(""), 400);
        return Ok((Ready::Blocked(said), style));
    }
    if let Some(reason) = answer.get("skip").and_then(Value::as_str) {
        return Ok((Ready::Skip(clean(reason, 40)), style));
    }
    let fields: Vec<Field> = answer
        .get("fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_FIELDS)
        .map(|f| Field { label: label(f.get("label")), kind: FieldKind::from_code(f.get("kind").and_then(Value::as_str).unwrap_or("")) })
        .collect();
    let draft = reply(String::new(), answer.pointer("/draft/segments"), fields.len());
    if draft.pieces.is_empty() {
        return Err("Nora's answer holds no draft".to_string());
    }
    let short = answer
        .get("short_replies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|s| reply(label(s.get("label")), s.get("segments"), fields.len()))
        .filter(|r| !r.label.is_empty() && !r.pieces.is_empty())
        .take(MAX_SHORT)
        .collect();
    let language: String = answer.get("language").and_then(Value::as_str).unwrap_or("").chars().filter(char::is_ascii_lowercase).take(2).collect();
    Ok((Ready::Proposal(Proposal { language, draft, short, fields }), style))
}

/// A date in words, as a reply in `language` writes it: « vendredi 30 octobre », « Freitag, 30. Oktober »,
/// « venerdì 30 ottobre », « Friday 30 October ». `weekday` counts from Monday (0) to Sunday (6), `month` from 1.
pub fn date_words(language: &str, weekday: u32, day: u32, month: u32) -> String {
    const FR_DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
    const FR_MONTHS: [&str; 12] =
        ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    const DE_DAYS: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];
    const DE_MONTHS: [&str; 12] =
        ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
    const IT_DAYS: [&str; 7] = ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"];
    const IT_MONTHS: [&str; 12] =
        ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
    const EN_DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    const EN_MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let w = (weekday % 7) as usize;
    let m = (month.clamp(1, 12) - 1) as usize;
    match language {
        "de" => format!("{}, {day}. {}", DE_DAYS[w], DE_MONTHS[m]),
        "it" => format!("{} {day} {}", IT_DAYS[w], IT_MONTHS[m]),
        "en" => format!("{} {day} {}", EN_DAYS[w], EN_MONTHS[m]),
        // The day's first: « 1er » in French.
        _ if day == 1 => format!("{} 1er {}", FR_DAYS[w], FR_MONTHS[m]),
        _ => format!("{} {day} {}", FR_DAYS[w], FR_MONTHS[m]),
    }
}

/// An amount the person typed, written the Swiss way with its currency: « 1250 » → « 1'250.00 CHF ». `None` when it is
/// not a number (it is then written as typed). A currency typed with it (CHF, EUR, USD, Fr.) is kept.
pub fn amount_words(typed: &str) -> Option<String> {
    let text = typed.trim();
    let currency = ["CHF", "EUR", "USD", "Fr.", "€", "$"]
        .into_iter()
        .find(|c| text.to_ascii_uppercase().starts_with(&c.to_ascii_uppercase()) || text.to_ascii_uppercase().ends_with(&c.to_ascii_uppercase()))
        .unwrap_or("CHF");
    let digits: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')).collect();
    // « 1'250,50 » and « 1250.50 »: the last separator before two digits is the decimal one.
    let normalised = match digits.rfind([',', '.']) {
        Some(at) if digits.len() - at - 1 <= 2 => format!("{}.{}", digits[..at].replace([',', '.'], ""), &digits[at + 1..]),
        _ => digits.replace([',', '.'], ""),
    };
    let value: f64 = normalised.parse().ok()?;
    if !value.is_finite() || value.abs() >= 1e15 {
        return None;
    }
    let cents = (value.abs() * 100.0).round() as u64;
    let (units, fraction) = (cents / 100, cents % 100);
    let digits = units.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('\'');
        }
        grouped.push(c);
    }
    let sign = if value < 0.0 { "-" } else { "" };
    Some(format!("{sign}{grouped}.{fraction:02} {currency}"))
}

#[cfg(test)]
#[path = "proposal_tests.rs"]
mod tests;
