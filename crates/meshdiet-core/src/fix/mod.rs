//! `meshdiet fix` (SPEC-FIX-001, ADR-004): compress the assets a scan flagged with gltfpack, check
//! every output with the same reader as `scan`, and report only savings measured on disk.
//!
//! gltfpack always runs with `-kn -km -ke` (named nodes, named materials and extras survive) and
//! never with a simplification flag. An input is only replaced with `--in-place`, after its output
//! passed every check, and only when git has a clean copy of it (or `--force`).

pub mod gltfpack;
pub mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

pub use gltfpack::{Gltfpack, LocateError};

use crate::{Asset, AssetKind, Report, asset};

/// Version of the `fix --format json` shape.
pub const SCHEMA: u32 = 1;

/// Findings that make an asset worth compressing (FR-002).
pub const FIXABLE: &[&str] = &["MESH-001", "TEX-001", "TEX-002", "TEX-003", "TEX-004"];

/// gltfpack flags that simplify meshes; `fix` never passes them (ADR-004).
const SIMPLIFY_FLAGS: &[&str] = &["-si", "-se", "-sv", "-sp", "-sa", "-slb"];

/// How many lines of gltfpack's stderr a failure keeps.
const STDERR_LINES: usize = 20;

/// What happens to textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Textures {
    /// WebP (`-tw`): smaller downloads, no extra loader.
    Webp,
    /// KTX2 with Basis Universal (`-tc`): stays compressed on the GPU, needs `KTX2Loader`.
    Ktx2,
    /// Leave images as they are.
    Keep,
}

/// Where the results go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Mirror the input tree into this folder.
    Out(PathBuf),
    /// Replace the inputs; without `force`, only files git has a clean copy of.
    InPlace {
        /// Skip the git check.
        force: bool,
    },
    /// Compress into a temporary folder, report, write nothing.
    DryRun,
}

/// How `fix` runs.
#[derive(Debug, Clone)]
pub struct FixOptions {
    /// Where results go.
    pub target: Target,
    /// Texture handling.
    pub textures: Textures,
    /// Largest texture side in pixels (`-tl`), ignored with [`Textures::Keep`].
    pub max_texture: u32,
    /// Compress every asset, not only the flagged ones.
    pub all: bool,
    /// Write `.gltf` inputs back as `.gltf` instead of `.glb`.
    pub keep_gltf: bool,
    /// Extra gltfpack flags, passed as given (`--gltfpack-arg`).
    pub extra_args: Vec<String>,
    /// gltfpack processes at once.
    pub jobs: usize,
}

impl FixOptions {
    /// The gltfpack flags every asset gets, after `-i IN -o OUT` (FR-003).
    #[must_use]
    pub fn flags(&self) -> Vec<String> {
        let mut flags: Vec<String> = ["-kn", "-km", "-ke", "-cc"].map(String::from).to_vec();
        match self.textures {
            Textures::Webp => flags.push("-tw".into()),
            Textures::Ktx2 => flags.push("-tc".into()),
            Textures::Keep => {}
        }
        if self.textures != Textures::Keep {
            flags.extend(["-tl".to_owned(), self.max_texture.to_string()]);
        }
        flags.extend(self.extra_args.iter().cloned());
        flags
    }
}

/// Default for `--jobs`: logical CPUs, at most 8 (FR-008).
#[must_use]
pub fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
}

/// Refuse extra gltfpack flags that would change the input or output, or simplify meshes.
///
/// # Errors
/// Names the first refused flag.
pub fn check_extra_args(args: &[String]) -> Result<(), String> {
    for a in args {
        if a == "-i" || a == "-o" {
            return Err(format!(
                "--gltfpack-arg {a}: meshdiet sets the input and output itself"
            ));
        }
        if SIMPLIFY_FLAGS.contains(&a.as_str()) {
            return Err(format!(
                "--gltfpack-arg {a}: meshdiet never simplifies meshes, so apps keep the geometry they were built for"
            ));
        }
    }
    Ok(())
}

/// What happened to one asset (SPEC-FIX-001 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Not passed to gltfpack; `reason` says why.
    Skipped,
    /// Compressed, checked and smaller.
    Fixed,
    /// The output was not smaller; the original is kept.
    NotImproved,
    /// The output lost something or cannot be read; the original is kept.
    VerifyFailed,
    /// gltfpack exited with an error; the original is kept.
    GltfpackFailed,
}

