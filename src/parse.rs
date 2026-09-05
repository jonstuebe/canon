//! Filename parsing: find the anchor, extract candidates, decide the show name.
//!
//! Pure logic only -- no I/O, no prompting. `main` drives this.
//!
//! `canon` operates on one show directory per invocation: every file passed
//! in is assumed to belong to the same show, so `plan_dir` settles on a
//! single name (using batch consensus across the whole directory to tell a
//! repeating show name from a one-off episode title) and applies it to
//! every file.

use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// The anchor. Everything hinges on locating this; the show name sits on one
// side of it and disposable release metadata on the other.
//
// Rust's regex crate has no lookaround, so the word-boundary guards that
// wrapped this pattern in the original are applied by hand in `find_anchor`.
static SXXEYY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        (?:
            s(?P<s1>\d{1,2})[\s._-]*e(?P<e1>\d{1,3})   # S07E08, s07.e08, S7 E8
          | (?P<s2>\d{1,2})x(?P<e2>\d{1,3})            # 7x08
          | season[\s._-]*(?P<s3>\d{1,2})[\s._-]*episode[\s._-]*(?P<e3>\d{1,3})
        )",
    )
    .unwrap()
});

// Release-metadata tokens, trimmed from the TRAILING end of a candidate only.
// Trailing-only is deliberate: it protects real title words such as the "US"
// in "The Office US" and the "SVU" in "Law & Order SVU".
static JUNK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)^(?:
        \d{3,4}p|\d{3,4}i|4k|uhd|hdr\d*|sdr|hevc|h\.?26[45]|x\.?26[45]|xvid|divx|av1|
        aac\d?|ac3|eac3|dts(?:-?hd)?|truehd|atmos|mp3|flac|\d+bit|\d+ch|
        web-?dl|web-?rip|webrip|bluray|blu-ray|bdrip|brrip|dvdrip|dvd|hdtv|pdtv|
        hdrip|remux|proper|repack|internal|extended|uncut|limited|complete|
        amzn|nf|hulu|dsnp|atvp|hmax|pcok|stan|itunes|
        subs?|multi|dual|ita|eng|vostfr|
        rarbg|yts|yify|ettv|eztv|fov|killers|sparks|ntb|ion10|
        upload|uploaded|by
        )$",
    )
    .unwrap()
});

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(BRACKETED, r"[\(\[\{][^\)\]\}]*[\)\]\}]");
re!(CODEC_DOT, r"(?i)\b([hx])[\s._-]?(26[45]|265)\b");
re!(HYPHEN_JOIN, r"(?i)\b(web|blu|dts)[-._ ](dl|rip|ray|hd)\b");
re!(DOMAIN, r"(?i)(?:www\.)?[a-z0-9-]+\.(?:com|net|org|to|tv|me|io|cc|info)\b");
re!(SCENE_GROUP, r"-[A-Za-z0-9]{2,}$");
// Separator dashes only: spaced, or a doubled run. An intra-word hyphen
// ("Spider-Man", "X-Men") is part of the title and must survive.
re!(DASHES, r"\s+[-\x{2013}\x{2014}]+\s*|[-\x{2013}\x{2014}]+\s+|[-\x{2013}\x{2014}]{2,}");
re!(SPACES, r"\s{2,}");
// A season marker trailing a directory name -- "Breaking Bad Season 5",
// "Breaking.Bad.S05" -- with no episode part, so the anchor regex above
// (which requires an episode number) never catches it.
re!(DIR_SEASON_TAIL, r"(?i)[\s._-]*(?:season[\s._-]*\d{1,2}|s\d{1,2})$");

