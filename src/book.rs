//! Filename parsing for audiobooks: split on separator dashes, then decide
//! which segment is the author, which is the series, and what's left is the title.
//!
//! Pure logic only -- no I/O, no prompting. `main` drives this.
//!
//! The one outside signal is a dictionary of given names (`gender_guesser`),
//! used to tell a person from a title when the structure alone can't.
//!
//! Like movies, an audiobooks directory holds one file per *different* book,
//! so each file is decided on its own with no batch consensus. Unlike either
//! of the other two, there is no single unambiguous anchor: audiobook names
//! are usually already human-written rather than scene-generated, so the
//! structure comes from the ` - ` separators and the shape of each segment.

use crate::parse::{clean, safe, Confidence};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

// Separator dashes, same shape as the shows cleaner uses: spaced, or a
// doubled run. An intra-word hyphen ("Jean-Luc", "Ready-Made") is part of a
// name or title and must survive.
re!(SEGMENT, r"\s+[-\x{2013}\x{2014}]+\s*|[-\x{2013}\x{2014}]+\s+|[-\x{2013}\x{2014}]{2,}");
re!(BRACKETED, r"[\(\[\{]([^\)\]\}]*)[\)\]\}]");

// "Sanderson, Brandon" -- an unambiguous author signal, and the one form
// that has to be reordered rather than used as-is.
re!(SURNAME_FIRST, r"^(?P<last>[\p{L}'\x{2019}-]+),\s*(?P<first>[\p{L}'\x{2019}.\s-]+)$");

// "The Final Empire by Brandon Sanderson" -- also unambiguous, but only
// when what follows "by" is actually name-shaped, so a title like
// "Gone by Midnight" isn't torn in half.
re!(BY_AUTHOR, r"(?i)^(?P<rest>.*?)\bby\s+(?P<author>[^-]+)$");

// A series with an explicit keyword ("Mistborn, Book 1", "Discworld Vol 3",
// "Mistborn #1") can carry up to three digits, since the keyword is doing
// the disambiguating.
re!(SERIES_KEYWORD, r"(?i)^(?P<name>.*\p{L})[,]?\s+(?:(?:book|bk|vol|volume|no\.?)\s*|#\s*)(?P<num>\d{1,3})$");
// A bare trailing number ("Mistborn 01") is only read as a series number at
// one or two digits. This is what keeps "Fahrenheit 451" whole; the
// whitespace requirement is what keeps "1984" whole.
re!(SERIES_BARE, r"^(?P<name>.*\p{L})\s+(?P<num>\d{1,2})$");

// Audiobook-specific trailing junk. Deliberately narrow: no bare "book" or
// "audio", which would eat "The Jungle Book" and "Audio Culture".
re!(
    BOOK_JUNK,
    r"(?i)^(?:unabridged|abridged|audiobook|audiobooks|m4b|m4a|mp3|aac|flac|ogg|opus|\d+kbps|\d+kb|kbps|retail|complete)$"
);
// "narrated by Michael Kramer", "read by Stephen Fry" -- a performer credit,
// never part of the title, and never the author we want.
re!(NARRATOR, r"(?i)[,;]?\s*\b(?:narrated|read|performed)\s+by\s+.*$");

/// Reduce a name to bare letters and digits, so that spacing and punctuation
/// stop mattering when comparing two spellings of one name: "C S Lewis",
/// "CS Lewis" and "c.s. lewis" all squash to the same key.
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Trim audiobook release junk from the trailing end of an already-cleaned
/// segment, the same trailing-only way `parse::clean` treats video junk.
fn trim_book_junk(s: &str) -> String {
    let s = NARRATOR.replace(s, "");
    let mut tokens: Vec<&str> = s.split_whitespace().collect();
    while tokens.last().is_some_and(|t| BOOK_JUNK.is_match(t)) {
        tokens.pop();
    }
    tokens.join(" ").trim_matches(|c| c == ' ' || c == ',').to_string()
}

// The dictionary is built lazily inside the crate on first lookup, so the
// 4.5 MB of name data costs nothing until `canon books` actually runs.
static NAMES: gender_guesser::Detector = gender_guesser::Detector::new();

/// Whether the first word of a segment is a given name the dictionary knows.
///
/// This is used as positive evidence ONLY, never negative. The dictionary's
/// coverage is uneven across cultures, so a name it doesn't recognise means
/// "no information", not "not a person" -- treating absence as evidence would
/// systematically demote authors whose names are underrepresented in it.
fn is_given_name(seg: &str) -> bool {
    let first = seg.split_whitespace().next().unwrap_or("");
    !matches!(NAMES.get_gender(first), gender_guesser::Gender::NotFound)
}

