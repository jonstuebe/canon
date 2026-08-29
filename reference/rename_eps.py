#!/usr/bin/env python3
"""Normalize TV filenames to "Show Name SxxEyy.ext".

The season/episode marker is the anchor. The show name may sit on EITHER side
of it, so both sides are extracted as candidates and one is chosen by rule.
Proposals are grouped by show and confirmed -- with the rejected candidates
offered as numbered choices -- before anything is renamed.
"""
import os, re, sys, argparse
from collections import Counter, OrderedDict

SXXEYY = re.compile(r"""
    (?<![a-z0-9])
    (?:
        s(?P<s1>\d{1,2})[\s._-]*e(?P<e1>\d{1,3})   # S07E08, s07.e08, S7 E8
      | (?P<s2>\d{1,2})x(?P<e2>\d{1,3})            # 7x08
      | season[\s._-]*(?P<s3>\d{1,2})[\s._-]*episode[\s._-]*(?P<e3>\d{1,3})
    )
    (?![a-z0-9])
""", re.I | re.X)

# Junk tokens, trimmed from the TRAILING end of a candidate only.
JUNK = re.compile(r"""^(
    \d{3,4}p|\d{3,4}i|4k|uhd|hdr\d*|sdr|hevc|h\.?26[45]|x\.?26[45]|xvid|divx|av1|
    aac\d?|ac3|eac3|dts(?:-?hd)?|truehd|atmos|mp3|flac|\d+bit|\d+ch|
    web-?dl|web-?rip|webrip|bluray|blu-ray|bdrip|brrip|dvdrip|dvd|hdtv|pdtv|
    hdrip|remux|proper|repack|internal|extended|uncut|limited|complete|
    amzn|nf|hulu|dsnp|atvp|hmax|pcok|stan|itunes|
    subs?|multi|dual|ita|eng|vostfr|
    rarbg|yts|yify|ettv|eztv|fov|killers|sparks|ntb|ion10|
    upload|uploaded|by
)$""", re.I | re.X)

CODEC_DOT = re.compile(r"\b([hx])[\s._-]?(26[45]|265)\b", re.I)
# Rejoin hyphenated release tags before separators are flattened to spaces.
HYPHEN_JOIN = re.compile(r"\b(web|blu|dts)[-._ ](dl|rip|ray|hd)\b", re.I)
DOMAIN = re.compile(r"(?:www\.)?[a-z0-9-]+\.(?:com|net|org|to|tv|me|io|cc|info)\b", re.I)
ILLEGAL = re.compile(r'[/\\:*?"<>|\x00-\x1f]')
SCENE_GROUP = re.compile(r"-[A-Za-z0-9]{2,}$")

def clean(raw: str) -> str:
    """Turn one side of the anchor into a show-name candidate."""
    raw = re.sub(r"[\(\[\{][^\)\]\}]*[\)\]\}]", " ", raw)   # (Kaley Cuoco), [HorribleSubs]
    raw = DOMAIN.sub(" ", raw)                              # www.Torrenting.com
    raw = CODEC_DOT.sub(r"\1\2", raw)                       # H.264 -> H264, before dots die
    raw = HYPHEN_JOIN.sub(r"\1\2", raw)                     # WEB-DL -> WEBDL, Blu-ray -> Bluray
    stripped = raw.strip()                                  # dotted scene name: x264-GROUP
    if " " not in stripped and "." in stripped:             # dots required: keep "Spider-Man"
        raw = SCENE_GROUP.sub("", stripped)
    raw = raw.replace("_", " ").replace(".", " ")
    raw = re.sub(r"\s+[-–—]+\s*|[-–—]+\s+|[-–—]{2,}", " ", raw)
    tokens = [t for t in raw.split() if t]
    while tokens and JUNK.match(tokens[-1]):                # trailing end only
        tokens.pop()
    return re.sub(r"\s{2,}", " ", " ".join(tokens)).strip(" -,")

def split(filename: str):
    """-> (season, episode, left_candidate, right_candidate, ext) or None."""
    stem, ext = os.path.splitext(filename)
    m = SXXEYY.search(stem)
    if not m:
        return None
    g = m.groupdict()
    return (int(g["s1"] or g["s2"] or g["s3"]),
            int(g["e1"] or g["e2"] or g["e3"]),
            clean(stem[:m.start()]),
            clean(stem[m.end():]),
            ext.lower())

