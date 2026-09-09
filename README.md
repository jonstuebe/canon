# canon

Rename TV episode and movie files to a canonical name, deterministically.

```
The Big Bang Theory (Kaley Cuoco) S07E08 1080p H.264 (moviesbyrizzo upload).mp4
   -> The Big Bang Theory S07E08.mp4

Elio.2025.1080p.WEBRip.x264.AAC5.1-[YTS.MX].mp4
   -> Elio (2025).mp4
```

No network calls, no metadata provider, no fuzzy matching against a show or
movie database. Everything is derived from the filenames you pass in.

## Install

```sh
cargo install --path .
```

## Use

`canon` has two subcommands, one per media type:

```sh
canon shows ~/Downloads/Breaking\ Bad\ Season\ 5    # dry run: show what would change
canon shows --apply ~/Downloads/Breaking\ Bad\ Season\ 5

canon movies ~/Downloads/Movies                     # dry run
canon movies --apply ~/Downloads/Movies
```

| flag | effect |
|---|---|
| `--apply` | perform the renames (default is a dry run) |
| `--yes` | skip confirmation and accept every detected name |

### `canon shows`

Point it at one show's directory at a time. Every file directly inside it is
assumed to belong to the same show, and the name is decided **across all of
them at once** — see "Batch consensus" below — then applied uniformly.

### `canon movies`

Point it at a directory of movies — every file inside is assumed to be a
*different* movie, so there's no batch consensus: each file is renamed to
`Title (Year).ext` on its own. The anchor is the release year — the **last**
`19xx`/`20xx`-shaped number in the name, so a sequel number (`Incredibles 2`)
or a year baked into the title itself (`Blade Runner 2049`) doesn't get
mistaken for it; whatever comes after the year is always release junk and is
discarded, since (unlike shows) there's no ambiguity about which side the
title is on.

Every planned rename is listed at once; high-confidence lines are accepted
silently, and only a `LOW` line — no year could be found — stops to ask for a
title (or drops that file on an empty answer):

```
Elio (2025).mp4                     [high]
Fantasia (1940).mkv                 [high]
Some Weird Movie No Year At All.mp4 [LOW] -> type title or Enter to skip:
```

## How `canon shows` works

**1. The anchor.** Locate the season/episode marker — `S07E08`, `s07.e08`,
`S7 E8`, `7x03`, `Season 2 Episode 5`. It splits the filename in two, and it is
the only thing in a release name that can be trusted.

**2. Both sides are candidates.** The show name may sit on *either* side:

```
Breaking.Bad.S05E14.Ozymandias.1080p.mkv     <- name before, episode title after
[SubsPlease] S01E02 - Frieren [1080p].mkv    <- junk before, name after
```

Each side is cleaned independently: bracketed asides (`(Kaley Cuoco)`,
`[SubsPlease]`) and tracker domains are deleted, separators are flattened, the
scene release-group suffix (`x264-GROUP`) is dropped, and release-metadata
tokens (`1080p`, `WEB-DL`, `AMZN`, `x265`) are trimmed **from the trailing end
only**. Trailing-only is deliberate: it is what preserves the `US` in
`The Office US` and the `SVU` in `Law & Order SVU`, which other parsers truncate.

**3. Deciding between them.** If only one side survives cleaning, it wins. If
both do, the left side wins by default — *unless* batch consensus overrules it.
If neither side survives, the directory name itself is the fallback, since a
person chose it to describe what's inside.

**Batch consensus:** show names repeat across a season, episode titles do not.
So a right-hand candidate that appears in several files beats a left-hand one
that appears in only one:

```
Ozymandias.S05E14.Breaking.Bad.1080p.mkv      -> Breaking Bad S05E14.mkv
Granite.State.S05E15.Breaking.Bad.1080p.mkv   -> Breaking Bad S05E15.mkv
```

This is why the tool looks at the whole directory rather than one file at a
time: the decision for any one filename depends on the others in it.

