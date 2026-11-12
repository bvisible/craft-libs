// //// Neoffice — added file (no upstream equivalent)
//! Dictionaries a program carries inside itself, for the languages read through the shared affix engine
//! ([`crate::ProofLang::Pack`], German and Bulgarian).
//!
//! A program that can't read a folder or download a pack (a build that runs in a browser) has no other way to
//! spell a language than to carry its dictionary: the `.aff` and `.dic` bytes, as they are or compressed,
//! handed to [`embed_dictionary`] once at start-up. Nothing is read or parsed until the language is first
//! checked (a document without it never pays), and a dictionary is parsed once however many regions ask for it:
//! `fr-FR`, `fr-CH` and `fr-CA` share the one registered as `fr`.
//!
//! A dictionary found by a locale's tag, then by the tag with its subtags dropped (`fr-ch`, then `fr`), is used
//! after the dictionaries folders the user set ([`crate::set_dictionaries_folder`]: an explicit choice) and before
//! an installed Language pack or the system's Hunspell dictionaries. A folder dictionary that fails to load
//! leaves its reason ([`crate::dictionary_load_error`]) and the embedded one is used meanwhile.
//!
//! English has built-in data of its own ([`crate::english_source`]) and the languages with a dictionary slot of
//! their own (Polish, Italian, Spanish…) have `install_dictionary`: neither reads these.

use std::borrow::Cow;
use std::sync::{Arc, PoisonError, RwLock};

use crate::affix::AffixDict;
use crate::lang::{PackLocale, ProofLang};

/// How the bytes of an embedded dictionary's two files are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packing {
    /// The files as they are (`include_bytes!` of the `.aff` and `.dic`).
    Plain,
    /// Each file a raw DEFLATE stream (RFC 1951, no zlib or gzip header): about a quarter of the size
    /// for a Hunspell dictionary, inflated when the dictionary is first needed.
    Deflate,
}

/// A registered dictionary.
pub(crate) struct Entry {
    /// The lowercase BCP 47 tag it serves (`fr`, `de-ch`).
    tag: String,
    label: String,
    aff: Cow<'static, [u8]>,
    dic: Cow<'static, [u8]>,
    packing: Packing,
}

static REGISTRY: RwLock<Vec<Arc<Entry>>> = RwLock::new(Vec::new());

/// Carry a Hunspell dictionary for `tag` (`fr`, `de-CH`) inside the program: its `.aff` and `.dic` bytes, stored as
/// `packing` says, and what to call it in the program's own words (`label`: `Grammalecte 7.7 (MPL 2.0)`; shown by
/// [`label`]). A language is served by the dictionary registered for its tag, else by the one registered for the
/// tag with its subtags dropped, so `fr` serves every French region. Registering a tag again replaces what was there.
///
/// The bytes are only kept: nothing is inflated or parsed before the first check in the language, and a dictionary
/// that turns out unreadable (not a Hunspell pair, an unsupported directive, too large once inflated) is reported
/// then, like an installed one ([`crate::dictionary_load_error`]), and the language has no spelling marks. Cached
/// dictionaries are forgotten, so this can be called after some were loaded.
///
/// An error here means the registration is unusable: `tag` is no language, or a file is over the size
/// [`crate::hunspell::MAX_FILE`] allows, or the language is not read through the embedded dictionaries (English, and the languages with
/// a dictionary slot of their own: `install_dictionary`), or `tag` is a region the language is never looked up under (`de-AT` is read as
/// German, so register `de`).
pub fn embed_dictionary(
    tag: &str,
    label: &str,
    aff: impl Into<Cow<'static, [u8]>>,
    dic: impl Into<Cow<'static, [u8]>>,
    packing: Packing,
) -> Result<(), String> {
    let locale = PackLocale::new(tag).ok_or_else(|| format!("{tag:?} is not a language tag"))?;
    let lang = ProofLang::from_tag(Some(tag));
    let Some(read_as) = lang.spelling_locale() else {
        return Err(format!(
            "{tag:?} is not a language read through embedded dictionaries (English and the languages with their own `install_dictionary` are not)"
        ));
    };
    if !crate::pack_lang::hyphenation_locales(read_as).iter().any(|t| t == locale.as_str()) {
        return Err(format!("{tag:?} is looked up as {read_as}: register {:?}", read_as.primary()));
    }
    let (aff, dic) = (aff.into(), dic.into());
    for (name, bytes) in [(".aff", &aff), (".dic", &dic)] {
        if bytes.len() as u64 > crate::hunspell::MAX_FILE {
            return Err(format!("the embedded {name} file is too large ({} bytes)", bytes.len()));
        }
    }
    let entry = Arc::new(Entry { tag: locale.as_str().to_string(), label: label.to_string(), aff, dic, packing });
    {
        let mut all = REGISTRY.write().unwrap_or_else(PoisonError::into_inner);
        all.retain(|e| e.tag != entry.tag);
        all.push(entry);
    }
    crate::hunspell::clear_load_errors();
    crate::reload_pack_dictionaries();
    Ok(())
}

