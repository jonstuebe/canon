use canon::book::{plan_books, BookPlan};
use canon::parse::Confidence;
use std::path::PathBuf;

fn plan(files: &[&str]) -> BookPlan {
    let paths: Vec<PathBuf> = files.iter().map(|f| PathBuf::from(format!("/books/{f}"))).collect();
    plan_books(&paths)
}

fn targets(p: &BookPlan) -> Vec<String> {
    p.items.iter().map(|i| i.target()).collect()
}

fn one(file: &str) -> String {
    let p = plan(&[file]);
    targets(&p).first().cloned().unwrap_or_default()
}

// --- the canonical shapes ---

#[test]
fn author_series_title_is_already_canonical() {
    assert_eq!(one("Brandon Sanderson - Mistborn 01 - The Final Empire.m4b"), "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b");
}

#[test]
fn author_then_title_drops_the_series_segment() {
    assert_eq!(one("Andy Weir - Project Hail Mary.m4b"), "Andy Weir - Project Hail Mary.m4b");
}

#[test]
fn title_then_author_is_reordered() {
    assert_eq!(one("Project Hail Mary - Andy Weir.m4b"), "Andy Weir - Project Hail Mary.m4b");
}

#[test]
fn series_first_ordering_is_rearranged() {
    assert_eq!(
        one("Mistborn 01 - The Final Empire - Brandon Sanderson.m4b"),
        "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b"
    );
}

#[test]
fn series_number_is_zero_padded() {
    assert_eq!(one("Brandon Sanderson - Mistborn 2 - The Well of Ascension.m4b"), "Brandon Sanderson - Mistborn 02 - The Well of Ascension.m4b");
}

// --- unambiguous author signals ---

#[test]
fn surname_comma_first_name_is_reordered() {
    assert_eq!(
        one("Sanderson, Brandon - Mistborn 01 - The Final Empire.m4b"),
        "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b"
    );
}

#[test]
fn by_author_is_extracted_from_a_single_segment() {
    assert_eq!(one("The Final Empire by Brandon Sanderson.m4b"), "Brandon Sanderson - The Final Empire.m4b");
}

#[test]
fn by_inside_a_title_is_not_an_author_credit() {
    let p = plan(&["Gone by Midnight.m4b"]);
    assert_eq!(targets(&p), vec!["Gone by Midnight.m4b"]);
    assert_eq!(p.items[0].confidence, Confidence::Low);
}

#[test]
fn strong_signal_outranks_position() {
    assert_eq!(one("Project Hail Mary by Andy Weir - Unabridged.m4b"), "Andy Weir - Project Hail Mary.m4b");
}

// --- telling a name from a title ---

#[test]
fn a_leading_article_rules_out_a_person() {
    assert_eq!(one("Brandon Sanderson - The Final Empire.m4b"), "Brandon Sanderson - The Final Empire.m4b");
}

#[test]
fn a_one_word_title_is_never_the_author() {
    assert_eq!(one("Andy Weir - Artemis.m4b"), "Andy Weir - Artemis.m4b");
    assert_eq!(one("Artemis - Andy Weir.m4b"), "Andy Weir - Artemis.m4b");
}

#[test]
fn the_shorter_name_shaped_end_wins() {
    // Both ends are name-shaped; "Andy Weir" is more so than "Project Hail Mary".
    assert_eq!(one("Project Hail Mary - Andy Weir.m4b"), "Andy Weir - Project Hail Mary.m4b");
}

#[test]
fn a_known_given_name_breaks_an_otherwise_equal_tie() {
    // Both ends are two capitalised words, so shape alone can't separate
    // them -- but "Neal" is a given name the dictionary knows and "Snow"
    // isn't, which settles it.
    let p = plan(&["Snow Crash - Neal Stephenson.m4b"]);
    assert_eq!(targets(&p), vec!["Neal Stephenson - Snow Crash.m4b"]);
    assert_eq!(p.items[0].confidence, Confidence::High);
}

