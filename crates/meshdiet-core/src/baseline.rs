//! Finding fingerprints and the ratchet baseline (SPEC-CLI-001 FR-007, ADR-009).
//!
//! `--write-baseline` records every finding and every asset size. `--baseline` then hides what was
//! already there and reports only what is new or got worse by more than the tolerance, so a site
//! that is already heavy can turn the gate on today and stop the growth first. Fingerprints first
//! written for vibeguard (ADR-011).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rule::RuleInfo;
use crate::weight::human;
use crate::{Confidence, Finding, Measured, Report, Severity, Summary};

/// Version of the baseline file shape.
pub const BASELINE_SCHEMA: u32 = 1;

/// Default `--tolerance`: 1%, in hundredths of a percent.
pub const DEFAULT_TOLERANCE: u32 = 100;

/// BASE-GROW: a file, or the build, grew past the baseline. Only reported with `--baseline`.
pub const BASE_GROW: RuleInfo = RuleInfo {
    id: "BASE-GROW",
    title: "Grew since the baseline",
    severity: Severity::High,
    confidence: Confidence::High,
    summary: "with --baseline: an asset, or the whole build, is heavier than the baseline by more than --tolerance (1% by default)",
};

/// FNV-1a, 64 bit, as hex. Stable across Rust versions and platforms, unlike `DefaultHasher`.
fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A path written the same way on every system: forward slashes, no leading `./`.
fn normalise(path: &str) -> String {
    let p = path.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_owned()
}

/// The size class of a value: its number of binary digits, so 70 KB and 90 KB fall in the same
/// class and a few bytes of change keep the same fingerprint.
fn bucket(value: u64) -> u32 {
    u64::BITS - value.leading_zeros()
}

/// A finding's stable id: rule + normalised path + what was measured + the value's size class.
#[must_use]
pub fn fingerprint(f: &Finding) -> String {
    fnv(&format!(
        "{}|{}|{}|{}",
        f.rule,
        normalise(&f.path),
        f.measured.name,
        bucket(f.measured.value)
    ))
}

/// One finding as the baseline remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// [`fingerprint`] when the baseline was written.
    pub fingerprint: String,
    /// Rule id.
    pub rule: String,
    /// Asset path, relative to the scan root.
    pub path: String,
    /// What was measured (`geometry bytes`, `longest side`, …).
    pub measure: String,
    /// The measured value.
    pub value: u64,
}

impl Entry {
    fn key(&self) -> (&str, &str, &str) {
        (&self.rule, &self.path, &self.measure)
    }
}

/// What `--write-baseline` stores: every finding, every asset size and the total. JSON, sorted, so
/// a change to it reads well in review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    /// [`BASELINE_SCHEMA`].
    pub schema: u32,
    /// meshdiet version that wrote it.
    pub meshdiet: String,
    /// Bytes of every asset together, shared files once.
    pub total: u64,
    /// Bytes of each asset (with its `.bin` and images), by path.
    pub files: BTreeMap<String, u64>,
    /// Every finding, sorted by rule, path and measure.
    pub findings: Vec<Entry>,
}

/// A file that shrank or disappeared since the baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// Asset path.
    pub path: String,
    /// Bytes in the baseline.
    pub before: u64,
    /// Bytes now; 0 when the file is gone.
    pub after: u64,
}

/// How the report compares with the baseline. Removed and smaller files are the improvements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BaselineDiff {
    /// The baseline file, as given.
    pub file: String,
    /// `--tolerance` in hundredths of a percent (100 = 1%).
    pub tolerance: u32,
    /// Findings hidden because the baseline already had them.
    pub known: usize,
    /// Assets in the baseline that are gone.
    pub removed: Vec<Change>,
    /// Assets more than the tolerance smaller than in the baseline.
    pub shrunk: Vec<Change>,
    /// The build total in the baseline.
    pub total_before: u64,
}

