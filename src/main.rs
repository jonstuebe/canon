//! Interactive CLI: `canon shows <dir>`, `canon movies <dir>` and `canon books <dir>`.

use canon::book::{plan_books, BookPlan};
use canon::movie::{plan_movies, MoviePlan};
use canon::parse::{plan_dir, Confidence, Plan, Season};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YEL: &str = "\x1b[33m";
const GRN: &str = "\x1b[32m";
const OFF: &str = "\x1b[0m";

const USAGE: &str = "\
canon -- rename media files to a canonical name

USAGE:
    canon shows  [OPTIONS] <DIRECTORY>
    canon movies [OPTIONS] <DIRECTORY>
    canon books  [OPTIONS] <DIRECTORY>

OPTIONS:
    --apply           perform the renames (default: dry run)
    --yes             skip confirmation, accept every detected name
    -h, --help        show this help
    -V, --version     show version

shows:
    Every file directly inside DIRECTORY is assumed to belong to one show.
    canon decides a single name for all of them (agreement across files,
    falling back to the directory name) and confirms it once, renaming to
    \"Show Name SxxEyy.ext\".

movies:
    Every file directly inside DIRECTORY is assumed to be a different movie.
    canon renames each to \"Title (Year).ext\", listing every result; it only
    stops to ask when a file's year (and so its title) can't be found.

books:
    Every file directly inside DIRECTORY is assumed to be a different
    audiobook. canon renames each to \"Author - Series NN - Title.ext\",
    dropping the author or series when the name doesn't reveal one; it only
    stops to ask when no author could be told apart from the title.
";

struct Args {
    dir: PathBuf,
    apply: bool,
    yes: bool,
}

fn parse_common(sub: &str, args: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
    let mut dir = None;
    let (mut apply, mut yes) = (false, false);
    for a in args {
        match a.as_str() {
            "--apply" => apply = true,
            "--yes" | "-y" => yes = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("canon {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown flag: {s}")),
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            s => return Err(format!("canon {sub} takes one directory; got an extra argument: {s}")),
        }
    }
    let dir = dir.ok_or_else(|| format!("no directory given (try `canon {sub} --help`)"))?;
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }
    Ok(Some(Args { dir, apply, yes }))
}

/// Every regular file directly inside `dir`, sorted for deterministic output.
fn read_dir_files(dir: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect();
    files.sort();
    Ok(files)
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
            _ => "n".to_string(), // real EOF: treat as decline, never as blanket yes
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

// --- shows ---

fn show_summary(plan: &Plan) {
    println!();
    let name = if plan.name.is_empty() { format!("{DIM}(none found){OFF}") } else { plan.name.clone() };
    println!("{BOLD}Show Name:{OFF} {name}");
    match plan.season {
        Season::Single(n) => println!("{BOLD}Season Number:{OFF} {n}"),
        Season::Varies(a, b) => println!("{BOLD}Season Number:{OFF} varies ({a}-{b})"),
        Season::Unknown => {}
    }
    println!("{BOLD}Confidence:{OFF} {}", mark(plan.confidence));
    if let Some(p) = plan.preview() {
        let count = plan.episodes.len();
        let suffix = if count > 1 { format!("  {DIM}({count} files){OFF}") } else { String::new() };
        println!("{BOLD}Preview:{OFF} {p}{suffix}");
    }
}

/// One confirmation for the whole directory: accept, decline, or type a name.
/// Returns false if the user declined.
fn confirm_show(plan: &mut Plan, auto_yes: bool) -> bool {
    let mut console = Console::new();
    loop {
        show_summary(plan);
        if auto_yes {
            return true;
        }
        match console.ask("\naccept: Y/n/e > ").to_lowercase().as_str() {
            "" | "y" => return true,
            "n" => return false,
            "e" => {
                let typed = console.ask("Show name > ");
                if !typed.trim().is_empty() {
                    plan.set_name(&typed);
                }
            }
            _ => println!("{YEL}?{OFF}"),
        }
    }
}

fn cmd_shows(args: Args) -> ExitCode {
    let paths = match read_dir_files(&args.dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("canon: can't read {}: {e}", args.dir.display());
            return ExitCode::from(2);
        }
    };

    let mut plan = plan_dir(&paths);
    if plan.episodes.is_empty() {
        println!("Nothing to rename.");
        for (base, why) in &plan.skipped {
            println!("  {DIM}SKIP  {base}  ({why}){OFF}");
        }
        return ExitCode::SUCCESS;
    }

    if args.yes && plan.name.is_empty() {
        eprintln!("canon: --yes given but no show name could be detected; run without --yes to type one");
        return ExitCode::from(1);
    }

    if !confirm_show(&mut plan, args.yes) {
        println!("\nAborted, nothing renamed.");
        return ExitCode::SUCCESS;
    }

    apply_renames(plan.targets(), &plan.skipped, args.apply)
}

// --- movies ---