/// Forget every embedded dictionary (the tests of this crate).
#[cfg(test)]
pub(crate) fn forget_dictionaries() {
    REGISTRY.write().unwrap_or_else(PoisonError::into_inner).clear();
    crate::hunspell::clear_load_errors();
    crate::reload_pack_dictionaries();
}

/// The embedded dictionary that serves the first of `tags` it is registered for (lowercase, most specific first).
pub(crate) fn find(tags: &[String]) -> Option<Arc<Entry>> {
    let all = REGISTRY.read().unwrap_or_else(PoisonError::into_inner);
    tags.iter().find_map(|t| all.iter().find(|e| e.tag == *t).cloned())
}

/// What the embedded dictionary registered for `lang` is called (the `label` it was registered with), or `None` when none is: English,
/// a language with a dictionary slot of its own, or one nothing was registered for. (A dictionary in a folder the user set is read
/// instead of it, [`crate::pack_dictionary_file`] says whether one is.)
pub fn label(lang: ProofLang) -> Option<String> {
    let locale = lang.spelling_locale()?;
    find(&crate::pack_lang::hyphenation_locales(locale)).map(|e| e.label.clone())
}

impl Entry {
    /// The tag the dictionary was registered for.
    pub(crate) fn tag(&self) -> &str {
        &self.tag
    }

    /// The pseudo file name an error or the status shows for it (an embedded dictionary has no file).
    pub(crate) fn file_name(&self) -> String {
        format!("embedded {} ({})", self.tag, self.label)
    }

