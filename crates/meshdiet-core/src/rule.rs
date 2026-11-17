//! The rule interface and the list of rules the engine runs.

use serde::Serialize;

use crate::{Asset, Confidence, Config, Finding, Severity};

/// What a rule is, for `meshdiet rules`, `meshdiet explain` and SARIF.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RuleInfo {
    /// Stable id, for example `MESH-001`.
    pub id: &'static str,
    /// Title used on each finding.
    pub title: &'static str,
    /// Default severity.
    pub severity: Severity,
    /// Confidence of its findings.
    pub confidence: Confidence,
    /// One line for the rule list.
    pub summary: &'static str,
}

/// A single check over one asset. Rules are stateless and never read pixels or decode meshes.
pub trait Rule: Send + Sync {
    /// What this rule is.
    fn info(&self) -> &'static RuleInfo;
    /// Look at one readable asset and report findings.
    fn check(&self, asset: &Asset, config: &Config) -> Vec<Finding>;
    /// Stable id, for example `MESH-001`.
    fn id(&self) -> &'static str {
        self.info().id
    }
}

/// GLTF-001 comes from the reader itself, not from a [`Rule`]: a file that cannot be read has
/// nothing for the rules to look at.
pub const GLTF_001: RuleInfo = RuleInfo {
    id: "GLTF-001",
    title: "Cannot read the asset",
    severity: Severity::High,
    confidence: Confidence::High,
    summary: "broken GLB container, invalid JSON, a missing referenced file or a bufferView past its buffer",
};

/// Every rule, in the order they run. The asset rules arrive with TASK-RULE-001…003.
#[must_use]
pub fn all() -> Vec<Box<dyn Rule>> {
    Vec::new()
}

/// Info for every rule the engine reports, in id order.
#[must_use]
pub fn catalogue() -> Vec<&'static RuleInfo> {
    let mut list: Vec<&'static RuleInfo> = all().iter().map(|r| r.info()).collect();
    list.push(&GLTF_001);
    list.sort_by_key(|i| i.id);
    list
}

/// Info for one rule id (case-insensitive).
#[must_use]
pub fn find(id: &str) -> Option<&'static RuleInfo> {
    catalogue()
        .into_iter()
        .find(|i| i.id.eq_ignore_ascii_case(id))
}