impl Status {
    /// Words for the terminal.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Fixed => "fixed",
            Self::NotImproved => "not improved",
            Self::VerifyFailed => "verify failed",
            Self::GltfpackFailed => "gltfpack failed",
        }
    }

    /// Whether this makes `fix` exit 1.
    #[must_use]
    pub fn failed(self) -> bool {
        matches!(self, Self::VerifyFailed | Self::GltfpackFailed)
    }
}

/// One asset's result.
#[derive(Debug, Clone, Serialize)]
pub struct FixResult {
    /// The input, relative to the scan root.
    pub input: String,
    /// Where the result was written; `None` in a dry run.
    pub output: Option<String>,
    /// Bytes of the input and its files.
    pub before: u64,
    /// Bytes of what is there now: the output when fixed, else the original.
    pub after: u64,
    /// What happened.
    pub status: Status,
    /// Why, when not fixed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// gltfpack flags used (empty when skipped).
    pub args: Vec<String>,
    /// Things worth knowing, such as a shared image copied into several outputs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Totals measured on disk; a file shared by several assets counts once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FixTotals {
    /// Assets looked at.
    pub files: usize,
    /// Bytes before.
    pub before: u64,
    /// Bytes after (fixed outputs plus kept originals).
    pub after: u64,
}

/// Everything `fix` did.
#[derive(Debug, Clone, Serialize)]
pub struct FixReport {
    /// JSON shape version.
    pub schema: u32,
    /// Engine version.
    pub version: &'static str,
    /// The gltfpack that ran.
    pub gltfpack: Gltfpack,
    /// `out`, `in-place` or `dry-run`.
    pub mode: &'static str,
    /// The output folder (`out` mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    /// Texture handling.
    pub textures: Textures,
    /// One per asset, sorted by path.
    pub results: Vec<FixResult>,
    /// Totals.
    pub totals: FixTotals,
    /// Decoders the app needs to load the outputs: `meshopt`, `ktx2`.
    pub decoders: Vec<&'static str>,
}

impl FixReport {
    /// Whether any asset failed (exit 1).
    #[must_use]
    pub fn failed(&self) -> bool {
        self.results.iter().any(|r| r.status.failed())
    }

    /// How many assets were fixed.
    #[must_use]
    pub fn fixed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == Status::Fixed)
            .count()
    }
}

/// `path` as forward-slash text.
fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Only the plain folder and file names of `rel`, so an output can never land outside its folder.
fn contained(rel: &str) -> Option<PathBuf> {
    let path = Path::new(rel);
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        .then(|| {
            path.components()
                .filter(|c| matches!(c, Component::Normal(_)))
                .collect()
        })
}

fn already_compressed(a: &Asset) -> bool {
    (a.compression.meshopt || a.primitives == 0)
        && a.images
            .iter()
            .all(|i| matches!(i.mime.as_deref(), Some("image/ktx2" | "image/webp")))
}

fn skip_reason(a: &Asset, flagged: bool, options: &FixOptions) -> Option<String> {
    let reason = if a.document.is_none() || !a.problems.is_empty() {
        "cannot be read (GLTF-001); fix the file first"
    } else if a.compression.draco {
        "already compressed with Draco, which gltfpack cannot read"
    } else if already_compressed(a) {
        "already compressed (meshopt geometry, KTX2 or WebP textures)"
    } else if !flagged && !options.all {
        "no MESH-001 or TEX finding; --all compresses it anyway"
    } else if contained(&a.path).is_none() {
        "outside the scanned folder; fix it from its own folder"
    } else if matches!(options.target, Target::InPlace { .. }) && a.kind == AssetKind::Gltf {
        "--in-place rewrites .glb files only; use --out for .gltf"
    } else {
        return None;
    };
    Some(reason.to_owned())
}

/// `Ok` when git tracks `path` and it has no local changes (FR-004).
fn git_clean(path: &Path) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or(path.as_os_str());
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .arg("--")
            .arg(name)
            .output()
    };
    let refuse = "not committed to git, so --in-place will not overwrite it (commit it first, or pass --force)";
    match git(&["ls-files", "--error-unmatch"]) {
        Ok(o) if o.status.success() => {}
        _ => return Err(refuse.to_owned()),
    }
    match git(&["status", "--porcelain"]) {
        Ok(o) if o.status.success() && o.stdout.is_empty() => Ok(()),
        _ => Err(
            "has uncommitted changes, so --in-place will not overwrite it (commit first, or pass --force)"
                .to_owned(),
        ),
    }
}

