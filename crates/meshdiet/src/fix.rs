//! `meshdiet fix`: compress flagged models with gltfpack and report what was saved (SPEC-FIX-001).

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::ValueEnum;
use meshdiet_core::Options;
use meshdiet_core::fix::{self, FixOptions, FixReport, Status, Target, Textures};

use crate::output::size;

/// Arguments of `meshdiet fix`.
#[derive(clap::Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "command line switches; clap fills them"
)]
pub struct FixArgs {
    /// Folders or model files to fix [default: .].
    pub paths: Vec<PathBuf>,
    /// Write the results here, mirroring the input folders [default: meshdiet-out].
    #[arg(long, conflicts_with = "in_place")]
    pub out: Option<PathBuf>,
    /// Overwrite the inputs, only those git has a clean copy of, after each output passed its checks.
    #[arg(long)]
    pub in_place: bool,
    /// With --in-place: also overwrite files git does not have a clean copy of.
    #[arg(long, requires = "in_place")]
    pub force: bool,
    /// What to do with textures.
    #[arg(long, value_enum, default_value_t = TextureArg::Webp)]
    pub textures: TextureArg,
    /// Largest texture side in pixels, a power of two from 256 to 8192 [default: 2048].
    #[arg(long, value_parser = parse_max_texture)]
    pub max_texture: Option<u32>,
    /// Compress every model, not only the ones with MESH-001 or TEX findings.
    #[arg(long)]
    pub all: bool,
    /// Write .gltf inputs back as .gltf instead of .glb.
    #[arg(long)]
    pub keep_gltf: bool,
    /// Compress into a temporary folder and report; write nothing.
    #[arg(long, conflicts_with_all = ["out", "in_place"])]
    pub dry_run: bool,
    /// gltfpack processes at once [default: logical CPUs, at most 8].
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=64))]
    pub jobs: Option<u16>,
    /// The gltfpack binary (else `$MESHDIET_GLTFPACK`, else gltfpack on PATH).
    #[arg(long, value_name = "PATH")]
    pub gltfpack: Option<PathBuf>,
    /// Extra flag passed to gltfpack as it is (repeat for more).
    #[arg(long = "gltfpack-arg", value_name = "ARG", allow_hyphen_values = true)]
    pub gltfpack_args: Vec<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = FixFormat::Pretty)]
    pub format: FixFormat,
    /// Limits such as `texture_max_px` [default: meshdiet.toml in the folder given, else here].
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
}

impl FixArgs {
    /// `fix` with every default, for `scan --fix`.
    pub fn defaults(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            out: None,
            in_place: false,
            force: false,
            textures: TextureArg::Webp,
            max_texture: None,
            all: false,
            keep_gltf: false,
            dry_run: false,
            jobs: None,
            gltfpack: None,
            gltfpack_args: Vec::new(),
            format: FixFormat::Pretty,
            config: None,
        }
    }
}

/// Texture handling on the command line.
#[derive(Clone, Copy, ValueEnum)]
pub enum TextureArg {
    /// WebP (gltfpack -tw): smaller downloads, no extra loader.
    Webp,
    /// KTX2 / Basis Universal (gltfpack -tc): stays compressed on the GPU, needs the three.js KTX2 loader.
    Ktx2,
    /// Leave textures as they are.
    Keep,
}

/// How the fix report is written.
#[derive(Clone, Copy, ValueEnum)]
pub enum FixFormat {
    /// Readable terminal output.
    Pretty,
    /// Machine-readable JSON.
    Json,
}

fn parse_max_texture(s: &str) -> Result<u32, String> {
    let n: u32 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if n.is_power_of_two() && (256..=8192).contains(&n) {
        Ok(n)
    } else {
        Err("must be a power of two from 256 to 8192".to_owned())
    }
}

