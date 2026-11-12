//! Proofing for any language by its locale ([`crate::ProofLang::Pack`]): its Hunspell dictionary (the
//! dictionaries folder the user set, then one the program carries ([`crate::embedded`]), then an installed
//! Language pack, then the system's), read by the shared affix engine, with its own suggestion ranking
//! ([`crate::pack_suggest`]) and no grammar rules (French has repeated-word and spacing checks).
//!
//! The loaded dictionaries share a memory budget ([`MAX_LOADED_BYTES`], estimated from their
//! files); the one used least recently goes first, but never one used during the current
//! [`in_one_check`] call, so a paragraph in many languages loads each dictionary once. Locales
//! with no dictionary are remembered apart and take no room.
//!
//! A word none of whose letters is in a script the dictionary's words are written in (a Cyrillic
//! word in a Dutch run) isn't this language's: it is never marked, as Word leaves a word in another
//! script alone. A word in the dictionary's script is always checked, whatever its letters.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
// //// Neoffice — `Mutex`: the lock held while a dictionary is read (see `LOADING`).
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::affix::AffixDict;
// //// Neoffice — suggestions moved to `pack_suggest`, which has the tuning.
use crate::lang::PackLocale;

/// The memory budget of the loaded dictionaries, in estimated bytes.
pub(crate) const MAX_LOADED_BYTES: u64 = 256 << 20;
/// Estimated memory per byte of `.aff` and `.dic` file.
const BYTES_PER_FILE_BYTE: u64 = 6;
/// Most locales remembered as having no dictionary (the oldest is forgotten first when full).
const MAX_MISSING: usize = 256;

/// The budget in force (tests lower it).
static BUDGET: AtomicU64 = AtomicU64::new(MAX_LOADED_BYTES);
/// Dictionaries read and parsed so far, from disk or from the program (tests count them).
static LOADS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The clock when this thread's current [`in_one_check`] call started.
    static CHECK_FROM: Cell<Option<u64>> = const { Cell::new(None) };
}

struct Loaded {
    locale: PackLocale,
    dict: Arc<AffixDict>,
    /// The scripts its words are written in ([`scripts`]), sorted.
    scripts: Arc<[u32]>,
    /// The `.dic` it came from.
    file: Option<PathBuf>,
    // //// Neoffice — the locales of one embedded language share one parsed dictionary.
    /// The tag of the embedded dictionary it is ([`crate::embedded`]): the locales of one language share it.
    embedded: Option<String>,
    /// Estimated memory.
    bytes: u64,
    used: u64,
}

struct Cache {
    loaded: Vec<Loaded>,
    /// Locales with no dictionary (or one that failed to load).
    missing: Vec<PackLocale>,
    clock: u64,
    /// Counts reloads, so a load that started before one is not kept.
    reloads: u64,
}

static CACHE: RwLock<Cache> = RwLock::new(Cache { loaded: Vec::new(), missing: Vec::new(), clock: 0, reloads: 0 });

/// Forget every loaded dictionary (the dictionaries folder or the packs changed).
pub(crate) fn reload() {
    let mut c = CACHE.write().unwrap_or_else(PoisonError::into_inner);
    c.loaded.clear();
    c.missing.clear();
    c.reloads = c.reloads.wrapping_add(1);
}

/// Run `f` as one check (a paragraph): no dictionary it uses is evicted before it returns, even
/// over the budget.
pub(crate) fn in_one_check<R>(f: impl FnOnce() -> R) -> R {
    let outer = CHECK_FROM.with(Cell::get);
    let _end = outer.is_none().then(|| {
        let now = CACHE.read().unwrap_or_else(PoisonError::into_inner).clock;
        CHECK_FROM.with(|c| c.set(Some(now.wrapping_add(1))));
        EndCheck
    });
    f()
}

/// Ends the outermost [`in_one_check`] when dropped: also when `f` panics (`Session::run` catches
/// it), or every dictionary would stay pinned on this thread and the budget never hold again.
struct EndCheck;

impl Drop for EndCheck {
    fn drop(&mut self) {
        CHECK_FROM.with(|c| c.set(None));
    }
}