/// The last lines gltfpack printed, for a failure report.
fn tail(out: &std::process::Output) -> String {
    let text = if out.stderr.iter().any(|b| !b.is_ascii_whitespace()) {
        String::from_utf8_lossy(&out.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let kept = &lines[lines.len().saturating_sub(STDERR_LINES)..];
    format!("gltfpack exited with {}: {}", out.status, kept.join("\n"))
}

/// One asset to compress.
struct Job<'a> {
    asset: &'a Asset,
    abs: PathBuf,
    /// Output path relative to the output root.
    out_rel: String,
}

/// What compressing one asset gave.
struct Outcome {
    status: Status,
    reason: Option<String>,
    output: Option<Asset>,
}

impl Outcome {
    fn kept(status: Status, reason: String) -> Self {
        Self {
            status,
            reason: Some(reason),
            output: None,
        }
    }
}

/// Run gltfpack on one asset into `tmp`, then read and check what it wrote (FR-005).
fn compress(tool: &Gltfpack, job: &Job<'_>, tmp: &Path, flags: &[String]) -> Outcome {
    let dest = tmp.join(&job.out_rel);
    if let Some(parent) = dest.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return Outcome::kept(
            Status::GltfpackFailed,
            format!("cannot make a temp folder: {e}"),
        );
    }
    // An argument list, never a shell, so any path is safe (BR-003).
    let run = Command::new(&tool.path)
        .arg("-i")
        .arg(&job.abs)
        .arg("-o")
        .arg(&dest)
        .args(flags)
        .output();
    match run {
        Err(e) => {
            return Outcome::kept(Status::GltfpackFailed, format!("gltfpack did not run: {e}"));
        }
        Ok(out) if !out.status.success() => {
            return Outcome::kept(Status::GltfpackFailed, tail(&out));
        }
        Ok(_) if !dest.is_file() => {
            return Outcome::kept(Status::GltfpackFailed, "gltfpack wrote no output".into());
        }
        Ok(_) => {}
    }
    let after = asset::read(&job.out_rel, &dest);
    if let Some(problem) = after.problems.first() {
        return Outcome::kept(
            Status::VerifyFailed,
            format!("the output cannot be read: {problem}"),
        );
    }
    if let (Some(before), Some(now)) = (&job.asset.document, &after.document)
        && let Err(e) = verify::same_structure(before, now)
    {
        return Outcome::kept(Status::VerifyFailed, e);
    }
    if after.bytes >= job.asset.bytes {
        return Outcome::kept(
            Status::NotImproved,
            format!(
                "the output is {} bytes, the input {}; the original is kept",
                after.bytes, job.asset.bytes
            ),
        );
    }
    Outcome {
        status: Status::Fixed,
        reason: None,
        output: Some(after),
    }
}

/// Compress every job with up to `jobs` gltfpack processes; results come back in job order.
fn compress_all(
    tool: &Gltfpack,
    jobs: &[Job<'_>],
    tmp: &Path,
    flags: &[String],
    workers: usize,
) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, Outcome)>> = Mutex::new(Vec::with_capacity(jobs.len()));
    std::thread::scope(|s| {
        for _ in 0..workers.clamp(1, jobs.len().max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let outcome = compress(tool, job, tmp, flags);
                    done.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((i, outcome));
                }
            });
        }
    });
    let mut done = done
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, o)| o).collect()
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("cannot write {}: {e}", to.display()))
}

/// Notes for `.gltf` inputs that share an image file: each GLB output carries its own copy.
fn shared_image_notes(jobs: &[Job<'_>], keep_gltf: bool) -> BTreeMap<String, Vec<String>> {
    let mut users: BTreeMap<&str, usize> = BTreeMap::new();
    for job in jobs
        .iter()
        .filter(|j| j.asset.kind == AssetKind::Gltf && !keep_gltf)
    {
        let files: BTreeSet<&str> = job
            .asset
            .images
            .iter()
            .filter_map(|i| i.file.as_deref())
            .collect();
        for f in files {
            *users.entry(f).or_default() += 1;
        }
    }
    let mut notes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for job in jobs
        .iter()
        .filter(|j| j.asset.kind == AssetKind::Gltf && !keep_gltf)
    {
        for image in job.asset.images.iter().filter_map(|i| i.file.as_deref()) {
            if users.get(image).copied().unwrap_or(0) > 1 {
                notes
                    .entry(job.asset.path.clone())
                    .or_default()
                    .push(format!(
                        "{image} is shared with other models; this GLB embeds its own copy"
                    ));
            }
        }
    }
    notes
}

/// One asset `run` looks at: its result so far and, unless skipped, the job that compresses it.
struct Planned<'a> {
    asset: &'a Asset,
    result: FixResult,
    job: Option<usize>,
}