/// Reduce one side of the anchor to a bare show-name candidate.
pub fn clean(raw: &str) -> String {
    let s = BRACKETED.replace_all(raw, " "); // (Kaley Cuoco), [HorribleSubs], {2007}
    let s = DOMAIN.replace_all(&s, " "); // www.Torrenting.com
    let s = CODEC_DOT.replace_all(&s, "$1$2"); // H.264 -> H264, before dots are flattened
    let s = HYPHEN_JOIN.replace_all(&s, "$1$2"); // WEB-DL -> WEBDL, Blu-ray -> Bluray
    let trimmed = s.trim();
    // A dotted scene name ends in the release group: "...x264-GROUP". Require
    // dot delimiting, not merely the absence of spaces -- otherwise a hyphenated
    // title such as "Spider-Man" loses its second half.
    let s = if trimmed.contains(' ') || !trimmed.contains('.') {
        trimmed.to_string()
    } else {
        SCENE_GROUP.replace(trimmed, "").into_owned()
    };
    let s = s.replace(['_', '.'], " ");
    let s = DASHES.replace_all(&s, " ").into_owned();

    let mut tokens: Vec<&str> = s.split_whitespace().collect();
    while tokens.last().is_some_and(|t| JUNK.is_match(t)) {
        tokens.pop();
    }
    let joined = tokens.join(" ");
    SPACES
        .replace_all(&joined, " ")
        .trim_matches(|c| c == ' ' || c == '-' || c == ',')
        .to_string()
}

/// Strip characters no filesystem will accept in a basename.
pub fn safe(name: &str) -> String {
    let stripped: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control())
        .collect();
    SPACES.replace_all(&stripped, " ").trim().to_string()
}

/// Locate the anchor, enforcing by hand the boundaries the crate can't express.
fn find_anchor(stem: &str) -> Option<regex::Captures<'_>> {
    for caps in SXXEYY.captures_iter(stem) {
        let m = caps.get(0).unwrap();
        let before_ok = stem[..m.start()].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after_ok = stem[m.end()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return Some(caps);
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// One file, with a show-name candidate drawn from each side of the anchor.
#[derive(Clone, Debug)]
pub struct Episode {
    pub path: PathBuf,
    pub season: u32,
    pub episode: u32,
    pub ext: String,
    pub left: String,
    pub right: String,
    pub dir: String,
}

impl Episode {
    /// The renamed basename this episode would get under the given show name.
    pub fn target_with(&self, name: &str) -> String {
        format!("{} S{:02}E{:02}{}", name, self.season, self.episode, self.ext)
    }

    pub fn basename(&self) -> String {
        self.path.file_name().unwrap_or_default().to_string_lossy().into_owned()
    }
}

/// Basename of a path's parent directory, without touching the filesystem.
fn parent_name(path: &Path) -> String {
    let parent = path.parent().unwrap_or(Path::new(""));
    let abs = if parent.as_os_str().is_empty() || parent.is_relative() {
        std::env::current_dir().unwrap_or_default().join(parent)
    } else {
        parent.to_path_buf()
    };
    // Resolve trailing "." / ".." components lexically so "dir/." keeps "dir".
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for c in abs.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(p) => parts.push(p.to_os_string()),
            _ => {}
        }
    }
    parts.last().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Split one path around its anchor. Returns None if it has no anchor.
pub fn parse(path: &Path) -> Option<Episode> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let caps = find_anchor(&stem)?;
    let m = caps.get(0).unwrap();
    let num = |a: &str, b: &str, c: &str| -> u32 {
        caps.name(a).or_else(|| caps.name(b)).or_else(|| caps.name(c))
            .and_then(|v| v.as_str().parse().ok())
            .unwrap_or(0)
    };
    let dir_raw = parent_name(path);
    let dir_raw = DIR_SEASON_TAIL.replace(&dir_raw, "");
    Some(Episode {
        path: path.to_path_buf(),
        season: num("s1", "s2", "s3"),
        episode: num("e1", "e2", "e3"),
        ext,
        left: clean(&stem[..m.start()]),
        right: clean(&stem[m.end()..]),
        dir: clean(&dir_raw),
    })
}

/// How season numbers look across every file in the directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Season {
    /// No file had an anchor to read a season number from.
    Unknown,
    Single(u32),
    /// Files disagreed on the season number -- shown as a range.
    Varies(u32, u32),
}