/// Remember that `locale` has no dictionary; when [`MAX_MISSING`] are remembered, the oldest is
/// forgotten (clearing them all would look every other one up again).
fn remember_missing(missing: &mut Vec<PackLocale>, locale: PackLocale) {
    if missing.len() >= MAX_MISSING {
        missing.remove(0);
    }
    missing.push(locale);
}

/// The script of the letter `c`, as a number: the main alphabetic scripts by their Unicode blocks
/// (Latin with its extensions, Greek, Cyrillic, Armenian, Hebrew, Arabic, Georgian, Hangul, kana,
/// Han, Ethiopic); any other letter by its 128-character block. Only told apart, never named.
fn script_of(c: char) -> u32 {
    const SCRIPTS: &[(u32, u32, u32)] = &[
        (0x0041, 0x024F, 1),
        (0x0250, 0x02AF, 1),
        (0x1E00, 0x1EFF, 1),
        (0x2C60, 0x2C7F, 1),
        (0xA720, 0xA7FF, 1),
        (0xAB30, 0xAB6F, 1),
        (0xFF21, 0xFF5A, 1),
        (0x0370, 0x03FF, 2),
        (0x1F00, 0x1FFF, 2),
        (0x0400, 0x052F, 3),
        (0x1C80, 0x1C8F, 3),
        (0x2DE0, 0x2DFF, 3),
        (0xA640, 0xA69F, 3),
        (0x0530, 0x058F, 4),
        (0xFB13, 0xFB17, 4),
        (0x0590, 0x05FF, 5),
        (0xFB1D, 0xFB4F, 5),
        (0x0600, 0x06FF, 6),
        (0x0750, 0x077F, 6),
        (0x08A0, 0x08FF, 6),
        (0xFB50, 0xFDFF, 6),
        (0xFE70, 0xFEFF, 6),
        (0x10A0, 0x10FF, 7),
        (0x1C90, 0x1CBF, 7),
        (0x2D00, 0x2D2F, 7),
        (0x1100, 0x11FF, 8),
        (0x3130, 0x318F, 8),
        (0xAC00, 0xD7AF, 8),
        (0x3040, 0x30FF, 9),
        (0x3400, 0x4DBF, 10),
        (0x4E00, 0x9FFF, 10),
        (0x1200, 0x139F, 11),
    ];
    let u = u32::from(c);
    SCRIPTS.iter().find(|(lo, hi, _)| (*lo..=*hi).contains(&u)).map_or(0x100 + (u >> 7), |(_, _, s)| *s)
}

/// The scripts the words of `d` are written in ([`script_of`] of their letters), sorted.
fn scripts(d: &AffixDict) -> Arc<[u32]> {
    let mut seen = std::collections::BTreeSet::new();
    for c in d.stems().flat_map(str::chars).filter(|c| c.is_alphabetic()) {
        seen.insert(script_of(c));
    }
    seen.into_iter().collect()
}

/// Is `word` in another script than the dictionary's (`scripts`): none of its letters is in one
/// of them? A word in the dictionary's script is never, whatever its letters (`zzzz` in Dutch).
fn other_script(word: &str, scripts: &[u32]) -> bool {
    !scripts.is_empty() && !word.chars().filter(|c| c.is_alphabetic()).any(|c| scripts.binary_search(&script_of(c)).is_ok())
}

