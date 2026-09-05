//! Filename parsing for movies: find the year, everything before it is the title.
//!
//! Pure logic only -- no I/O, no prompting. `main` drives this.
//!
//! Unlike shows, a movies directory holds one file per *different* movie, so
//! there's no batch consensus to lean on -- each file is decided on its own.

use crate::parse::{clean, safe, Confidence};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// The anchor: a bare 19xx/20xx year. `\b` (a boundary the regex crate does
// support) keeps this from matching inside a longer run of digits, so a
// single-digit sequel number ("Incredibles 2") never collides with it.
//
// Take the LAST match in the filename, not the first: release names put the
// real year right before the quality tags, so if the title itself contains
// what looks like a year ("Blade Runner 2049"), the genuine release year
// still comes after it and wins.
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:19|20)\d{2}\b").unwrap());

fn find_year(stem: &str) -> Option<regex::Match<'_>> {
    YEAR.find_iter(stem).last()
}

/// One movie file: a title, and the release year if one was found.
#[derive(Clone, Debug)]
pub struct MovieItem {
    pub path: PathBuf,
    pub title: String,
    pub year: Option<u32>,
    pub ext: String,
    pub confidence: Confidence,
}

impl MovieItem {
    /// The renamed basename this file would get: "Title (Year).ext", or
    /// just "Title.ext" when no year was found.
    pub fn target(&self) -> String {
        match self.year {
            Some(y) => format!("{} ({}){}", self.title, y, self.ext),
            None => format!("{}{}", self.title, self.ext),
        }
    }

    pub fn basename(&self) -> String {
        self.path.file_name().unwrap_or_default().to_string_lossy().into_owned()
    }

    /// Apply a hand-typed title, overriding whatever was detected. The
    /// detected year (if any) is kept.
    pub fn set_title(&mut self, name: &str) {
        self.title = safe(name);
        self.confidence = Confidence::High;
    }
}

/// Split one path around its year anchor. Returns None if nothing usable
/// (no title text survives cleaning either side of a year, or at all).
pub fn parse(path: &Path) -> Option<MovieItem> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    let year_match = find_year(&stem);
    let (title_raw, year) = match year_match {
        Some(m) => (&stem[..m.start()], m.as_str().parse().ok()),
        None => (stem.as_str(), None),
    };
    let title = clean(title_raw);
    if title.is_empty() {
        return None;
    }
    let confidence = if year.is_some() { Confidence::High } else { Confidence::Low };
    Some(MovieItem { path: path.to_path_buf(), title, year, ext, confidence })
}

/// The result of scanning one movies directory: every file resolved
/// independently, plus whatever couldn't be parsed at all.
#[derive(Clone, Debug)]
pub struct MoviePlan {
    pub items: Vec<MovieItem>,
    pub skipped: Vec<(String, String)>,
}

/// Resolve every file in one movies directory into a `MoviePlan`.
pub fn plan_movies(paths: &[PathBuf]) -> MoviePlan {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for path in paths {
        let base = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match parse(path) {
            Some(item) => items.push(item),
            None => skipped.push((base, "no usable title".into())),
        }
    }
    items.sort_by(|a, b| a.title.cmp(&b.title).then(a.year.cmp(&b.year)));
    MoviePlan { items, skipped }
}
