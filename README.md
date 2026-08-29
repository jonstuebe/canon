# canon

Rename TV episode files to a canonical `Show Name SxxEyy.ext`, deterministically.

```
The Big Bang Theory (Kaley Cuoco) S07E08 1080p H.264 (moviesbyrizzo upload).mp4
   -> The Big Bang Theory S07E08.mp4
```

No network calls, no metadata provider, no fuzzy matching against a show
database. Everything is derived from the filenames you pass in.

## Install

```sh
cargo install --path .
```

## Use

```sh
canon ~/Downloads/*.mkv           # dry run: show what would change
canon --apply ~/Downloads/*.mkv   # confirm each show, then rename
```

Pass a whole season at once. Names are decided **across the batch**, so more
files in one invocation means better guesses — see "Batch consensus" below.

| flag | effect |
|---|---|
| `--apply` | perform the renames (default is a dry run) |
| `--yes` | skip the confirm step and accept every default |
| `--prefer left\|right` | force which side of the anchor holds the show name |
| `--fallback-dir` | when the name after the anchor doesn't repeat, use the folder name |
| `--show <NAME>` | override the show name for every file |

## How it works

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

**Batch consensus:** show names repeat across a season, episode titles do not.
So a right-hand candidate that appears in several files beats a left-hand one
that appears in only one:

```
Ozymandias.S05E14.Breaking.Bad.1080p.mkv      -> Breaking Bad S05E14.mkv
Granite.State.S05E15.Breaking.Bad.1080p.mkv   -> Breaking Bad S05E15.mkv
```

This is why the tool is batch-oriented rather than per-file: the decision for
any one filename depends on the others.

**4. Confirm.** Proposals are grouped by show and confirmed once per show, not
once per file, with the rejected candidates offered as numbered choices:

```
Cold Harbor  (1 file)   confidence: LOW
  why: right side only, and it does NOT repeat -- could be an episode title
  S02E05 - Cold Harbor 1080p.mkv
     -> Cold Harbor S02E05.mkv
  choices:
    [1] Cold Harbor   (text after SxxEyy)  <- current
    [2] Severance     (folder name)
  [enter] accept  [1-9] pick  [t] type a name  [s] skip  [l] list all  [a] accept all  [q] quit >
```

Confidence is derived, not decorative: `high` means the batch corroborated the
name, `medium` means it defaulted with only one file to go on, `LOW` means it is
genuinely ambiguous. In practice you hold enter through the greens and stop at
the reds.

Before touching disk it refuses to run if two files would land on the same name,
or if a target already exists — all or nothing, never half a rename.

## Known limitations

These are design boundaries, not bugs:

- **No anchor, no rename.** Date-based shows (`The Daily Show 2024.03.14`) and
  anime absolute numbering (`Show Name - 043`) are skipped rather than guessed.
- **Multi-episode files** (`S01E01-E02`) keep only the first episode number.
- **Unbracketed years** stay in the title — stripping them would break
  legitimate titles like `Class of 1999`.
- **Casing is preserved, never corrected.** `its always sunny` stays lowercase.
- **Dotted initialisms** lose their dots: `S.H.I.E.L.D.` becomes `S H I E L D`.
- **Leading junk** is only removed when bracketed, a domain, or the entire side.

## Why not an existing crate

`hunch`, `torrent-name-parser` and `media_filename` were each measured against
this repo's corpus. They score 8/13, 7/13 and 3/13 on title extraction; `canon`
scores 13/13. All three assume the show name precedes the anchor and return no
title at all for the after-anchor cases — `hunch`'s `hunch_with_context`, which
looks like the same batch idea, does not change that. Two also truncate
`The Office US` to `The Office`.

## Development

```sh
cargo test          # 34 tests covering anchors, cleaning, decisions, gaps
```

`reference/rename_eps.py` is the original Python prototype, kept as a
differential-testing oracle. Both implementations must agree on the corpus:

```sh
cargo run -q -- --yes $(cat corpus.txt) | grep '\->'
python3 reference/rename_eps.py --yes $(cat corpus.txt) | grep '\->'
```
