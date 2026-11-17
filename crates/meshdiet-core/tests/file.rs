//! File rule GLTF-002 (SPEC-RULE-001 FR-010, AC-007). Fixture folders are checked by `fixtures.rs`;
//! these pin the numbers and the cases around the 16 KB limit.

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use meshdiet_core::{Confidence, Finding, Options, Severity, scan};
use serde_json::json;
use xtask::gltf::{Builder, base64, glb_from};
use xtask::texture::{Pixels, Texture};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn gltf_002(path: &Path) -> Vec<Finding> {
    scan(&[path.to_path_buf()], &Options::default())
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.rule == "GLTF-002")
        .collect()
}

#[test]
fn ac_007_base64_buffer_is_gltf_002() {
    let found = gltf_002(&fixtures().join("GLTF-002/bad"));
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.path, "rock.gltf");
    assert_eq!(f.severity, Severity::Medium);
    assert_eq!(f.confidence, Confidence::High);
    assert_eq!(f.measured.name, "base64 bytes");
    assert!(f.measured.value > 100_000, "{}", f.measured.value);
    assert!(!f.measured.estimated);
    assert_eq!(f.threshold.as_ref().unwrap().value, 16_000);
    assert!(
        f.message
            .starts_with("1 buffer embedded as base64: 101.7 KB of text for 76.2 KB of data"),
        "{}",
        f.message
    );
    assert!(f.fix.contains("meshdiet fix") && f.fix.contains("gltf-transform"));
}

#[test]
fn small_data_uri_is_fine() {
    assert_eq!(gltf_002(&fixtures().join("GLTF-002/good")), []);
}

#[test]
fn embedded_image_counts_too() {
    let dir = tempfile::tempdir().unwrap();
    let png = Texture::png("noise", 128, 128, false, Pixels::Noise(7)).encode();
    let uri = format!("data:image/png;base64,{}", base64(&png));
    assert!(uri.len() > 16_000);
    let doc = json!({
        "asset": { "version": "2.0" },
        "images": [{ "uri": uri }],
        "textures": [{ "source": 0 }],
    });
    fs::write(
        dir.path().join("tex.gltf"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    let found = gltf_002(dir.path());
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].message.starts_with("1 image embedded"),
        "{}",
        found[0].message
    );
    assert_eq!(found[0].measured.value, uri.len() as u64);
}

#[test]
fn glb_and_external_files_are_not_gltf_002() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let mesh = b.grid_mesh("big", 4_000, None);
    b.node("big", mesh, 0.0);
    fs::write(dir.path().join("big.glb"), b.glb()).unwrap();
    for (name, bytes) in b.gltf_external("split") {
        fs::write(dir.path().join(name), bytes).unwrap();
    }
    // a GLB whose JSON carries a data URI is a different problem; the rule is about .gltf
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64(&vec![0; 20_000])
    );
    let doc =
        json!({ "asset": { "version": "2.0" }, "buffers": [{ "uri": uri, "byteLength": 20_000 }] });
    fs::write(dir.path().join("odd.glb"), glb_from(&doc, &[])).unwrap();
    assert_eq!(gltf_002(dir.path()), []);
}