/// Nobiliary particles, which are lowercase by convention and so would
/// otherwise fail the capitalisation test in `author_score`: the "du" in
/// "Daphne du Maurier", the "van" in "Ludwig van Beethoven".
fn is_particle(t: &str) -> bool {
    matches!(
        t.to_lowercase().as_str(),
        "de" | "du" | "da" | "di" | "del" | "della" | "der" | "den" | "van" | "von" | "la" | "le" | "dos" | "bin" | "ibn" | "al" | "ter" | "ten"
    )
}

/// Words that never start a person's name, and that a title readily does.
///
/// This list is load-bearing for `is_given_name`, not just for shape: the
/// dictionary contains entries for common English function words, so a
/// segment has to clear this filter *before* it is ever looked up.
/// Presence of any of these is what separates "The Final Empire" from
/// "Brandon Sanderson" without a database of surnames.
fn is_title_word(t: &str) -> bool {
    matches!(
        t.to_lowercase().as_str(),
        "the" | "a" | "an" | "of" | "and" | "or" | "in" | "on" | "at" | "to" | "for" | "from" | "with" | "into" | "over" | "under" | "his" | "her" | "their" | "my" | "our"
            // These are in the name dictionary too ("the" comes back
            // MayBeFemale, "so" and "it" NotSure), so they have to be ruled
            // out here, before any lookup can mistake them for evidence.
            | "so" | "it" | "no" | "is" | "was" | "be"
    )
}

/// How name-shaped a segment is, 0 meaning "not a person". Higher wins when
/// two segments compete for the author slot.
///
/// Two-token names score above three-token ones, which is the whole trick
/// behind resolving "Project Hail Mary - Andy Weir": both look like names,
/// but "Andy Weir" looks more like one.
///
/// A recognised given name adds to that score rather than merely breaking
/// ties, because shape alone gets it wrong outright: "Ludwig van Beethoven"
/// is three tokens and "Some Memoir" is two, so without the dictionary the
/// title would win.
fn author_score(seg: &str) -> u32 {
    let tokens: Vec<&str> = seg.split_whitespace().collect();
    if tokens.is_empty() || tokens.len() > 4 {
        return 0;
    }
    if tokens.iter().any(|t| t.chars().any(|c| c.is_ascii_digit()) || is_title_word(t)) {
        return 0;
    }
    if !tokens.iter().all(|t| t.chars().next().is_some_and(|c| c.is_uppercase()) || is_particle(t)) {
        return 0;
    }
    let shape = match tokens.len() {
        2 => 4,
        3 => 3,
        4 => 2,
        _ => 1,
    };
    let known = is_given_name(seg);
    // A mononym has no shape evidence at all, so only the dictionary can make
    // it a person ("Homer"). It still scores below any full name, which is why
    // "Artemis" yields to "Andy Weir".
    if tokens.len() == 1 && !known {
        return 0;
    }
    shape + if known { 2 } else { 0 }
}

/// Read a "Name NN" / "Name, Book N" segment as a series and its number.
fn as_series(seg: &str) -> Option<(String, u32)> {
    let caps = SERIES_KEYWORD.captures(seg).or_else(|| SERIES_BARE.captures(seg))?;
    let name = caps.name("name")?.as_str().trim().to_string();
    let num = caps.name("num")?.as_str().parse().ok()?;
    Some((name, num))
}

/// One audiobook file: a title, plus an author and series when they could be
/// told apart from it.
#[derive(Clone, Debug)]
pub struct BookItem {
    pub path: PathBuf,
    pub title: String,
    pub author: Option<String>,
    pub series: Option<(String, u32)>,
    pub ext: String,
    pub confidence: Confidence,
}

impl BookItem {
    /// The renamed basename this file would get: "Author - Series NN - Title.ext",
    /// dropping either leading segment when it wasn't found.
    pub fn target(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(a) = &self.author {
            parts.push(a.clone());
        }
        if let Some((name, num)) = &self.series {
            parts.push(format!("{name} {num:02}"));
        }
        parts.push(self.title.clone());
        format!("{}{}", parts.join(" - "), self.ext)
    }

    pub fn basename(&self) -> String {
        self.path.file_name().unwrap_or_default().to_string_lossy().into_owned()
    }

    /// Apply a hand-typed author, overriding whatever was detected. The
    /// detected series is kept, and so is the title -- except for any segment
    /// of it that *is* the typed author, which is dropped.
    ///
    /// That subtraction is the point: when canon can't identify the author it
    /// leaves every segment in the title, so the name the user is typing is
    /// usually still sitting in there. Without this, answering "CS Lewis" to
    /// "The Chronicles of Narnia - C S Lewis - The Magician's Nephew" spells
    /// the author twice.
    pub fn set_author(&mut self, name: &str) {
        let name = safe(name);
        let key = squash(&name);
        if !key.is_empty() {
            let kept: Vec<&str> = self.title.split(" - ").filter(|seg| squash(seg) != key).collect();
            // Never let the subtraction empty the title out entirely.
            if !kept.is_empty() {
                self.title = kept.join(" - ");
            }
        }
        self.author = (!name.is_empty()).then_some(name);
        self.confidence = Confidence::High;
    }
}