#[cfg(test)]
pub(crate) fn loads() -> u64 {
    LOADS.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn set_budget(bytes: u64) {
    BUDGET.store(bytes, Ordering::Relaxed);
}

/// Hunspell file names for `locale`, best first: `fr-fr` → `fr_FR`, `fr-FR`, `fr`;
/// `sr-latn` → `sr_Latn`, `sr-Latn` (never plain `sr`, which is Cyrillic); `sr-cyrl-rs` →
/// `sr_Cyrl_RS`, `sr-Cyrl-RS`, `sr_Cyrl`, `sr-Cyrl`, `sr_RS`, `sr-RS`, `sr` (Cyrillic is Serbian's
/// default script, [`crate::lang::default_script`]); `sr` → `sr`, `sr_Cyrl`, `sr-Cyrl`.
pub(crate) fn names(locale: PackLocale) -> Vec<String> {
    let parts: Vec<&str> = locale.as_str().split('-').collect();
    let script = parts.iter().position(|p| crate::lang::is_script(p));
    let cased: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i == 0 {
                (*p).to_string()
            } else if Some(i) == script {
                let mut c = p.chars();
                c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
            } else if p.len() == 2 {
                p.to_ascii_uppercase()
            } else {
                (*p).to_string()
            }
        })
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |parts: &[String]| {
        for sep in ["_", "-"] {
            let n = parts.join(sep);
            if !out.contains(&n) {
                out.push(n);
            }
        }
    };
    let primary = locale.primary();
    let default = crate::lang::default_script(primary);
    // Each shorter prefix too (`sr-latn-rs` → `sr-latn`), never dropping the script subtag.
    let keep = script.map_or(1, |s| s + 1);
    // The region subtag, when one follows the language (and script): a variant after it
    // (`ca-ES-valencia`) is also looked up without it (`ca-valencia`), before the region alone,
    // as packs and dictionaries name a variant by the language and the variant.
    let region = crate::lang::region_index(&parts);
    for n in (keep..=cased.len()).rev() {
        let prefix = cased.get(..n).unwrap_or_default();
        push(prefix);
        if let Some(r) = region
            && r + 1 < n
        {
            push(&crate::lang::without_index(prefix, r));
        }
    }
    match script {
        // The language's default script: its script-less names are in that script too.
        Some(s) if parts.get(s).is_some_and(|p| Some(*p) == default) => {
            let without = crate::lang::without_index(&cased, s);
            let region = crate::lang::region_index(&without);
            for n in (1..=without.len()).rev() {
                let prefix = without.get(..n).unwrap_or_default();
                push(prefix);
                if let Some(r) = region
                    && r + 1 < n
                {
                    push(&crate::lang::without_index(prefix, r));
                }
            }
        }
        Some(_) => {}
        // No script named: the language's default one may be spelled out (`sr-Cyrl`).
        None => {
            if let Some(d) = default {
                let mut c = d.chars();
                let d = c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default();
                push(&[primary.to_string(), d]);
            }
        }
    }
    out
}

/// `locale`'s dictionary, loaded on first use; an empty one when there is none.
fn get(locale: PackLocale) -> (Arc<AffixDict>, Option<PathBuf>) {
    let (d, file, _) = get_with_scripts(locale);
    (d, file)
}

// //// Neoffice — the cache lookup, the read lock and the sources (folders, embedded, packs) are split out of `get_with_scripts` (see `choose`).
/// The loaded dictionary of `locale` (or the knowledge that it has none), when the cache has it: the lookup counts as a use.
type Found = (Arc<AffixDict>, Option<PathBuf>, Arc<[u32]>);

fn cached(locale: PackLocale) -> Option<Found> {
    let mut c = CACHE.write().unwrap_or_else(PoisonError::into_inner);
    c.clock = c.clock.wrapping_add(1);
    let now = c.clock;
    if let Some(l) = c.loaded.iter_mut().find(|l| l.locale == locale) {
        l.used = now;
        let (found, group) = ((l.dict.clone(), l.file.clone(), l.scripts.clone()), l.embedded.clone());
        // A locale that shares an embedded dictionary keeps its twins in use: they are evicted together.
        if let Some(tag) = group {
            c.loaded.iter_mut().filter(|l| l.embedded.as_deref() == Some(tag.as_str())).for_each(|l| l.used = now);
        }
        return Some(found);
    }
    c.missing.contains(&locale).then(|| (Arc::default(), None, Arc::from([])))
}

/// One lock per dictionary being read (its embedded tag, or its locale), held while it is read and parsed, so that threads
/// asking for the same one at once wait for a single read instead of each parsing megabytes (a large dictionary takes a tenth of a
/// second or more): whoever comes next finds it cached. Another language's first use does not wait for it.
static LOADING: Mutex<Vec<(String, Arc<Mutex<()>>)>> = Mutex::new(Vec::new());

/// The lock of the dictionary `key`.
fn loading_lock(key: &str) -> Arc<Mutex<()>> {
    let mut locks = LOADING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, lock)) = locks.iter().find(|(k, _)| k == key) {
        return lock.clone();
    }
    let lock = Arc::new(Mutex::new(()));
    locks.push((key.to_string(), lock.clone()));
    lock
}

/// Most locales that share one embedded dictionary are cached beside the one that parsed it (more are answered the same way, uncached).
const MAX_TWINS: usize = 16;

