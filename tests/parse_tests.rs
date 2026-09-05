//! Behavioural tests for the parser and the single-name-per-directory logic.
//!
//! Absolute paths are used throughout so the `dir` candidate is deterministic
//! and never depends on the directory the test runner happens to start in.
//! All test files share a directory (`/m/` or similar) since `canon` now
//! resolves one show name per directory rather than per batch.

use canon::parse::{clean, parse, plan_dir, safe, Confidence, Season};
use std::path::{Path, PathBuf};

fn ep(name: &str) -> canon::parse::Episode {
    parse(Path::new(name)).unwrap_or_else(|| panic!("no anchor found in {name:?}"))
}

fn plan(files: &[&str]) -> canon::parse::Plan {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    plan_dir(&paths)
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
    let e = ep("/m/24.S1E1.mkv");
    assert_eq!(e.target_with(&e.left), "24 S01E01.mkv");
    let e = ep("/m/Show S00E01.mkv");
    assert_eq!(e.target_with(&e.left), "Show S00E01.mkv");
    let e = ep("/m/Show S01E100.mkv");
    assert_eq!(e.target_with(&e.left), "Show S01E100.mkv");
    let e = ep("/m/Show S12E345.mkv");
    assert_eq!(e.target_with(&e.left), "Show S12E345.mkv");
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
    let e = ep("/m/Show S01E01");
    assert_eq!(e.target_with(&e.left), "Show S01E01");
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

#[test]
fn dir_candidate_strips_a_trailing_season_marker() {
    assert_eq!(ep("/Volumes/Media/Breaking Bad Season 5/S05E14.mkv").dir, "Breaking Bad");
    assert_eq!(ep("/Volumes/Media/Breaking.Bad.S05/S05E14.mkv").dir, "Breaking Bad");
}

// ------------------------------------------------------- one name per dir --

#[test]
fn show_name_before_the_anchor() {
    for (file, want) in [
        ("/m/The Big Bang Theory (Kaley Cuoco) S07E08 1080p H.264 (moviesbyrizzo upload).mp4", "The Big Bang Theory S07E08.mp4"),
        ("/m/Breaking.Bad.S05E14.Ozymandias.1080p.WEB-DL.x264-GROUP.mkv", "Breaking Bad S05E14.mkv"),
        ("/m/its.always.sunny.in.philadelphia.7x03.HDTV.XviD-FQM.avi", "its always sunny in philadelphia S07E03.avi"),
        ("/m/The.Office.US.S03E10.720p.BluRay.x265-RARBG.mp4", "The Office US S03E10.mp4"),
        ("/m/Doctor Who (2005) S11E01 1080p.mkv", "Doctor Who S11E01.mkv"),
        ("/m/24.S1E1.mkv", "24 S01E01.mkv"),
        ("/m/Law & Order- SVU S22E03 WEBRip.mp4", "Law & Order SVU S22E03.mp4"),
        ("/m/The Wire S03E07 Back Burners 720p Blu-ray DTS-HD x265.mkv", "The Wire S03E07.mkv"),
        ("/m/Severance - Season 2 Episode 5 - 2160p HDR AMZN WEB-DL.mp4", "Severance S02E05.mp4"),
    ] {
        let p = plan(&[file]);
        assert_eq!(p.targets()[0].1, want, "for {file}");
    }
}

// -------------------------------------------------------- name after anchor --

#[test]
fn show_name_after_the_anchor_when_left_is_junk() {
    for (file, want) in [
        ("/m/[SubsPlease] S01E02 - Frieren [1080p][A1B2].mkv", "Frieren S01E02.mkv"),
        ("/m/www.Torrenting.com - S02E05 - Severance - 2160p HDR AMZN WEB-DL.mp4", "Severance S02E05.mp4"),
        ("/m/S07E08 - The Big Bang Theory 1080p H.264 (moviesbyrizzo upload).mp4", "The Big Bang Theory S07E08.mp4"),
    ] {
        let p = plan(&[file]);
        assert_eq!(p.targets()[0].1, want, "for {file}");
    }
}

#[test]
fn batch_consensus_picks_the_repeating_side() {
    // Episode title before the anchor, show name after it. Only the repetition
    // across the directory reveals which is which.
    let p = plan(&[
        "/m/Ozymandias.S05E14.Breaking.Bad.1080p.mkv",
        "/m/Granite.State.S05E15.Breaking.Bad.1080p.mkv",
        "/m/Felina.S05E16.Breaking.Bad.1080p.mkv",
    ]);
    assert_eq!(p.name, "Breaking Bad");
    let m: Vec<(PathBuf, String)> = p.targets();
    let get = |base: &str| m.iter().find(|(p, _)| p.ends_with(base)).unwrap().1.clone();
    assert_eq!(get("Ozymandias.S05E14.Breaking.Bad.1080p.mkv"), "Breaking Bad S05E14.mkv");
    assert_eq!(get("Felina.S05E16.Breaking.Bad.1080p.mkv"), "Breaking Bad S05E16.mkv");
}

#[test]
fn a_single_file_cannot_resolve_that_ambiguity() {
    // Alone, the same filename defaults to the left side and gets it wrong.
    // This is the case the confirm step exists to catch.
    let p = plan(&["/m/Ozymandias.S05E14.Breaking.Bad.1080p.mkv"]);
    assert_eq!(p.name, "Ozymandias");
    assert_eq!(p.confidence, Confidence::Medium);
}

#[test]
fn repeating_left_side_is_not_overruled_by_a_repeating_right_side() {
    // Both repeat: left must win, or a re-released season would flip.
    let p = plan(&[
        "/m/Breaking.Bad.S05E14.1080p.WEBDL.mkv",
        "/m/Breaking.Bad.S05E15.1080p.WEBDL.mkv",
    ]);
    assert_eq!(p.name, "Breaking Bad");
}

// ------------------------------------------------------------- confidence --

#[test]
fn confidence_reflects_how_much_evidence_there_was() {
    // Repeated across every file in the directory -> high.
    let p = plan(&["/m/Show.S01E01.1080p.mkv", "/m/Show.S01E02.1080p.mkv"]);
    assert_eq!(p.confidence, Confidence::High);

    // A lone file with only one viable side -> medium.
    let p = plan(&["/m/Show.S01E01.1080p.mkv"]);
    assert_eq!(p.confidence, Confidence::Medium);

    // Name after the anchor that never repeats -> low, because it is
    // indistinguishable from an episode title.
    let p = plan(&["/m/Severance/S02E05 - Cold Harbor 1080p.mkv"]);
    assert_eq!(p.confidence, Confidence::Low);
    assert_eq!(p.name, "Cold Harbor");
}

#[test]
fn no_candidate_on_either_side_falls_back_to_the_directory_name() {
    let p = plan(&["/Volumes/Media/Severance/S02E05 1080p WEBRip.mkv"]);
    assert_eq!(p.name, "Severance");
    assert_eq!(p.confidence, Confidence::Medium);
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

// ------------------------------------------------------------- Plan API --

#[test]
fn plan_sorts_by_episode_and_applies_one_name_to_every_file() {
    let p = plan(&[
        "/m/Show.S01E03.1080p.mkv",
        "/m/Show.S01E01.1080p.mkv",
        "/m/Show.S01E02.1080p.mkv",
    ]);
    assert_eq!(p.episodes.iter().map(|e| e.episode).collect::<Vec<_>>(), vec![1, 2, 3]);
    let targets = p.targets();
    assert_eq!(targets[0].1, "Show S01E01.mkv");
    assert_eq!(targets[1].1, "Show S01E02.mkv");
    assert_eq!(targets[2].1, "Show S01E03.mkv");
}

#[test]
fn plan_reports_skips_without_dropping_them_silently() {
    let p = plan(&["/m/Show.S01E01.mkv", "/m/Some Movie 2019.mkv"]);
    assert_eq!(p.episodes.len(), 1);
    assert_eq!(p.skipped.len(), 1);
    assert_eq!(p.skipped[0].0, "Some Movie 2019.mkv");
    assert!(p.skipped[0].1.contains("anchor"));
}

#[test]
fn plan_season_is_single_when_every_file_agrees() {
    let p = plan(&["/m/Show.S05E01.mkv", "/m/Show.S05E02.mkv"]);
    assert_eq!(p.season, Season::Single(5));
}

#[test]
fn plan_season_varies_when_files_disagree() {
    let p = plan(&["/m/Show.S04E10.mkv", "/m/Show.S05E01.mkv"]);
    assert_eq!(p.season, Season::Varies(4, 5));
}

#[test]
fn plan_preview_shows_the_first_episode_renamed() {
    let p = plan(&["/m/Show.S01E02.mkv", "/m/Show.S01E01.mkv"]);
    assert_eq!(p.preview(), Some("Show S01E01.mkv".to_string()));
}

#[test]
fn set_name_overrides_the_detected_name_and_is_sanitised() {
    let mut p = plan(&["/m/whatever.S01E01.mkv", "/m/[grp] S01E02 - Other.mkv"]);
    p.set_name("A/B: Show");
    assert_eq!(p.confidence, Confidence::High);
    let targets = p.targets();
    assert!(targets.iter().all(|(_, new)| new.starts_with("AB Show S01E0")));
}

// ------------------------------------------------------ known limitations --

#[test]
fn documented_gaps_behave_predictably() {
    // Multi-episode files keep only the first episode number.
    let p = plan(&["/m/Show.S01E01-E02.mkv"]);
    assert_eq!(p.targets()[0].1, "Show S01E01.mkv");

    // An unbracketed year stays in the title (stripping it would break
    // legitimate titles such as "Class of 1999").
    let p = plan(&["/m/Doctor Who 2005 S11E01 1080p.mkv"]);
    assert_eq!(p.targets()[0].1, "Doctor Who 2005 S11E01.mkv");

    // Initialisms written with dots lose them along with the separators.
    let p = plan(&["/m/Marvel's Agents of S.H.I.E.L.D. S03E12 1080p.mkv"]);
    assert_eq!(p.targets()[0].1, "Marvel's Agents of S H I E L D S03E12.mkv");

    // Casing is preserved, never corrected.
    let p = plan(&["/m/its.always.sunny.S07E03.mkv"]);
    assert_eq!(p.targets()[0].1, "its always sunny S07E03.mkv");
}

#[test]
fn already_canonical_names_are_stable() {
    // Running canon twice must not drift.
    let p = plan(&["/m/Severance S02E05.mkv"]);
    assert_eq!(p.targets()[0].1, "Severance S02E05.mkv");
    let p = plan(&["/m/The Office US S03E10.mp4"]);
    assert_eq!(p.targets()[0].1, "The Office US S03E10.mp4");
}
