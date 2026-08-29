//! Behavioural tests for the parser and the side-decision logic.
//!
//! Absolute paths are used throughout so the `dir` candidate is deterministic
//! and never depends on the directory the test runner happens to start in.

use canon::parse::{clean, parse, plan, safe, Confidence, Episode, Group, Options, Side};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn opts() -> Options {
    Options { prefer: None, use_dir: false, show: None }
}

fn ep(name: &str) -> Episode {
    parse(Path::new(name)).unwrap_or_else(|| panic!("no anchor found in {name:?}"))
}

/// Run a batch through `plan`, accepting every default. -> {input basename: new name}
fn auto_with(files: &[&str], o: Options) -> HashMap<String, String> {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let (groups, _) = plan(&paths, &o);
    let mut out = HashMap::new();
    for g in &groups {
        for (path, new) in g.targets() {
            out.insert(path.file_name().unwrap().to_string_lossy().into_owned(), new);
        }
    }
    out
}

fn auto(files: &[&str]) -> HashMap<String, String> {
    auto_with(files, opts())
}

fn groups_of(files: &[&str]) -> Vec<Group> {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    plan(&paths, &opts()).0
}

fn got<'a>(m: &'a HashMap<String, String>, k: &str) -> &'a str {
    m.get(k).unwrap_or_else(|| panic!("{k:?} missing; have {:?}", m.keys().collect::<Vec<_>>()))
}

// ---------------------------------------------------------------- anchors --

#[test]
fn anchor_standard_sxxeyy() {
    let e = ep("/m/Show.S07E08.mkv");
    assert_eq!((e.season, e.episode), (7, 8));
}

#[test]
fn anchor_accepts_every_spelling() {
    for (name, want) in [
        ("/m/Show S07E08.mkv", (7, 8)),
        ("/m/Show.s07.e08.mkv", (7, 8)),
        ("/m/Show S7 E8.mkv", (7, 8)),
        ("/m/Show.7x08.mkv", (7, 8)),
        ("/m/Show 1x02.mkv", (1, 2)),
        ("/m/Show - Season 2 Episode 5.mkv", (2, 5)),
        ("/m/Show season 12 episode 3.mkv", (12, 3)),
        ("/m/Show s01e02.mkv", (1, 2)),
        ("/m/Show S01_E02.mkv", (1, 2)),
        ("/m/Show S01-E02.mkv", (1, 2)),
    ] {
        let e = ep(name);
        assert_eq!((e.season, e.episode), want, "for {name}");
    }
}

#[test]
fn anchor_pads_and_preserves_numbers() {
    assert_eq!(ep("/m/24.S1E1.mkv").target(Side::Left, None), "24 S01E01.mkv");
    assert_eq!(ep("/m/Show S00E01.mkv").target(Side::Left, None), "Show S00E01.mkv");
    assert_eq!(ep("/m/Show S01E100.mkv").target(Side::Left, None), "Show S01E100.mkv");
    assert_eq!(ep("/m/Show S12E345.mkv").target(Side::Left, None), "Show S12E345.mkv");
}

#[test]
fn anchor_absent_returns_none() {
    for name in [
        "/m/Some Movie 2019 1080p.mkv",
        "/m/The Daily Show 2024.03.14.mkv",  // date-based, documented gap
        "/m/Show Name - 043 [1080p].mkv",    // anime absolute numbering, documented gap
        "/m/vacation-photo.jpg",
        "/m/.hidden",
        "/m/",
    ] {
        assert!(parse(Path::new(name)).is_none(), "should not have parsed {name}");
    }
}

#[test]
fn anchor_is_not_confused_by_lookalikes() {
    // x264 / x265 must not read as the NxNN form, and resolutions are not anchors.
    assert!(parse(Path::new("/m/Movie.1080p.x264-GRP.mkv")).is_none());
    assert!(parse(Path::new("/m/Movie.720p.h264.mkv")).is_none());
    // A bare year is not a season/episode.
    assert!(parse(Path::new("/m/Movie 2019.mkv")).is_none());
}

#[test]
fn anchor_requires_clean_boundaries() {
    // Glued to surrounding alphanumerics, this is not an anchor.
    assert!(parse(Path::new("/m/PASSWORDS01E02X.mkv")).is_none());
    // But punctuation on either side is fine.
    assert_eq!((ep("/m/Show.S01E02.mkv").season, ep("/m/Show.S01E02.mkv").episode), (1, 2));
}

// --------------------------------------------------------------- cleaning --