def choose(left, right, lc, rc, dirn, prefer, use_dir):
    """Pick which side holds the show name. -> (kind, confidence, reason)"""
    if prefer in ("left", "right"):
        return (prefer, "high", f"--prefer {prefer}")
    if left and not right:
        return ("left", "high" if lc[left] > 1 else "medium",
                "only the left side survived cleaning")
    if right and not left:
        # Right may be a show name OR a bare episode title -- undecidable from
        # one filename. A repeat across the batch proves it's the show name.
        if rc[right] > 1:
            return ("right", "high", "right side, repeats across the batch")
        if dirn and use_dir:
            return ("dir", "medium", "--fallback-dir: name after anchor is unique")
        return ("right", "low",
                "right side only, and it does NOT repeat -- could be an episode title")
    if left and right:
        # Show names repeat across a batch, episode titles don't -- so a
        # repeating right side beats a unique left side.
        if rc[right] > 1 and lc[left] == 1:
            return ("right", "high", "right side repeats, left side is unique")
        return ("left", "high" if lc[left] > 1 else "medium",
                "both sides populated, defaulted to left")
    if dirn:
        return ("dir", "low", "no candidate on either side, used folder name")
    return (None, None, "no show name found")

BOLD, DIM, RED, YEL, GRN, CYN, OFF = (
    "\033[1m", "\033[2m", "\033[31m", "\033[33m", "\033[32m", "\033[36m", "\033[0m")
MARK = {"high": f"{GRN}high{OFF}", "medium": f"{YEL}medium{OFF}", "low": f"{RED}LOW{OFF}"}
LABEL = {"left": "text before SxxEyy", "right": "text after SxxEyy", "dir": "folder name"}
IDX = {"left": 4, "right": 5, "dir": 6}          # positions in a file tuple

class Console:
    """Prompt on /dev/tty when stdin is a glob/pipe, falling back to stdin."""
    def __init__(self):
        self.inp, self.out = sys.stdin, sys.stdout
        if not sys.stdin.isatty():
            try:
                tty = open("/dev/tty", "r+")
                self.inp = self.out = tty
            except OSError:
                pass
    def ask(self, msg):
        for attempt in (0, 1):
            try:
                self.out.write(msg); self.out.flush()
                line = self.inp.readline()
                if line:
                    return line.strip()
            except (EOFError, OSError):
                pass
            if self.inp is sys.stdin:
                return "q"                      # real EOF or unusable stdin
            self.inp, self.out = sys.stdin, sys.stdout   # retry once on stdin
        return "q"

def prompt(msg, tty):
    return tty.ask(msg)

def values_for(files, kind):
    """Distinct values a given side takes across a group, in order."""
    seen = []
    for f in files:
        v = f[IDX[kind]]
        if v and v not in seen: seen.append(v)
    return seen

