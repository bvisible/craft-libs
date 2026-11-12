// //// Neoffice — added file (no upstream equivalent)
//! Suggestions for a [`crate::ProofLang::Pack`] language: the shared ranking ([`crate::affix_suggest`]) with the
//! dictionary's own tables, plus what the dictionary makes possible without a language's own rules:
//!
//! - a first letter is seldom the wrong one, so a candidate that changes it ranks after those that fix the rest
//!   (`rendez-vou` is `rendez-vous`, not `rendez-cou`);
//! - a missing apostrophe in front of a word that needs a correction too (`dacord` is `d'accord`, `lecole` is `l'école`),
//!   where an elision is a prefix the dictionary's rules add that ends with an apostrophe (French `l'`, `d'`, `qu'`, Italian
//!   `dell'`); the shared ranking offers the elision of a word that is right as it stands;
//! - the wrong part of a hyphenated word (`parle-t-ill` is `parle-t-il`), the hyphenated words the dictionary breaks at the
//!   hyphen being right when each part is ([`crate::pack_lang::is_correct`]).

use crate::affix::AffixDict;
use crate::affix_suggest::{COST_MISTYPE, MAX_LETTERS, Tuning, suggest_in};

/// The shared ranking with the first letter dearer to change.
const TUNING: Tuning = Tuning { mistype_cost, ..Tuning::DEFAULT };

fn mistype_cost(_: &[char], at: usize, _: char) -> u32 {
    if at == 0 { COST_MISTYPE + 2 } else { COST_MISTYPE }
}

/// Longest elided first part tried, in letters (`quoiqu'`, `lorsqu'`, `quell'`).
const MAX_ELIDED: usize = 7;

/// Spelling suggestions for `word` against `d`, best first, at most `max`, in the word's letter case.
pub(crate) fn suggest(d: &AffixDict, word: &str, max: usize) -> Vec<String> {
    let word = word.trim();
    if word.is_empty() || max == 0 || d.is_empty() || word.chars().count() > MAX_LETTERS || word.contains(char::is_whitespace) {
        return Vec::new();
    }
    // A dictionary that reads a typographic apostrophe as a straight one (`ICONV`) takes the word so, and shows what it suggests as its `OCONV`
    // writes it; one that doesn't keeps the apostrophe the writer typed, which is a letter like another to it.
    let curly = d.reads_typographic_apostrophe();
    let word = if curly { word.replace('’', "'") } else { word.to_string() };
    let same = |a: &str, b: &str| a == b || (curly && a.replace('’', "'") == b.replace('’', "'"));
    let mut out: Vec<String> = Vec::new();
    // A candidate made here is shown through `OCONV`; the shared ranking's suggestions were already.
    let add = |out: &mut Vec<String>, s: String, made_here: bool| {
        let s = if made_here { d.output(&s).into_owned() } else { s };
        if !out.iter().any(|o| same(o, &s)) {
            out.push(s);
        }
    };
    // The wrong part of a hyphenated word comes first: the shared ranking has no idea of it.
    if word.contains('-') {
        for s in hyphenated(d, &word, max) {
            add(&mut out, s, true);
        }
    }
    // The elision of a word that needs a correction too ranks after the two best of the shared ranking.
    let mut guesses = if word.contains(['\'', '-']) { Vec::new() } else { elided(d, &word) };
    for (i, s) in suggest_in(d, &word, max, &TUNING).into_iter().enumerate() {
        if i == 2 {
            for g in std::mem::take(&mut guesses) {
                add(&mut out, g, true);
            }
        }
        add(&mut out, s, false);
    }
    for g in guesses {
        add(&mut out, g, true);
    }
    out.retain(|s| !same(s, &word));
    out.truncate(max);
    out
}

