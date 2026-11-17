//! Build totals, shared files, page groups, budgets and the gzip estimate (SPEC-WEIGHT-001).

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::Path;

use meshdiet_core::weight::{NO_PAGE, human};
use meshdiet_core::{Config, Finding, Options, Report, Severity, scan};
use xtask::gltf::Builder;

/// A plain uncompressed grid model with `triangles` triangles, as GLB bytes.
fn model(name: &str, triangles: u32) -> Vec<u8> {
    let mut b = Builder::new();
    let mesh = b.grid_mesh(name, triangles, None);
    b.node(name, mesh, 0.0);
    b.glb()
}

fn write(dir: &Path, rel: &str, bytes: &[u8]) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn run(dir: &Path, config: Config) -> Report {
    let options = Options {
        config,
        ..Options::default()
    };
    scan(&[dir.to_path_buf()], &options).unwrap()
}

fn config(toml: &str) -> Config {
    Config::from_toml(toml).unwrap()
}

fn of<'a>(report: &'a Report, rule: &str) -> Vec<&'a Finding> {
    report.findings.iter().filter(|f| f.rule == rule).collect()
}

#[test]
fn ac_001_37_uncompressed_models_49_8_mb() {
    let dir = tempfile::tempdir().unwrap();
    let written = xtask::story::write(dir.path()).unwrap();
    let r = run(dir.path(), Config::default());
    let t = r.totals;
    assert_eq!(t.assets, 37);
    assert_eq!(t.files, 37);
    assert_eq!(t.bytes, written);
    assert_eq!(human(t.bytes), "49.8 MB");
    assert_eq!(t.uncompressed_assets, 37);
    assert_eq!(t.uncompressed_bytes, t.bytes);
    assert!(t.geometry_bytes > t.bytes * 99 / 100, "{t:?}");
    assert_eq!(t.texture_bytes, 0);
    assert!(t.est_vram.estimated);
    assert!(t.gzip.is_none(), "gzip only with --gzip");
    assert_eq!(r.pages, Vec::new());
}

#[test]
fn ac_002_shared_bin_counted_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let mesh = b.grid_mesh("chair", 50_000, None);
    b.node("chair", mesh, 0.0);
    let files = b.gltf_external("chair");
    let (_, gltf) = files.iter().find(|(p, _)| p == "chair.gltf").unwrap();
    let (_, bin) = files.iter().find(|(p, _)| p == "chair.bin").unwrap();
    assert!(
        bin.len() > 1_000_000,
        "the shared .bin is {} bytes",
        bin.len()
    );
    write(dir.path(), "chair.bin", bin);
    write(dir.path(), "chair.gltf", gltf);
    write(dir.path(), "chair-red.gltf", gltf);
    let r = run(dir.path(), Config::default());
    assert_eq!(r.assets.len(), 2);
    let gltf_len = gltf.len() as u64;
    let bin_len = bin.len() as u64;
    assert!(r.assets.iter().all(|a| a.bytes == gltf_len + bin_len));
    assert_eq!(r.totals.files, 3);
    assert_eq!(r.totals.bytes, 2 * gltf_len + bin_len);
    assert!(r.totals.geometry_bytes <= bin_len, "{:?}", r.totals);
    assert_eq!(r.totals.uncompressed_bytes, r.totals.bytes);

    let pages = config("[pages]\n\"/\" = { assets = [\"*.gltf\"] }");
    let r = run(dir.path(), pages);
    assert_eq!(r.pages.len(), 1);
    assert_eq!(r.pages[0].bytes, 2 * gltf_len + bin_len);
}

#[test]
fn ac_003_build_over_budget_is_size_002() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        write(
            dir.path(),
            &format!("m{i}.glb"),
            &model(&format!("m{i}"), 180_000),
        );
    }
    let mut c = config("[budgets]\nbuild_max = \"20 MB\"\nfile_max = \"10 MB\"");
    c.source = Some(dir.path().join("meshdiet.toml"));
    let r = run(dir.path(), c);
    assert!(r.totals.bytes > 25_000_000, "{}", r.totals.bytes);
    let found = of(&r, "SIZE-002");
    assert_eq!(found.len(), 1);
    let f = found[0];
    assert_eq!(f.severity, Severity::High);
    assert_eq!(f.path, "meshdiet.toml");
    assert_eq!(f.measured.value, r.totals.bytes);
    assert_eq!(f.threshold.as_ref().unwrap().value, 20_000_000);
    assert!(
        f.message.contains("over the 20.0 MB build budget"),
        "{}",
        f.message
    );
    assert_eq!(of(&r, "SIZE-001"), Vec::<&Finding>::new());

    let r = run(dir.path(), config("[budgets]\nfile_max = \"10 MB\""));
    assert!(of(&r, "SIZE-002").is_empty(), "off unless configured");
}