#[test]
fn clean_strips_bracketed_asides() {
    assert_eq!(clean("The Big Bang Theory (Kaley Cuoco)"), "The Big Bang Theory");
    assert_eq!(clean("[HorribleSubs] Show Name"), "Show Name");
    assert_eq!(clean("Doctor Who (2005)"), "Doctor Who");
    assert_eq!(clean("Show {2007} [x264]"), "Show");
}

#[test]
fn clean_strips_tracker_domains() {
    assert_eq!(clean("www.Torrenting.com - Severance"), "Severance");
    assert_eq!(clean("rarbg.to Show Name"), "Show Name");
}

#[test]
fn clean_normalises_separators() {
    assert_eq!(clean("Breaking.Bad"), "Breaking Bad");
    assert_eq!(clean("Breaking_Bad"), "Breaking Bad");
    assert_eq!(clean("Law & Order- SVU"), "Law & Order SVU");
    // Separator dashes go; hyphens inside a title stay.
    assert_eq!(clean("Show - Name"), "Show Name");
    assert_eq!(clean("Show -Name"), "Show Name");
    assert_eq!(clean("Show--Name"), "Show Name");
    assert_eq!(clean("X-Men"), "X-Men");
    assert_eq!(clean("Kolchak- The Night Stalker"), "Kolchak The Night Stalker");
    assert_eq!(clean("Show   Name"), "Show Name");
}

#[test]
fn clean_trims_release_metadata_from_the_end() {
    assert_eq!(clean("Severance 2160p HDR AMZN WEB-DL"), "Severance");
    assert_eq!(clean("Show 1080p H.264"), "Show");
    assert_eq!(clean("Show 720p Blu-ray DTS-HD x265"), "Show");
    assert_eq!(clean("Show HDTV XviD"), "Show");
    assert_eq!(clean("Show WEBRip AAC5 10bit"), "Show");
}

#[test]
fn clean_strips_scene_release_group_suffix() {
    assert_eq!(clean("Ozymandias.1080p.WEB-DL.x264-GROUP"), "Ozymandias");
    assert_eq!(clean("Show.720p.HDTV.x264-FQM"), "Show");
    // ...but only on dot-delimited scene names. A hyphenated title must survive,
    // with or without surrounding spaces.
    assert_eq!(clean("Spider-Man"), "Spider-Man");
    assert_eq!(clean("Marvel's Spider-Man"), "Marvel's Spider-Man");
    assert_eq!(clean("Ratched-"), "Ratched");
    // A dotted title whose last token is a real word is the unavoidable cost:
    // "It's.Always.Sunny-FQM" cannot be told from "Show.x264-FQM".
    assert_eq!(clean("Show.1080p.WEBRip-NTb"), "Show");
}

#[test]
fn clean_trims_only_the_trailing_end() {
    // This is what protects real title words that collide with junk tokens.
    assert_eq!(clean("The Office US"), "The Office US");
    assert_eq!(clean("Law & Order SVU"), "Law & Order SVU");
    assert_eq!(clean("Max"), "Max");
    assert_eq!(clean("Web Therapy"), "Web Therapy");
    // Leading junk survives when a real word follows it -- a documented gap.
    assert_eq!(clean("HDTV Some Show"), "HDTV Some Show");
    // ...but a side that is nothing but junk collapses to empty.
    assert_eq!(clean("HDTV"), "");
    assert_eq!(clean("[SubsPlease]"), "");
    assert_eq!(clean("1080p WEB-DL x264"), "");
}

#[test]
fn clean_handles_unicode_and_empty_input() {
    assert_eq!(clean(""), "");
    assert_eq!(clean("   "), "");
    assert_eq!(clean("Pokémon"), "Pokémon");
    assert_eq!(clean("Show – Name"), "Show Name"); // en dash
    assert_eq!(clean("Show — Name"), "Show Name"); // em dash
    assert_eq!(clean("Æon Flux"), "Æon Flux");
    assert_eq!(clean("日本のショー"), "日本のショー");
}

// ------------------------------------------------------------- extensions --

#[test]
fn extension_is_lowercased_and_preserved() {
    assert_eq!(ep("/m/Show S01E01.MKV").ext, ".mkv");
    assert_eq!(ep("/m/Show S01E01.Mp4").ext, ".mp4");
    assert_eq!(ep("/m/Show S01E01.avi").ext, ".avi");
    assert_eq!(ep("/m/Show S01E01").ext, ""); // no extension at all
}