#[test]
fn the_tie_break_works_from_either_side() {
    for (input, want) in [
        ("Gone Girl - Gillian Flynn.m4b", "Gillian Flynn - Gone Girl.m4b"),
        ("Dune Messiah - Frank Herbert.m4b", "Frank Herbert - Dune Messiah.m4b"),
        ("Atomic Habits - James Clear.m4b", "James Clear - Atomic Habits.m4b"),
        ("Dark Matter - Blake Crouch.m4b", "Blake Crouch - Dark Matter.m4b"),
        ("Jurassic Park - Michael Crichton.m4b", "Michael Crichton - Jurassic Park.m4b"),
        ("Michael Crichton - Jurassic Park.m4b", "Michael Crichton - Jurassic Park.m4b"),
    ] {
        assert_eq!(one(input), want, "input: {input}");
    }
}

#[test]
fn two_recognised_names_stay_undecidable_at_medium() {
    // "Storm" is itself a given name, so the dictionary can't separate these
    // either. Convention wins, but the line is marked rather than asserted.
    let p = plan(&["Storm Front - Jim Butcher.m4b"]);
    assert_eq!(targets(&p), vec!["Storm Front - Jim Butcher.m4b"]);
    assert_eq!(p.items[0].confidence, Confidence::Medium);
}

#[test]
fn a_mononym_author_is_recognised_by_the_dictionary() {
    assert_eq!(one("Homer - The Odyssey.m4b"), "Homer - The Odyssey.m4b");
}

#[test]
fn a_mononym_always_loses_to_a_full_name() {
    // "Artemis" is a given name, but it is the title here.
    assert_eq!(one("Artemis - Andy Weir.m4b"), "Andy Weir - Artemis.m4b");
    assert_eq!(one("Andy Weir - Artemis.m4b"), "Andy Weir - Artemis.m4b");
}

#[test]
fn an_unknown_mononym_is_not_promoted_to_author() {
    // Neither end is name-shaped: "Xyzzy" is an unrecognised single word and
    // "The Odyssey" opens with an article.
    let p = plan(&["Xyzzy - The Odyssey.m4b"]);
    assert!(p.items[0].author.is_none());
    assert_eq!(p.items[0].confidence, Confidence::Low);
}

#[test]
fn a_lowercase_particle_does_not_disqualify_a_name() {
    assert_eq!(one("Rebecca - Daphne du Maurier.m4b"), "Daphne du Maurier - Rebecca.m4b");
    assert_eq!(one("Ludwig van Beethoven - Some Memoir.m4b"), "Ludwig van Beethoven - Some Memoir.m4b");
}

#[test]
fn an_unrecognised_name_is_never_demoted() {
    // The dictionary is positive evidence only: a name it doesn't know still
    // wins on shape, and still comes back high.
    let p = plan(&["Qwertyu Asdfghj - The Long Title.m4b"]);
    assert_eq!(targets(&p), vec!["Qwertyu Asdfghj - The Long Title.m4b"]);
    assert_eq!(p.items[0].confidence, Confidence::High);
}

#[test]
fn initials_survive_as_a_name() {
    assert_eq!(one("J.R.R. Tolkien - The Hobbit.m4b"), "J R R Tolkien - The Hobbit.m4b");
}

#[test]
fn no_separator_at_all_is_low_confidence_title_only() {
    let p = plan(&["Project Hail Mary.m4b"]);
    assert_eq!(targets(&p), vec!["Project Hail Mary.m4b"]);
    assert_eq!(p.items[0].confidence, Confidence::Low);
    assert!(p.items[0].author.is_none());
}

// --- series detection and its guards ---

#[test]
fn bracketed_series_is_lifted_out() {
    assert_eq!(
        one("Brandon Sanderson - The Final Empire (Mistborn, Book 1).m4b"),
        "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b"
    );
}

#[test]
fn bracketed_series_without_a_keyword_still_reads() {
    assert_eq!(one("Brandon Sanderson - The Final Empire [Mistborn 01].m4b"), "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b");
}

#[test]
fn a_bracketed_year_is_not_a_series() {
    assert_eq!(one("Andy Weir - Project Hail Mary (2021).m4b"), "Andy Weir - Project Hail Mary.m4b");
}

#[test]
fn a_three_digit_number_in_a_title_is_not_a_series_number() {
    assert_eq!(one("Ray Bradbury - Fahrenheit 451.m4b"), "Ray Bradbury - Fahrenheit 451.m4b");
}

#[test]
fn a_numeric_title_is_not_a_series() {
    assert_eq!(one("George Orwell - 1984.m4b"), "George Orwell - 1984.m4b");
}