/// `1%`, `0.5%`, `2`, `0`: a percentage with at most two decimals, as hundredths of a percent.
///
/// # Errors
/// Anything else, or more than 1000%.
pub fn parse_tolerance(text: &str) -> Result<u32, String> {
    let err = || format!("--tolerance {text}: write a percentage such as 1% or 0.5%");
    let t = text.trim();
    let t = t.strip_suffix('%').unwrap_or(t).trim();
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    if whole.is_empty() || frac.len() > 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(err());
    }
    let whole: u32 = whole.parse().map_err(|_| err())?;
    let frac: u32 = format!("{frac:0<2}").parse().map_err(|_| err())?;
    let value = whole
        .checked_mul(100)
        .and_then(|w| w.checked_add(frac))
        .filter(|v| *v <= 100_000)
        .ok_or_else(err)?;
    Ok(value)
}

/// `1%`, `0.5%`, `12.25%`.
#[must_use]
pub fn tolerance_label(tolerance: u32) -> String {
    let (whole, frac) = (tolerance / 100, tolerance % 100);
    match frac {
        0 => format!("{whole}%"),
        f if f % 10 == 0 => format!("{whole}.{}%", f / 10),
        f => format!("{whole}.{f:02}%"),
    }
}

/// `now` is more than `tolerance` above `before`.
fn grew(before: u64, now: u64, tolerance: u32) -> bool {
    u128::from(now) * 10_000 > u128::from(before) * (10_000 + u128::from(tolerance))
}

/// `now` is more than `tolerance` below `before`.
fn shrank(before: u64, now: u64, tolerance: u32) -> bool {
    u128::from(now) * 10_000 < u128::from(before) * (10_000_u128.saturating_sub(tolerance.into()))
}

/// The largest value still inside the tolerance.
fn allowed(before: u64, tolerance: u32) -> u64 {
    let v = u128::from(before) * (10_000 + u128::from(tolerance)) / 10_000;
    u64::try_from(v).unwrap_or(u64::MAX)
}

/// `+9.6%`.
fn growth(before: u64, now: u64) -> String {
    if before == 0 {
        return "new".to_owned();
    }
    let tenths = (u128::from(now.saturating_sub(before)) * 1000 + u128::from(before) / 2)
        / u128::from(before);
    format!("+{}.{}%", tenths / 10, tenths % 10)
}

fn grow_finding(path: &str, before: u64, now: u64, tolerance: u32, message: String) -> Finding {
    let mut f = Finding {
        rule: BASE_GROW.id.to_owned(),
        title: BASE_GROW.title.to_owned(),
        severity: BASE_GROW.severity,
        confidence: BASE_GROW.confidence,
        path: path.to_owned(),
        measured: Measured::exact("bytes", now, "bytes"),
        threshold: Some(Measured::exact(
            &format!("baseline + {}", tolerance_label(tolerance)),
            allowed(before, tolerance),
            "bytes",
        )),
        message,
        why: "The baseline is the weight the team accepted; every byte past it is new weight users download, and it only goes one way unless something stops it.".to_owned(),
        fix: "Compress or shrink what grew (`meshdiet fix`), or, if the growth is intended, write the baseline again with `--write-baseline` and commit it so the review shows the new weight.".to_owned(),
        fingerprint: String::new(),
    };
    f.fingerprint = fingerprint(&f);
    f
}

impl Baseline {
    /// Everything the report holds now.
    #[must_use]
    pub fn of(report: &Report) -> Self {
        let mut findings: Vec<Entry> = report
            .findings
            .iter()
            .filter(|f| f.rule != BASE_GROW.id)
            .map(|f| Entry {
                fingerprint: fingerprint(f),
                rule: f.rule.clone(),
                path: normalise(&f.path),
                measure: f.measured.name.clone(),
                value: f.measured.value,
            })
            .collect();
        findings.sort_by(|a, b| a.key().cmp(&b.key()).then(a.value.cmp(&b.value)));
        Self {
            schema: BASELINE_SCHEMA,
            meshdiet: crate::VERSION.to_owned(),
            total: report.totals.bytes,
            files: report
                .assets
                .iter()
                .map(|a| (normalise(&a.path), a.bytes))
                .collect(),
            findings,
        }
    }

