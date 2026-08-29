//! Filename parsing: find the anchor, extract candidates, decide between them.
//!
//! Pure logic only -- no I/O, no prompting. `main` drives this.

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
pub enum Side {
    Left,
    Right,
    Dir,
}

impl Side {
    pub const ALL: [Side; 3] = [Side::Left, Side::Right, Side::Dir];
    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "text before SxxEyy",
            Side::Right => "text after SxxEyy",
            Side::Dir => "folder name",
        }
    }
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
    pub fn candidate(&self, side: Side) -> &str {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
            Side::Dir => &self.dir,
        }
    }

    pub fn target(&self, side: Side, override_name: Option<&str>) -> String {
        let name = override_name.unwrap_or_else(|| self.candidate(side));
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
    Some(Episode {
        path: path.to_path_buf(),
        season: num("s1", "s2", "s3"),
        episode: num("e1", "e2", "e3"),
        ext,
        left: clean(&stem[..m.start()]),
        right: clean(&stem[m.end()..]),
        dir: clean(&parent_name(path)),
    })
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub side: Option<Side>,
    pub confidence: Confidence,
    pub reason: &'static str,
}

/// Pick which side of the anchor holds the show name.
///
/// The core ambiguity: text after the anchor is either the show name or the
/// episode title, and one filename cannot distinguish them. Batch frequency
/// can -- show names repeat across a season, episode titles do not.
pub fn decide(
    ep: &Episode,
    left_counts: &HashMap<String, usize>,
    right_counts: &HashMap<String, usize>,
    prefer: Option<Side>,
    use_dir: bool,
) -> Decision {
    let d = |side, confidence, reason| Decision { side: Some(side), confidence, reason };
    if let Some(side) = prefer {
        return d(side, Confidence::High, "forced by --prefer");
    }
    let count = |m: &HashMap<String, usize>, k: &str| *m.get(k).unwrap_or(&0);
    let (has_left, has_right) = (!ep.left.is_empty(), !ep.right.is_empty());

    if has_left && !has_right {
        let c = if count(left_counts, &ep.left) > 1 { Confidence::High } else { Confidence::Medium };
        return d(Side::Left, c, "only the left side survived cleaning");
    }
    if has_right && !has_left {
        if count(right_counts, &ep.right) > 1 {
            return d(Side::Right, Confidence::High, "right side, repeats across the batch");
        }
        if !ep.dir.is_empty() && use_dir {
            return d(Side::Dir, Confidence::Medium, "--fallback-dir: name after anchor is unique");
        }
        return d(Side::Right, Confidence::Low,
            "right side only, and it does NOT repeat -- could be an episode title");
    }
    if has_left && has_right {
        if count(right_counts, &ep.right) > 1 && count(left_counts, &ep.left) == 1 {
            return d(Side::Right, Confidence::High, "right side repeats, left side is unique");
        }
        let c = if count(left_counts, &ep.left) > 1 { Confidence::High } else { Confidence::Medium };
        return d(Side::Left, c, "both sides populated, defaulted to left");
    }
    if !ep.dir.is_empty() {
        return d(Side::Dir, Confidence::Low, "no candidate on either side, used folder name");
    }
    Decision { side: None, confidence: Confidence::Low, reason: "no show name found" }
}

/// A set of episodes that resolved to the same show name.
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub side: Side,
    pub confidence: Confidence,
    pub reason: String,
    pub files: Vec<Episode>,
    /// Set when the user typed a name by hand; wins over `side`.
    pub override_name: Option<String>,
}

impl Group {
    pub fn display_name(&self) -> &str {
        self.override_name.as_deref().unwrap_or(&self.name)
    }
    pub fn targets(&self) -> Vec<(PathBuf, String)> {
        self.files.iter()
            .map(|e| (e.path.clone(), e.target(self.side, self.override_name.as_deref())))
            .collect()
    }
    /// Distinct values a side takes across this group, in order.
    pub fn values_for(&self, side: Side) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for e in &self.files {
            let v = e.candidate(side);
            if !v.is_empty() && !seen.contains(&v) {
                seen.push(v);
            }
        }
        seen
    }
    /// A side is offerable only if every file in the group has one.
    pub fn offerable(&self, side: Side) -> bool {
        self.files.iter().all(|e| !e.candidate(side).is_empty())
    }
}

pub struct Options {
    pub prefer: Option<Side>,
    pub use_dir: bool,
    pub show: Option<String>,
}

/// Resolve a whole batch into groups keyed by show name, plus skips.
///
/// Batch-wide by design: `decide` needs to see every filename before it can
/// settle any single one, so this cannot be applied file-by-file.
pub fn plan(paths: &[PathBuf], opts: &Options) -> (Vec<Group>, Vec<(String, String)>) {
    let parsed: Vec<(&PathBuf, Option<Episode>)> =
        paths.iter().map(|p| (p, parse(p))).collect();

    let mut left_counts: HashMap<String, usize> = HashMap::new();
    let mut right_counts: HashMap<String, usize> = HashMap::new();
    for (_, ep) in &parsed {
        if let Some(e) = ep {
            if !e.left.is_empty() {
                *left_counts.entry(e.left.clone()).or_insert(0) += 1;
            }
            if !e.right.is_empty() {
                *right_counts.entry(e.right.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut groups: Vec<Group> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for (path, ep) in parsed {
        let base = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Some(ep) = ep else {
            skipped.push((base, "no SxxEyy anchor".into()));
            continue;
        };
        let (side, confidence, reason, name) = if let Some(show) = &opts.show {
            (Side::Left, Confidence::High, "--show override", safe(show))
        } else {
            let d = decide(&ep, &left_counts, &right_counts, opts.prefer, opts.use_dir);
            let Some(side) = d.side else {
                skipped.push((base, d.reason.into()));
                continue;
            };
            let name = ep.candidate(side).to_string();
            if name.is_empty() {
                skipped.push((base, "the chosen side was empty".into()));
                continue;
            }
            (side, d.confidence, d.reason, name)
        };

        match groups.iter_mut().find(|g| {
            g.name == name && g.side == side && g.confidence == confidence && g.reason == reason
        }) {
            Some(g) => g.files.push(ep),
            None => groups.push(Group {
                name,
                side,
                confidence,
                reason: reason.to_string(),
                files: vec![ep],
                override_name: opts.show.as_ref().map(|s| safe(s)),
            }),
        }
    }
    for g in &mut groups {
        g.files.sort_by_key(|e| (e.season, e.episode));
    }
    (groups, skipped)
}
