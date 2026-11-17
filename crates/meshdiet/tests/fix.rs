//! `meshdiet fix` end to end (SPEC-FIX-001).
//!
//! This test binary has its own `main` (`harness = false` in Cargo.toml) because it doubles as a
//! fake gltfpack: started with `FAKE_GLTFPACK_MODE` set, it behaves like gltfpack that copies,
//! grows, breaks or fails, so the error paths run without the real tool. Tests marked "real" run
//! the real gltfpack from `MESHDIET_TEST_GLTFPACK` or `PATH`, and say so when they skip.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup; a failure should stop the test"
)]

mod fake_gltfpack;

use fake_gltfpack as fake;

use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};

use serde_json::{Value, json};
use xtask::gltf::{Builder, glb_from};

type Test = (&'static str, fn());

const TESTS: &[Test] = &[
    (
        "ac_001_missing_gltfpack_exits_2_with_install_lines",
        ac_001_missing_gltfpack_exits_2_with_install_lines,
    ),
    ("old_gltfpack_exits_2", old_gltfpack_exits_2),
    (
        "ac_002_fix_shrinks_and_outputs_parse",
        ac_002_fix_shrinks_and_outputs_parse,
    ),
    ("ac_003_named_nodes_survive", ac_003_named_nodes_survive),
    (
        "ac_004_dry_run_changes_nothing",
        ac_004_dry_run_changes_nothing,
    ),
    (
        "ac_005_compressed_asset_is_skipped",
        ac_005_compressed_asset_is_skipped,
    ),
    (
        "ac_006_larger_output_keeps_original",
        ac_006_larger_output_keeps_original,
    ),
    ("lost_names_fail_verification", lost_names_fail_verification),
    (
        "gltfpack_errors_are_reported_per_file",
        gltfpack_errors_are_reported_per_file,
    ),
    ("val_001_bad_arguments_exit_2", val_001_bad_arguments_exit_2),
    (
        "in_place_needs_a_clean_git_copy",
        in_place_needs_a_clean_git_copy,
    ),
    (
        "scan_fix_runs_fix_after_the_scan",
        scan_fix_runs_fix_after_the_scan,
    ),
    (
        "in_place_replaces_a_committed_file",
        in_place_replaces_a_committed_file,
    ),
];

fn main() -> ExitCode {
    if let Ok(mode) = std::env::var(fake::MODE) {
        return fake::run(&mode);
    }
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let chosen: Vec<&Test> = TESTS
        .iter()
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|f| name.contains(f.as_str())))
        .collect();
    println!("\nrunning {} tests", chosen.len());
    let mut failed = Vec::new();
    for (name, test) in &chosen {
        let ok = catch_unwind(AssertUnwindSafe(test)).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(*name);
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed\n",
        if failed.is_empty() { "ok" } else { "FAILED" },
        chosen.len() - failed.len(),
        failed.len()
    );
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---- helpers ---------------------------------------------------------------------------------

fn meshdiet() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_meshdiet"));
    cmd.env_remove("MESHDIET_GLTFPACK").env_remove(fake::MODE);
    cmd
}

/// `meshdiet fix ARGS` using this binary as a fake gltfpack in `mode`.
fn fix_with_fake(mode: &str, dir: &Path, args: &[&str]) -> Output {
    let me = std::env::current_exe().unwrap();
    meshdiet()
        .current_dir(dir)
        .env(fake::MODE, mode)
        .env(fake::LOG, dir.join("gltfpack.log"))
        .arg("fix")
        .arg("--gltfpack")
        .arg(me)
        .args(args)
        .output()
        .unwrap()
}

/// The real gltfpack, or `None` with a message saying the test is skipped.
fn real_gltfpack(test: &str) -> Option<PathBuf> {
    let found = std::env::var_os("MESHDIET_TEST_GLTFPACK")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            meshdiet_core::fix::gltfpack::locate(None)
                .ok()
                .map(|g| g.path)
        });
    if found.is_none() {
        println!(
            "    {test}: skipped, needs gltfpack 1.3 or newer (set MESHDIET_TEST_GLTFPACK=/path/to/gltfpack or put it on PATH)"
        );
    }
    found
}

