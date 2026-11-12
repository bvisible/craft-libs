// //// Neoffice — added file (no upstream equivalent)
//! Generic repeated-word and spacing checks with French messages, for the French [`crate::ProofLang::Pack`] languages (`fr`,
//! `fr-CH`…). No French-specific grammar, agreement or typography rule is inferred: in French a space before `:`, `;`, `!` and `?`
//! is right (a no-break one is better still), so only the space before a comma or a full stop is reported.

use crate::{Issue, IssueKind, ProofLang, words};

fn issue(lang: ProofLang, start: usize, end: usize, message: String, suggestions: Vec<String>) -> Issue {
    Issue { start, end, kind: IssueKind::Grammar, message, suggestions, lang }
}

/// Words French repeats on purpose (`nous nous levons`, `vous vous trompez`, `très très bien`, `oui oui`, `ah ah`).
const REPEATABLE: &[&str] = &[
    "nous",
    "vous",
    "bien",
    "très",
    "oui",
    "non",
    "si",
    "ça",
    "ha",
    "ah",
    "eh",
    "hé",
    "hop",
    "bye",
    "tic",
    "tac",
    "bla",
    "toc",
    "miam",
    "boum",
    "ding",
    "dong",
    "pan",
    "paf",
    "hi",
    "ho",
    "bip",
    "vroum",
    "chut",
    "allez",
    "vite",
    "doucement",
    "là",
    "ouais",
    "merci",
    "voilà",
    "bravo",
    "ben",
    "bon",
    "bah",
    "ouf",
];

/// The grammar issues of `text`, as French ones of the language `lang`.
pub(crate) fn check(text: &str, lang: ProofLang) -> Vec<Issue> {
    let mut issues = Vec::new();
    repeated_words(text, &words(text), lang, &mut issues);
    spaces(text, lang, &mut issues);
    issues.sort_by_key(|i| (i.start, i.end));
    issues
}

fn repeated_words(text: &str, ws: &[(usize, usize)], lang: ProofLang, v: &mut Vec<Issue>) {
    for pair in ws.windows(2) {
        let [(a0, b0), (a1, b1)] = pair else { continue };
        let (Some(w0), Some(w1), Some(gap)) = (text.get(*a0..*b0), text.get(*a1..*b1), text.get(*b0..*a1)) else { continue };
        let lower = w0.to_lowercase();
        // A line break between two words is no repetition (a list, a table of one column).
        if !gap.is_empty()
            && gap.chars().all(|c| c.is_whitespace() && c != '\n' && c != '\r')
            && w0.chars().all(char::is_alphabetic)
            && lower == w1.to_lowercase()
            && !REPEATABLE.contains(&lower.as_str())
        {
            v.push(issue(lang, *a0, *b1, format!("Mot répété : « {w1} »"), vec![w0.to_string()]));
        }
    }
}

fn spaces(text: &str, lang: ProofLang, v: &mut Vec<Issue>) {
    for (i, c) in text.char_indices() {
        if c != ' ' {
            continue;
        }
        let rest = text.get(i + 1..).unwrap_or("");
        let mut it = rest.chars();
        let Some(p) = it.next() else { continue };
        let after = it.next();
        // ` ...` is an ellipsis, ` .5` a number, ` .pdf` and ` .ch` an extension or a domain; a comma, or a full stop that ends a
        // sentence, never takes a space before it.
        let flag = match p {
            ',' => true,
            '.' => after.is_none_or(|a| a.is_whitespace() || matches!(a, '"' | '”' | '»' | '’' | ')' | ']')),
            _ => false,
        };
        if flag {
            v.push(issue(lang, i, i + 1 + p.len_utf8(), "Espace avant la ponctuation".into(), vec![p.to_string()]));
        }
    }
    // A run of spaces is one issue over the whole run: the fix (one space) leaves no other.
    for run in crate::space_runs(text) {
        v.push(issue(lang, run.start, run.end, "Espace en trop".into(), vec![" ".into()]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn french() -> ProofLang {
        ProofLang::from_tag(Some("fr-CH"))
    }

    #[test]
    fn repeated_words_and_spaces_have_french_messages() {
        let text = "Le le chat dort ,  bien.";
        let issues = check(text, french());
        assert_eq!(issues.len(), 3, "{issues:?}");
        assert_eq!(issues[0].message, "Mot répété : « le »");
        assert_eq!(issues[0].suggestions, ["Le"]);
        assert!(issues.iter().any(|i| i.message == "Espace avant la ponctuation" && &text[i.start..i.end] == " ,"));
        assert!(issues.iter().any(|i| i.message == "Espace en trop"));
        // A space before a full stop that ends the sentence is reported too.
        assert!(check("Il est parti .", french()).iter().any(|i| i.message == "Espace avant la ponctuation"));
        // A run of three spaces is one issue over the three.
        let runs = check("a   b", french());
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!((runs[0].start, runs[0].end), (1, 4));
        // A line break between two words is no repeated word; the idioms French repeats are left alone.
        assert!(check("Total\nTotal", french()).is_empty() && check("Oh là là ! Merci merci, voilà voilà.", french()).is_empty());
        assert!(issues.iter().all(|i| i.lang == french() && i.kind == IssueKind::Grammar));
    }

    #[test]
    fn french_spacing_and_intended_repeats_are_left_alone() {
        for text in [
            "Nous nous levons tôt.",
            "Vous vous trompez !",
            "C'est très très bien.",
            "Quoi ? Pas de réponse : rien ; tant pis !",
            "Il a dit : « oui oui ».",
            "Le chat, le chien et le chat.",
            "Et puis... non. Le prix est de .5 franc.",
            "Enregistrez le fichier au format .pdf puis envoyez-le à l'adresse .ch indiquée.",
            "Toc toc ! Miam miam, c'est prêt.",
        ] {
            assert!(check(text, french()).is_empty(), "{text}: {:?}", check(text, french()));
        }
    }

    /// The paragraph checks reach it through the public entry points, for every French region and no other language, and only
    /// while French has a dictionary (a language without its proofing tools has no marks).
    #[test]
    fn it_is_the_grammar_of_the_french_pack_languages() {
        crate::hold_dictionary_tests();
        crate::embedded::forget_dictionaries();
        let text = "Il a a mangé le le gâteau, nous nous régalons.";
        assert!(crate::check_grammar_in(text, french()).is_empty(), "no dictionary, no marks");
        crate::embedded::embed_dictionary("fr", "Test", b"SET UTF-8\n".to_vec(), b"2\nil\na\n".to_vec(), crate::embedded::Packing::Plain).unwrap();
        for tag in ["fr", "fr-CH", "fr-FR", "fr-CA", "fra"] {
            let lang = ProofLang::from_tag(Some(tag));
            let messages: Vec<String> = crate::check_grammar_in(text, lang).into_iter().map(|i| i.message).collect();
            assert_eq!(messages, ["Mot répété : « a »", "Mot répété : « le »"], "{tag}");
        }
        assert!(crate::check_grammar_in(text, ProofLang::from_tag(Some("nl-NL"))).is_empty());
        crate::embedded::forget_dictionaries();
    }

    proptest::proptest! {
        #[test]
        fn never_panics_and_ranges_are_valid(s in "\\PC{0,80}") {
            for issue in check(&s, french()) {
                proptest::prop_assert!(issue.start <= issue.end && issue.end <= s.len());
                proptest::prop_assert!(s.is_char_boundary(issue.start) && s.is_char_boundary(issue.end));
            }
        }
    }
}
