//! `meshdiet.toml`, `--rules/--skip`, ignores and CFG-IGN, the baseline ratchet, SARIF against the
//! 2.1.0 schema, Markdown, and the `rules` / `explain` commands, through the real binary
//! (SPEC-CLI-001 FR-003, FR-005…FR-007, VAL-001, VAL-002).

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
        .env_remove("NO_COLOR")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn json_in(dir: &Path, args: &[&str]) -> (Value, Option<i32>) {
    let mut all = vec!["scan", "--format", "json"];
    all.extend_from_slice(args);
    let o = run_in(dir, &all);
    (
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic_text(&o)),
        o.status.code(),
    )
}

fn panic_text(o: &Output) -> Value {
    Value::String(format!("not JSON: {}{}", stdout(o), stderr(o)))
}

fn rules_of(report: &Value) -> Vec<String> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap().to_owned())
        .collect()
}

fn demo_site() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/demo-site")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the demo site, so a test can add a `meshdiet.toml` next to it.
fn demo_copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&demo_site(), dir.path());
    dir
}

/// An uncompressed GLB with one grid mesh of `triangles` triangles.
fn model(name: &str, triangles: u32) -> Vec<u8> {
    let mut b = Builder::new();
    let mesh = b.grid_mesh(name, triangles, None);
    b.node(name, mesh, 0.0);
    b.glb()
}

fn models(dir: &Path, list: &[(&str, u32)]) {
    for (name, triangles) in list {
        fs::write(dir.join(format!("{name}.glb")), model(name, *triangles)).unwrap();
    }
}

// ---------------------------------------------------------------- meshdiet.toml

#[test]
fn fail_on_paths_ignore_and_skip_come_from_the_config() {
    let dir = demo_copy();
    fs::write(
        dir.path().join("meshdiet.toml"),
        r#"
fail_on = "never"
paths = ["public"]
ignore = ["**/lamp.gltf"]

[rules]
skip = ["SIZE-001"]
"#,
    )
    .unwrap();
    // no PATH: `paths` from the config, relative to it
    let (report, code) = json_in(dir.path(), &[]);
    assert_eq!(code, Some(0), "fail_on = never");
    assert_eq!(report["root"], "public");
    let paths: Vec<&str> = report["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["models/chair.glb", "models/hero.glb", "models/sofa.glb"]
    );
    let rules = rules_of(&report);
    assert!(!rules.contains(&"SIZE-001".to_owned()), "{rules:?}");
    assert!(rules.contains(&"MESH-001".to_owned()), "{rules:?}");
    // a flag beats the config
    let (_, code) = json_in(dir.path(), &["--fail-on", "high"]);
    assert_eq!(code, Some(1));
}

#[test]
fn rules_and_skip_flags_select_rules() {
    let site = demo_site();
    let (report, _) = json_in(&site, &["--rules", "tex-001,TEX-004"]);
    let mut rules = rules_of(&report);
    rules.sort();
    assert_eq!(rules, ["TEX-001", "TEX-004"]);
    let (report, _) = json_in(&site, &["--skip", "MESH-001", "--skip", "SIZE-001"]);
    let rules = rules_of(&report);
    assert!(!rules.iter().any(|r| r == "MESH-001" || r == "SIZE-001"));
    assert_ne!(rules, Vec::<String>::new());
}

#[test]
fn unknown_rule_ids_exit_2_with_the_valid_ones() {
    for args in [
        &["scan", ".", "--rules", "MESH-009"][..],
        &["scan", ".", "--skip", "TEXX-001"][..],
    ] {
        let o = run_in(&demo_site(), args);
        assert_eq!(o.status.code(), Some(2));
        assert!(
            stderr(&o).contains("valid rules: BASE-GROW, CFG-IGN"),
            "{}",
            stderr(&o)
        );
    }
}

#[test]
fn unknown_config_key_exits_2() {
    let dir = demo_copy();
    fs::write(dir.path().join("meshdiet.toml"), "fail-on = \"high\"\n").unwrap();
    let o = run_in(dir.path(), &["scan"]);
    assert_eq!(o.status.code(), Some(2));
    let err = stderr(&o);
    assert!(err.contains("unknown field `fail-on`"), "{err}");
    assert!(err.contains("fail_on") && err.contains("budgets"), "{err}");
}