/// [`get`], with the dictionary's [`scripts`].
fn get_with_scripts(locale: PackLocale) -> Found {
    // //// Neoffice — the cache first, then one reader at a time (`LOADING`), and the source of an embedded dictionary (`Source`) shared by its locales.
    if let Some(found) = cached(locale) {
        return found;
    }
    let reloads = CACHE.read().unwrap_or_else(PoisonError::into_inner).reloads;
    // //// Neoffice — the source is chosen before anything is parsed (`choose`), and one thread reads each dictionary at a time (`LOADING`).
    let source = choose(locale);
    let lock = loading_lock(&match &source {
        Source::Embedded(e) => format!("embedded {}", e.tag()),
        _ => locale.as_str().to_string(),
    });
    let _loading = lock.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = cached(locale) {
        return found;
    }
    // //// Neoffice — another locale of the same embedded language already holds this dictionary: share it, parse nothing.
    if let Source::Embedded(e) = &source {
        let mut c = CACHE.write().unwrap_or_else(PoisonError::into_inner);
        let twin = c.loaded.iter().find(|l| l.embedded.as_deref() == Some(e.tag())).map(|l| (l.dict.clone(), l.scripts.clone()));
        if let Some((dict, scripts)) = twin {
            let twins = c.loaded.iter().filter(|l| l.embedded.as_deref() == Some(e.tag())).count();
            if c.reloads == reloads && twins < MAX_TWINS {
                let used = c.clock;
                c.loaded.push(Loaded {
                    locale,
                    dict: dict.clone(),
                    scripts: scripts.clone(),
                    file: None,
                    embedded: Some(e.tag().to_string()),
                    bytes: 0,
                    used,
                });
            }
            return (dict, None, scripts);
        }
    }
    // //// Neoffice — the dictionary is parsed from its source (`parse`); an embedded one is noted so that its twins can share it.
    let (dict, file, bytes) = parse(locale, &source);
    let embedded = match &source {
        Source::Embedded(e) if !dict.is_empty() => Some(e.tag().to_string()),
        _ => None,
    };
    // //// Neoffice — (the dictionary was read by `parse` above; the scripts of its words are looked at once it is there.)
    let scripts = scripts(&dict);
    let mut c = CACHE.write().unwrap_or_else(PoisonError::into_inner);
    if c.reloads != reloads || c.loaded.iter().any(|l| l.locale == locale) || c.missing.contains(&locale) {
        return (dict, file, scripts);
    }
    if dict.is_empty() {
        remember_missing(&mut c.missing, locale);
        return (dict, file, scripts);
    }
    LOADS.fetch_add(1, Ordering::Relaxed);
    let budget = BUDGET.load(Ordering::Relaxed);
    let pinned_from = CHECK_FROM.with(Cell::get).unwrap_or(u64::MAX);
    let mut total: u64 = c.loaded.iter().map(|l| l.bytes).sum();
    while total.saturating_add(bytes) > budget {
        // The least recently used dictionary not used during this check.
        let Some(oldest) = c.loaded.iter().enumerate().filter(|(_, l)| l.used < pinned_from).min_by_key(|(_, l)| l.used).map(|(i, _)| i) else {
            break;
        };
        let gone = c.loaded.swap_remove(oldest);
        total = total.saturating_sub(gone.bytes);
        // //// Neoffice — the locales that share an embedded dictionary go with it: the memory is released only when none holds it.
        if let Some(tag) = gone.embedded {
            c.loaded.retain(|l| l.embedded.as_deref() != Some(tag.as_str()));
            total = c.loaded.iter().map(|l| l.bytes).sum();
        }
    }
    let used = c.clock;
    // //// Neoffice — `embedded`: which embedded dictionary it is, if one.
    c.loaded.push(Loaded { locale, dict: dict.clone(), scripts: scripts.clone(), file: file.clone(), embedded, bytes, used });
    (dict, file, scripts)
}

// //// Neoffice — the sources of a locale's dictionary, with the one the program carries between the user's folders and the packs.
/// Where a locale's dictionary comes from, in the order they are tried: the dictionaries folders the user set, the dictionary
/// the program carries ([`crate::embedded`]), then an installed Language pack or the system's Hunspell dictionaries.
enum Source {
    Files(PathBuf, PathBuf),
    Embedded(Arc<crate::embedded::Entry>),
    None,
}

