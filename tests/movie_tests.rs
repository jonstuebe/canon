use canon::movie::{plan_movies, MoviePlan};
use canon::parse::Confidence;
use std::path::PathBuf;

fn plan(files: &[&str]) -> MoviePlan {
    let paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(format!("/movies/{f}"))).collect();
    plan_movies(&paths)
}

fn targets(p: &MoviePlan) -> Vec<String> {
    p.items.iter().map(|i| i.target()).collect()
}

#[test]
fn year_before_quality_tags_is_the_anchor() {
    let p = plan(&["Elio.2025.1080p.WEBRip.x264.AAC5.1-[YTS.MX].mp4"]);
    assert_eq!(targets(&p), vec!["Elio (2025).mp4"]);
    assert_eq!(p.items[0].confidence, Confidence::High);
}

#[test]
fn single_digit_sequel_number_is_not_mistaken_for_a_year() {
    let p = plan(&["Incredibles.2.2018.1080p.BluRay.x264-[YTS.AM].mp4"]);
    assert_eq!(targets(&p), vec!["Incredibles 2 (2018).mp4"]);
}

#[test]
fn sequel_number_before_year_survives_in_the_title() {
    let p = plan(&["Inside.Out.2.2024.1080p.BluRay.x264.AAC5.1-[YTS.MX].mp4"]);
    assert_eq!(targets(&p), vec!["Inside Out 2 (2024).mp4"]);
}

#[test]
fn same_title_different_year_are_distinct_movies() {
    let p = plan(&["Inside.Out.2015.1080p.BluRay.x264.YIFY.mp4", "Inside.Out.2.2024.1080p.BluRay.x264.AAC5.1-[YTS.MX].mp4"]);
    let mut t = targets(&p);
    t.sort();
    assert_eq!(t, vec!["Inside Out (2015).mp4", "Inside Out 2 (2024).mp4"]);
}

#[test]
fn a_year_shaped_number_in_the_title_does_not_fool_the_anchor() {
    // The real release year (2017) comes after the title's own "2049" and
    // must win, since taking the LAST year-shaped match is the whole point.
    let p = plan(&["Blade.Runner.2049.2017.1080p.BluRay.x264-GROUP.mkv"]);
    assert_eq!(targets(&p), vec!["Blade Runner 2049 (2017).mkv"]);
}

#[test]
fn dotted_codec_and_bracketed_release_group_are_cleaned() {
    let p = plan(&["Fantasia.1940.1080p.BluRay.DDP5.1.x265.10bit-GalaxyRG265.mkv"]);
    assert_eq!(targets(&p), vec!["Fantasia (1940).mkv"]);
}

#[test]
fn no_year_found_falls_back_to_the_whole_cleaned_name_at_low_confidence() {
    let p = plan(&["Some.Weird.Movie.No.Year.At.All.mp4"]);
    assert_eq!(targets(&p), vec!["Some Weird Movie No Year At All.mp4"]);
    assert_eq!(p.items[0].confidence, Confidence::Low);
}

#[test]
fn set_title_overrides_the_title_but_keeps_the_detected_year() {
    let mut p = plan(&["Elio.2025.1080p.WEBRip.x264.AAC5.1-[YTS.MX].mp4"]);
    p.items[0].set_title("My Movie");
    assert_eq!(p.items[0].target(), "My Movie (2025).mp4");
    assert_eq!(p.items[0].confidence, Confidence::High);
}

#[test]
fn items_with_no_usable_title_at_all_are_skipped_not_planned() {
    let p = plan(&["1080p.WEBRip.x264.mp4"]);
    assert!(p.items.is_empty());
    assert_eq!(p.skipped.len(), 1);
    assert_eq!(p.skipped[0].0, "1080p.WEBRip.x264.mp4");
}

#[test]
fn results_are_sorted_by_title_then_year() {
    let p = plan(&["Peter.Pan.1953.1080p.BluRay.x264.YIFY.mp4", "Elio.2025.1080p.WEBRip.x264.AAC5.1-[YTS.MX].mp4", "Fantasia.1940.1080p.BluRay.DDP5.1.x265.10bit-GalaxyRG265.mkv"]);
    assert_eq!(targets(&p), vec!["Elio (2025).mp4", "Fantasia (1940).mkv", "Peter Pan (1953).mp4"]);
}