#[test]
fn extension_absent_still_renames() {
    assert_eq!(ep("/m/Show S01E01").target(Side::Left, None), "Show S01E01");
}

// ------------------------------------------------------- side extraction --

#[test]
fn candidates_are_taken_from_both_sides() {
    let e = ep("/media/Breaking.Bad.S05E14.Ozymandias.1080p.WEB-DL.x264-GROUP.mkv");
    assert_eq!(e.left, "Breaking Bad");
    assert_eq!(e.right, "Ozymandias");
    assert_eq!(e.dir, "media");
}

#[test]
fn dir_candidate_comes_from_the_parent_folder() {
    assert_eq!(ep("/Volumes/Media/Severance/S02E05 - Cold Harbor.mkv").dir, "Severance");
    assert_eq!(ep("/Volumes/Media/The.Wire/S03E07.mkv").dir, "The Wire");
}

// ------------------------------------------------------- name before anchor --

#[test]
fn show_name_before_the_anchor() {
    let m = auto(&[
        "/m/The Big Bang Theory (Kaley Cuoco) S07E08 1080p H.264 (moviesbyrizzo upload).mp4",
        "/m/Breaking.Bad.S05E14.Ozymandias.1080p.WEB-DL.x264-GROUP.mkv",
        "/m/its.always.sunny.in.philadelphia.7x03.HDTV.XviD-FQM.avi",
        "/m/The.Office.US.S03E10.720p.BluRay.x265-RARBG.mp4",
        "/m/Doctor Who (2005) S11E01 1080p.mkv",
        "/m/24.S1E1.mkv",
        "/m/Law & Order- SVU S22E03 WEBRip.mp4",
        "/m/The Wire S03E07 Back Burners 720p Blu-ray DTS-HD x265.mkv",
        "/m/Severance - Season 2 Episode 5 - 2160p HDR AMZN WEB-DL.mp4",
    ]);
    assert_eq!(got(&m, "The Big Bang Theory (Kaley Cuoco) S07E08 1080p H.264 (moviesbyrizzo upload).mp4"), "The Big Bang Theory S07E08.mp4");
    assert_eq!(got(&m, "Breaking.Bad.S05E14.Ozymandias.1080p.WEB-DL.x264-GROUP.mkv"), "Breaking Bad S05E14.mkv");
    assert_eq!(got(&m, "its.always.sunny.in.philadelphia.7x03.HDTV.XviD-FQM.avi"), "its always sunny in philadelphia S07E03.avi");
    assert_eq!(got(&m, "The.Office.US.S03E10.720p.BluRay.x265-RARBG.mp4"), "The Office US S03E10.mp4");
    assert_eq!(got(&m, "Doctor Who (2005) S11E01 1080p.mkv"), "Doctor Who S11E01.mkv");
    assert_eq!(got(&m, "24.S1E1.mkv"), "24 S01E01.mkv");
    assert_eq!(got(&m, "Law & Order- SVU S22E03 WEBRip.mp4"), "Law & Order SVU S22E03.mp4");
    assert_eq!(got(&m, "The Wire S03E07 Back Burners 720p Blu-ray DTS-HD x265.mkv"), "The Wire S03E07.mkv");
    assert_eq!(got(&m, "Severance - Season 2 Episode 5 - 2160p HDR AMZN WEB-DL.mp4"), "Severance S02E05.mp4");
}

// -------------------------------------------------------- name after anchor --

#[test]
fn show_name_after_the_anchor_when_left_is_junk() {
    let m = auto(&[
        "/m/[SubsPlease] S01E02 - Frieren [1080p][A1B2].mkv",
        "/m/www.Torrenting.com - S02E05 - Severance - 2160p HDR AMZN WEB-DL.mp4",
        "/m/S07E08 - The Big Bang Theory 1080p H.264 (moviesbyrizzo upload).mp4",
    ]);
    assert_eq!(got(&m, "[SubsPlease] S01E02 - Frieren [1080p][A1B2].mkv"), "Frieren S01E02.mkv");
    assert_eq!(got(&m, "www.Torrenting.com - S02E05 - Severance - 2160p HDR AMZN WEB-DL.mp4"), "Severance S02E05.mp4");
    assert_eq!(got(&m, "S07E08 - The Big Bang Theory 1080p H.264 (moviesbyrizzo upload).mp4"), "The Big Bang Theory S07E08.mp4");
}