**4. Confirm.** One name is decided for the whole directory and confirmed
once, not once per file:

```
Show Name: Breaking Bad
Season Number: 5
Confidence: high
Preview: Breaking Bad S05E14.mkv  (24 files)

accept: Y/n/e >
```

`Y` (or enter) accepts, `n` aborts with nothing renamed, and `e` lets you type
the correct name, after which the summary re-renders for another confirm.

Confidence is derived, not decorative: `high` means the files corroborated the
name, `medium` means it defaulted with limited evidence, `LOW` means it is
genuinely ambiguous — that's your cue to reach for `e` instead of enter.

Before touching disk it refuses to run if two files would land on the same name,
or if a target already exists — all or nothing, never half a rename.

## Known limitations

These are design boundaries, not bugs:

- **No anchor, no rename.** Date-based shows (`The Daily Show 2024.03.14`) and
  anime absolute numbering (`Show Name - 043`) are skipped rather than guessed.
- **Multi-episode files** (`S01E01-E02`) keep only the first episode number.
- **Unbracketed years** stay in a show title — stripping them would break
  legitimate titles like `Class of 1999`.
- **Casing is preserved, never corrected.** `its always sunny` stays lowercase.
- **Dotted initialisms** lose their dots: `S.H.I.E.L.D.` becomes `S H I E L D`.
- **Leading junk** is only removed when bracketed, a domain, or the entire side.
- **Movies: a title containing two year-shaped numbers** is resolved by
  taking the *last* one as the release year (`Blade Runner 2049 (2017)`),
  which is right for scene-style names but can misfire on unusual ones.
- **Movies: no year, no confidence.** A movie release with no `19xx`/`20xx`
  token anywhere in the name always comes back `LOW` and asks for a title.

## Acceptable use

`canon` is a filename renaming utility. It reads names, computes new names, and
optionally calls `rename` — it does not download, acquire, decrypt, distribute,
stream, or play anything, and it has no network access of any kind.

It is intended for organizing media you have the legal right to possess: discs
you own and ripped for personal use, files you purchased or downloaded from a
licensed service, and content you created or that is in the public domain.

**Using this tool on illegally obtained or pirated files is expressly
prohibited.** Do not use `canon` to organize, catalogue, prepare, or otherwise
handle material you acquired by infringing copyright, circumventing DRM, or
violating the terms of any service. Copyright law varies by jurisdiction and
you are solely responsible for knowing and complying with the law where you
live.

The author does not condone, endorse, or provide support for copyright
infringement, and takes no responsibility for what you point this tool at. Per
the MIT license, the software is provided "as is", without warranty of any
kind; the author is not liable for any claim, damages, or other liability
arising from its use. Nothing here is legal advice.

## Why not an existing crate

`hunch`, `torrent-name-parser` and `media_filename` were each measured against
this repo's corpus. They score 8/13, 7/13 and 3/13 on title extraction; `canon`
scores 13/13. All three assume the show name precedes the anchor and return no
title at all for the after-anchor cases — `hunch`'s `hunch_with_context`, which
looks like the same batch idea, does not change that. Two also truncate
`The Office US` to `The Office`.

## Development

```sh
cargo test          # tests covering anchors, cleaning, decisions, gaps
```

`src/parse.rs` (shows) and `src/movie.rs` (movies) are independent, pure-logic
modules with their own test files (`tests/parse_tests.rs`, `tests/movie_tests.rs`);
`main.rs` only wires I/O and prompting around whichever one the subcommand picks.

`reference/rename_eps.py` is the original Python prototype. Its `clean`/`split`
functions are kept as a differential-testing oracle for the anchor and
cleaning logic in `src/parse.rs` — those must keep agreeing on `corpus.txt`.
Its own CLI still reflects the old multi-show-per-invocation design and has
not been updated to match `canon`'s current one-directory interface.
