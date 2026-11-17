//! `meshdiet`: find the 3D models that make your website slow.

#![forbid(unsafe_code)]

mod fix;
mod output;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use meshdiet_core::{Config, FailOn, Options, Severity};

/// Find the 3D models that make your website slow.
#[derive(Parser)]
#[command(name = "meshdiet", version = meshdiet_core::VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read every .glb and .gltf under the given paths and report what makes them heavy.
    Scan(ScanArgs),
    /// Compress the flagged models with gltfpack, check every output, and report what was saved.
    Fix(fix::FixArgs),
}

#[derive(clap::Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "command line switches; clap fills them"
)]
struct ScanArgs {
    /// Folders or model files to scan [default: .].
    paths: Vec<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Pretty)]
    format: Format,
    /// Write the report to this file instead of the terminal.
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Exit with code 1 when a finding is at least this severe.
    #[arg(long, value_enum, default_value_t = Threshold::High)]
    fail_on: Threshold,
    /// Let medium-confidence findings fail the build too.
    #[arg(long)]
    strict: bool,
    /// Folder with the app's JS/TS/HTML, searched for decoder setup (LOAD-001) [default: the
    /// scanned folder].
    #[arg(long, value_name = "DIR")]
    src: Option<PathBuf>,
    /// Print only the summary line.
    #[arg(long, short, conflicts_with = "verbose")]
    quiet: bool,
    /// Print each file on stderr as it is read.
    #[arg(long, short)]
    verbose: bool,
    /// Budgets and page groups [default: meshdiet.toml in the scanned folder, else here].
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Add a gzip -6 transfer estimate per file (reads every byte).
    #[arg(long)]
    gzip: bool,
    /// Show every row of the weight table, not only the 20 largest.
    #[arg(long)]
    all: bool,
    /// Then run `meshdiet fix` with its defaults on the same paths.
    #[arg(long)]
    fix: bool,
}

/// How the report is written.
#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// Readable terminal output.
    Pretty,
    /// Machine-readable JSON (schema 1).
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum Threshold {
    High,
    Medium,
    Low,
    Never,
}

impl From<Threshold> for FailOn {
    fn from(t: Threshold) -> Self {
        match t {
            Threshold::High => FailOn::AtLeast(Severity::High),
            Threshold::Medium => FailOn::AtLeast(Severity::Medium),
            Threshold::Low => FailOn::AtLeast(Severity::Low),
            Threshold::Never => FailOn::Never,
        }
    }
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Scan(args) => scan(&args),
        Command::Fix(args) => fix::run(&args),
    }
    .unwrap_or_else(|e| {
        eprintln!("meshdiet: {e}");
        ExitCode::from(2)
    })
}

fn progress(path: &str) {
    eprintln!("reading {path}");
}

/// `--config`, else `meshdiet.toml` in the one folder given, else in the current folder, else the
/// defaults.
fn config(file: Option<&std::path::Path>, paths: &[PathBuf]) -> Result<Config, String> {
    if let Some(file) = file {
        return Config::load(file);
    }
    let mut candidates = Vec::new();
    if let [dir] = paths
        && dir.is_dir()
    {
        candidates.push(dir.join("meshdiet.toml"));
    }
    candidates.push(PathBuf::from("meshdiet.toml"));
    match candidates.into_iter().find(|p| p.is_file()) {
        Some(file) => Config::load(&file),
        None => Ok(Config::default()),
    }
}

fn scan(args: &ScanArgs) -> Result<ExitCode, String> {
    if let Some(src) = &args.src
        && !src.is_dir()
    {
        return Err(format!("--src {} is not a folder", src.display()));
    }
    if args.fix && matches!(args.format, Format::Json) {
        return Err(
            "--fix prints a second report; use `meshdiet fix --format json` for JSON".into(),
        );
    }
    let options = Options {
        config: config(args.config.as_deref(), &args.paths)?,
        progress: args.verbose.then_some(progress as fn(&str)),
        gzip: args.gzip,
        src: args.src.clone(),
        only: Vec::new(),
        skip: Vec::new(),
    };
    let report = meshdiet_core::scan(&args.paths, &options)?;
    for err in &report.internal_errors {
        eprintln!("meshdiet: {err} (the scan carried on without it)");
    }
    if report.assets.is_empty() {
        eprintln!(
            "meshdiet: no .glb or .gltf files under {}; point it at the folder your models are in, for example `meshdiet scan public`",
            report.root
        );
    }
    let to_terminal = args.output.is_none() && std::io::stdout().is_terminal();
    let color = to_terminal && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    let text = match args.format {
        Format::Pretty => output::pretty(
            &report,
            &output::Style {
                color,
                quiet: args.quiet,
                all_rows: args.all,
            },
        ),
        Format::Json => output::json(&report),
    };
    match &args.output {
        Some(file) => std::fs::write(file, text),
        None => std::io::stdout().lock().write_all(text.as_bytes()),
    }
    .map_err(|e| format!("cannot write the report: {e}"))?;
    if args.fix {
        println!();
        let mut defaults = fix::FixArgs::defaults(args.paths.clone());
        defaults.config.clone_from(&args.config);
        return fix::run(&defaults);
    }
    Ok(if report.fails(args.fail_on.into(), args.strict) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
