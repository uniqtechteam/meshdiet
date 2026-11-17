//! A whole scan: walk, read every asset, run the rules, and decide whether it fails a build.
//! Exit-code logic first written for vibeguard (ADR-011).

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::baseline::{BaselineDiff, fingerprint};
use crate::config::CFG_IGN;
use crate::rule::{self, GLTF_001, Rule};
use crate::rules::load;
use crate::rules::size;
use crate::source_index::SourceIndex;
use crate::weight::{self, PageTotal, Totals};
use crate::{Asset, Confidence, Config, Finding, Ignore, Measured, Severity, asset, walk};

/// Version of the JSON report shape. Bumped only on breaking changes.
pub const SCHEMA: u32 = 1;

/// How a scan runs.
#[derive(Debug, Default)]
pub struct Options {
    /// Budgets and limits.
    pub config: Config,
    /// Called with each asset path before it is read (`--verbose`).
    pub progress: Option<fn(&str)>,
    /// Compute the gzip transfer estimate (`--gzip`); reads every byte of every file.
    pub gzip: bool,
    /// Where the app's JS/TS/HTML sources are, for LOAD-001 (`--src`); the scan root when `None`.
    pub src: Option<PathBuf>,
    /// Run only these rules (`--rules`); every rule when empty. CFG findings always run.
    pub only: Vec<String>,
    /// Never run these rules (`--skip`), on top of `[rules] skip` in the config.
    pub skip: Vec<String>,
}

impl Options {
    /// Check `only` and `skip` against the rule list (ids are not case-sensitive).
    ///
    /// # Errors
    /// An unknown rule id, with every valid one.
    pub fn validate(&self) -> Result<(), String> {
        for (list, flag) in [(&self.only, "--rules"), (&self.skip, "--skip")] {
            for id in list {
                crate::config::rule_id(id, flag)?;
            }
        }
        Ok(())
    }

    /// Whether findings of rule `id` are reported.
    #[must_use]
    pub fn runs(&self, id: &str) -> bool {
        let listed = |list: &[String]| list.iter().any(|x| x.eq_ignore_ascii_case(id));
        (self.only.is_empty() || listed(&self.only) || id.starts_with("CFG-"))
            && !listed(&self.skip)
            && !listed(&self.config.skip)
    }
}

/// A finding kept out of the result by an `ignore` entry in `meshdiet.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ignored {
    /// The finding.
    pub finding: Finding,
    /// The entry's reason; `None` when it has none (also reported as CFG-IGN).
    pub reason: Option<String>,
}

/// Counts per severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// High findings.
    pub high: usize,
    /// Medium findings.
    pub medium: usize,
    /// Low findings.
    pub low: usize,
}

impl Summary {
    /// Count `findings` by severity.
    #[must_use]
    pub fn of(findings: &[Finding]) -> Self {
        let mut summary = Self::default();
        for f in findings {
            match f.severity {
                Severity::High => summary.high += 1,
                Severity::Medium => summary.medium += 1,
                Severity::Low => summary.low += 1,
            }
        }
        summary
    }
}

/// Everything a scan produced.
#[derive(Debug, Serialize)]
pub struct Report {
    /// JSON shape version.
    pub schema: u32,
    /// Engine version.
    pub version: &'static str,
    /// The folder paths are relative to.
    pub root: String,
    /// Wall time of the scan in milliseconds.
    pub duration_ms: u128,
    /// Every asset, sorted by path.
    pub assets: Vec<Asset>,
    /// Totals, shared files once.
    pub totals: Totals,
    /// Page groups from `[pages]`, then `(no page)`; empty without pages.
    pub pages: Vec<PageTotal>,
    /// gzip level 6 size of every file (`--gzip`), keyed by path. An estimate: servers may use
    /// brotli or nothing.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub gzip_estimate: BTreeMap<String, u64>,
    /// Findings, most severe first.
    pub findings: Vec<Finding>,
    /// Counts per severity.
    pub summary: Summary,
    /// Rules that crashed (the scan carried on without them).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub internal_errors: Vec<String>,
    /// Findings kept out by `ignore` entries in `meshdiet.toml`, with their reasons.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignored: Vec<Ignored>,
    /// How this scan compares with `--baseline`, when one was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineDiff>,
}

/// When a scan should fail the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOn {
    /// Any finding at or above this severity fails.
    AtLeast(Severity),
    /// Never fail.
    Never,
}

