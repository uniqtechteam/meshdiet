//! What a rule reports. Shape from SPEC-RULE-001 §8, first written for vibeguard (ADR-011).

use serde::{Deserialize, Serialize};

/// How bad a finding is. Ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth doing when there is time.
    Low,
    /// Costs users load time or memory; fix soon.
    Medium,
    /// Breaks the page or a budget; fix before shipping.
    High,
}

impl Severity {
    /// Lower-case name, as in the JSON output.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// How sure the rule is. Medium-confidence findings fail a build only with `--strict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// A heuristic that can be wrong; check before acting.
    Medium,
    /// Measured from the file itself.
    High,
}

/// A number a finding is about. Every finding carries one (SPEC-RULE-001 BR-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measured {
    /// What was measured, for example `geometry bytes`.
    pub name: String,
    /// The value, in `unit`.
    pub value: u64,
    /// `bytes`, `pixels`, `triangles`, …
    pub unit: String,
    /// True when the number is an estimate (GPU memory, for example), never read from the file.
    pub estimated: bool,
}

impl Measured {
    /// An exact measurement.
    #[must_use]
    pub fn exact(name: &str, value: u64, unit: &str) -> Self {
        Self {
            name: name.to_owned(),
            value,
            unit: unit.to_owned(),
            estimated: false,
        }
    }
}

/// One problem in one asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Rule id, for example `MESH-001`.
    pub rule: String,
    /// Short title of the rule.
    pub title: String,
    /// How bad it is.
    pub severity: Severity,
    /// How sure the rule is.
    pub confidence: Confidence,
    /// Asset path relative to the scan root, always with forward slashes.
    pub path: String,
    /// The number this finding is about.
    pub measured: Measured,
    /// The limit it was compared with, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<Measured>,
    /// What is wrong, with the numbers.
    pub message: String,
    /// One sentence: why it costs the user.
    pub why: String,
    /// One sentence: how to fix it.
    pub fix: String,
}
