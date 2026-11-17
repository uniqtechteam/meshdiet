//! The speed goal from `_analysis/00`: 200 models, about 500 MB, audited in under 2 seconds in a
//! release build. The set is generated into a temporary folder, never committed. Run it with
//! `cargo test --release -p meshdiet-core --test speed -- --ignored --nocapture`.

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::time::{Duration, Instant};

use meshdiet_core::{Options, scan};
use xtask::gltf::{Builder, ImageStore};
use xtask::texture::{Pixels, Texture};

const MODELS: usize = 200;
const TOTAL_MIN: u64 = 500_000_000;

/// About 2.5 MB: an 80,000-triangle grid and a 512×512 noise texture, in four variations so the
/// files are not all identical.
fn model(variant: u64) -> Vec<u8> {
    let texture = Texture::png("albedo", 512, 512, false, Pixels::Noise(variant + 1));
    let mut b = Builder::new();
    let material = b.textured_material("albedo", &texture, ImageStore::BufferView);
    let triangles = 80_000 + u32::try_from(variant).unwrap() * 1_000;
    let mesh = b.grid_mesh("part", triangles, Some(material));
    b.node("part", mesh, 0.0);
    b.glb()
}

#[test]
#[ignore = "writes 500 MB; run in release mode, see the module docs"]
fn two_hundred_files_and_500_mb_in_under_two_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let variants: Vec<Vec<u8>> = (0..4).map(model).collect();
    let mut written = 0u64;
    for i in 0..MODELS {
        let bytes = &variants[i % variants.len()];
        let folder = dir.path().join(format!("set-{}", i / 50));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("model-{i:03}.glb")), bytes).unwrap();
        written += bytes.len() as u64;
    }
    assert!(written >= TOTAL_MIN, "the set is only {written} bytes");

    let started = Instant::now();
    let report = scan(&[dir.path().to_path_buf()], &Options::default()).unwrap();
    let took = started.elapsed();
    println!(
        "{} files, {} MB, {} findings, scanned in {} ms",
        report.totals.files,
        report.totals.bytes / 1_000_000,
        report.findings.len(),
        took.as_millis()
    );
    assert_eq!(report.assets.len(), MODELS);
    assert!(took < Duration::from_secs(2), "took {took:?}");
}