/// Run `meshdiet fix`. Exit 0 when every model was fixed or kept on purpose, 1 when gltfpack failed
/// or an output did not pass its checks, 2 when gltfpack is missing or the arguments are wrong.
pub fn run(args: &FixArgs) -> Result<ExitCode, String> {
    fix::check_extra_args(&args.gltfpack_args)?;
    let tool = match fix::gltfpack::locate(args.gltfpack.as_deref()) {
        Ok(tool) => tool,
        Err(e) => {
            eprintln!("meshdiet: {e}\n\n{}", fix::gltfpack::install_help());
            return Ok(ExitCode::from(2));
        }
    };
    let config = crate::config(args.config.as_deref(), &args.paths)?;
    let target = if args.dry_run {
        Target::DryRun
    } else if args.in_place {
        Target::InPlace { force: args.force }
    } else {
        Target::Out(
            args.out
                .clone()
                .unwrap_or_else(|| PathBuf::from("meshdiet-out")),
        )
    };
    let options = FixOptions {
        target,
        textures: match args.textures {
            TextureArg::Webp => Textures::Webp,
            TextureArg::Ktx2 => Textures::Ktx2,
            TextureArg::Keep => Textures::Keep,
        },
        max_texture: args.max_texture.unwrap_or(config.texture_max_px),
        all: args.all,
        keep_gltf: args.keep_gltf,
        extra_args: args.gltfpack_args.clone(),
        jobs: args.jobs.map_or_else(fix::default_jobs, usize::from),
    };
    let scan = meshdiet_core::scan(
        &args.paths,
        &Options {
            config,
            ..Options::default()
        },
    )?;
    if scan.assets.is_empty() {
        eprintln!("meshdiet: no .glb or .gltf files under {}", scan.root);
    }
    let report = fix::run(&scan, &tool, &options)?;
    let text = match args.format {
        FixFormat::Pretty => pretty(&report),
        FixFormat::Json => {
            let mut t = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
            t.push('\n');
            t
        }
    };
    std::io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .map_err(|e| format!("cannot write the report: {e}"))?;
    Ok(if report.failed() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `-89%` from two byte counts; `0%` when nothing changed.
#[allow(
    clippy::cast_precision_loss,
    reason = "a whole percent of a byte count"
)]
fn percent(before: u64, after: u64) -> String {
    if before == 0 || before == after {
        return "0%".to_owned();
    }
    let p = (after as f64 - before as f64) / before as f64 * 100.0;
    if p < 0.0 {
        format!("\u{2212}{:.0}%", -p)
    } else {
        format!("+{p:.0}%")
    }
}

/// The readable report (FR-006).
pub fn pretty(report: &FixReport) -> String {
    let mut out = String::new();
    let flags = report
        .results
        .iter()
        .find(|r| !r.args.is_empty())
        .map(|r| r.args.join(" "));
    let _ = writeln!(
        out,
        "gltfpack {} ({}){}\n",
        report.gltfpack.version,
        report.gltfpack.path.display(),
        flags.map(|f| format!(" with {f}")).unwrap_or_default()
    );
    let width = report
        .results
        .iter()
        .map(|r| r.input.chars().count())
        .max()
        .unwrap_or(0);
    for r in &report.results {
        let sizes = if r.status == Status::Fixed {
            format!(
                "{} \u{2192} {}  {}",
                size(r.before),
                size(r.after),
                percent(r.before, r.after)
            )
        } else {
            size(r.before)
        };
        let _ = writeln!(out, "{:<width$}  {sizes}  {}", r.input, r.status.label());
        if let Some(reason) = &r.reason {
            for line in reason.lines() {
                let _ = writeln!(out, "    {line}");
            }
        }
        for note in &r.notes {
            let _ = writeln!(out, "    note: {note}");
        }
    }
    let t = report.totals;
    let _ = writeln!(
        out,
        "\n{} {} \u{b7} {} \u{2192} {} ({}) \u{b7} {} fixed, measured on disk",
        t.files,
        if t.files == 1 { "file" } else { "files" },
        size(t.before),
        size(t.after),
        percent(t.before, t.after),
        report.fixed()
    );
    match (report.mode, &report.out) {
        ("dry-run", _) => {
            let _ = writeln!(out, "Dry run: nothing was written.");
        }
        (_, Some(dir)) => {
            let _ = writeln!(
                out,
                "Written to {dir}/ (models not fixed are copied as they are)."
            );
        }
        _ => {}
    }
    loader_help(&mut out, report);
    out
}

fn loader_help(out: &mut String, report: &FixReport) {
    if report.decoders.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\nThe fixed models use meshopt compression; tell three.js how to read them:\n\
         \n  import {{ MeshoptDecoder }} from 'three/examples/jsm/libs/meshopt_decoder.module.js';\
         \n  loader.setMeshoptDecoder(MeshoptDecoder);"
    );
    if report.decoders.contains(&"ktx2") {
        let _ = writeln!(
            out,
            "\nand KTX2 textures:\n\
             \n  import {{ KTX2Loader }} from 'three/examples/jsm/loaders/KTX2Loader.js';\
             \n  loader.setKTX2Loader(new KTX2Loader().setTranscoderPath('/basis/').detectSupport(renderer));"
        );
    }
    let target = report.out.as_deref().unwrap_or("your models folder");
    let _ = writeln!(
        out,
        "\nThen run `meshdiet scan {target}` again (LOAD-001 checks the decoder wiring)."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_texture_must_be_a_power_of_two_in_range() {
        assert_eq!(parse_max_texture("1024"), Ok(1024));
        assert!(parse_max_texture("1000").is_err());
        assert!(parse_max_texture("128").is_err());
        assert!(parse_max_texture("16384").is_err());
    }

    #[test]
    fn percent_is_signed() {
        assert_eq!(percent(100, 11), "\u{2212}89%");
        assert_eq!(percent(100, 100), "0%");
        assert_eq!(percent(100, 150), "+50%");
    }
}