#[test]
fn bad_sizes_and_words_in_the_config_exit_2() {
    for text in [
        "[budgets]\nfile_max = \"5 MiB\"\n",
        "[budgets]\nfile_max = \"-1 MB\"\n",
        "fail_on = \"critical\"\n",
        "[rules]\nskip = [\"TEX-099\"]\n",
    ] {
        let dir = demo_copy();
        fs::write(dir.path().join("meshdiet.toml"), text).unwrap();
        let o = run_in(dir.path(), &["scan"]);
        assert_eq!(o.status.code(), Some(2), "{text}");
        assert!(stderr(&o).contains("meshdiet.toml"), "{}", stderr(&o));
    }
}

// ---------------------------------------------------------------- ignores

#[test]
fn ignore_entries_keep_findings_out_with_their_reason() {
    let dir = demo_copy();
    fs::write(
        dir.path().join("meshdiet.toml"),
        r#"
[[ignore]]
rule = "TEX-001"
path = "public/models/sofa.glb"
reason = "close-up fabric, shown full screen"
"#,
    )
    .unwrap();
    let (report, _) = json_in(dir.path(), &[]);
    assert!(!rules_of(&report).contains(&"TEX-001".to_owned()));
    assert_eq!(report["ignored"][0]["finding"]["rule"], "TEX-001");
    assert_eq!(
        report["ignored"][0]["reason"],
        "close-up fabric, shown full screen"
    );
    let o = run_in(dir.path(), &["scan"]);
    let out = stdout(&o);
    assert!(out.contains("1 finding ignored by meshdiet.toml (--show-ignored lists them)"));
    assert!(out.contains(" · 1 ignored · "), "{out}");
    let out = stdout(&run_in(dir.path(), &["scan", "--show-ignored"]));
    assert!(out.contains("Ignored by meshdiet.toml"), "{out}");
    assert!(
        out.contains("(close-up fabric, shown full screen)"),
        "{out}"
    );
}

#[test]
fn ignore_without_reason_is_cfg_ign() {
    let dir = demo_copy();
    fs::write(
        dir.path().join("meshdiet.toml"),
        "ignore = [{ rule = \"SIZE-001\", path = \"**/hero.glb\" }]\n",
    )
    .unwrap();
    let (report, _) = json_in(dir.path(), &[]);
    let rules = rules_of(&report);
    // the entry still applies, and is reported as a low finding on the config file
    assert!(!rules.contains(&"SIZE-001".to_owned()));
    let f = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["rule"] == "CFG-IGN")
        .unwrap();
    assert_eq!(f["severity"], "low");
    assert_eq!(f["path"], "meshdiet.toml");
    assert!(
        f["message"]
            .as_str()
            .unwrap()
            .contains("SIZE-001 on `**/hero.glb` without a reason")
    );
}

// ---------------------------------------------------------------- baseline

#[test]
fn ac_003_unchanged_baseline_reports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("baseline.json");
    let site = demo_site();
    let o = run_in(
        &site,
        &["scan", ".", "--write-baseline", base.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(1), "the site has high findings");
    let written: Value = serde_json::from_str(&fs::read_to_string(&base).unwrap()).unwrap();
    assert_eq!(written["schema"], 1);
    assert_eq!(written["findings"].as_array().unwrap().len(), 7);
    assert_eq!(written["files"]["public/models/hero.glb"], 5_146_708);
    let (report, code) = json_in(&site, &[".", "--baseline", base.to_str().unwrap()]);
    assert_eq!(code, Some(0));
    assert_eq!(report["findings"], serde_json::json!([]));
    assert_eq!(report["baseline"]["known"], 7);
    let out = stdout(&run_in(
        &site,
        &["scan", ".", "--baseline", base.to_str().unwrap()],
    ));
    assert!(out.contains("Nothing new since the baseline."), "{out}");
    assert!(
        out.contains("0 high · 0 medium · 0 low · 7 in baseline"),
        "{out}"
    );
}

#[test]
fn ac_004_grown_file_breaks_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    models(root, &[("a", 20_000), ("b", 20_000), ("c", 20_000)]);
    assert_eq!(
        run_in(root, &["scan", ".", "--write-baseline", "base.json"])
            .status
            .code(),
        Some(1)
    );
    // grows by 10%
    models(root, &[("b", 22_000)]);
    let (report, code) = json_in(root, &[".", "--baseline", "base.json"]);
    assert_eq!(code, Some(1));
    let findings = report["findings"].as_array().unwrap();
    assert_ne!(findings.len(), 0);
    for f in findings {
        assert_eq!(f["path"], "b.glb", "{f}");
    }
    let grow = findings.iter().find(|f| f["rule"] == "BASE-GROW").unwrap();
    assert_eq!(grow["severity"], "high");
    assert!(
        grow["message"]
            .as_str()
            .unwrap()
            .contains("more than the 1% tolerance"),
        "{grow}"
    );
    // the same growth fits a 15% tolerance
    let (_, code) = json_in(
        root,
        &[".", "--baseline", "base.json", "--tolerance", "15%"],
    );
    assert_eq!(code, Some(0));
}