impl Report {
    /// Most severe first, then by path and rule.
    pub fn sort_findings(&mut self) {
        self.findings.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.rule.cmp(&b.rule))
                .then_with(|| b.measured.value.cmp(&a.measured.value))
        });
    }

    /// The scan's wall time.
    #[must_use]
    pub fn duration(&self) -> Duration {
        Duration::from_millis(u64::try_from(self.duration_ms).unwrap_or(u64::MAX))
    }

    /// Whether the findings should fail the build. Medium-confidence findings count only when
    /// `strict` (SPEC-CLI-001 FR-004).
    #[must_use]
    pub fn fails(&self, fail_on: FailOn, strict: bool) -> bool {
        let FailOn::AtLeast(min) = fail_on else {
            return false;
        };
        self.findings
            .iter()
            .any(|f| f.severity >= min && (strict || f.confidence == Confidence::High))
    }
}

fn unreadable(asset: &Asset, problem: &str) -> Finding {
    Finding {
        rule: GLTF_001.id.to_owned(),
        title: GLTF_001.title.to_owned(),
        severity: GLTF_001.severity,
        confidence: GLTF_001.confidence,
        path: asset.path.clone(),
        measured: Measured::exact("file size", asset.bytes, "bytes"),
        threshold: None,
        message: problem.to_owned(),
        why: "Loaders stop at the first broken part, so this model, or a part of it, never shows up on the page.".to_owned(),
        fix: "Export the model again (one .glb cannot lose its files), and check it with the Khronos glTF Validator (https://github.khronos.org/glTF-Validator/) for the exact error.".to_owned(),
        fingerprint: String::new(),
    }
}

struct Weighed {
    totals: Totals,
    pages: weight::Pages,
    gzip: BTreeMap<String, u64>,
}

/// Totals, page groups and, with `--gzip`, the transfer estimate.
fn weigh(assets: &[Asset], root: &std::path::Path, root_shown: &str, options: &Options) -> Weighed {
    let mut totals = weight::totals(assets, &options.config);
    let mut pages = weight::pages(assets, &options.config.pages, root_shown);
    let gzip = if options.gzip {
        weight::gzip_files(assets, root)
    } else {
        BTreeMap::new()
    };
    if options.gzip {
        totals.gzip = Some(weight::gzip_of(assets, &gzip));
        for page in &mut pages.totals {
            let members = assets.iter().filter(|a| page.assets.contains(&a.path));
            page.gzip = Some(weight::gzip_of(members, &gzip));
        }
    }
    Weighed {
        totals,
        pages,
        gzip,
    }
}

/// Walk `paths`, read every asset, run every rule, and sort the findings.
///
/// # Errors
/// When a path does not exist or is not a model file.
pub fn scan(paths: &[PathBuf], options: &Options) -> Result<Report, String> {
    scan_with(paths, &rule::all(), options)
}

/// [`scan`] with a chosen rule list.
///
/// # Errors
/// When a path does not exist or is not a model file.
pub fn scan_with(
    paths: &[PathBuf],
    rules: &[Box<dyn Rule>],
    options: &Options,
) -> Result<Report, String> {
    let started = Instant::now();
    options.validate()?;
    let walked = walk::walk(paths)?;
    let root_shown = walked.root.to_string_lossy().replace('\\', "/");
    let skipped = Ignores::new(&options.config, &root_shown);
    let mut assets = Vec::with_capacity(walked.files.len());
    let mut findings = Vec::new();
    let mut internal_errors = Vec::new();
    for file in walked.files.iter().filter(|f| !skipped.skips_file(&f.rel)) {
        if let Some(progress) = options.progress {
            progress(&file.rel);
        }
        let asset = asset::read(&file.rel, &file.abs);
        findings.extend(asset.problems.iter().map(|p| unreadable(&asset, p)));
        if asset.document.is_some() {
            for rule in rules.iter().filter(|r| options.runs(r.id())) {
                match catch_unwind(AssertUnwindSafe(|| rule.check(&asset, &options.config))) {
                    Ok(found) => findings.extend(found),
                    Err(_) => internal_errors.push(format!(
                        "internal error in {} on {}",
                        rule.id(),
                        asset.path
                    )),
                }
            }
        }
        assets.push(asset);
    }
    let weighed = weigh(&assets, &walked.root, &root_shown, options);
    let config_path = options.config.source.as_deref().map_or_else(
        || "meshdiet.toml".to_owned(),
        |p| walk::relative(&walked.root, p),
    );
    findings.extend(size::build_findings(
        &assets,
        &weighed.totals,
        &weighed.pages,
        &options.config,
        &config_path,
    ));
    if options.runs(load::LOAD_001.id) && assets.iter().any(load::needs_decoder) {
        let src: &Path = options.src.as_deref().unwrap_or(&walked.root);
        findings.extend(load::check(&assets, &SourceIndex::build(src)));
    }
    findings.extend(no_reason(&options.config, &config_path));
    findings.retain(|f| options.runs(&f.rule));
    for f in &mut findings {
        f.fingerprint = fingerprint(f);
    }
    let (findings, ignored) = skipped.split(findings);
    let mut report = Report {
        schema: SCHEMA,
        version: crate::VERSION,
        root: root_shown,
        duration_ms: started.elapsed().as_millis(),
        totals: weighed.totals,
        pages: weighed.pages.totals,
        gzip_estimate: weighed.gzip,
        assets,
        summary: Summary::of(&findings),
        findings,
        internal_errors,
        ignored,
        baseline: None,
    };
    report.sort_findings();
    Ok(report)
}