/// The first source `locale` has. A dictionary in a folder the user set wins over the embedded one; if that file
/// can't be read, [`parse`] falls back to the embedded one when there is one.
fn choose(locale: PackLocale) -> Source {
    // //// Neoffice — the user's dictionaries folders, then the embedded dictionary, then installed packs and the system's.
    let names = names(locale);
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Some((aff, dic)) = crate::hunspell::user_dirs().iter().find_map(|dir| crate::hunspell::find_in(dir, &refs)) {
        return Source::Files(aff, dic);
    }
    if let Some(e) = crate::embedded::find(&hyphenation_locales(locale)) {
        return Source::Embedded(e);
    }
    match crate::hunspell::find(&refs) {
        Some((aff, dic)) => Source::Files(aff, dic),
        None => Source::None,
    }
}

/// Read and parse `source`'s dictionary for `locale`, with its estimated memory. A file that can't be read leaves its reason
/// ([`load_error`]); the embedded dictionary, if there is one, is used then.
fn parse(locale: PackLocale, source: &Source) -> (Arc<AffixDict>, Option<PathBuf>, u64) {
    let names = names(locale);
    let key = names.first().map_or("", String::as_str);
    // //// Neoffice — an embedded dictionary is inflated and parsed here; a failure leaves its reason like an installed file's.
    let embedded = |e: &crate::embedded::Entry| match e.parse() {
        Ok((d, size)) => {
            crate::hunspell::clear_load_error(key);
            (Arc::new(d), None, size.saturating_mul(BYTES_PER_FILE_BYTE).max(1))
        }
        Err(reason) => {
            log::warn!("{locale} spelling: {reason}");
            let _ = crate::hunspell::note_load::<()>(key, std::path::Path::new(&e.file_name()), Err(reason));
            (Arc::default(), None, 0)
        }
    };
    // //// Neoffice — what each source reads (it was one `load` of the files found).
    match source {
        Source::Embedded(e) => embedded(e),
        Source::None => {
            crate::hunspell::clear_load_error(key);
            (Arc::default(), None, 0)
        }
        // //// Neoffice — installed files, as `load` read them, with the embedded dictionary as a fallback.
        Source::Files(aff, dic) => {
            let size = |p: &std::path::Path| std::fs::metadata(p).map_or(0, |m| m.len());
            let bytes = size(aff).saturating_add(size(dic)).saturating_mul(BYTES_PER_FILE_BYTE).max(1);
            match crate::hunspell::note_load(key, dic, crate::hunspell::load(aff, dic)) {
                Ok(d) => (Arc::new(d), Some(dic.clone()), bytes),
                Err(e) => {
                    log::warn!("{locale} spelling: {e}");
                    // The reason stays for File › Options; the program's own dictionary, if it carries one, is used meanwhile.
                    match crate::embedded::find(&hyphenation_locales(locale)) {
                        Some(emb) => {
                            let (d, file, size) = embedded(&emb);
                            // `embedded` forgot the load error of the file: put it back.
                            let _ = crate::hunspell::note_load::<()>(key, dic, Err(e));
                            (d, file, size)
                        }
                        None => (Arc::default(), None, 0),
                    }
                }
            }
        }
    }
}

/// Is a dictionary for `locale` available (its spelling is checked)?
pub(crate) fn has_dictionary(locale: PackLocale) -> bool {
    !get(locale).0.is_empty()
}

/// The `.dic` file `locale`'s spelling is read from now.
pub(crate) fn dictionary_file(locale: PackLocale) -> Option<PathBuf> {
    let (d, file) = get(locale);
    file.filter(|_| !d.is_empty())
}

/// Why `locale`'s dictionary failed to load, if it did.
pub(crate) fn load_error(locale: PackLocale) -> Option<String> {
    crate::hunspell::load_error(names(locale).first()?).map(|(_, e)| e)
}

