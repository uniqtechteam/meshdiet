//! SARIF 2.1.0 output for GitHub code scanning (SPEC-CLI-001 §8). Writer first written for
//! vibeguard (ADR-011).
//!
//! Findings are about whole files, so a result has an artifact location but no region.

use std::fmt::Write as _;

use serde_json::{Value, json};

use meshdiet_core::rule::catalogue;
use meshdiet_core::{Confidence, Finding, Report, Severity};

use crate::explain;

fn level(s: Severity) -> &'static str {
    match s {
        Severity::High => "error",
        Severity::Medium => "warning",
        Severity::Low => "note",
    }
}

fn precision(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high",
        Confidence::Medium => "medium",
    }
}

/// Percent-encode what a URI reference cannot hold as is (spaces, `#`, `%`, non-ASCII).
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The path GitHub needs: relative to where meshdiet ran (the repository root in CI). Report paths
/// are relative to the scan root, so a relative root such as `public` goes in front.
fn uri(report: &Report, path: &str) -> String {
    let root = report.root.trim_end_matches('/');
    let relative_root = !(root.is_empty()
        || root == "."
        || root.starts_with('/')
        || root.as_bytes().get(1) == Some(&b':'));
    let root = root.strip_prefix("./").unwrap_or(root);
    if relative_root {
        encode(&format!("{root}/{path}"))
    } else {
        encode(path)
    }
}

fn result(report: &Report, f: &Finding, index: Option<usize>) -> Value {
    let mut properties = json!({
        "confidence": precision(f.confidence),
        "measured": f.measured,
        "estimated": f.measured.estimated,
    });
    if let Some(t) = &f.threshold {
        properties["threshold"] = json!(t);
    }
    let mut r = json!({
        "ruleId": f.rule,
        "level": level(f.severity),
        "message": { "text": format!("{}. {} {}", f.message, f.why, f.fix) },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": uri(report, &f.path), "uriBaseId": "%SRCROOT%" },
            }
        }],
        "partialFingerprints": { "meshdiet/v1": f.fingerprint },
        "properties": properties,
    });
    if let Some(i) = index {
        r["ruleIndex"] = json!(i);
    }
    r
}

/// The report as a SARIF 2.1.0 log: one run, every rule, one result per finding.
pub fn render(report: &Report) -> String {
    let list = catalogue();
    let rules: Vec<Value> = list
        .iter()
        .map(|i| {
            let help = explain::text(i.id).map_or_else(
                || i.summary.to_owned(),
                |t| {
                    format!(
                        "{}\n\nHow to fix it: {}\n\nLoader change: {}",
                        t.why, t.fix, t.loader
                    )
                },
            );
            json!({
                "id": i.id,
                "name": i.title.replace(' ', ""),
                "shortDescription": { "text": i.title },
                "fullDescription": { "text": i.summary },
                "help": { "text": help },
                "defaultConfiguration": { "level": level(i.severity) },
                "properties": {
                    "tags": ["performance", "3d"],
                    "precision": precision(i.confidence),
                },
            })
        })
        .collect();
    let results: Vec<Value> = report
        .findings
        .iter()
        .map(|f| result(report, f, list.iter().position(|i| i.id == f.rule)))
        .collect();
    let log = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "meshdiet",
                    "version": report.version,
                    "informationUri": "https://github.com/uniqtechteam/meshdiet",
                    "rules": rules,
                }
            },
            "results": results,
        }],
    });
    let mut text = serde_json::to_string_pretty(&log).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_are_encoded_and_relative_to_the_working_folder() {
        assert_eq!(encode("public/my model#1.glb"), "public/my%20model%231.glb");
        assert_eq!(encode("modèle.glb"), "mod%C3%A8le.glb");
    }
}
