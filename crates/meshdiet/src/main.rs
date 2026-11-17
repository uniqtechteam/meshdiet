//! `meshdiet`: find the 3D models that make your website slow.

#![forbid(unsafe_code)]

mod output;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use meshdiet_core::{FailOn, Options, Severity};

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
    /// Print only the summary line.
    #[arg(long, short, conflicts_with = "verbose")]
    quiet: bool,
    /// Print each file on stderr as it is read.
    #[arg(long, short)]
    verbose: bool,
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
        Command::Scan(args) => scan(&args).unwrap_or_else(|e| {
            eprintln!("meshdiet: {e}");
            ExitCode::from(2)
        }),
    }
}

fn progress(path: &str) {
    eprintln!("reading {path}");
}

fn scan(args: &ScanArgs) -> Result<ExitCode, String> {
    let options = Options {
        progress: args.verbose.then_some(progress as fn(&str)),
        ..Options::default()
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
        Format::Pretty => output::pretty(&report, color, args.quiet),
        Format::Json => output::json(&report),
    };
    match &args.output {
        Some(file) => std::fs::write(file, text),
        None => std::io::stdout().lock().write_all(text.as_bytes()),
    }
    .map_err(|e| format!("cannot write the report: {e}"))?;
    Ok(if report.fails(args.fail_on.into(), args.strict) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