def render(files, kind, override):
    """Resolve the final name for each file. -> [(path, newname)]"""
    out = []
    for path, s, e, ext, left, right, dirn in files:
        name = override or {"left": left, "right": right, "dir": dirn}[kind]
        out.append((path, f"{name} S{s:02d}E{e:02d}{ext}"))
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("paths", nargs="+")
    ap.add_argument("--apply", action="store_true", help="rename after confirmation (default: dry run)")
    ap.add_argument("--yes", action="store_true", help="skip the confirm step (non-interactive)")
    ap.add_argument("--prefer", choices=["auto", "left", "right"], default="auto",
                    help="force which side of SxxEyy holds the show name")
    ap.add_argument("--fallback-dir", action="store_true",
                    help="when the name after SxxEyy doesn't repeat, use the folder name instead")
    ap.add_argument("--show", help="override the show name entirely")
    a = ap.parse_args()

    parsed = [(p, split(os.path.basename(p))) for p in a.paths]
    lc = Counter(x[2] for _, x in parsed if x and x[2])
    rc = Counter(x[3] for _, x in parsed if x and x[3])

    groups, skipped = OrderedDict(), []
    for path, info in parsed:
        base = os.path.basename(path)
        if info is None:
            skipped.append((base, "no SxxEyy anchor")); continue
        season, episode, left, right, ext = info
        dirn = clean(os.path.basename(os.path.abspath(os.path.dirname(path))))
        kind, conf, reason = choose(left, right, lc, rc, dirn, a.prefer, a.fallback_dir)
        if a.show:
            kind, conf, reason = "left", "high", "--show override"
        if kind is None:
            skipped.append((base, reason)); continue
        name = a.show or {"left": left, "right": right, "dir": dirn}[kind]
        if not name:
            skipped.append((base, "chosen side was empty")); continue
        groups.setdefault((name, kind, conf, reason), []).append(
            (path, season, episode, ext, left, right, dirn))

    if not groups:
        print("Nothing to rename.")
        for b, why in skipped: print(f"  SKIP  {b}  ({why})")
        return

    tty = Console() if not a.yes else None
    accepted, accept_rest = [], (a.yes or bool(a.show))

    # --- confirm step: one decision per show, not per file ---
    for (name, kind, conf, reason), files in groups.items():
        files.sort(key=lambda f: (f[1], f[2]))
        override = a.show
        while True:
            print(f"\n{BOLD}{name}{OFF}  ({len(files)} file{'s'*(len(files)!=1)})"
                  f"   confidence: {MARK[conf]}")
            print(f"  {DIM}why: {reason}{OFF}")
            preview = render(files, kind, override)
            for (path, new) in preview[:3]:
                print(f"  {DIM}{os.path.basename(path)}{OFF}\n     -> {new}")
            if len(preview) > 3:
                print(f"  {DIM}... and {len(preview)-3} more{OFF}")
            if accept_rest: break

            # Offer the candidates this group actually contains, current one first.
            # A side is offerable only if every file in the group has one.
            choices = [kind] + [k for k in ("left", "right", "dir")
                                if k != kind and all(f[IDX[k]] for f in files)]
            print(f"  {BOLD}choices:{OFF}")
            for i, k in enumerate(choices, 1):
                vals = values_for(files, k)
                shown = vals[0] if len(vals) == 1 else f"{vals[0]} / {vals[1]} … (varies per file)"
                tag = f"  {CYN}<- current{OFF}" if (k == kind and not override) else ""
                print(f"    [{i}] {shown}   {DIM}({LABEL[k]}){tag}{OFF}")
            ans = prompt("  [enter] accept  [1-9] pick  [t] type a name  "
                         "[s] skip  [l] list all  [a] accept all  [q] quit > ", tty).lower()

            if ans in ("", "y"): break
            if ans == "a": accept_rest = True; break
            if ans == "s": files = []; break
            if ans == "q": print("\nAborted, nothing renamed."); return
            if ans == "l":
                for (path, new) in preview:
                    print(f"  {DIM}{os.path.basename(path)}{OFF}\n     -> {new}")
                continue
            if ans == "t":
                new = prompt("  new show name > ", tty)
                if new and new.lower() != "q":
                    override = ILLEGAL.sub("", new).strip()
                    name, conf, reason = override, "high", "typed by hand"
                continue
            if ans.isdigit() and 1 <= int(ans) <= len(choices):
                kind, override = choices[int(ans) - 1], None
                vals = values_for(files, kind)
                name, conf, reason = vals[0], "high", f"confirmed: {LABEL[kind]}"
                continue
            print(f"  {YEL}?{OFF}")
        accepted += render(files, kind, override)

    # --- safety checks before touching disk ---
    unchanged = [1 for path, new in accepted if os.path.basename(path) == new]
    accepted = [(path, new) for path, new in accepted if os.path.basename(path) != new]
    targets, collisions, exists = Counter(), [], []
    for path, new in accepted:
        dest = os.path.join(os.path.dirname(path), new)
        targets[dest] += 1
        if targets[dest] == 2: collisions.append(dest)
        if os.path.abspath(dest) != os.path.abspath(path) and os.path.exists(dest):
            exists.append(dest)

    print(f"\n{BOLD}Summary{OFF}")
    print(f"  {len(accepted)} file(s) to rename across {len(groups)} group(s)")
    if unchanged: print(f"  {DIM}{len(unchanged)} file(s) already correctly named{OFF}")
    for b, why in skipped: print(f"  {DIM}skipped: {b}  ({why}){OFF}")
    for d in collisions: print(f"  {RED}COLLISION: two files both want {os.path.basename(d)}{OFF}")
    for d in exists: print(f"  {RED}EXISTS: {os.path.basename(d)} is already on disk{OFF}")

    if not accepted: return
    if not a.apply:
        print(f"\n{DIM}Dry run. Re-run with --apply to perform these renames.{OFF}"); return
    if collisions or exists:
        print(f"\n{RED}Refusing to apply: resolve the conflicts above first.{OFF}"); return
    if not a.yes and prompt(f"\nApply {len(accepted)} rename(s)? [y/N] > ", tty).lower() != "y":
        print("Aborted, nothing renamed."); return

    done = 0
    for path, new in accepted:
        dest = os.path.join(os.path.dirname(path), new)
        if os.path.abspath(dest) == os.path.abspath(path): continue
        try:
            os.rename(path, dest); done += 1
        except OSError as err:
            print(f"  {RED}FAILED {os.path.basename(path)}: {err}{OFF}")
    print(f"Renamed {done} file(s).")

if __name__ == "__main__":
    main()