#[test]
fn growth_under_the_tolerance_is_not_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    models(root, &[("a", 20_000), ("b", 20_000)]);
    run_in(root, &["scan", ".", "--write-baseline", "base.json"]);
    models(root, &[("b", 20_100)]);
    let (report, code) = json_in(root, &[".", "--baseline", "base.json"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["findings"], serde_json::json!([]));
}

#[test]
fn new_files_that_grow_the_build_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    models(root, &[("a", 20_000), ("b", 20_000)]);
    run_in(
        root,
        &[
            "scan",
            ".",
            "--skip",
            "MESH-001",
            "--write-baseline",
            "base.json",
        ],
    );
    models(root, &[("d", 20_000)]);
    let (report, code) = json_in(
        root,
        &[".", "--skip", "MESH-001", "--baseline", "base.json"],
    );
    assert_eq!(code, Some(1));
    assert_eq!(rules_of(&report), ["BASE-GROW"]);
    let f = &report["findings"][0];
    assert_eq!(f["path"], "base.json");
    assert!(
        f["message"].as_str().unwrap().contains("; new: d.glb"),
        "{f}"
    );
}

#[test]
fn removed_files_are_listed_as_improvements() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    models(root, &[("a", 20_000), ("b", 20_000), ("c", 5_000)]);
    run_in(root, &["scan", ".", "--write-baseline", "base.json"]);
    fs::remove_file(root.join("b.glb")).unwrap();
    models(root, &[("c", 2_000)]);
    let o = run_in(root, &["scan", ".", "--baseline", "base.json"]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    assert!(out.contains("improved  b.glb removed"), "{out}");
    assert!(out.contains("improved  c.glb shrank from"), "{out}");
    let md = stdout(&run_in(
        root,
        &[
            "scan",
            ".",
            "--baseline",
            "base.json",
            "--format",
            "markdown",
        ],
    ));
    assert!(md.contains("### Since the baseline"), "{md}");
    assert!(md.contains("- removed `b.glb`"), "{md}");
}

