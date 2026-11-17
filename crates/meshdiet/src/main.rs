//! `meshdiet`: find the 3D models that make your website slow.

#![forbid(unsafe_code)]

mod explain;
mod fix;
mod output;
mod sarif;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use meshdiet_core::baseline::parse_tolerance;
use meshdiet_core::{Baseline, Config, FailOn, Options, Severity};

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
    Scan(Box<ScanArgs>),
    /// Compress the flagged models with gltfpack, check every output, and report what was saved.
    Fix(fix::FixArgs),
    /// List every rule with its severity, confidence and what it finds.
    Rules,
    /// Explain one rule: why it matters, how to fix it, and the loader change the fix needs.
    Explain {
        /// Rule id, for example MESH-001.
        id: String,
    },
}

#[derive(clap::Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "command line switches; clap fills them"
)]
struct ScanArgs {
    /// Folders or model files to scan [default: `paths` in meshdiet.toml, else .].
    paths: Vec<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Pretty)]
    format: Format,
    /// Write the report to this file instead of the terminal.
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Exit with code 1 when a finding is at least this severe [default: `fail_on` in
    /// meshdiet.toml, else high].
    #[arg(long, value_enum)]
    fail_on: Option<Threshold>,
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
    /// Run only these rules (comma-separated ids, see `meshdiet rules`).
    #[arg(long, value_delimiter = ',', value_name = "IDS")]
    rules: Vec<String>,
    /// Never run these rules (comma-separated ids), on top of `[rules] skip` in meshdiet.toml.
    #[arg(long, value_delimiter = ',', value_name = "IDS")]
    skip: Vec<String>,
    /// Also list the findings that `ignore` entries in meshdiet.toml keep out, with their reasons.
    #[arg(long)]
    show_ignored: bool,
    /// Report only what is not in this baseline file, or grew past it by more than --tolerance.
    #[arg(long, value_name = "FILE")]
    baseline: Option<PathBuf>,
    /// Record every finding and file size in this file, to use later with --baseline.
    #[arg(long, value_name = "FILE")]
    write_baseline: Option<PathBuf>,
    /// How much a file or the build may grow past the baseline before it is reported.
    #[arg(long, value_name = "PERCENT", default_value = "1%", value_parser = tolerance)]
    tolerance: u32,
    /// Config file [default: meshdiet.toml in the scanned folder, else here]. Sizes are decimal:
    /// 1 MB = 1,000,000 bytes.
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
    /// SARIF 2.1.0, for GitHub code scanning.
    Sarif,
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

fn tolerance(text: &str) -> Result<u32, String> {
    parse_tolerance(text)
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Scan(args) => scan(&args),
        Command::Fix(args) => fix::run(&args),
        Command::Rules => {
            print!("{}", explain::rule_list());
            Ok(ExitCode::SUCCESS)
        }
        Command::Explain { id } => match meshdiet_core::rule::find(&id) {
            Some(info) => {
                print!("{}", explain::explain(info));
                Ok(ExitCode::SUCCESS)
            }
            None => Err(format!(
                "unknown rule `{id}`; run `meshdiet rules` to see them all"
            )),
        },
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

/// A path as the user wrote it, with forward slashes.
fn shown(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
    let config = config(args.config.as_deref(), &args.paths)?;
    let fail_on = match (args.fail_on, config.fail_on) {
        (Some(t), _) => t.into(),
        (None, Some(f)) => f,
        (None, None) => FailOn::AtLeast(Severity::High),
    };
    let paths = if args.paths.is_empty() {
        config.paths.clone()
    } else {
        args.paths.clone()
    };
    let options = Options {
        config,
        progress: args.verbose.then_some(progress as fn(&str)),
        gzip: args.gzip,
        src: args.src.clone(),
        only: args.rules.clone(),
        skip: args.skip.clone(),
    };
    options.validate()?;
    // Read the baseline before scanning, so a bad file fails fast.
    let baseline = match &args.baseline {
        Some(file) => Some(Baseline::load(file)?),
        None => None,
    };
    let mut report = meshdiet_core::scan(&paths, &options)?;
    if let Some(file) = &args.write_baseline {
        std::fs::write(file, Baseline::of(&report).to_json())
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        eprintln!(
            "meshdiet: wrote {} findings and {} files to {}",
            report.findings.len(),
            report.assets.len(),
            file.display()
        );
    }
    if let (Some(b), Some(file)) = (&baseline, &args.baseline) {
        b.apply(&mut report, args.tolerance, &shown(file));
    }
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
                show_ignored: args.show_ignored,
            },
        ),
        Format::Json => output::json(&report),
        Format::Sarif => sarif::render(&report),
    };
    match &args.output {
        Some(file) => std::fs::write(file, text),
        None => std::io::stdout().lock().write_all(text.as_bytes()),
    }
    .map_err(|e| format!("cannot write the report: {e}"))?;
    if args.fix {
        println!();
        let mut defaults = fix::FixArgs::defaults(paths);
        defaults.config.clone_from(&args.config);
        return fix::run(&defaults);
    }
    Ok(if report.fails(fail_on, args.strict) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
