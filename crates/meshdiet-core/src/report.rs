//! A whole scan: walk, read every asset, run the rules, and decide whether it fails a build.
//! Exit-code logic first written for vibeguard (ADR-011).

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::rule::{self, GLTF_001, Rule};
use crate::rules::size;
use crate::weight::{self, PageTotal, Totals};
use crate::{Asset, Confidence, Config, Finding, Measured, Severity, asset, walk};

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
    let walked = walk::walk(paths)?;
    let mut assets = Vec::with_capacity(walked.files.len());
    let mut findings = Vec::new();
    let mut internal_errors = Vec::new();
    for file in &walked.files {
        if let Some(progress) = options.progress {
            progress(&file.rel);
        }
        let asset = asset::read(&file.rel, &file.abs);
        findings.extend(asset.problems.iter().map(|p| unreadable(&asset, p)));
        if asset.document.is_some() {
            for rule in rules {
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
    let root_shown = walked.root.to_string_lossy().replace('\\', "/");
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
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.rule.cmp(&b.rule))
    });
    let mut summary = Summary::default();
    for f in &findings {
        match f.severity {
            Severity::High => summary.high += 1,
            Severity::Medium => summary.medium += 1,
            Severity::Low => summary.low += 1,
        }
    }
    Ok(Report {
        schema: SCHEMA,
        version: crate::VERSION,
        root: root_shown,
        duration_ms: started.elapsed().as_millis(),
        totals: weighed.totals,
        pages: weighed.pages.totals,
        gzip_estimate: weighed.gzip,
        assets,
        findings,
        summary,
        internal_errors,
    })
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
            }],
            summary: Summary::default(),
            internal_errors: Vec::new(),
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
