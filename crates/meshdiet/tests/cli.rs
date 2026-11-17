//! End-to-end tests of the `meshdiet` binary: exit codes, formats, files and colours.

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use xtask::gltf::{Builder, ImageStore};
use xtask::texture::{Pixels, Texture};

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_meshdiet"));
    cmd.env_remove("NO_COLOR");
    cmd
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn json(args: &[&str]) -> Value {
    let mut all = vec!["scan", "--format", "json"];
    all.extend_from_slice(args);
    serde_json::from_str(&stdout(&run(&all))).unwrap()
}

#[test]
fn version_flag_prints_version() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out).trim(),
        format!("meshdiet {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_lists_the_scan_options() {
    let out = run(&["scan", "--help"]);
    assert!(out.status.success());
    for flag in [
        "--fail-on",
        "--format",
        "--output",
        "--quiet",
        "--verbose",
        "--strict",
        "--src",
    ] {
        assert!(stdout(&out).contains(flag), "missing {flag}");
    }
}

#[test]
fn high_finding_exits_1_by_default() {
    let bad = fixtures().join("GLTF-001/bad");
    let o = run(&["scan", path(&bad)]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stdout(&o).contains("7 files · 7.1 KB · 7 high · 0 medium · 0 low · scanned in"));
}

#[test]
fn ac_005_fail_on_never_exits_0() {
    let bad = fixtures().join("GLTF-001/bad");
    assert_eq!(
        run(&["scan", path(&bad), "--fail-on", "never"])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn clean_models_exit_0() {
    let o = run(&["scan", path(&fixtures().join("GLTF-001/good"))]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).starts_with("No problems found."));
}

#[test]
fn ac_006_gltf_with_external_files_is_one_asset() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    for (i, name) in ["wood", "metal"].into_iter().enumerate() {
        let tex = Texture::png(name, 64, 64, false, Pixels::Noise(i as u64));
        let m = b.textured_material(name, &tex, ImageStore::File);
        let mesh = b.grid_mesh(name, 100, Some(m));
        b.node(name, mesh, 0.0);
    }
    let mut total = 0;
    for (rel, bytes) in b.gltf_external("table") {
        total += bytes.len() as u64;
        fs::write(dir.path().join(rel), bytes).unwrap();
    }
    let report = json(&[path(dir.path())]);
    let assets = report["assets"].as_array().unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(
        assets[0]["files"],
        serde_json::json!(["table.gltf", "table.bin", "wood.png", "metal.png"])
    );
    assert_eq!(assets[0]["bytes"].as_u64(), Some(total));
    assert_eq!(report["totals"]["bytes"].as_u64(), Some(total));
}

#[test]
fn json_report_has_schema_and_labels_estimates() {
    let report = json(&[path(&fixtures().join("TEX-001/bad"))]);
    assert_eq!(report["schema"], 1);
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    let image = &report["assets"][0]["images"][0];
    assert_eq!(image["width"], 4096);
    assert_eq!(image["est_vram"]["bytes"], 89_478_485);
    assert_eq!(image["est_vram"]["estimated"], true);
    assert_eq!(report["assets"][0]["compression"]["meshopt"], false);
}

#[test]
fn json_findings_carry_a_measured_value() {
    let report = json(&[path(&fixtures().join("GLTF-001/bad/empty.glb"))]);
    let f = &report["findings"][0];
    assert_eq!(f["rule"], "GLTF-001");
    assert_eq!(f["severity"], "high");
    assert_eq!(f["confidence"], "high");
    assert_eq!(f["path"], "empty.glb");
    assert_eq!(f["measured"]["value"], 0);
    assert_eq!(f["measured"]["estimated"], false);
    assert_eq!(report["summary"]["high"], 1);
}

#[test]
fn paths_use_forward_slashes_relative_to_the_root() {
    let report = json(&[path(&fixtures().join("demo-site"))]);
    let paths: Vec<&str> = report["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "public/models/chair.glb",
            "public/models/hero.glb",
            "public/models/lamp.gltf",
            "public/models/sofa.glb"
        ]
    );
}

#[test]
fn missing_path_exits_2() {
    let o = run(&["scan", "/no/such/folder"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("does not exist"));
}

#[test]
fn bad_flag_value_exits_2() {
    assert_eq!(
        run(&["scan", "--fail-on", "critical"]).status.code(),
        Some(2)
    );
}

#[test]
fn no_models_exits_0_with_a_hint() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(&["scan", path(dir.path())]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).starts_with("0 files · 0 B · 0 high"));
    assert!(stderr(&o).contains("no .glb or .gltf files"));
}

#[test]
fn quiet_prints_only_the_summary() {
    let o = run(&["scan", "--quiet", path(&fixtures().join("GLTF-001/bad"))]);
    assert_eq!(stdout(&o).lines().count(), 1);
}

#[test]
fn verbose_reports_progress_on_stderr() {
    let o = run(&["scan", "--verbose", path(&fixtures().join("GLTF-001/good"))]);
    assert!(stderr(&o).contains("reading crate.glb"));
    assert!(!stdout(&o).contains("reading"));
}

#[test]
fn no_colour_when_piped_or_written_to_a_file() {
    let o = run(&["scan", path(&fixtures().join("GLTF-001/bad"))]);
    assert!(!stdout(&o).contains('\x1b'));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("report.txt");
    let o = run(&[
        "scan",
        path(&fixtures().join("GLTF-001/bad")),
        "-o",
        path(&out),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "");
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("GLTF-001") && !text.contains('\x1b'));
}

#[test]
fn several_paths_are_scanned_together() {
    let good = fixtures().join("GLTF-001/good");
    let report = json(&[
        path(&good.join("crate.glb")),
        path(&fixtures().join("TEX-001/bad")),
    ]);
    assert_eq!(report["totals"]["assets"], 2);
}

#[test]
fn demo_site_misses_the_meshopt_decoder() {
    let report = json(&[path(&fixtures().join("demo-site"))]);
    let load: Vec<&Value> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "LOAD-001")
        .collect();
    assert_eq!(load.len(), 1, "{load:#?}");
    assert_eq!(load[0]["path"], "public/models/chair.glb");
    assert_eq!(load[0]["confidence"], "medium");
    assert!(load[0]["message"].as_str().unwrap().contains("meshopt"));
}

#[test]
fn src_reads_sources_elsewhere() {
    let site = fixtures().join("demo-site");
    let models = site.join("public/models");
    let rules = |args: &[&str]| -> Vec<String> {
        json(args)["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["rule"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(!rules(&[path(&models)]).contains(&"LOAD-001".to_owned()));
    let with_src = rules(&[path(&models), "--src", path(&site.join("src"))]);
    assert!(with_src.contains(&"LOAD-001".to_owned()));
}

#[test]
fn src_must_be_a_folder() {
    let o = run(&[
        "scan",
        path(&fixtures().join("demo-site")),
        "--src",
        "/no/such/src",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("--src"));
}