fn fix_with_real(gltfpack: &Path, dir: &Path, args: &[&str]) -> Output {
    meshdiet()
        .current_dir(dir)
        .arg("fix")
        .arg("--gltfpack")
        .arg(gltfpack)
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn report(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}\n{}",
            text(&out.stdout),
            text(&out.stderr)
        )
    })
}

fn results(report: &Value) -> BTreeMap<String, Value> {
    report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["input"].as_str().unwrap().to_owned(), r.clone()))
        .collect()
}

/// A model with real geometry (MESH-001 fires), named `name`.
fn dense(name: &str) -> Vec<u8> {
    let mut b = Builder::new();
    let m = b.plain_material("paint");
    let mesh = b.grid_mesh(name, 20_000, Some(m));
    b.node(name, mesh, 0.0);
    b.glb()
}

/// Every file under `dir` with its bytes.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().map(Result::unwrap) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.clone(), fs::read(&p).unwrap());
            }
        }
    }
    out
}

fn disk_bytes(dir: &Path) -> u64 {
    tree(dir).values().map(|b| b.len() as u64).sum()
}

fn glb_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[20..20 + len]).unwrap()
}

// ---- tests -----------------------------------------------------------------------------------

fn ac_001_missing_gltfpack_exits_2_with_install_lines() {
    let dir = tempfile::tempdir().unwrap();
    let out = meshdiet()
        .current_dir(dir.path())
        .env("PATH", "")
        .args(["fix", "."])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = text(&out.stderr);
    for line in [
        "macOS:",
        "Linux:",
        "Windows:",
        "github.com/zeux/meshoptimizer/releases",
        "npm",
    ] {
        assert!(err.contains(line), "`{line}` missing from:\n{err}");
    }
}

fn old_gltfpack_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let out = meshdiet()
        .current_dir(dir.path())
        .env(fake::MODE, "copy")
        .env(fake::VERSION, "0.21")
        .arg("fix")
        .arg("--gltfpack")
        .arg(std::env::current_exe().unwrap())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = text(&out.stderr);
    assert!(
        err.contains("is gltfpack 0.21") && err.contains("needs 1.3"),
        "{err}"
    );
    assert!(err.contains("Windows:"), "{err}");
}