/// Refuse an `--out` that is the folder being fixed.
fn check_out(dir: &Path, root: &Path) -> Result<(), String> {
    if let (Ok(a), Ok(b)) = (dir.canonicalize(), root.canonicalize())
        && a == b
    {
        return Err(format!(
            "--out {} is the folder being fixed; pick another folder, or use --in-place",
            dir.display()
        ));
    }
    Ok(())
}

/// Decide, for every asset of `report`, whether it is skipped or compressed (FR-002, FR-004).
fn plan<'a>(
    report: &'a Report,
    root: &Path,
    options: &FixOptions,
) -> (Vec<Planned<'a>>, Vec<Job<'a>>) {
    let out_canon = match &options.target {
        Target::Out(dir) => dir.canonicalize().ok(),
        _ => None,
    };
    let flagged: BTreeSet<&str> = report
        .findings
        .iter()
        .filter(|f| FIXABLE.contains(&f.rule.as_str()))
        .map(|f| f.path.as_str())
        .collect();
    let mut planned = Vec::new();
    let mut jobs = Vec::new();
    for a in &report.assets {
        let abs = root.join(&a.path);
        // outputs of an earlier run inside the scanned folder are not inputs
        if let (Some(out), Ok(c)) = (&out_canon, abs.canonicalize())
            && c.starts_with(out)
        {
            continue;
        }
        let mut reason = skip_reason(a, flagged.contains(a.path.as_str()), options);
        if reason.is_none()
            && options.target == (Target::InPlace { force: false })
            && let Err(e) = git_clean(&abs)
        {
            reason = Some(e);
        }
        let job = match (&reason, contained(&a.path)) {
            (None, Some(rel)) => {
                let mut out_rel = slash(&rel);
                if a.kind == AssetKind::Gltf && !options.keep_gltf {
                    out_rel = slash(&rel.with_extension("glb"));
                }
                jobs.push(Job {
                    asset: a,
                    abs,
                    out_rel,
                });
                Some(jobs.len() - 1)
            }
            _ => None,
        };
        planned.push(Planned {
            asset: a,
            result: FixResult {
                input: a.path.clone(),
                output: None,
                before: a.bytes,
                after: a.bytes,
                status: Status::Skipped,
                reason,
                args: Vec::new(),
                notes: Vec::new(),
            },
            job,
        });
    }
    (planned, jobs)
}

/// Where `run` puts results, and the files that are there afterwards.
struct Writer<'a> {
    root: &'a Path,
    tmp: &'a Path,
    target: &'a Target,
    /// Every file now standing for the assets, relative to the output root, with its size.
    after_files: BTreeMap<String, u64>,
}

impl Writer<'_> {
    /// Put a checked output where it belongs.
    fn fixed(&mut self, r: &mut FixResult, job: &Job<'_>, out: &Asset) -> Result<(), String> {
        r.after = out.bytes;
        for (f, b) in out.files.iter().zip(&out.file_bytes) {
            self.after_files.insert(f.clone(), *b);
        }
        match self.target {
            Target::Out(dir) => {
                for f in &out.files {
                    copy(&self.tmp.join(f), &dir.join(f))?;
                }
                r.output = Some(slash(&dir.join(&job.out_rel)));
            }
            Target::InPlace { .. } => {
                copy(&self.tmp.join(&job.out_rel), &job.abs)?;
                r.output = Some(job.asset.path.clone());
            }
            Target::DryRun => {}
        }
        Ok(())
    }

    /// Keep the original. With `--out` it is copied, so the output folder is complete.
    fn kept(&mut self, r: &mut FixResult, a: &Asset) -> Result<(), String> {
        for (f, b) in a.files.iter().zip(&a.file_bytes) {
            self.after_files.insert(f.clone(), *b);
        }
        match self.target {
            Target::Out(dir) => {
                let mut all_copied = true;
                for f in &a.files {
                    match contained(f) {
                        Some(rel) => copy(&self.root.join(f), &dir.join(rel))?,
                        None => all_copied = false,
                    }
                }
                if all_copied {
                    r.output = Some(slash(&dir.join(&a.path)));
                }
            }
            Target::InPlace { .. } => r.output = Some(a.path.clone()),
            Target::DryRun => {}
        }
        Ok(())
    }
}