/// Confirm every planned movie rename in one pass: high-confidence lines
/// are listed and accepted silently, `Low` (no year found) stops to ask
/// for a title, or drops the file on an empty answer.
fn confirm_movies(plan: &mut MoviePlan, auto_yes: bool) -> Vec<(PathBuf, String)> {
    let mut console = Console::new();
    let width = plan.items.iter().map(|i| i.target().len()).max().unwrap_or(0);
    let mut targets = Vec::new();
    let mut newly_skipped = Vec::new();

    for item in plan.items.iter_mut() {
        if item.confidence == Confidence::Low {
            if auto_yes {
                newly_skipped.push((item.basename(), "low confidence, no year found".to_string()));
                continue;
            }
            let prompt = format!("{:width$} [{}] -> type title or Enter to skip: ", item.target(), mark(Confidence::Low));
            let typed = console.ask(&prompt);
            if typed.trim().is_empty() {
                newly_skipped.push((item.basename(), "no year found, skipped".to_string()));
                continue;
            }
            item.set_title(&typed);
        } else {
            println!("{:width$} [{}]", item.target(), mark(item.confidence));
        }
        targets.push((item.path.clone(), item.target()));
    }

    plan.skipped.extend(newly_skipped);
    targets
}

fn cmd_movies(args: Args) -> ExitCode {
    let paths = match read_dir_files(&args.dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("canon: can't read {}: {e}", args.dir.display());
            return ExitCode::from(2);
        }
    };

    let mut plan = plan_movies(&paths);
    if plan.items.is_empty() {
        println!("Nothing to rename.");
        for (base, why) in &plan.skipped {
            println!("  {DIM}SKIP  {base}  ({why}){OFF}");
        }
        return ExitCode::SUCCESS;
    }

    println!();
    let targets = confirm_movies(&mut plan, args.yes);
    apply_renames(targets, &plan.skipped, args.apply)
}

// --- books ---

/// Confirm every planned book rename in one pass, exactly as movies does:
/// high-confidence lines are listed and accepted silently, and only `Low`
/// (no author could be separated from the title) stops to ask.
fn confirm_books(plan: &mut BookPlan, auto_yes: bool) -> Vec<(PathBuf, String)> {
    let mut console = Console::new();
    let width = plan.items.iter().map(|i| i.target().len()).max().unwrap_or(0);
    let mut targets = Vec::new();
    let mut newly_skipped = Vec::new();

    for item in plan.items.iter_mut() {
        if item.confidence == Confidence::Low {
            if auto_yes {
                newly_skipped.push((item.basename(), "low confidence, no author found".to_string()));
                continue;
            }
            let prompt = format!("{:width$} [{}] -> type author or Enter to skip: ", item.target(), mark(Confidence::Low));
            let typed = console.ask(&prompt);
            if typed.trim().is_empty() {
                newly_skipped.push((item.basename(), "no author found, skipped".to_string()));
                continue;
            }
            item.set_author(&typed);
        } else {
            println!("{:width$} [{}]", item.target(), mark(item.confidence));
        }
        targets.push((item.path.clone(), item.target()));
    }

    plan.skipped.extend(newly_skipped);
    targets
}

fn cmd_books(args: Args) -> ExitCode {
    let paths = match read_dir_files(&args.dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("canon: can't read {}: {e}", args.dir.display());
            return ExitCode::from(2);
        }
    };

    let mut plan = plan_books(&paths);
    if plan.items.is_empty() {
        println!("Nothing to rename.");
        for (base, why) in &plan.skipped {
            println!("  {DIM}SKIP  {base}  ({why}){OFF}");
        }
        return ExitCode::SUCCESS;
    }

    println!();
    let targets = confirm_books(&mut plan, args.yes);
    apply_renames(targets, &plan.skipped, args.apply)
}

// --- shared: safety checks + apply, once a target list has been confirmed ---

fn apply_renames(targets: Vec<(PathBuf, String)>, skipped: &[(String, String)], apply: bool) -> ExitCode {
    let mut planned: Vec<(PathBuf, String)> = Vec::new();
    let mut unchanged = 0usize;
    for (path, new) in targets {
        if path.file_name().is_some_and(|f| f.to_string_lossy() == new) {
            unchanged += 1;
        } else {
            planned.push((path, new));
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
    println!("  {} file(s) to rename", planned.len());
    if unchanged > 0 {
        println!("  {DIM}{unchanged} file(s) already correctly named{OFF}");
    }
    for (base, why) in skipped {
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
    if !apply {
        println!("\n{DIM}Dry run. Re-run with --apply to perform these renames.{OFF}");
        return ExitCode::SUCCESS;
    }
    if !collisions.is_empty() || !exists.is_empty() {
        println!("\n{RED}Refusing to apply: resolve the conflicts above first.{OFF}");
        return ExitCode::from(1);
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

fn main() -> ExitCode {
    let mut raw_args = std::env::args().skip(1);
    let sub = match raw_args.next() {
        Some(s) => s,
        None => {
            print!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match sub.as_str() {
        "shows" => match parse_common("shows", raw_args) {
            Ok(Some(a)) => cmd_shows(a),
            Ok(None) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("canon: {e}");
                ExitCode::from(2)
            }
        },
        "movies" => match parse_common("movies", raw_args) {
            Ok(Some(a)) => cmd_movies(a),
            Ok(None) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("canon: {e}");
                ExitCode::from(2)
            }
        },
        "books" => match parse_common("books", raw_args) {
            Ok(Some(a)) => cmd_books(a),
            Ok(None) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("canon: {e}");
                ExitCode::from(2)
            }
        },
        "-h" | "--help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        "-V" | "--version" => {
            println!("canon {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("canon: unknown subcommand '{other}' (expected 'shows', 'movies' or 'books')");
            ExitCode::from(2)
        }
    }
}