#[test]
fn baseline_errors_exit_2() {
    let site = demo_site();
    let o = run_in(&site, &["scan", ".", "--baseline", "/no/such/base.json"]);
    assert_eq!(o.status.code(), Some(2));
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.json");
    fs::write(&bad, "{\"version\": 1}").unwrap();
    let o = run_in(&site, &["scan", ".", "--baseline", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        stderr(&o).contains("not a meshdiet baseline"),
        "{}",
        stderr(&o)
    );
    let o = run_in(&site, &["scan", ".", "--tolerance", "lots"]);
    assert_eq!(o.status.code(), Some(2));
}

// ---------------------------------------------------------------- SARIF and Markdown

fn sarif_schema() -> jsonschema::Validator {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/schemas/sarif-2.1.0.json");
    let schema: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    jsonschema::draft4::new(&schema).unwrap()
}

#[test]
fn ac_002_sarif_validates_against_schema() {
    // run from the fixtures folder, so paths carry the scanned folder like a repo checkout would
    let fixtures = demo_site().join("..");
    let o = run_in(&fixtures, &["scan", "demo-site", "--format", "sarif"]);
    assert_eq!(o.status.code(), Some(1));
    let sarif: Value = serde_json::from_slice(&o.stdout).unwrap();
    let errors: Vec<String> = sarif_schema()
        .iter_errors(&sarif)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let run = &sarif["runs"][0];
    let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
    let results = run["results"].as_array().unwrap();
    assert_eq!(results.len(), 7);
    for r in results {
        let uri = r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert!(uri.starts_with("demo-site/public/models/"), "{uri}");
        assert!(r["locations"][0]["physicalLocation"]["region"].is_null());
        let index = usize::try_from(r["ruleIndex"].as_u64().unwrap()).unwrap();
        assert_eq!(rules[index]["id"], r["ruleId"]);
        assert_eq!(
            r["partialFingerprints"]["meshdiet/v1"]
                .as_str()
                .unwrap()
                .len(),
            16
        );
        assert!(r["properties"]["measured"]["value"].is_u64());
        assert!(r["properties"]["estimated"].is_boolean());
    }
    let levels: Vec<&str> = results
        .iter()
        .map(|r| r["level"].as_str().unwrap())
        .collect();
    assert_eq!(
        levels,
        [
            "error", "error", "warning", "warning", "warning", "warning", "note"
        ]
    );
    let tex2 = results.iter().find(|r| r["ruleId"] == "TEX-002").unwrap();
    assert_eq!(tex2["properties"]["measured"]["estimated"], true);
    assert_eq!(tex2["properties"]["estimated"], true);
}

#[test]
fn sarif_with_no_findings_and_a_baseline_is_still_valid() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.json");
    let site = demo_site();
    run_in(&site, &["scan", "--write-baseline", base.to_str().unwrap()]);
    let o = run_in(
        &site,
        &[
            "scan",
            "--format",
            "sarif",
            "--baseline",
            base.to_str().unwrap(),
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    let sarif: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(sarif_schema().is_valid(&sarif));
    assert_eq!(sarif["runs"][0]["results"], serde_json::json!([]));
}

#[test]
fn markdown_has_summary_findings_weight_and_pages() {
    let dir = demo_copy();
    fs::write(
        dir.path().join("meshdiet.toml"),
        "[pages]\n\"/\" = { assets = [\"public/models/hero.glb\"], max = \"5 MB\" }\n",
    )
    .unwrap();
    let md = stdout(&run_in(dir.path(), &["scan", "--format", "markdown"]));
    assert!(
        md.starts_with("## meshdiet\n\n**4 files · 5.6 MB · "),
        "{md}"
    );
    let rows = md
        .lines()
        .filter(|l| {
            ["| high", "| medium", "| low"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .count();
    assert_eq!(rows, 8, "7 findings and SIZE-003\n{md}");
    assert!(md.contains("| high | MESH-001 Geometry not compressed | `public/models/hero.glb` |"));
    assert!(md.contains("### Weight, largest first"));
    assert!(md.contains("| `public/models/hero.glb` | 5.1 MB | 1.3 MB | 3.8 MB | none |"));
    assert!(md.contains("### Pages"));
    assert!(
        md.contains("| `/` | 1 | 5.1 MB | 5.0 MB | over budget |"),
        "{md}"
    );
    assert!(!md.contains('\x1b'));
}

#[test]
fn markdown_weight_table_stops_at_20_rows() {
    let dir = tempfile::tempdir().unwrap();
    let list: Vec<(String, u32)> = (0..23).map(|i| (format!("m{i:02}"), 200 + i)).collect();
    let refs: Vec<(&str, u32)> = list.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    models(dir.path(), &refs);
    let md = stdout(&run_in(dir.path(), &["scan", "--format", "markdown"]));
    let rows = md.lines().filter(|l| l.starts_with("| `m")).count();
    assert_eq!(rows, 20, "{md}");
    assert!(md.contains("\n3 more files, "), "{md}");
    let md = stdout(&run_in(
        dir.path(),
        &["scan", "--format", "markdown", "--all"],
    ));
    assert_eq!(md.lines().filter(|l| l.starts_with("| `m")).count(), 23);
}

// ---------------------------------------------------------------- rules and explain

#[test]
fn rules_lists_every_rule_with_severity_and_confidence() {
    let o = run_in(&demo_site(), &["rules"]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    assert!(out.starts_with("RULE "), "{out}");
    for id in [
        "MESH-001",
        "MESH-002",
        "MESH-003",
        "TEX-001",
        "TEX-002",
        "TEX-003",
        "TEX-004",
        "GLTF-001",
        "SIZE-001",
        "SIZE-002",
        "SIZE-003",
        "CFG-IGN",
        "BASE-GROW",
    ] {
        assert!(
            out.lines().any(|l| l.starts_with(id)),
            "{id} missing\n{out}"
        );
    }
    assert!(
        out.lines()
            .any(|l| l.starts_with("MESH-003") && l.contains("low       medium"))
    );
}

#[test]
fn explain_gives_why_fix_and_loader_change() {
    let o = run_in(&demo_site(), &["explain", "mesh-001"]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    assert!(out.starts_with("MESH-001 Geometry not compressed\nSeverity: high"));
    // wrapped at 96 columns; compare the words
    let flat = out.split_whitespace().collect::<Vec<_>>().join(" ");
    for part in [
        "Why it matters",
        "How to fix it",
        "gltfpack -i in.glb -o out.glb -cc",
        "gltf-transform meshopt in.glb out.glb",
        "Loader change",
        "setMeshoptDecoder",
        "[[ignore]]",
    ] {
        assert!(flat.contains(part), "{part} missing\n{out}");
    }
    assert!(out.lines().all(|l| l.chars().count() <= 96), "{out}");
    let o = run_in(&demo_site(), &["explain", "MESH-999"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("meshdiet rules"));
}