    /// The baseline as pretty JSON with a trailing newline.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Read a baseline file.
    ///
    /// # Errors
    /// When the file cannot be read or is not a meshdiet baseline.
    pub fn load(file: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read baseline {}: {e}", file.display()))?;
        let b: Self = serde_json::from_str(&text).map_err(|e| {
            format!(
                "{}: not a meshdiet baseline ({e}); write one with --write-baseline",
                file.display()
            )
        })?;
        if b.schema != BASELINE_SCHEMA {
            return Err(format!(
                "{}: baseline schema {} is not supported (this meshdiet writes {BASELINE_SCHEMA})",
                file.display(),
                b.schema
            ));
        }
        Ok(b)
    }

    /// Keep only what is new or grew by more than `tolerance` (hundredths of a percent), add a
    /// BASE-GROW finding for each asset that grew and for unexplained growth of the build, and
    /// record the improvements. `shown` is how the baseline file is named in the output.
    pub fn apply(&self, report: &mut Report, tolerance: u32, shown: &str) {
        let known = self.hide_known(&mut report.findings, tolerance);
        let mut diff = BaselineDiff {
            file: shown.to_owned(),
            tolerance,
            known,
            removed: Vec::new(),
            shrunk: Vec::new(),
            total_before: self.total,
        };
        let explained = self.compare_files(report, &mut diff, tolerance);
        self.compare_build(report, explained, tolerance, shown);
        report.sort_findings();
        report.summary = Summary::of(&report.findings);
        report.baseline = Some(diff);
    }

    /// Drop findings the baseline already had; returns how many.
    fn hide_known(&self, findings: &mut Vec<Finding>, tolerance: u32) -> usize {
        // Known findings: same rule, path and measure, and a value inside the tolerance. Several
        // findings may share a key (one per image), so match largest to largest.
        let key =
            |rule: &str, path: &str, measure: &str| format!("{rule}|{}|{measure}", normalise(path));
        let mut pool: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        for e in &self.findings {
            pool.entry(key(&e.rule, &e.path, &e.measure))
                .or_default()
                .push(e.value);
        }
        for values in pool.values_mut() {
            values.sort_unstable();
        }
        let mut order: Vec<usize> = (0..findings.len()).collect();
        order.sort_by(|a, b| {
            findings[*b]
                .measured
                .value
                .cmp(&findings[*a].measured.value)
        });
        let mut known = BTreeSet::new();
        for i in order {
            let f = &findings[i];
            if let Some(values) = pool.get_mut(&key(&f.rule, &f.path, &f.measured.name))
                && let Some(&best) = values.last()
                && !grew(best, f.measured.value, tolerance)
            {
                values.pop();
                known.insert(i);
            }
        }
        let mut kept = Vec::with_capacity(findings.len() - known.len());
        for (i, f) in std::mem::take(findings).into_iter().enumerate() {
            if !known.contains(&i) {
                kept.push(f);
            }
        }
        *findings = kept;
        known.len()
    }

    /// BASE-GROW for each asset that grew; record removed and smaller ones. Returns the bytes the
    /// grown assets added.
    fn compare_files(&self, report: &mut Report, diff: &mut BaselineDiff, tolerance: u32) -> u64 {
        let current: BTreeMap<String, u64> = report
            .assets
            .iter()
            .map(|a| (normalise(&a.path), a.bytes))
            .collect();
        let mut explained: u64 = 0;
        for a in &report.assets {
            let Some(&before) = self.files.get(&normalise(&a.path)) else {
                continue;
            };
            if grew(before, a.bytes, tolerance) {
                explained += a.bytes - before;
                let message = format!(
                    "grew from {} to {} ({}) since the baseline, more than the {} tolerance",
                    human(before),
                    human(a.bytes),
                    growth(before, a.bytes),
                    tolerance_label(tolerance)
                );
                report
                    .findings
                    .push(grow_finding(&a.path, before, a.bytes, tolerance, message));
            } else if shrank(before, a.bytes, tolerance) {
                diff.shrunk.push(Change {
                    path: a.path.clone(),
                    before,
                    after: a.bytes,
                });
            }
        }
        for (path, &before) in &self.files {
            if !current.contains_key(path) {
                diff.removed.push(Change {
                    path: path.clone(),
                    before,
                    after: 0,
                });
            }
        }
        explained
    }

