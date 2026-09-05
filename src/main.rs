//! Interactive CLI: scan one show directory, confirm a name once, then apply.

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
canon -- rename TV episode files to \"Show Name SxxEyy.ext\"

USAGE:
    canon [OPTIONS] <DIRECTORY>

OPTIONS:
    --apply           perform the renames (default: dry run)
    --yes             skip the confirm step, accept the detected name
    -h, --help        show this help
    -V, --version     show version

canon looks at every file directly inside DIRECTORY, decides a single show
name for all of them (using agreement across the files, falling back to the
directory name), and confirms that name once before renaming.
";

struct Args {
    dir: PathBuf,
    apply: bool,
    yes: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut dir = None;
    let (mut apply, mut yes) = (false, false);
    for a in std::env::args().skip(1) {
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
            s => return Err(format!("canon takes one directory; got an extra argument: {s}")),
        }
    }
    let dir = dir.ok_or("no directory given (try --help)")?;
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
fn confirm(plan: &mut Plan, auto_yes: bool) -> bool {
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

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("canon: {e}");
            return ExitCode::from(2);
        }
    };

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

    if !confirm(&mut plan, args.yes) {
        println!("\nAborted, nothing renamed.");
        return ExitCode::SUCCESS;
    }

    // --- safety checks before touching disk ---
    let mut planned: Vec<(PathBuf, String)> = Vec::new();
    let mut unchanged = 0usize;
    for (path, new) in plan.targets() {
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
    for (base, why) in &plan.skipped {
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
