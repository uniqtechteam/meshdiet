//! The weight table, page table, budgets from `meshdiet.toml` and `--gzip`, through the real binary
//! (SPEC-WEIGHT-001).

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use xtask::gltf::Builder;

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meshdiet"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn run(args: &[&str]) -> Output {
    run_in(Path::new(env!("CARGO_MANIFEST_DIR")), args)
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn story() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    xtask::story::write(&dir.path().join("models")).unwrap();
    dir
}

/// The weight table: the lines after its title, up to the blank line.
fn table(out: &str) -> Vec<&str> {
    out.lines()
        .skip_while(|l| *l != "Weight, largest first")
        .skip(2)
        .take_while(|l| !l.is_empty())
        .collect()
}

#[test]
fn ac_001_37_uncompressed_models_49_8_mb() {
    let dir = story();
    let o = run(&["scan", path(&dir.path().join("models"))]);
    let out = stdout(&o);
    assert!(
        out.contains("37 files · 49.8 MB · 37 uncompressed (49.8 MB) · 37 high"),
        "{out}"
    );
    // the 20 largest, then one line for the rest
    let rows = table(&out);
    assert_eq!(rows.len(), 21, "{out}");
    assert!(rows[0].starts_with("  product-37.glb"), "{out}");
    assert!(rows[0].contains("none"), "compression column: {out}");
    assert!(
        rows[20].starts_with("  17 more files, ") && rows[20].ends_with("(--all shows every row)"),
        "{out}"
    );
    assert!(out.contains("geometry") && out.contains("GPU (est.)"));

    let all = stdout(&run(&["scan", "--all", path(&dir.path().join("models"))]));
    assert_eq!(table(&all).len(), 37);
    assert!(!all.contains("more files"));
}

#[test]
fn ac_001_six_mb_glb_exits_1_with_one_high() {
    // 6 MB that only the file budget catches: a small box and 6 MB of unused buffer (MESH-003, low)
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let mesh = b.grid_mesh("box", 12, None);
    b.node("box", mesh, 0.0);
    b.dead_bytes(6_000_000);
    fs::write(dir.path().join("big.glb"), b.glb()).unwrap();
    let o = run(&["scan", path(dir.path())]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(
        out.contains("SIZE-001  6.0 MB, over the 5.0 MB file budget"),
        "{out}"
    );
    assert!(out.contains(" · 1 high · 0 medium · "), "{out}");
}

#[test]
fn size_001_names_the_biggest_image() {
    let out = stdout(&run(&["scan", path(&fixtures().join("SIZE-001/bad"))]));
    assert!(
        out.contains("SIZE-001  5.1 MB, over the 5.0 MB file budget"),
        "{out}"
    );
    assert!(
        out.contains("Biggest image: hero-photo (1400x1400 JPEG, 3.8 MB)"),
        "{out}"
    );
}

#[test]
fn json_has_totals_and_pages() {
    let o = run(&[
        "scan",
        "--format",
        "json",
        path(&fixtures().join("SIZE-003/bad")),
    ]);
    let r: Value = serde_json::from_slice(&o.stdout).unwrap();
    let t = &r["totals"];
    for key in [
        "assets",
        "files",
        "bytes",
        "geometry_bytes",
        "texture_bytes",
        "uncompressed_assets",
        "uncompressed_bytes",
    ] {
        assert!(t[key].is_u64(), "totals.{key}: {t}");
    }
    assert_eq!(t["est_vram"]["estimated"], true);
    assert!(t.get("gzip").is_none(), "gzip only with --gzip");
    let pages = r["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1]["name"], "/shop");
    assert_eq!(pages[1]["max"], 2_000_000);
    assert_eq!(pages[1]["assets"].as_array().unwrap().len(), 2);
    let size = r["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["rule"] == "SIZE-003")
        .unwrap();
    assert_eq!(size["path"], "meshdiet.toml");
}

#[test]
fn json_pages_is_empty_without_config() {
    let o = run(&[
        "scan",
        "--format",
        "json",
        path(&fixtures().join("SIZE-001/good")),
    ]);
    let r: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(r["pages"], serde_json::json!([]));
}

#[test]
fn page_table_shows_budgets() {
    let o = run(&["scan", path(&fixtures().join("SIZE-003/bad"))]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(1));
    let shop = out
        .lines()
        .find(|l| l.trim_start().starts_with("/shop"))
        .unwrap();
    assert!(
        shop.contains("2.1 MB") && shop.contains("2.0 MB") && shop.contains("over budget"),
        "{out}"
    );
}

#[test]
fn config_in_the_current_folder_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("public");
    fs::create_dir_all(&models).unwrap();
    for f in ["models/table.glb", "models/desk.glb"] {
        let to = models.join(Path::new(f).file_name().unwrap());
        fs::copy(fixtures().join("SIZE-002/bad").join(f), to).unwrap();
    }
    fs::write(
        dir.path().join("meshdiet.toml"),
        "[budgets]\nbuild_max = \"2 MB\"\n",
    )
    .unwrap();
    let o = run_in(dir.path(), &["scan", "public"]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stdout(&o).contains("SIZE-002"));
    assert!(stdout(&o).lines().any(|l| l == "meshdiet.toml"));

    // --config wins, and a broken config is a usage error
    fs::write(
        dir.path().join("loose.toml"),
        "[budgets]\nbuild_max = \"3 MB\"\n",
    )
    .unwrap();
    let o = run_in(dir.path(), &["scan", "public", "--config", "loose.toml"]);
    assert!(!stdout(&o).contains("SIZE-002"), "{}", stdout(&o));
    fs::write(
        dir.path().join("bad.toml"),
        "[budgets]\nbuild_max = \"3 MiB\"\n",
    )
    .unwrap();
    let o = run_in(dir.path(), &["scan", "public", "--config", "bad.toml"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("is not a size"));
}

#[test]
fn gzip_adds_a_labelled_estimate() {
    let bad = fixtures().join("SIZE-003/bad");
    let out = stdout(&run(&["scan", "--gzip", path(&bad)]));
    assert!(out.contains("gzip -6 est."), "{out}");
    let o = run(&["scan", "--gzip", "--format", "json", path(&bad)]);
    let r: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(r["totals"]["gzip"]["estimated"], true);
    assert!(r["totals"]["gzip"]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(r["pages"][1]["gzip"]["estimated"], true);
    assert_eq!(r["gzip_estimate"].as_object().unwrap().len(), 3);
}

#[test]
fn help_lists_the_weight_options() {
    let out = stdout(&run(&["scan", "--help"]));
    for flag in ["--config", "--gzip", "--all"] {
        assert!(out.contains(flag), "missing {flag}");
    }
}