#[test]
fn ac_004_page_over_budget_lists_top_five() {
    let dir = tempfile::tempdir().unwrap();
    // six shop models of about 3 MB each (18 MB), largest first by triangle count
    for (i, tris) in [132_000, 130_000, 128_000, 126_000, 124_000, 122_000]
        .into_iter()
        .enumerate()
    {
        write(
            dir.path(),
            &format!("public/models/shop/item-{i}.glb"),
            &model(&format!("item-{i}"), tris),
        );
    }
    write(dir.path(), "public/models/hero.glb", &model("hero", 10_000));
    let c =
        config("[pages]\n\"/shop\" = { assets = [\"public/models/shop/*.glb\"], max = \"15 MB\" }");
    let r = run(&dir.path().join("public"), c);
    assert_eq!(r.pages.len(), 2);
    let shop = &r.pages[0];
    assert_eq!(shop.name, "/shop");
    assert_eq!(shop.assets.len(), 6);
    assert!(
        shop.bytes > 16_000_000 && shop.bytes < 20_000_000,
        "{}",
        shop.bytes
    );
    assert_eq!(shop.max, Some(15_000_000));
    assert_eq!(r.pages[1].name, NO_PAGE);
    assert_eq!(r.pages[1].assets, ["models/hero.glb"]);

    let found = of(&r, "SIZE-003");
    assert_eq!(found.len(), 1);
    let msg = &found[0].message;
    for i in 0..5 {
        assert!(msg.contains(&format!("models/shop/item-{i}.glb")), "{msg}");
    }
    assert!(!msg.contains("item-5.glb"), "only the five largest: {msg}");
    assert!(msg.contains("and 1 more"), "{msg}");
    assert_eq!(found[0].severity, Severity::High);
    assert_eq!(found[0].measured.value, shop.bytes);
}

#[test]
fn ac_005_six_mb_file_is_size_001() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.glb", &model("big", 220_000));
    write(dir.path(), "small.glb", &model("small", 10_000));
    let r = run(dir.path(), Config::default());
    let big = r.assets.iter().find(|a| a.path == "big.glb").unwrap();
    assert!(
        big.bytes > 6_000_000 && big.bytes < 7_000_000,
        "{}",
        big.bytes
    );
    let found = of(&r, "SIZE-001");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "big.glb");
    assert_eq!(found[0].severity, Severity::High);
    assert_eq!(found[0].measured.value, big.bytes);
    assert!(
        found[0]
            .message
            .contains("over the 5.0 MB file budget: geometry")
    );
}

#[test]
fn ac_006_empty_page_glob_is_cfg_page() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "models/chair.glb", &model("chair", 100));
    let c = config(
        "[pages]\n\"/\" = { assets = [\"models/*.glb\", \"modles/hero/*.glb\"], max = \"1 MB\" }",
    );
    let r = run(dir.path(), c);
    let found = of(&r, "CFG-PAGE");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Low);
    assert_eq!(found[0].path, "meshdiet.toml");
    assert!(
        found[0].message.contains("modles/hero/*.glb"),
        "{}",
        found[0].message
    );
    assert_eq!(of(&r, "SIZE-003"), Vec::<&Finding>::new());
}

#[test]
fn an_asset_can_be_in_several_pages() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "logo.glb", &model("logo", 100));
    write(dir.path(), "shop/chair.glb", &model("chair", 100));
    let c = config(
        "[pages]\n\"/\" = { assets = [\"logo.glb\"] }\n\"/shop\" = { assets = [\"logo.glb\", \"shop/**\"] }",
    );
    let r = run(dir.path(), c);
    assert_eq!(
        r.pages.len(),
        2,
        "no (no page) row when every asset has a page"
    );
    assert_eq!(r.pages[1].assets.len(), 2);
}

#[test]
fn no_assets_means_zero_totals_and_no_size_findings() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(dir.path(), config("[budgets]\nbuild_max = \"0 B\""));
    assert_eq!(r.totals.bytes, 0);
    assert_eq!(r.totals.assets, 0);
    assert_eq!(r.findings, Vec::new());
}

#[test]
fn gzip_estimate_covers_every_file_once() {
    let dir = tempfile::tempdir().unwrap();
    xtask::story::write(dir.path()).unwrap();
    let options = Options {
        gzip: true,
        ..Options::default()
    };
    let r = scan(&[dir.path().to_path_buf()], &options).unwrap();
    let gz = r.totals.gzip.unwrap();
    assert!(gz.estimated);
    assert_eq!(r.gzip_estimate.len(), 37);
    assert_eq!(gz.bytes, r.gzip_estimate.values().sum::<u64>());
    assert!(gz.bytes > 0 && gz.bytes < r.totals.bytes);
    let one = dir.path().join("product-01.glb");
    assert_eq!(
        meshdiet_core::weight::gzip_size(&one).unwrap(),
        r.gzip_estimate["product-01.glb"]
    );
}