#[test]
fn batch_consensus_picks_the_repeating_side() {
    // Episode title before the anchor, show name after it. Only the repetition
    // across the batch reveals which is which.
    let m = auto(&[
        "/m/Ozymandias.S05E14.Breaking.Bad.1080p.mkv",
        "/m/Granite.State.S05E15.Breaking.Bad.1080p.mkv",
        "/m/Felina.S05E16.Breaking.Bad.1080p.mkv",
    ]);
    assert_eq!(got(&m, "Ozymandias.S05E14.Breaking.Bad.1080p.mkv"), "Breaking Bad S05E14.mkv");
    assert_eq!(got(&m, "Felina.S05E16.Breaking.Bad.1080p.mkv"), "Breaking Bad S05E16.mkv");
}

#[test]
fn a_single_file_cannot_resolve_that_ambiguity() {
    // Alone, the same filename defaults to the left side and gets it wrong.
    // This is the case the interactive confirm step exists to catch.
    let m = auto(&["/m/Ozymandias.S05E14.Breaking.Bad.1080p.mkv"]);
    assert_eq!(got(&m, "Ozymandias.S05E14.Breaking.Bad.1080p.mkv"), "Ozymandias S05E14.mkv");
}

#[test]
fn repeating_left_side_is_not_overruled_by_a_repeating_right_side() {
    // Both repeat: left must win, or a re-released season would flip.
    let m = auto(&[
        "/m/Breaking.Bad.S05E14.1080p.WEBDL.mkv",
        "/m/Breaking.Bad.S05E15.1080p.WEBDL.mkv",
    ]);
    assert_eq!(got(&m, "Breaking.Bad.S05E14.1080p.WEBDL.mkv"), "Breaking Bad S05E14.mkv");
}

// ------------------------------------------------------------- confidence --

#[test]
fn confidence_reflects_how_much_evidence_there_was() {
    // Repeated across the batch -> high.
    let g = groups_of(&["/m/Show.S01E01.1080p.mkv", "/m/Show.S01E02.1080p.mkv"]);
    assert_eq!(g[0].confidence, Confidence::High);

    // A lone file with only one viable side -> medium.
    let g = groups_of(&["/m/Show.S01E01.1080p.mkv"]);
    assert_eq!(g[0].confidence, Confidence::Medium);

    // Name after the anchor that never repeats -> low, because it is
    // indistinguishable from an episode title.
    let g = groups_of(&["/m/Severance/S02E05 - Cold Harbor 1080p.mkv"]);
    assert_eq!(g[0].confidence, Confidence::Low);
    assert_eq!(g[0].side, Side::Right);
    assert_eq!(g[0].name, "Cold Harbor");
}

// ----------------------------------------------------------------- options --

#[test]
fn prefer_forces_a_side() {
    let files = ["/m/Breaking.Bad.S05E14.Ozymandias.1080p.mkv"];
    let m = auto_with(&files, Options { prefer: Some(Side::Right), use_dir: false, show: None });
    assert_eq!(got(&m, "Breaking.Bad.S05E14.Ozymandias.1080p.mkv"), "Ozymandias S05E14.mkv");
    let m = auto_with(&files, Options { prefer: Some(Side::Left), use_dir: false, show: None });
    assert_eq!(got(&m, "Breaking.Bad.S05E14.Ozymandias.1080p.mkv"), "Breaking Bad S05E14.mkv");
}

#[test]
fn fallback_dir_uses_the_folder_for_unique_after_anchor_names() {
    let files = ["/Volumes/Media/Severance/S02E05 - Cold Harbor 1080p.mkv"];
    let m = auto_with(&files, Options { prefer: None, use_dir: true, show: None });
    assert_eq!(got(&m, "S02E05 - Cold Harbor 1080p.mkv"), "Severance S02E05.mkv");
    // Off by default: a real extracted name should not lose to a folder guess.
    let m = auto(&files);
    assert_eq!(got(&m, "S02E05 - Cold Harbor 1080p.mkv"), "Cold Harbor S02E05.mkv");
}

#[test]
fn show_override_wins_everywhere_and_is_sanitised() {
    let files = ["/m/whatever.S01E01.mkv", "/m/[grp] S01E02 - Other.mkv"];
    let m = auto_with(&files, Options { prefer: None, use_dir: false, show: Some("A/B: Show".into()) });
    assert_eq!(got(&m, "whatever.S01E01.mkv"), "AB Show S01E01.mkv");
    assert_eq!(got(&m, "[grp] S01E02 - Other.mkv"), "AB Show S01E02.mkv");
}

// ------------------------------------------------------------------ safe() --