/// CFG-IGN (VAL-002): one low finding per `ignore` entry with no reason. The entry still applies.
fn no_reason(config: &Config, config_path: &str) -> Vec<Finding> {
    config
        .ignores
        .iter()
        .enumerate()
        .filter(|(_, i)| i.reason.is_none())
        .map(|(n, i)| Finding {
            rule: CFG_IGN.id.to_owned(),
            title: CFG_IGN.title.to_owned(),
            severity: CFG_IGN.severity,
            confidence: CFG_IGN.confidence,
            path: config_path.to_owned(),
            measured: Measured::exact("ignore entry", n as u64, "index"),
            threshold: None,
            message: format!(
                "ignore[{n}] silences {} on `{}` without a reason",
                i.rule, i.path
            ),
            why: "An ignore outlives the reason it was added for; without the reason written down, nobody can tell later whether it still holds.".to_owned(),
            fix: format!(
                "Add reason = \"...\" to the entry, for example {{ rule = \"{}\", path = \"{}\", reason = \"hero model, shown full screen\" }}.",
                i.rule, i.path
            ),
            fingerprint: String::new(),
        })
        .collect()
}

/// The config's `ignore` entries, compiled once per scan.
struct Ignores<'a> {
    root: &'a str,
    files: Vec<globset::GlobMatcher>,
    findings: Vec<(&'a Ignore, globset::GlobMatcher)>,
}

impl<'a> Ignores<'a> {
    fn new(config: &'a Config, root: &'a str) -> Self {
        // Globs were checked when the config was read; one that fails here is dropped.
        Self {
            root,
            files: config
                .ignore_paths
                .iter()
                .filter_map(|g| weight::glob(g).ok())
                .collect(),
            findings: config
                .ignores
                .iter()
                .filter_map(|i| weight::glob(&i.path).ok().map(|m| (i, m)))
                .collect(),
        }
    }

    fn matches(&self, m: &globset::GlobMatcher, path: &str) -> bool {
        weight::spellings(self.root, path)
            .iter()
            .any(|p| m.is_match(p))
    }

    /// A plain glob in `ignore` names this file: it is not scanned at all.
    fn skips_file(&self, path: &str) -> bool {
        self.files.iter().any(|m| self.matches(m, path))
    }

    /// Move findings that an `ignore` entry covers out of the result.
    fn split(&self, findings: Vec<Finding>) -> (Vec<Finding>, Vec<Ignored>) {
        let mut kept = Vec::with_capacity(findings.len());
        let mut ignored = Vec::new();
        for f in findings {
            match self
                .findings
                .iter()
                .find(|(i, m)| i.rule == f.rule && self.matches(m, &f.path))
            {
                Some((i, _)) => ignored.push(Ignored {
                    finding: f,
                    reason: i.reason.clone(),
                }),
                None => kept.push(f),
            }
        }
        (kept, ignored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(severity: Severity, confidence: Confidence) -> Report {
        Report {
            schema: SCHEMA,
            version: "test",
            root: ".".into(),
            duration_ms: 0,
            assets: Vec::new(),
            totals: Totals::default(),
            pages: Vec::new(),
            gzip_estimate: BTreeMap::new(),
            findings: vec![Finding {
                rule: "TEST-001".into(),
                title: "t".into(),
                severity,
                confidence,
                path: "a.glb".into(),
                measured: Measured::exact("n", 1, "bytes"),
                threshold: None,
                message: "m".into(),
                why: "w".into(),
                fix: "f".into(),
                fingerprint: String::new(),
            }],
            summary: Summary::default(),
            internal_errors: Vec::new(),
            ignored: Vec::new(),
            baseline: None,
        }
    }

    #[test]
    fn fail_on_compares_severity() {
        let r = report(Severity::Medium, Confidence::High);
        assert!(!r.fails(FailOn::AtLeast(Severity::High), false));
        assert!(r.fails(FailOn::AtLeast(Severity::Medium), false));
        assert!(r.fails(FailOn::AtLeast(Severity::Low), false));
        assert!(!r.fails(FailOn::Never, true));
    }

    #[test]
    fn medium_confidence_fails_only_with_strict() {
        let r = report(Severity::High, Confidence::Medium);
        assert!(!r.fails(FailOn::AtLeast(Severity::High), false));
        assert!(r.fails(FailOn::AtLeast(Severity::High), true));
    }
}