fn season_summary(episodes: &[Episode]) -> Season {
    let mut seasons: Vec<u32> = episodes.iter().map(|e| e.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    match seasons.as_slice() {
        [] => Season::Unknown,
        [only] => Season::Single(*only),
        [first, .., last] => Season::Varies(*first, *last),
    }
}

/// Pick a single show name for the whole directory.
///
/// The core ambiguity: text after the anchor is either the show name or the
/// episode title, and one filename cannot distinguish them. Batch frequency
/// can -- show names repeat across a season, episode titles do not. When
/// neither side repeats, the directory name itself (the folder the user
/// pointed canon at) is the fallback, since it was chosen by a person to
/// describe what's in it.
fn decide_dir(episodes: &[Episode]) -> (String, Confidence) {
    let mut left_counts: HashMap<&str, usize> = HashMap::new();
    let mut right_counts: HashMap<&str, usize> = HashMap::new();
    for e in episodes {
        if !e.left.is_empty() {
            *left_counts.entry(e.left.as_str()).or_insert(0) += 1;
        }
        if !e.right.is_empty() {
            *right_counts.entry(e.right.as_str()).or_insert(0) += 1;
        }
    }
    let top = |m: &HashMap<&str, usize>| -> Option<(String, usize)> {
        m.iter().max_by_key(|&(_, &c)| c).map(|(k, &c)| (k.to_string(), c))
    };
    let total = episodes.len();
    let full = |c: usize| c > 1 && c == total;

    match (top(&left_counts), top(&right_counts)) {
        (Some((lname, lc)), Some((rname, rc))) => {
            if rc > 1 && lc <= 1 {
                (rname, if full(rc) { Confidence::High } else { Confidence::Medium })
            } else if lc > 1 {
                (lname, if full(lc) { Confidence::High } else { Confidence::Medium })
            } else if rc > 1 {
                (rname, Confidence::Medium)
            } else {
                (lname, Confidence::Medium) // single file, or nothing repeats: default left
            }
        }
        (Some((lname, lc)), None) => (lname, if lc > 1 { Confidence::High } else { Confidence::Medium }),
        (None, Some((rname, rc))) => (rname, if rc > 1 { Confidence::High } else { Confidence::Low }),
        (None, None) => {
            let dir = episodes.first().map(|e| e.dir.clone()).unwrap_or_default();
            if !dir.is_empty() {
                (dir, Confidence::Medium)
            } else {
                (String::new(), Confidence::Low)
            }
        }
    }
}

/// The result of scanning one show directory: a single name and season
/// picture applied to every file in it, plus whatever couldn't be parsed.
#[derive(Clone, Debug)]
pub struct Plan {
    pub name: String,
    pub confidence: Confidence,
    pub season: Season,
    pub episodes: Vec<Episode>,
    pub skipped: Vec<(String, String)>,
}

impl Plan {
    /// One example rename, to show what the chosen name looks like applied.
    pub fn preview(&self) -> Option<String> {
        self.episodes.first().map(|e| e.target_with(&self.name))
    }

    pub fn targets(&self) -> Vec<(PathBuf, String)> {
        self.episodes.iter().map(|e| (e.path.clone(), e.target_with(&self.name))).collect()
    }

    /// Apply a hand-typed name, overriding whatever was detected.
    pub fn set_name(&mut self, name: &str) {
        self.name = safe(name);
        self.confidence = Confidence::High;
    }
}

/// Resolve every file in one show directory into a single `Plan`.
pub fn plan_dir(paths: &[PathBuf]) -> Plan {
    let mut episodes = Vec::new();
    let mut skipped = Vec::new();
    for path in paths {
        let base = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match parse(path) {
            Some(e) => episodes.push(e),
            None => skipped.push((base, "no SxxEyy anchor".into())),
        }
    }
    episodes.sort_by_key(|e| (e.season, e.episode));
    let (name, confidence) = decide_dir(&episodes);
    let season = season_summary(&episodes);
    Plan { name, confidence, season, episodes, skipped }
}