    /// The bytes of one of the files as they are, or inflated when the dictionary is stored deflated (at most [`crate::hunspell::MAX_FILE`]).
    fn inflate<'a>(&self, data: &'a [u8], what: &str) -> Result<Cow<'a, [u8]>, String> {
        match self.packing {
            Packing::Plain => Ok(Cow::Borrowed(data)),
            Packing::Deflate => {
                let limit = usize::try_from(crate::hunspell::MAX_FILE).unwrap_or(usize::MAX);
                miniz_oxide::inflate::decompress_to_vec_with_limit(data, limit)
                    .map(Cow::Owned)
                    .map_err(|e| format!("embedded {} {what}: not a DEFLATE stream or too large: {:?}", self.tag, e.status))
            }
        }
    }

    /// Inflate (if stored deflated) and decode the two files, as installed ones are, then parse them. Also the
    /// size of what was read, in bytes, for the memory budget.
    pub(crate) fn parse(&self) -> Result<(AffixDict, u64), String> {
        let (aff, dic) = (self.inflate(&self.aff, ".aff")?, self.inflate(&self.dic, ".dic")?);
        let size = (aff.len() as u64).saturating_add(dic.len() as u64);
        let (aff, dic) = crate::hunspell::decode(&aff, &dic)?;
        let source = format!("Embedded dictionary {} ({})", self.tag, self.label);
        Ok((AffixDict::from_hunspell(&aff, &dic, &[source.as_str()])?, size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registered under tags no other test uses (the registry is process-wide), and removed again.
    fn tags_of(locale: &str) -> Vec<String> {
        crate::pack_lang::hyphenation_locales(PackLocale::new(locale).unwrap())
    }

    #[test]
    fn a_tag_serves_its_regions_and_the_most_specific_one_wins() {
        crate::hold_dictionary_tests();
        let aff = b"SET UTF-8\n".to_vec();
        embed_dictionary("zq", "General Zq", aff.clone(), b"1\nzqa\n".to_vec(), Packing::Plain).unwrap();
        embed_dictionary("ZQ_ch", "Swiss Zq", aff, b"1\nzqb\n".to_vec(), Packing::Plain).unwrap();
        let label_of = |t: &str| find(&tags_of(t)).map(|e| e.label.clone());
        assert_eq!(label_of("zq-FR").as_deref(), Some("General Zq"));
        assert_eq!(label_of("zq-CH").as_deref(), Some("Swiss Zq"));
        assert_eq!(label_of("zq").as_deref(), Some("General Zq"));
        assert_eq!(label_of("zr-CH"), None);
        // Registering a tag again replaces it.
        embed_dictionary("zq", "Newer Zq", b"SET UTF-8\n".to_vec(), b"1\nzqc\n".to_vec(), Packing::Plain).unwrap();
        assert_eq!(label_of("zq-FR").as_deref(), Some("Newer Zq"));
        forget_dictionaries();
        assert_eq!(label_of("zq-FR"), None);
    }

    #[test]
    fn a_registration_that_cannot_be_used_is_refused() {
        crate::hold_dictionary_tests();
        for tag in ["", "x-none", "und", "1x", "-"] {
            assert!(embed_dictionary(tag, "L", Vec::new(), Vec::new(), Packing::Plain).is_err(), "{tag:?}");
        }
        assert!(find(&["und".to_string()]).is_none());
        // English and the languages with a dictionary slot of their own are not read through here; a region that is never looked up
        // under (`de-AT` is German) says which tag to register.
        for tag in ["en", "en-GB", "pl", "it-IT", "es-MX", "pt-BR", "ru", "cs"] {
            assert!(embed_dictionary(tag, "L", b"SET UTF-8\n".to_vec(), b"1\nkot\n".to_vec(), Packing::Plain).is_err(), "{tag}");
        }
        let at = embed_dictionary("de-AT", "L", b"SET UTF-8\n".to_vec(), b"1\nkot\n".to_vec(), Packing::Plain).unwrap_err();
        assert!(at.contains("\"de\""), "{at}");
        for tag in ["de", "de-CH", "bg", "fr", "fr-CA", "pt-PT", "nl"] {
            assert!(embed_dictionary(tag, "L", b"SET UTF-8\n".to_vec(), b"1\nkot\n".to_vec(), Packing::Plain).is_ok(), "{tag}");
        }
        forget_dictionaries();
    }

    #[test]
    fn plain_and_deflated_files_parse_the_same() {
        let aff = "SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n";
        let dic = "2\nkot/S\nsłowo/S\n";
        let deflated = |s: &str| miniz_oxide::deflate::compress_to_vec(s.as_bytes(), 6);
        for (aff_bytes, dic_bytes, packing) in
            [(aff.as_bytes().to_vec(), dic.as_bytes().to_vec(), Packing::Plain), (deflated(aff), deflated(dic), Packing::Deflate)]
        {
            let entry = Entry { tag: "zz".into(), label: "Test".into(), aff: aff_bytes.into(), dic: dic_bytes.into(), packing };
            let (d, size) = entry.parse().unwrap();
            assert!(d.check("kots") && d.check("słowos") && !d.check("kotz"), "{packing:?}");
            assert_eq!(size, (aff.len() + dic.len()) as u64);
        }
        // Bytes that are not a deflate stream say so; a Hunspell pair the engine refuses says why.
        let not_deflate = Entry {
            tag: "zz".into(),
            label: "T".into(),
            aff: b"SET UTF-8\n".as_slice().into(),
            dic: b"1\nkot\n".as_slice().into(),
            packing: Packing::Deflate,
        };
        assert!(not_deflate.parse().unwrap_err().contains("DEFLATE"), "not deflate");
        let refused = Entry {
            tag: "zz".into(),
            label: "T".into(),
            aff: b"COMPLEXPREFIXES X\n".as_slice().into(),
            dic: b"1\nkot\n".as_slice().into(),
            packing: Packing::Plain,
        };
        assert!(refused.parse().is_err());
    }

    /// The language is spelled from the bytes handed over, read on first use only and once for all its regions; a dictionary the
    /// user keeps in a folder wins, and an unreadable one leaves its reason and the embedded dictionary is used.
    #[test]
    fn an_embedded_dictionary_spells_its_language_lazily_and_once() {
        crate::hold_dictionary_tests();
        forget_dictionaries();
        let aff = "SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n";
        let packed = miniz_oxide::deflate::compress_to_vec(aff.as_bytes(), 6);
        let dic = miniz_oxide::deflate::compress_to_vec("2\nkot/S\nsłowo/S\n".as_bytes(), 6);
        embed_dictionary("zy", "Zy test (MIT)", packed, dic, Packing::Deflate).unwrap();
        let (aa, bb) = (ProofLang::from_tag(Some("zy-AA")), ProofLang::from_tag(Some("zy-BB")));
        assert_ne!(aa, bb, "two regions, two locales");
        let before = crate::pack_lang::loads();
        assert_eq!(label(aa).as_deref(), Some("Zy test (MIT)"));
        assert_eq!(crate::pack_lang::loads(), before, "registered, not read");
        assert!(aa.is_proofed() && crate::is_correct_in("kots", aa) && !crate::is_correct_in("kotz", aa));
        assert!(bb.is_proofed() && crate::is_correct_in("słowos", bb) && !crate::is_correct_in("kotz", bb));
        assert_eq!(crate::pack_lang::loads(), before + 1, "parsed once for both regions");
        assert_eq!(crate::suggest_in("kotz", 3, aa).first().map(String::as_str), Some("kot"));
        assert_eq!(crate::pack_dictionary_file(aa), None, "an embedded dictionary has no file");
        assert_eq!(crate::missing_dictionary(Some("zy-AA")), None);
        assert_eq!(crate::dictionary_load_error(aa), None);
        // A language with nothing registered or installed stays unchecked.
        assert!(!ProofLang::from_tag(Some("zx-AA")).is_proofed());

        // The user's own folder wins; a file in it that can't be read leaves the reason and the embedded dictionary serves.
        let dir = std::env::temp_dir().join(format!("craft-proof-embedded-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("zy_AA.aff"), "SET UTF-8\n").unwrap();
        std::fs::write(dir.join("zy_AA.dic"), "1\nfolderword\n").unwrap();
        crate::set_dictionaries_folder(Some(dir.clone()));
        assert!(crate::is_correct_in("folderword", aa) && !crate::is_correct_in("kots", aa), "the folder's dictionary");
        assert!(crate::is_correct_in("kots", bb), "another region has no file in the folder");
        assert!(crate::pack_dictionary_file(aa).is_some_and(|f| f.ends_with("zy_AA.dic")));
        std::fs::write(dir.join("zy_AA.aff"), "COMPLEXPREFIXES X\n").unwrap();
        crate::set_dictionaries_folder(Some(dir.clone()));
        assert!(crate::is_correct_in("kots", aa) && !crate::is_correct_in("kotz", aa), "the embedded dictionary serves");
        assert!(crate::dictionary_load_error(aa).is_some(), "and the folder's file says why it was not read");
        crate::set_dictionaries_folder(None);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(crate::is_correct_in("kots", aa) && crate::dictionary_load_error(aa).is_none());
        forget_dictionaries();
        assert!(!aa.is_proofed(), "forgotten");
    }

    /// An embedded dictionary that can't be read says why, and the language has no marks.
    #[test]
    fn an_unreadable_embedded_dictionary_reports_its_reason() {
        crate::hold_dictionary_tests();
        forget_dictionaries();
        embed_dictionary("zw", "Broken", b"COMPLEXPREFIXES X\n".to_vec(), b"1\nkot\n".to_vec(), Packing::Plain).unwrap();
        let lang = ProofLang::from_tag(Some("zw"));
        assert!(!lang.is_proofed() && crate::is_correct_in("anything", lang));
        let reason = crate::dictionary_load_error(lang).unwrap();
        assert!(reason.contains("COMPLEXPREFIXES"), "{reason}");
        forget_dictionaries();
    }

    /// A deflate bomb stops at the size a file may have, and the entry cannot be registered over it.
    #[test]
    fn inflating_is_bounded() {
        let big = vec![b'a'; (crate::hunspell::MAX_FILE as usize) + 1];
        let packed = miniz_oxide::deflate::compress_to_vec(&big, 1);
        assert!(packed.len() < 1 << 20, "{}", packed.len());
        let entry = Entry { tag: "zz".into(), label: "T".into(), aff: packed.clone().into(), dic: packed.into(), packing: Packing::Deflate };
        assert!(entry.parse().is_err());
        assert!(embed_dictionary("zz", "T", vec![0u8; (crate::hunspell::MAX_FILE as usize) + 1], Vec::new(), Packing::Plain).is_err());
    }
}