/// The 37-model story set and the demo site, through the real gltfpack.
fn ac_002_fix_shrinks_and_outputs_parse() {
    let Some(gltfpack) = real_gltfpack("ac_002_fix_shrinks_and_outputs_parse") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    xtask::story::write(&models.join("story")).unwrap();
    let demo =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/demo-site/public/models");
    fs::create_dir(models.join("demo")).unwrap();
    for e in fs::read_dir(demo).unwrap().map(Result::unwrap) {
        fs::copy(e.path(), models.join("demo").join(e.file_name())).unwrap();
    }
    let before = tree(&models);
    let out = fix_with_real(
        &gltfpack,
        dir.path(),
        &["models", "--out", "opt", "--format", "json"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let r = report(&out);
    assert_eq!(tree(&models), before, "inputs must not change");

    let rows = results(&r);
    let fixed: Vec<&Value> = rows.values().filter(|v| v["status"] == "fixed").collect();
    // 37 story models + hero, sofa and lamp from the demo site; chair has no finding.
    assert_eq!(fixed.len(), 40, "{r:#}");
    assert_eq!(rows["demo/chair.glb"]["status"], "skipped");

    let scan = meshdiet()
        .args(["scan", "--format", "json", "--fail-on", "never"])
        .arg(dir.path().join("opt"))
        .output()
        .unwrap();
    let scan: Value = serde_json::from_slice(&scan.stdout).unwrap();
    let assets: BTreeMap<&str, &Value> = scan["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["path"].as_str().unwrap(), a))
        .collect();
    assert!(
        !scan["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "GLTF-001"),
        "every output parses"
    );
    for row in fixed {
        let output = Path::new(row["output"].as_str().unwrap());
        let rel = output.strip_prefix("opt").unwrap().to_str().unwrap();
        let a = assets[rel];
        assert_eq!(
            a["compression"]["meshopt"], true,
            "{rel} uses EXT_meshopt_compression"
        );
        assert!(
            row["after"].as_u64() < row["before"].as_u64(),
            "{rel} is smaller"
        );
        assert_eq!(
            a["bytes"], row["after"],
            "{rel}: reported size is the size on disk"
        );
    }
    assert_eq!(
        r["totals"]["after"].as_u64().unwrap(),
        disk_bytes(&dir.path().join("opt"))
    );
    assert_eq!(r["totals"]["before"].as_u64().unwrap(), disk_bytes(&models));
    assert_eq!(r["decoders"], json!(["meshopt"]));
    println!(
        "    ac_002: {} files, {} -> {} bytes",
        r["totals"]["files"], r["totals"]["before"], r["totals"]["after"]
    );
}

/// A door whose handle is a child node, both with extras, and a named material with extras.
fn door() -> Vec<u8> {
    let mut b = Builder::new();
    let wood = b.plain_material("Wood");
    let brass = b.plain_material("Brass");
    let door = b.grid_mesh("door", 20_000, Some(wood));
    let handle = b.grid_mesh("handle", 2_000, Some(brass));
    b.node("Door", door, 0.0);
    b.node("Handle", handle, 1.0);
    let glb = b.glb();
    let mut doc = glb_json_bytes(&glb);
    doc["nodes"][0]["children"] = json!([1]);
    doc["nodes"][0]["extras"] = json!({ "opens": "left", "id": 7 });
    doc["nodes"][1]["extras"] = json!({ "grab": true });
    doc["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "Spawn", "extras": { "team": "red" } }));
    doc["scenes"][0]["nodes"] = json!([0, 2]);
    doc["materials"][0]["extras"] = json!({ "sku": "OAK-1" });
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    glb_from(&doc, &glb[20 + json_len + 8..])
}

fn glb_json_bytes(glb: &[u8]) -> Value {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&glb[20..20 + len]).unwrap()
}

/// The named nodes under `index`, as `Parent/Child` paths with their extras; unnamed nodes skipped.
fn named_paths(doc: &Value, index: usize, prefix: &str, out: &mut BTreeMap<String, Value>) {
    let node = &doc["nodes"][index];
    let prefix = match node["name"].as_str() {
        Some(name) => {
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            out.insert(path.clone(), node["extras"].clone());
            path
        }
        None => prefix.to_owned(),
    };
    for child in node["children"].as_array().into_iter().flatten() {
        named_paths(
            doc,
            usize::try_from(child.as_u64().unwrap()).unwrap(),
            &prefix,
            out,
        );
    }
}

fn scene_names(doc: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for root in doc["scenes"][0]["nodes"].as_array().unwrap() {
        named_paths(
            doc,
            usize::try_from(root.as_u64().unwrap()).unwrap(),
            "",
            &mut out,
        );
    }
    out
}

fn materials(doc: &Value) -> BTreeMap<String, Value> {
    doc["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["name"].as_str().unwrap_or("").to_owned(),
                m["extras"].clone(),
            )
        })
        .collect()
}

fn ac_003_named_nodes_survive() {
    let Some(gltfpack) = real_gltfpack("ac_003_named_nodes_survive") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("in")).unwrap();
    fs::write(dir.path().join("in/door.glb"), door()).unwrap();
    let out = fix_with_real(
        &gltfpack,
        dir.path(),
        &["in", "--out", "out", "--format", "json"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(results(&report(&out))["door.glb"]["status"], "fixed");

    let before = glb_json(&dir.path().join("in/door.glb"));
    let after = glb_json(&dir.path().join("out/door.glb"));
    let names = scene_names(&after);
    assert_eq!(
        names,
        scene_names(&before),
        "same named nodes, parents and extras"
    );
    assert!(names.contains_key("Door/Handle"), "{names:?}");
    assert_eq!(names["Door"], json!({ "opens": "left", "id": 7 }));
    assert_eq!(names["Spawn"], json!({ "team": "red" }));
    assert_eq!(materials(&after), materials(&before));
    assert!(
        after["extensionsUsed"]
            .as_array()
            .unwrap()
            .contains(&json!("EXT_meshopt_compression"))
    );
}

fn ac_004_dry_run_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    fs::create_dir(&models).unwrap();
    fs::write(models.join("a.glb"), dense("a")).unwrap();
    fs::write(models.join("b.glb"), dense("b")).unwrap();
    let before = tree(dir.path());
    let out = fix_with_fake("copy", dir.path(), &["models", "--dry-run"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("Dry run: nothing was written."));
    fs::remove_file(dir.path().join("gltfpack.log")).unwrap();
    assert_eq!(tree(dir.path()), before);

    if let Some(gltfpack) = real_gltfpack("ac_004 (real gltfpack part)") {
        let out = fix_with_real(
            &gltfpack,
            dir.path(),
            &["models", "--dry-run", "--format", "json"],
        );
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        let r = report(&out);
        assert_eq!(r["mode"], "dry-run");
        assert!(r["totals"]["after"].as_u64() < r["totals"]["before"].as_u64());
        assert_eq!(tree(dir.path()), before);
    }
}

fn ac_005_compressed_asset_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    fs::create_dir(dir.path().join("m")).unwrap();
    for (from, to) in [
        ("MESH-001/good/dense-meshopt.glb", "meshopt.glb"),
        ("MESH-001/good/dense-draco.glb", "draco.glb"),
    ] {
        fs::copy(fixtures.join(from), dir.path().join("m").join(to)).unwrap();
    }
    let out = fix_with_fake("copy", dir.path(), &["m", "--all", "--format", "json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let rows = results(&report(&out));
    assert_eq!(rows["meshopt.glb"]["status"], "skipped");
    assert!(
        rows["meshopt.glb"]["reason"]
            .as_str()
            .unwrap()
            .contains("already compressed")
    );
    assert_eq!(rows["draco.glb"]["status"], "skipped");
    assert!(
        rows["draco.glb"]["reason"]
            .as_str()
            .unwrap()
            .contains("Draco")
    );
    assert!(
        !dir.path().join("gltfpack.log").exists(),
        "gltfpack must not be called"
    );
}

fn ac_006_larger_output_keeps_original() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("my models");
    fs::create_dir(&models).unwrap();
    let original = dense("chair");
    fs::write(models.join("big chair.glb"), &original).unwrap();
    let out = fix_with_fake(
        "larger",
        dir.path(),
        &["my models", "--out", "out", "--format", "json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "not improved is not a failure: {}",
        text(&out.stderr)
    );
    let r = report(&out);
    let row = &results(&r)["big chair.glb"];
    assert_eq!(row["status"], "not-improved");
    assert_eq!(row["after"], row["before"]);
    assert_eq!(
        fs::read(dir.path().join("out/big chair.glb")).unwrap(),
        original
    );
    assert_eq!(fs::read(models.join("big chair.glb")).unwrap(), original);
    // BR-003: the path with spaces reached gltfpack as one argument
    let log = fs::read_to_string(dir.path().join("gltfpack.log")).unwrap();
    assert!(log.lines().any(|l| l.ends_with("big chair.glb")), "{log}");
}

fn lost_names_fail_verification() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("m")).unwrap();
    fs::write(dir.path().join("m/door.glb"), door()).unwrap();
    let out = fix_with_fake(
        "drop-names",
        dir.path(),
        &["m", "--out", "out", "--format", "json"],
    );
    assert_eq!(out.status.code(), Some(1));
    let row = &results(&report(&out))["door.glb"];
    assert_eq!(row["status"], "verify-failed");
    assert!(row["reason"].as_str().unwrap().contains("Door"), "{row}");
    assert_eq!(fs::read(dir.path().join("out/door.glb")).unwrap(), door());
}

fn gltfpack_errors_are_reported_per_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("m")).unwrap();
    fs::write(dir.path().join("m/a.glb"), dense("a")).unwrap();
    fs::write(dir.path().join("m/b.glb"), dense("b")).unwrap();
    let out = fix_with_fake(
        "fail",
        dir.path(),
        &["m", "--format", "json", "--jobs", "2"],
    );
    assert_eq!(out.status.code(), Some(1));
    let r = report(&out);
    let rows = results(&r);
    assert_eq!(rows.len(), 2, "both files tried");
    let reason = rows["a.glb"]["reason"].as_str().unwrap();
    assert_eq!(rows["a.glb"]["status"], "gltfpack-failed");
    assert!(
        reason.contains("error line 30") && reason.contains("error line 11"),
        "{reason}"
    );
    assert!(
        !reason.contains("error line 10\n"),
        "only the last 20 lines: {reason}"
    );
    // the default output folder still gets the originals
    assert!(dir.path().join("meshdiet-out/a.glb").is_file());
}