// //// Neoffice — what only decorates a word (a unit's exponent, an elision before an acronym) is set aside (see `decorated`).
/// Is `word` right in `locale`? Every word is when there is no dictionary.
///
/// A hyphenated word the dictionary doesn't have whole is right when each of its parts is (`est-ce`, `parle-t-il`): the
/// dictionaries' `BREAK` for the hyphen, which the affix engine doesn't read. A part, or the word, is also right when only
/// what decorates it is not a word ([`decorated`]).
pub(crate) fn is_correct(word: &str, locale: PackLocale, opts: &crate::Options) -> bool {
    let (d, _, scripts) = get_with_scripts(locale);
    if d.is_empty() {
        return true;
    }
    let Some(w) = crate::spelling::word_to_check_in(&d, word, opts) else { return true };
    if other_script(w, &scripts) {
        return true;
    }
    let known = |w: &str| d.check(w) || crate::in_user_dictionary(&w.to_lowercase());
    // //// Neoffice — `part`: a word or one part of a hyphenated one, with what decorates it set aside.
    let part = |p: &str| known(p) || decorated(&d, p, opts, &known);
    if w.contains('-') && !part(w) {
        return w.split('-').all(|p| p.chars().count() <= 1 || part(p));
    }
    // //// Neoffice — `part`: the word itself, with what decorates it set aside.
    part(w)
}