/// Compress the assets of `report` with `tool` (SPEC-FIX-001).
///
/// # Errors
/// When the output folder is the input folder, the temp folder cannot be made, or a result cannot
/// be written.
pub fn run(report: &Report, tool: &Gltfpack, options: &FixOptions) -> Result<FixReport, String> {
    check_extra_args(&options.extra_args)?;
    let root = PathBuf::from(&report.root);
    if let Target::Out(dir) = &options.target {
        check_out(dir, &root)?;
    }
    let flags = options.flags();
    let (planned, jobs) = plan(report, &root, options);
    let tmp = tempfile::Builder::new()
        .prefix("meshdiet-fix-")
        .tempdir()
        .map_err(|e| format!("cannot make a temp folder: {e}"))?;
    let mut notes = shared_image_notes(&jobs, options.keep_gltf);
    let outcomes = compress_all(tool, &jobs, tmp.path(), &flags, options.jobs);

    let mut before_files: BTreeMap<String, u64> = BTreeMap::new();
    let mut writer = Writer {
        root: &root,
        tmp: tmp.path(),
        target: &options.target,
        after_files: BTreeMap::new(),
    };
    let mut results = Vec::with_capacity(planned.len());
    for Planned {
        asset: a,
        mut result,
        job,
    } in planned
    {
        for (f, b) in a.files.iter().zip(&a.file_bytes) {
            before_files.insert(f.clone(), *b);
        }
        let done = job.and_then(|j| Some((jobs.get(j)?, outcomes.get(j)?)));
        if let Some((_, o)) = done {
            result.args.clone_from(&flags);
            result.status = o.status;
            result.reason.clone_from(&o.reason);
            result.notes = notes.remove(&a.path).unwrap_or_default();
            if let Some((
                job,
                Outcome {
                    status: Status::Fixed,
                    ..
                },
            )) = done
                && a.kind == AssetKind::Gltf
                && !options.keep_gltf
            {
                result.notes.push(format!(
                    "written as {} (one file); change the path your code loads, or pass --keep-gltf",
                    job.out_rel
                ));
            }
        }
        match done.and_then(|(job, o)| Some((job, o.output.as_ref()?))) {
            Some((job, out)) => writer.fixed(&mut result, job, out)?,
            None => writer.kept(&mut result, a)?,
        }
        results.push(result);
    }

    let mut decoders = Vec::new();
    if results.iter().any(|r| r.status == Status::Fixed) {
        decoders.push("meshopt");
        if options.textures == Textures::Ktx2 {
            decoders.push("ktx2");
        }
    }
    Ok(FixReport {
        schema: SCHEMA,
        version: crate::VERSION,
        gltfpack: tool.clone(),
        mode: match options.target {
            Target::Out(_) => "out",
            Target::InPlace { .. } => "in-place",
            Target::DryRun => "dry-run",
        },
        out: match &options.target {
            Target::Out(dir) => Some(slash(dir)),
            _ => None,
        },
        textures: options.textures,
        totals: FixTotals {
            files: results.len(),
            before: before_files.values().sum(),
            after: writer.after_files.values().sum(),
        },
        results,
        decoders,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(textures: Textures) -> FixOptions {
        FixOptions {
            target: Target::DryRun,
            textures,
            max_texture: 2048,
            all: false,
            keep_gltf: false,
            extra_args: vec!["-v".into()],
            jobs: 1,
        }
    }

    #[test]
    fn flags_keep_names_and_extras_and_never_simplify() {
        assert_eq!(
            options(Textures::Webp).flags(),
            ["-kn", "-km", "-ke", "-cc", "-tw", "-tl", "2048", "-v"]
        );
        assert_eq!(
            options(Textures::Ktx2).flags(),
            ["-kn", "-km", "-ke", "-cc", "-tc", "-tl", "2048", "-v"]
        );
        assert_eq!(
            options(Textures::Keep).flags(),
            ["-kn", "-km", "-ke", "-cc", "-v"]
        );
        assert!(check_extra_args(&["-si".into()]).is_err());
        assert!(check_extra_args(&["-o".into()]).is_err());
        assert!(check_extra_args(&["-cz".into()]).is_ok());
    }

    #[test]
    fn outputs_stay_inside_their_folder() {
        assert_eq!(contained("a/b.glb"), Some(PathBuf::from("a/b.glb")));
        assert_eq!(contained("./a.glb"), Some(PathBuf::from("a.glb")));
        assert_eq!(contained("../a.glb"), None);
        assert_eq!(contained("/abs/a.glb"), None);
    }
}
