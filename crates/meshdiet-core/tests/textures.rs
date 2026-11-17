//! SPEC-RULE-001 AC-003 and AC-004 against the generated fixtures.

#![allow(clippy::panic, reason = "a failed scan should fail the test loudly")]

use std::path::{Path, PathBuf};

use meshdiet_core::{Finding, Options, scan};

fn fixture(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(dir)
}

fn findings(dir: &str, rule: &str) -> Vec<Finding> {
    scan(&[fixture(dir)], &Options::default())
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .findings
        .into_iter()
        .filter(|f| f.rule == rule)
        .collect()
}

#[test]
fn ac_003_4096_png_is_tex_001_with_estimates() {
    let found = findings("TEX-001/bad", "TEX-001");
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.measured.value, 4096);
    assert_eq!(f.threshold.as_ref().map(|t| t.value), Some(2048));
    assert!(f.message.contains("4096×4096"), "{}", f.message);
    assert!(
        f.message.contains("est. 89.5 MB GPU memory"),
        "{}",
        f.message
    );
    assert!(
        f.message.contains("est. 22.4 MB at 2048×2048"),
        "{}",
        f.message
    );
    assert_eq!(findings("TEX-001/good", "TEX-001"), Vec::<Finding>::new());
}

#[test]
fn ac_004_rgb_png_is_tex_003_rgba_is_not() {
    let found = findings("TEX-003/bad", "TEX-003");
    assert_eq!(found.len(), 1);
    assert!(found[0].measured.value > 256_000);
    assert!(found[0].fix.contains("JPEG"));
    // glass.glb has an RGBA PNG of about the same size
    assert_eq!(findings("TEX-003/good", "TEX-003"), Vec::<Finding>::new());
}

#[test]
fn ktx2_textures_never_trigger_tex_002() {
    assert_eq!(findings("TEX-002/bad", "TEX-002").len(), 1);
    assert_eq!(findings("TEX-002/good", "TEX-002"), Vec::<Finding>::new());
}

#[test]
fn tex_004_reports_the_stored_bytes() {
    let found = findings("TEX-004/bad", "TEX-004");
    assert_eq!(found.len(), 1);
    assert!(found[0].measured.value > 1_000_000);
    assert!(!found[0].measured.estimated);
}