// //// Neoffice — what only decorates a word (see `is_correct`).
/// Is `w` right once what only decorates it is set aside: a unit's superscript exponent (`km²`: the unit is the word), or an
/// elision in front of a word that needs no lookup (`l'OTAN`, `d'IBM`: an acronym or a number, under the options that skip
/// those) or is the user's own (`d'Apple`)? An elision is a prefix the dictionary's rules add that ends with an apostrophe
/// (French `l'`, `qu'`, `jusqu'`): other letters before an apostrophe are not one.
fn decorated(d: &AffixDict, w: &str, opts: &crate::Options, known: &dyn Fn(&str) -> bool) -> bool {
    let unit = w.trim_end_matches(['²', '³', '¹', '⁰', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹', '⁻']);
    if unit.len() < w.len() && !unit.is_empty() && known(unit) {
        return true;
    }
    let Some((head, tail)) = w.split_once(['\'', '’']) else { return false };
    if tail.chars().count() < 2 || !(d.adds_prefix(&format!("{head}'")) || d.adds_prefix(&format!("{}'", head.to_lowercase()))) {
        return false;
    }
    crate::in_user_dictionary(&tail.to_lowercase()) || crate::spelling::word_to_check_in(d, tail, opts).is_none()
}

/// Suggestions for `word` in `locale`, best first, at most `max`.
pub(crate) fn suggest(word: &str, max: usize, locale: PackLocale) -> Vec<String> {
    let (d, _) = get(locale);
    if d.is_empty() {
        return Vec::new();
    }
    // //// Neoffice — the shared ranking with a dearer first letter, elisions of a word to correct, and a hyphenated word's wrong part.
    crate::pack_suggest::suggest(&d, word, max)
}

/// The pack locales whose hyphenation patterns serve a [`crate::ProofLang::Pack`] language: the locale,
/// then each shorter prefix not past a script subtag (`fr-ca` → `fr`).
pub(crate) fn hyphenation_locales(locale: PackLocale) -> Vec<String> {
    names(locale).into_iter().filter(|n| !n.contains('_')).map(|n| n.to_ascii_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(t: &str) -> PackLocale {
        PackLocale::new(t).unwrap()
    }

    /// Review follow-up (craft-libs#485): a full list of locales with no dictionary forgets its
    /// oldest one, not all of them, so a document in more languages doesn't look every one up again.
    #[test]
    fn missing_locales_forget_the_oldest_first() {
        let tag = |i: usize| l(&format!("q{}{}", (b'a' + (i / 26 % 26) as u8) as char, (b'a' + (i % 26) as u8) as char));
        let mut missing = Vec::new();
        for i in 0..MAX_MISSING + 10 {
            remember_missing(&mut missing, tag(i));
            assert!(missing.len() <= MAX_MISSING);
        }
        assert_eq!(missing.len(), MAX_MISSING);
        assert!(!missing.contains(&tag(0)) && !missing.contains(&tag(9)), "the oldest went first");
        assert!(missing.contains(&tag(10)) && missing.contains(&tag(MAX_MISSING + 9)), "the rest stay");
    }

    /// Review follow-up (craft-libs#485): a panic inside a check (caught by the app) doesn't leave
    /// this thread's dictionaries pinned past the budget for good.
    #[test]
    fn a_panic_inside_a_check_ends_it() {
        let caught = std::panic::catch_unwind(|| in_one_check(|| panic!("a bug inside a check")));
        assert!(caught.is_err());
        assert_eq!(CHECK_FROM.with(Cell::get), None);
        in_one_check(|| {
            assert!(CHECK_FROM.with(Cell::get).is_some());
            in_one_check(|| ());
            assert!(CHECK_FROM.with(Cell::get).is_some(), "an inner check doesn't end the outer one");
        });
        assert_eq!(CHECK_FROM.with(Cell::get), None);
    }

    /// Review follow-up (craft-libs#485): a word with no letter in the dictionary's scripts is
    /// in another script (a Cyrillic word in a Dutch run); a word in its script is always checked,
    /// even with letters no dictionary word uses (CodeRabbit on #496: `zzzz`).
    #[test]
    fn words_in_another_script_are_not_the_dictionarys() {
        let nl = AffixDict::from_hunspell("SET UTF-8\n", "3\nhuis\nkat\nÉén\n", &[]).unwrap();
        let latin = scripts(&nl);
        assert_eq!(latin.len(), 1);
        for w in ["привет", "Привет", "αβγ", "שלום", "日本"] {
            assert!(other_script(w, &latin), "{w}");
        }
        for w in ["huus", "kät", "huisп", "ÉÉN", "zzzz", "qxqw", "ŋʒ"] {
            assert!(!other_script(w, &latin), "{w}");
        }
        assert!(!other_script("привет", &[]), "no scripts: every word is checked");
        let ru = AffixDict::from_hunspell("SET UTF-8\n", "1\nдом\n", &[]).unwrap();
        assert!(!other_script("щщщ", &scripts(&ru)) && other_script("dom", &scripts(&ru)));
        assert_ne!(script_of('ა'), script_of('a'));
        assert_eq!(script_of('ă'), script_of('Ạ'));
    }

    // //// Neoffice — what decorates a word is not the word.
    /// What decorates a word is not the word: a unit's exponent, and an elision before an acronym, a number or the user's word.
    #[test]
    fn units_with_exponents_and_elided_acronyms_are_right() {
        crate::hold_dictionary_tests();
        crate::embedded::forget_dictionaries();
        let aff = "SET UTF-8\nPFX L Y 2\nPFX L 0 l' [aeiouéAEIOUÉ]\nPFX L 0 d' [aeiouéAEIOUÉ]\nPFX Q Y 1\nPFX Q 0 qu' [aeiouéAEIOUÉ]\n";
        crate::embedded::embed_dictionary(
            "zv",
            "Test",
            aff.as_bytes().to_vec(),
            "6\nkm/\nm\nécole/LQ\nami/LQ\nhomme/LQ\ncafé\n".as_bytes().to_vec(),
            crate::embedded::Packing::Plain,
        )
        .unwrap();
        let lang = crate::ProofLang::from_tag(Some("zv-AA"));
        let strict = crate::Options { ignore_uppercase: false, ignore_numbers: false, ignore_internet: false };
        let ok = |w: &str, o: &crate::Options| crate::is_correct_in_with(w, lang, o);
        let defaults = crate::Options::default();
        for w in ["km²", "m³", "km²-m³", "café²", "l'OTAN", "d\u{2019}IBM", "L'OTAN", "qu'IBM", "l'école", "l'2e", "d'OTAN-café"] {
            assert!(ok(w, &defaults), "{w}");
        }
        for w in ["l'xyzzy", "x'OTAN", "l'OTANNNNNNNNNN", "kmx²", "xyz²", "l'OTAN-xyzzy", "qu'xyzzy"] {
            assert!(!ok(w, &defaults), "{w}");
        }
        // The options that skip acronyms are what let one after an elision pass; looked up it is a word or it is not.
        assert!(!ok("l'OTAN", &strict) && ok("km²", &strict) && ok("l'école", &strict));
        // The user's word after an elision, alone or as a part.
        assert!(!ok("d'Zqapple", &defaults) && !ok("d'Zqapple-café", &defaults));
        crate::add_word("Zqapple");
        assert!(ok("d'Zqapple", &strict) && ok("l\u{2019}Zqapple", &strict) && ok("d'Zqapple-café", &strict) && ok("café-d'Zqapple", &strict));
        assert!(!ok("x'Zqapple", &strict) && !ok("d'Zqapplee", &strict));
        crate::remove_word("Zqapple");
        crate::embedded::forget_dictionaries();
    }

    #[test]
    fn locales_and_file_names() {
        assert_eq!(l("nl-NL").as_str(), "nl-nl");
        assert_eq!(l("fr_FR.UTF-8").as_str(), "fr-fr");
        assert_eq!(l("ca-ES-valencia").as_str(), "ca-es-valencia");
        assert_eq!(l("sr-Latn-RS-x-private").as_str(), "sr-latn-rs");
        assert_eq!(l("zh-Hant-TW-somethinglong").as_str(), "zh-hant-tw");
        for bad in ["x-none", "i-klingon", "und", "zxx", "mul", "e", "-", "", "1x", "näh"] {
            assert_eq!(PackLocale::new(bad), None, "{bad}");
        }
        assert_eq!(names(l("nl")), ["nl"]);
        assert_eq!(names(l("fr-FR")), ["fr_FR", "fr-FR", "fr"]);
        assert_eq!(names(l("sr-Latn")), ["sr_Latn", "sr-Latn"]);
        assert_eq!(names(l("sr-Latn-RS")), ["sr_Latn_RS", "sr-Latn-RS", "sr_Latn", "sr-Latn"]);
        assert_eq!(names(l("sr-Cyrl-RS")), ["sr_Cyrl_RS", "sr-Cyrl-RS", "sr_Cyrl", "sr-Cyrl", "sr_RS", "sr-RS", "sr"]);
        assert_eq!(names(l("bs-Latn-BA")), ["bs_Latn_BA", "bs-Latn-BA", "bs_Latn", "bs-Latn", "bs_BA", "bs-BA", "bs"]);
        assert_eq!(names(l("sr")), ["sr", "sr_Cyrl", "sr-Cyrl"]);
        // A four-character subtag that isn't letters is no script.
        assert_eq!(names(l("de-1996")), ["de_1996", "de-1996", "de"]);
        // A variant after the region is looked up without the region before the region alone
        // (`ca-ES-valencia` is the `ca-valencia` pack's), and the region alone still is `ca-ES`.
        assert_eq!(names(l("ca-ES-valencia")), ["ca_ES_valencia", "ca-ES-valencia", "ca_valencia", "ca-valencia", "ca_ES", "ca-ES", "ca"]);
        assert_eq!(names(l("ca-ES")), ["ca_ES", "ca-ES", "ca"]);
        // Any variant within the length limit counts, known or not: `sr-RS-ijekavsk` (14 bytes)
        // keeps it, so its names are the full one, without the region, the region alone, the bare one, then the default script's.
        assert_eq!(l("sr-RS-ijekavsk").as_str(), "sr-rs-ijekavsk");
        assert_eq!(
            names(l("sr-RS-ijekavsk")),
            ["sr_RS_ijekavsk", "sr-RS-ijekavsk", "sr_ijekavsk", "sr-ijekavsk", "sr_RS", "sr-RS", "sr", "sr_Cyrl", "sr-Cyrl"]
        );
        // With a script the same variant passes the length limit (15 bytes), and is cut by it.
        assert_eq!(l("sr-Cyrl-RS-ijekavsk").as_str(), "sr-cyrl-rs");
        // A variant that fits with the default script is looked up without the region too, script-less as well.
        let n = names(l("sr-Cyrl-RS-1994"));
        assert!(n.contains(&"sr_Cyrl_1994".to_string()), "{n:?}");
        assert!(n.contains(&"sr_1994".to_string()), "{n:?}");
        assert!(n.iter().position(|x| x == "sr_1994") < n.iter().position(|x| x == "sr_RS"), "{n:?}");
        assert_eq!(hyphenation_locales(l("ca-ES-valencia")), ["ca-es-valencia", "ca-valencia", "ca-es", "ca"]);
        assert_eq!(hyphenation_locales(l("fr-CA")), ["fr-ca", "fr"]);
        assert_eq!(hyphenation_locales(l("sr-Latn")), ["sr-latn"]);
        assert_eq!(hyphenation_locales(l("sr-Cyrl")), ["sr-cyrl", "sr"]);
    }
}