#[test]
fn safe_removes_characters_filesystems_reject() {
    assert_eq!(safe("Show/Name"), "ShowName");
    assert_eq!(safe("Show: The Return"), "Show The Return");
    assert_eq!(safe(r#"a\b*c?d"e<f>g|h"#), "abcdefgh");
    assert_eq!(safe("  padded  "), "padded");
    assert_eq!(safe("tab\there"), "tabhere");
}

// ------------------------------------------------------------ grouping API --

#[test]
fn plan_groups_by_show_and_sorts_by_episode() {
    let g = groups_of(&[
        "/m/Show.S01E03.1080p.mkv",
        "/m/Show.S01E01.1080p.mkv",
        "/m/Other.S02E01.1080p.mkv",
        "/m/Show.S01E02.1080p.mkv",
    ]);
    assert_eq!(g.len(), 2);
    let show = g.iter().find(|x| x.name == "Show").unwrap();
    assert_eq!(show.files.len(), 3);
    assert_eq!(show.files.iter().map(|e| e.episode).collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn plan_reports_skips_without_dropping_them_silently() {
    let paths: Vec<PathBuf> = ["/m/Show.S01E01.mkv", "/m/Some Movie 2019.mkv"]
        .iter().map(PathBuf::from).collect();
    let (groups, skipped) = plan(&paths, &opts());
    assert_eq!(groups.len(), 1);
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].0, "Some Movie 2019.mkv");
    assert!(skipped[0].1.contains("anchor"));
}

#[test]
fn group_offers_only_sides_every_file_has() {
    let g = groups_of(&["/m/Show.S01E01.1080p.mkv", "/m/Show.S01E02.Title.1080p.mkv"]);
    let show = &g[0];
    assert!(show.offerable(Side::Left));
    assert!(show.offerable(Side::Dir));
    // Only the second file has text after the anchor, so "right" is not offerable.
    assert!(!show.offerable(Side::Right));
}

#[test]
fn group_values_for_lists_distinct_candidates_in_order() {
    let g = groups_of(&[
        "/m/Ozymandias.S05E14.Breaking.Bad.1080p.mkv",
        "/m/Granite.State.S05E15.Breaking.Bad.1080p.mkv",
    ]);
    let show = &g[0];
    assert_eq!(show.values_for(Side::Right), vec!["Breaking Bad"]);
    assert_eq!(show.values_for(Side::Left), vec!["Ozymandias", "Granite State"]);
}

#[test]
fn group_override_beats_the_chosen_side() {
    let mut g = groups_of(&["/m/Show.S01E01.mkv"]).remove(0);
    g.override_name = Some("Renamed".into());
    assert_eq!(g.targets()[0].1, "Renamed S01E01.mkv");
    assert_eq!(g.display_name(), "Renamed");
}

// ------------------------------------------------------ known limitations --

#[test]
fn documented_gaps_behave_predictably() {
    // Multi-episode files keep only the first episode number.
    let m = auto(&["/m/Show.S01E01-E02.mkv"]);
    assert_eq!(got(&m, "Show.S01E01-E02.mkv"), "Show S01E01.mkv");

    // An unbracketed year stays in the title (stripping it would break
    // legitimate titles such as "Class of 1999").
    let m = auto(&["/m/Doctor Who 2005 S11E01 1080p.mkv"]);
    assert_eq!(got(&m, "Doctor Who 2005 S11E01 1080p.mkv"), "Doctor Who 2005 S11E01.mkv");

    // Initialisms written with dots lose them along with the separators.
    let m = auto(&["/m/Marvel's Agents of S.H.I.E.L.D. S03E12 1080p.mkv"]);
    assert_eq!(got(&m, "Marvel's Agents of S.H.I.E.L.D. S03E12 1080p.mkv"),
               "Marvel's Agents of S H I E L D S03E12.mkv");

    // Casing is preserved, never corrected.
    let m = auto(&["/m/its.always.sunny.S07E03.mkv"]);
    assert_eq!(got(&m, "its.always.sunny.S07E03.mkv"), "its always sunny S07E03.mkv");
}

#[test]
fn already_canonical_names_are_stable() {
    // Running canon twice must not drift.
    let m = auto(&["/m/Severance S02E05.mkv"]);
    assert_eq!(got(&m, "Severance S02E05.mkv"), "Severance S02E05.mkv");
    let m = auto(&["/m/The Office US S03E10.mp4"]);
    assert_eq!(got(&m, "The Office US S03E10.mp4"), "The Office US S03E10.mp4");
}