fn val_001_bad_arguments_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["--in-place", "--out", "x"][..],
        &["--max-texture", "1000"],
        &["--max-texture", "16384"],
        &["--force"],
        &["--gltfpack-arg", "-si", "--gltfpack-arg", "0.5"],
    ] {
        let out = fix_with_fake("copy", dir.path(), args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn in_place_needs_a_clean_git_copy() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.glb"), dense("a")).unwrap();
    let out = fix_with_fake(
        "copy",
        dir.path(),
        &["a.glb", "--in-place", "--format", "json"],
    );
    let row = &results(&report(&out))["a.glb"];
    assert_eq!(row["status"], "skipped");
    assert!(row["reason"].as_str().unwrap().contains("--force"), "{row}");
    assert!(!dir.path().join("gltfpack.log").exists());
}

fn in_place_replaces_a_committed_file() {
    let Some(gltfpack) = real_gltfpack("in_place_replaces_a_committed_file") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let original = dense("a");
    fs::write(dir.path().join("a.glb"), &original).unwrap();
    fs::write(dir.path().join("b.glb"), &original).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "a.glb", "b.glb"]);
    git(dir.path(), &["commit", "-q", "-m", "models"]);
    // b has a local change: it must be left alone
    let mut changed = original.clone();
    changed.extend_from_slice(&[0; 4]);
    fs::write(dir.path().join("b.glb"), &changed).unwrap();
    let out = fix_with_real(
        &gltfpack,
        dir.path(),
        &[".", "--in-place", "--format", "json"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let rows = results(&report(&out));
    assert_eq!(rows["a.glb"]["status"], "fixed");
    assert!(fs::read(dir.path().join("a.glb")).unwrap().len() < original.len());
    assert_eq!(rows["b.glb"]["status"], "skipped");
    assert_eq!(fs::read(dir.path().join("b.glb")).unwrap(), changed);
}

fn scan_fix_runs_fix_after_the_scan() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("m")).unwrap();
    fs::write(dir.path().join("m/a.glb"), dense("a")).unwrap();
    let me = std::env::current_exe().unwrap();
    let out = meshdiet()
        .current_dir(dir.path())
        .env(fake::MODE, "copy")
        .env("MESHDIET_GLTFPACK", &me)
        .args(["scan", "m", "--fix"])
        .output()
        .unwrap();
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("MESH-001"),
        "the scan report comes first: {stdout}"
    );
    assert!(
        stdout.contains("a.glb") && stdout.contains("not improved"),
        "{stdout}"
    );
    assert!(dir.path().join("meshdiet-out/a.glb").is_file());

    let json = meshdiet()
        .current_dir(dir.path())
        .args(["scan", "m", "--fix", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(2));
}