/// Split one path into author / series / title. Returns None if no title
/// text survives cleaning.
pub fn parse(path: &Path) -> Option<BookItem> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    // Brackets are read for a series before `clean` deletes them: "(Mistborn,
    // Book 1)" and "[Mistborn 01]" are series, "(Unabridged)" and "(2021)"
    // are not, and the regexes tell them apart without a special case.
    let mut series = BRACKETED
        .captures_iter(&stem)
        .filter_map(|c| as_series(clean(c.get(1).map_or("", |m| m.as_str())).trim()))
        .next();

    // Dotted or underscored names ("Brandon.Sanderson.-.The.Final.Empire")
    // carry no spaces to split on, so flatten those separators first.
    let flattened = stem.replace('_', " ");
    let flattened = if flattened.contains(' ') { flattened } else { flattened.replace('.', " ") };

    let mut segments: Vec<String> = SEGMENT
        .split(&flattened)
        .map(|s| trim_book_junk(&clean(s)))
        .filter(|s| !s.is_empty())
        .collect();

    // "... by Brandon Sanderson" is an explicit author credit wherever it
    // appears, and outranks any positional guess.
    let mut strong_author = None;
    for i in 0..segments.len() {
        let Some(caps) = BY_AUTHOR.captures(&segments[i]) else { continue };
        let author = caps.name("author").map_or("", |m| m.as_str()).trim().to_string();
        if author_score(&author) == 0 {
            continue;
        }
        let rest = caps.name("rest").map_or("", |m| m.as_str()).trim().to_string();
        strong_author = Some(author);
        segments[i] = rest;
        break;
    }
    segments.retain(|s| !s.is_empty());

    // "Sanderson, Brandon" is the other unambiguous form; reorder it.
    if strong_author.is_none() {
        for i in 0..segments.len() {
            let Some(caps) = SURNAME_FIRST.captures(&segments[i]) else { continue };
            let (last, first) = (caps.name("last").unwrap().as_str(), caps.name("first").unwrap().as_str());
            let name = format!("{} {}", first.trim(), last.trim());
            if author_score(&name) == 0 {
                continue;
            }
            strong_author = Some(name);
            segments.remove(i);
            break;
        }
    }

    // Otherwise the author is whichever segment looks most like a person.
    // Every segment is scored, not just the two ends: "Series - Author -
    // Title" is a real convention, as in
    // "The Chronicles of Narnia - C S Lewis - The Magician's Nephew".
    let mut confidence = Confidence::High;
    let author = match strong_author {
        Some(a) => Some(a),
        // One segment is all title -- there is nothing to take an author from.
        None if segments.len() < 2 => {
            confidence = Confidence::Low;
            None
        }
        None => {
            let scores: Vec<u32> = segments.iter().map(|s| author_score(s)).collect();
            let best = scores.iter().copied().max().unwrap_or(0);
            if best == 0 {
                confidence = Confidence::Low;
                None
            } else {
                // Several segments tied for the top score, as in "Storm Front
                // - Jim Butcher" where "Storm" is itself a given name.
                // Convention says the author leads, so the earliest wins --
                // but nothing corroborates it, so say so.
                if scores.iter().filter(|&&s| s == best).count() > 1 {
                    confidence = Confidence::Medium;
                }
                Some(segments.remove(scores.iter().position(|&s| s == best).unwrap()))
            }
        }
    };

    // A series may be claimed from any remaining segment except the last one
    // standing, which is always the title.
    if series.is_none() && segments.len() > 1 {
        if let Some(i) = segments.iter().take(segments.len() - 1).position(|s| as_series(s).is_some()) {
            series = as_series(&segments[i]);
            segments.remove(i);
        }
    }

    let title = safe(&segments.join(" - "));
    if title.is_empty() {
        return None;
    }
    Some(BookItem { path: path.to_path_buf(), title, author, series, ext, confidence })
}

/// The result of scanning one audiobooks directory: every file resolved
/// independently, plus whatever couldn't be parsed at all.
#[derive(Clone, Debug)]
pub struct BookPlan {
    pub items: Vec<BookItem>,
    pub skipped: Vec<(String, String)>,
}

/// Resolve every file in one audiobooks directory into a `BookPlan`.
pub fn plan_books(paths: &[PathBuf]) -> BookPlan {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for path in paths {
        let base = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match parse(path) {
            Some(item) => items.push(item),
            None => skipped.push((base, "no usable title".into())),
        }
    }
    items.sort_by(|a, b| {
        a.author.cmp(&b.author).then(a.series.cmp(&b.series)).then(a.title.cmp(&b.title))
    });
    BookPlan { items, skipped }
}