    /// BASE-GROW for the build when it grew by more than the grown assets explain: new files.
    fn compare_build(&self, report: &mut Report, explained: u64, tolerance: u32, shown: &str) {
        let total = report.totals.bytes;
        let unexplained = total.saturating_sub(self.total).saturating_sub(explained);
        let unexplained_grew = unexplained > 0
            && u128::from(unexplained) * 10_000 > u128::from(self.total) * u128::from(tolerance);
        if grew(self.total, total, tolerance) && unexplained_grew {
            let new: Vec<&str> = report
                .assets
                .iter()
                .filter(|a| !self.files.contains_key(&normalise(&a.path)))
                .map(|a| a.path.as_str())
                .collect();
            let mut message = format!(
                "the build grew from {} to {} ({}) since the baseline, more than the {} tolerance",
                human(self.total),
                human(total),
                growth(self.total, total),
                tolerance_label(tolerance)
            );
            if !new.is_empty() {
                let more = new.len().saturating_sub(5);
                let _ = write!(message, "; new: {}", new[..new.len().min(5)].join(", "));
                if more > 0 {
                    let _ = write!(message, " and {more} more");
                }
            }
            report
                .findings
                .push(grow_finding(shown, self.total, total, tolerance, message));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test that cannot parse should stop")]
mod tests {
    use super::*;

    #[test]
    fn tolerance_is_a_percentage() {
        assert_eq!(parse_tolerance("1%"), Ok(100));
        assert_eq!(parse_tolerance("0.5%"), Ok(50));
        assert_eq!(parse_tolerance("2"), Ok(200));
        assert_eq!(parse_tolerance("0"), Ok(0));
        assert_eq!(parse_tolerance("12.25 %"), Ok(1225));
        assert!(parse_tolerance("-1%").is_err());
        assert!(parse_tolerance("1.234%").is_err());
        assert!(parse_tolerance("lots").is_err());
        assert!(parse_tolerance("5000%").is_err());
        assert_eq!(tolerance_label(100), "1%");
        assert_eq!(tolerance_label(50), "0.5%");
        assert_eq!(tolerance_label(1225), "12.25%");
    }

    #[test]
    fn growth_is_compared_without_rounding() {
        assert!(!grew(1_000_000, 1_010_000, 100));
        assert!(grew(1_000_000, 1_010_001, 100));
        assert!(!grew(1_000_000, 1_000_000, 0));
        assert!(grew(0, 1, 100));
        assert!(shrank(1_000_000, 989_999, 100));
        assert!(!shrank(1_000_000, 990_000, 100));
        assert_eq!(growth(1_000_000, 1_096_000), "+9.6%");
    }

    #[test]
    fn fingerprint_keeps_small_changes_and_normalises_paths() {
        let f = |path: &str, value: u64| Finding {
            rule: "MESH-001".into(),
            title: String::new(),
            severity: Severity::High,
            confidence: Confidence::High,
            path: path.into(),
            measured: Measured::exact("geometry bytes", value, "bytes"),
            threshold: None,
            message: String::new(),
            why: String::new(),
            fix: String::new(),
            fingerprint: String::new(),
        };
        let a = fingerprint(&f("public/a.glb", 70_000));
        assert_eq!(a.len(), 16);
        assert_eq!(a, fingerprint(&f("public\\a.glb", 90_000)));
        assert_eq!(a, fingerprint(&f("./public/a.glb", 70_001)));
        assert_ne!(a, fingerprint(&f("public/a.glb", 700_000)));
        assert_ne!(a, fingerprint(&f("public/b.glb", 70_000)));
    }
}
