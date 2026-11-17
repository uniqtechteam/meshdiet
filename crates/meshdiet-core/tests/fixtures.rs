//! Walks every fixture folder under `tests/fixtures/<RULE-ID>/{bad,good}`.
//!
//! `bad/` must produce exactly the findings listed in `bad/expected.json`; `good/` must produce none
//! of that rule. A new rule adds folders, not test code. Rules the engine does not have yet are
//! checked for shape only (the folder is complete and `expected.json` is valid). A folder with a
//! `meshdiet.toml` is scanned with it, for the budget rules.

#![allow(
    clippy::panic,
    reason = "a fixture with a missing or broken expected.json should fail the test loudly"
)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
struct Expected {
    rule: String,
    path: String,
    /// The measured value, when the fixture pins one.
    #[serde(default)]
    value: Option<f64>,
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Rule folders: named like a rule id (`MESH-001`), not `demo-site`.
fn rule_dirs() -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(fixtures_root()) else {
        panic!("tests/fixtures is missing; run `cargo xtask gen-fixtures`");
    };
    let mut dirs: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_owned();
            is_rule_id(&name).then_some((name, p))
        })
        .collect();
    dirs.sort();
    dirs
}

fn is_rule_id(name: &str) -> bool {
    name.split_once('-').is_some_and(|(group, number)| {
        !group.is_empty()
            && group.chars().all(|c| c.is_ascii_uppercase())
            && number.len() == 3
            && number.chars().all(|c| c.is_ascii_digit())
    })
}

/// The rules the engine runs today; other rule folders are checked for shape only.
fn engine_rules() -> Vec<&'static str> {
    meshdiet_core::rule::catalogue()
        .iter()
        .map(|i| i.id)
        .collect()
}

/// Options for `dir`: its `meshdiet.toml` when it has one (budget rules), else the defaults.
fn options(dir: &Path) -> meshdiet_core::Options {
    let file = dir.join("meshdiet.toml");
    let config = if file.is_file() {
        meshdiet_core::Config::load(&file).unwrap_or_else(|e| panic!("{e}"))
    } else {
        meshdiet_core::Config::default()
    };
    meshdiet_core::Options {
        config,
        ..meshdiet_core::Options::default()
    }
}

/// Findings the engine reports for `dir`, as (rule, path, value).
fn engine_findings(dir: &Path) -> Vec<Expected> {
    meshdiet_core::scan(&[dir.to_path_buf()], &options(dir))
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .findings
        .into_iter()
        .map(|f| Expected {
            rule: f.rule,
            path: f.path,
            #[allow(
                clippy::cast_precision_loss,
                reason = "fixture values are far below 2^52"
            )]
            value: Some(f.measured.value as f64),
        })
        .collect()
}

fn read_expected(bad: &Path) -> Vec<Expected> {
    let raw = fs::read_to_string(bad.join("expected.json"))
        .unwrap_or_else(|_| panic!("{} needs an expected.json", bad.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{}: bad expected.json: {e}", bad.display()))
}

#[test]
fn fixtures_harness_walks_every_folder() {
    let dirs = rule_dirs();
    assert!(!dirs.is_empty(), "no rule fixture folders found");
    let rules = engine_rules();
    for (rule, dir) in &dirs {
        let (bad, good) = (dir.join("bad"), dir.join("good"));
        assert!(bad.is_dir(), "{} has no bad/ folder", dir.display());
        assert!(good.is_dir(), "{} has no good/ folder", dir.display());
        let mut expected = read_expected(&bad);
        assert!(
            !expected.is_empty(),
            "{}: expected.json lists nothing",
            bad.display()
        );
        for e in &expected {
            assert_eq!(&e.rule, rule, "{}: finding for another rule", bad.display());
            assert!(
                bad.join(&e.path).is_file(),
                "{}: {} does not exist",
                bad.display(),
                e.path
            );
        }
        if !rules.contains(&rule.as_str()) {
            continue;
        }
        let mut got: Vec<Expected> = engine_findings(&bad)
            .into_iter()
            .filter(|f| &f.rule == rule)
            .collect();
        expected.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        got.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // a fixture that pins no value accepts any measured value
        for (g, e) in got.iter_mut().zip(&expected) {
            if e.value.is_none() {
                g.value = None;
            }
        }
        assert_eq!(got, expected, "fixture {}", bad.display());
        let hits: Vec<Expected> = engine_findings(&good)
            .into_iter()
            .filter(|f| &f.rule == rule)
            .collect();
        assert!(
            hits.is_empty(),
            "fixture {} reported {hits:?}",
            good.display()
        );
    }
}

/// Only the deliberately broken files may be unreadable: every other fixture, including the files
/// made by gltfpack and glTF Transform, must read cleanly.
#[test]
fn every_other_fixture_reads_cleanly() {
    let root = fixtures_root();
    let report = meshdiet_core::scan(
        std::slice::from_ref(&root),
        &meshdiet_core::Options::default(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        report.assets.len() > 30,
        "only {} assets",
        report.assets.len()
    );
    let stray: Vec<String> = report
        .findings
        .iter()
        .filter(|f| f.rule == "GLTF-001" && !f.path.starts_with("GLTF-001/bad/"))
        .map(|f| format!("{}: {}", f.path, f.message))
        .collect();
    assert!(stray.is_empty(), "unexpected GLTF-001: {stray:#?}");
}

#[test]
fn rule_ids_are_recognised() {
    assert!(is_rule_id("MESH-001"));
    assert!(is_rule_id("GLTF-002"));
    assert!(!is_rule_id("demo-site"));
    assert!(!is_rule_id("TEX-01"));
}