/// The elisions of a word that also needs a correction (`d'accord` for `dacord`: `acord` is not a word, its first suggestion is).
fn elided(d: &AffixDict, word: &str) -> Vec<String> {
    let lower = word.to_lowercase();
    let mut out = Vec::new();
    for head in d.elisions().into_iter().filter(|h| h.chars().count() <= MAX_ELIDED) {
        let n = head.chars().count();
        if !lower.starts_with(head.as_str()) || word.chars().count() <= n {
            continue;
        }
        let split = word.char_indices().nth(n).map_or(word.len(), |(i, _)| i);
        let (Some(h), Some(rest)) = (word.get(..split), word.get(split..)) else { continue };
        if rest.chars().count() < 3 || d.can_suggest(&format!("{h}'{rest}")) {
            continue;
        }
        for s in suggest_in(d, rest, 2, &TUNING) {
            let candidate = format!("{h}'{}", s.replace('’', "'"));
            if !s.contains(' ') && d.can_suggest(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

/// A hyphenated word with one wrong part: the part is corrected and the word put together again, if the dictionary takes it.
fn hyphenated(d: &AffixDict, word: &str, max: usize) -> Vec<String> {
    let parts: Vec<&str> = word.split('-').collect();
    let wrong: Vec<usize> = parts.iter().enumerate().filter(|(_, p)| p.chars().count() > 1 && !d.check(p)).map(|(i, _)| i).collect();
    let [k] = wrong.as_slice() else { return Vec::new() };
    let Some(part) = parts.get(*k) else { return Vec::new() };
    let mut out = Vec::new();
    for s in suggest_in(d, part, max, &TUNING) {
        let s = s.replace('’', "'");
        let rebuilt: Vec<&str> = parts.iter().enumerate().map(|(i, p)| if i == *k { s.as_str() } else { p }).collect();
        let joined = rebuilt.join("-");
        if !s.contains(' ') && (d.can_suggest(&joined) || rebuilt.iter().all(|p| p.chars().count() <= 1 || d.check(p))) {
            out.push(joined);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> AffixDict {
        let aff = "SET UTF-8\nTRY abcdefghijklmnopqrstuvwxyzé\nICONV 1\nICONV ’ '\nOCONV 1\nOCONV ' ’\nMAP 1\nMAP eé\nPFX D Y 1\nPFX D 0 d' [aeiouéh]\nPFX L Y 1\nPFX L 0 l' [aeiouéh]\nSFX S Y 1\nSFX S 0 s .\n";
        AffixDict::from_hunspell(aff, "12\naccord/D\nécole/DLS\nil\nparle\nt\nvous/S\ncous\ntous\nbout\nest\nrendez\nmidi\n", &[]).unwrap()
    }

    #[test]
    fn a_missing_apostrophe_and_a_correction_are_both_made() {
        let d = dict();
        for (wrong, right) in [("dacord", "d’accord"), ("lecole", "l’école"), ("Dacord", "D’accord")] {
            assert!(suggest(&d, wrong, 3).contains(&right.to_string()), "{wrong}: {:?}", suggest(&d, wrong, 3));
        }
        // A word that has its apostrophe or its hyphen is not elided again.
        let again = suggest(&d, "d'acord", 4);
        assert!(again.contains(&"d’accord".to_string()) && again.iter().all(|s| s.matches('’').count() <= 1), "{again:?}");
    }

    #[test]
    fn the_wrong_part_of_a_hyphenated_word_is_corrected() {
        let d = dict();
        assert!(suggest(&d, "il-t-parlle", 3).iter().any(|s| s == "il-t-parle"), "{:?}", suggest(&d, "il-t-parlle", 3));
        assert!(suggest(&d, "rendez-vou", 3).iter().any(|s| s == "rendez-vous"), "{:?}", suggest(&d, "rendez-vou", 3));
        // Two wrong parts are left to the shared ranking.
        assert!(hyphenated(&d, "rendzz-vou", 3).is_empty());
    }

    #[test]
    fn the_first_letter_is_the_last_to_change() {
        let d = dict();
        let found = suggest(&d, "bous", 5);
        let (bout, tous) = (found.iter().position(|s| s == "bout"), found.iter().position(|s| s == "tous"));
        assert!(bout.is_some() && tous.is_some() && bout < tous, "{found:?}");
    }

    #[test]
    fn nothing_for_what_cannot_be_suggested() {
        let d = dict();
        for w in ["", " ", "deux mots", &"é".repeat(60)] {
            assert!(suggest(&d, w, 8).is_empty(), "{w:?}");
        }
        assert!(suggest(&d, "vouz", 0).is_empty());
        assert!(suggest(&AffixDict::default(), "vouz", 8).is_empty());
        assert!(!suggest(&d, "vouz", 8).contains(&"vouz".to_string()));
    }
}