#[test]
fn the_last_remaining_segment_is_always_the_title() {
    // "Catch 22" is series-shaped, but it is all the title there is.
    assert_eq!(one("Joseph Heller - Catch 22.m4b"), "Joseph Heller - Catch 22.m4b");
}

#[test]
fn a_keyword_series_carries_three_digits() {
    assert_eq!(
        one("Terry Pratchett - Discworld Book 100 - Some Title.m4b"),
        "Terry Pratchett - Discworld 100 - Some Title.m4b"
    );
}

// --- junk ---

#[test]
fn unabridged_and_bitrate_tags_are_trimmed() {
    assert_eq!(one("Andy Weir - Project Hail Mary (Unabridged) 64kbps.m4b"), "Andy Weir - Project Hail Mary.m4b");
}

#[test]
fn a_narrator_credit_is_dropped() {
    assert_eq!(
        one("Brandon Sanderson - The Final Empire, narrated by Michael Kramer.m4b"),
        "Brandon Sanderson - The Final Empire.m4b"
    );
}

#[test]
fn a_trailing_book_word_in_a_title_survives() {
    assert_eq!(one("Rudyard Kipling - The Jungle Book.m4b"), "Rudyard Kipling - The Jungle Book.m4b");
}

#[test]
fn dotted_names_are_flattened_and_split() {
    assert_eq!(one("Brandon.Sanderson.-.The.Final.Empire.m4b"), "Brandon Sanderson - The Final Empire.m4b");
}

#[test]
fn an_intra_word_hyphen_is_not_a_separator() {
    assert_eq!(one("Jean-Luc Picard - Some Memoir.m4b"), "Jean-Luc Picard - Some Memoir.m4b");
}

// --- extension and plan behaviour ---

#[test]
fn the_extension_is_lowercased_and_kept() {
    assert_eq!(one("Andy Weir - Artemis.M4B"), "Andy Weir - Artemis.m4b");
}

#[test]
fn a_file_with_no_usable_title_is_skipped() {
    let p = plan(&["(Unabridged).m4b"]);
    assert!(p.items.is_empty());
    assert_eq!(p.skipped.len(), 1);
}

#[test]
fn items_are_sorted_by_author_then_series() {
    let p = plan(&[
        "Brandon Sanderson - Mistborn 02 - The Well of Ascension.m4b",
        "Andy Weir - Artemis.m4b",
        "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b",
    ]);
    assert_eq!(
        targets(&p),
        vec![
            "Andy Weir - Artemis.m4b",
            "Brandon Sanderson - Mistborn 01 - The Final Empire.m4b",
            "Brandon Sanderson - Mistborn 02 - The Well of Ascension.m4b",
        ]
    );
}

#[test]
fn a_typed_author_is_applied_at_high_confidence() {
    let mut p = plan(&["Project Hail Mary.m4b"]);
    p.items[0].set_author("Andy Weir");
    assert_eq!(p.items[0].target(), "Andy Weir - Project Hail Mary.m4b");
    assert_eq!(p.items[0].confidence, Confidence::High);
}

#[test]
fn dictionary_function_words_cannot_pose_as_an_author() {
    // "So" and "The" are both in the name dictionary ("the" comes back
    // MayBeFemale), so without the stoplist guard the title would outscore
    // the real author and win the leading slot.
    assert_eq!(one("So Long - Douglas Adams.m4b"), "Douglas Adams - So Long.m4b");
    assert_eq!(one("The Stand - Stephen King.m4b"), "Stephen King - The Stand.m4b");
}

#[test]
fn an_unrecognised_non_western_name_still_wins_on_shape() {
    // Chimamanda, Yaa and Arundhati are absent from the dictionary. Because
    // it is positive evidence only, shape alone still resolves these.
    for (input, want) in [
        ("Chimamanda Ngozi Adichie - Americanah.m4b", "Chimamanda Ngozi Adichie - Americanah.m4b"),
        ("Yaa Gyasi - Homegoing.m4b", "Yaa Gyasi - Homegoing.m4b"),
        ("Arundhati Roy - The God of Small Things.m4b", "Arundhati Roy - The God of Small Things.m4b"),
    ] {
        assert_eq!(one(input), want, "input: {input}");
    }
}
