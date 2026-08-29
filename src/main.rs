//! Interactive CLI: propose renames, confirm them per show, then apply.

use canon::parse::{plan, safe, Confidence, Group, Options, Side};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YEL: &str = "\x1b[33m";
const GRN: &str = "\x1b[32m";
const CYN: &str = "\x1b[36m";
const OFF: &str = "\x1b[0m";

const USAGE: &str = "\
canon -- rename TV episode files to \"Show Name SxxEyy.ext\"

USAGE:
    canon [OPTIONS] <FILE>...

OPTIONS:
    --apply           perform the renames (default: dry run)
    --yes             skip the confirm step, accept every default
    --prefer <SIDE>   force which side of SxxEyy holds the show name: left|right
    --fallback-dir    when the name after SxxEyy does not repeat, use the folder name
    --show <NAME>     override the show name for every file
    -h, --help        show this help
    -V, --version     show version

Pass a whole season at once: canon ~/Downloads/*.mkv
Names are decided across the batch, so more files means better guesses.
";

struct Args {
    paths: Vec<PathBuf>,
    apply: bool,
    yes: bool,
    opts: Options,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut paths = Vec::new();
    let (mut apply, mut yes, mut use_dir) = (false, false, false);
    let (mut prefer, mut show) = (None, None);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--apply" => apply = true,
            "--yes" | "-y" => yes = true,
            "--fallback-dir" => use_dir = true,
            "--prefer" => {
                prefer = match it.next().as_deref() {
                    Some("left") => Some(Side::Left),
                    Some("right") => Some(Side::Right),
                    other => return Err(format!("--prefer wants left|right, got {other:?}")),
                }
            }
            "--show" => {
                show = Some(it.next().ok_or("--show wants a name")?);
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("canon {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown flag: {s}")),
            s => paths.push(PathBuf::from(s)),
        }
    }
    if paths.is_empty() {
        return Err("no files given (try --help)".into());
    }
    Ok(Some(Args { paths, apply, yes, opts: Options { prefer, use_dir, show } }))
}

/// Prompt on /dev/tty when stdin is a glob or pipe, falling back to stdin.
struct Console {
    tty: Option<(BufReader<File>, File)>,
}

impl Console {
    fn new() -> Self {
        let tty = OpenOptions::new().read(true).write(true).open("/dev/tty").ok().and_then(|f| {
            let w = f.try_clone().ok()?;
            Some((BufReader::new(f), w))
        });
        Console { tty }
    }

    fn ask(&mut self, msg: &str) -> String {
        if let Some((reader, writer)) = self.tty.as_mut() {
            let _ = write!(writer, "{msg}");
            let _ = writer.flush();
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(n) if n > 0 => return line.trim().to_string(),
                _ => self.tty = None, // tty unusable, fall through to stdin
            }
        }
        print!("{msg}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(n) if n > 0 => line.trim().to_string(),
            _ => "q".to_string(), // real EOF: treat as quit, never as blanket yes
        }
    }
}

fn mark(c: Confidence) -> String {
    match c {
        Confidence::High => format!("{GRN}high{OFF}"),
        Confidence::Medium => format!("{YEL}medium{OFF}"),
        Confidence::Low => format!("{RED}LOW{OFF}"),
    }
}

fn show_group(g: &Group, limit: Option<usize>) {
    let targets = g.targets();
    let n = limit.unwrap_or(targets.len()).min(targets.len());
    for (path, new) in &targets[..n] {
        let base = path.file_name().unwrap_or_default().to_string_lossy();
        println!("  {DIM}{base}{OFF}\n     -> {new}");
    }
    if n < targets.len() {
        println!("  {DIM}... and {} more{OFF}", targets.len() - n);
    }
}

/// Walk the groups, letting the user accept, re-pick a side, rename, or skip.
/// Returns the groups to act on.
fn confirm(groups: &mut [Group], auto_accept: bool) -> Option<Vec<usize>> {
    let mut console = Console::new();
    let mut accept_rest = auto_accept;
    let mut keep = Vec::new();

    for (i, g) in groups.iter_mut().enumerate() {
        loop {
            {
                println!(
                    "\n{BOLD}{}{OFF}  ({} file{})   confidence: {}",
                    g.display_name(),
                    g.files.len(),
                    if g.files.len() == 1 { "" } else { "s" },
                    mark(g.confidence)
                );
                println!("  {DIM}why: {}{OFF}", g.reason);
                show_group(g, Some(3));
            }
            if accept_rest {
                keep.push(i);
                break;
            }

            // A side is offerable only if every file in the group has one.
            let mut choices = vec![g.side];
            for s in Side::ALL {
                if s != g.side && g.offerable(s) {
                    choices.push(s);
                }
            }
            println!("  {BOLD}choices:{OFF}");
            for (n, s) in choices.iter().enumerate() {
                let vals = g.values_for(*s);
                let shown = match vals.len() {
                    0 => "(none)".to_string(),
                    1 => vals[0].to_string(),
                    _ => format!("{} / {} … (varies per file)", vals[0], vals[1]),
                };
                let cur = if *s == g.side && g.override_name.is_none() {
                    format!("  {CYN}<- current{OFF}")
                } else {
                    String::new()
                };
                println!("    [{}] {}   {DIM}({}){}{OFF}", n + 1, shown, s.label(), cur);
            }

            let ans = console
                .ask("  [enter] accept  [1-9] pick  [t] type a name  [s] skip  [l] list all  [a] accept all  [q] quit > ")
                .to_lowercase();
            match ans.as_str() {
                "" | "y" => {
                    keep.push(i);
                    break;
                }
                "a" => {
                    accept_rest = true;
                    keep.push(i);
                    break;
                }
                "s" => break,
                "q" => {
                    println!("\nAborted, nothing renamed.");
                    return None;
                }
                "l" => show_group(g, None),
                "t" => {
                    let typed = console.ask("  new show name > ");
                    let cleaned = safe(&typed);
                    if !cleaned.is_empty() && typed.to_lowercase() != "q" {
                        g.override_name = Some(cleaned.clone());
                        g.name = cleaned;
                        g.confidence = Confidence::High;
                        g.reason = "typed by hand".to_string();
                    }
                }
                d if d.parse::<usize>().is_ok_and(|n| n >= 1 && n <= choices.len()) => {
                    let side = choices[d.parse::<usize>().unwrap() - 1];
                    let picked = g.values_for(side).first().map(|s| s.to_string());
                    if let Some(name) = picked {
                        g.side = side;
                        g.override_name = None;
                        g.name = name;
                        g.confidence = Confidence::High;
                        g.reason = format!("confirmed: {}", side.label());
                    }
                }
                _ => println!("  {YEL}?{OFF}"),
            }
        }
    }
    Some(keep)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("canon: {e}");
            return ExitCode::from(2);
        }
    };

    let (mut groups, skipped) = plan(&args.paths, &args.opts);
    if groups.is_empty() {
        println!("Nothing to rename.");
        for (base, why) in &skipped {
            println!("  {DIM}SKIP  {base}  ({why}){OFF}");
        }
        return ExitCode::SUCCESS;
    }

    let auto_accept = args.yes || args.opts.show.is_some();
    let Some(keep) = confirm(&mut groups, auto_accept) else {
        return ExitCode::SUCCESS;
    };

    // --- safety checks before touching disk ---
    let mut planned: Vec<(PathBuf, String)> = Vec::new();
    let mut unchanged = 0usize;
    for i in keep {
        for (path, new) in groups[i].targets() {
            if path.file_name().is_some_and(|f| f.to_string_lossy() == new) {
                unchanged += 1;
            } else {
                planned.push((path, new));
            }
        }
    }

    let mut seen: Vec<PathBuf> = Vec::new();
    let (mut collisions, mut exists) = (Vec::new(), Vec::new());
    for (path, new) in &planned {
        let dest = path.parent().unwrap_or(std::path::Path::new("")).join(new);
        if seen.contains(&dest) {
            collisions.push(new.clone());
        }
        seen.push(dest.clone());
        if dest.exists() {
            exists.push(new.clone());
        }
    }

    println!("\n{BOLD}Summary{OFF}");
    println!("  {} file(s) to rename across {} group(s)", planned.len(), groups.len());
    if unchanged > 0 {
        println!("  {DIM}{unchanged} file(s) already correctly named{OFF}");
    }
    for (base, why) in &skipped {
        println!("  {DIM}skipped: {base}  ({why}){OFF}");
    }
    for n in &collisions {
        println!("  {RED}COLLISION: two files both want {n}{OFF}");
    }
    for n in &exists {
        println!("  {RED}EXISTS: {n} is already on disk{OFF}");
    }

    if planned.is_empty() {
        return ExitCode::SUCCESS;
    }
    if !args.apply {
        println!("\n{DIM}Dry run. Re-run with --apply to perform these renames.{OFF}");
        return ExitCode::SUCCESS;
    }
    if !collisions.is_empty() || !exists.is_empty() {
        println!("\n{RED}Refusing to apply: resolve the conflicts above first.{OFF}");
        return ExitCode::from(1);
    }
    if !args.yes {
        let mut console = Console::new();
        let ans = console.ask(&format!("\nApply {} rename(s)? [y/N] > ", planned.len()));
        if ans.to_lowercase() != "y" {
            println!("Aborted, nothing renamed.");
            return ExitCode::SUCCESS;
        }
    }

    let (mut done, mut failed) = (0, 0);
    for (path, new) in &planned {
        let dest = path.parent().unwrap_or(std::path::Path::new("")).join(new);
        match std::fs::rename(path, &dest) {
            Ok(()) => done += 1,
            Err(e) => {
                failed += 1;
                let base = path.file_name().unwrap_or_default().to_string_lossy();
                println!("  {RED}FAILED {base}: {e}{OFF}");
            }
        }
    }
    println!("Renamed {done} file(s).");
    if failed > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
