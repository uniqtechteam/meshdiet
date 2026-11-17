//! The generator is deterministic, the committed fixtures are what it builds today, and the story set
//! adds up to the numbers from the audit.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::Path;

#[test]
fn generator_is_deterministic() {
    let first = xtask::fixtures::all();
    let second = xtask::fixtures::all();
    assert_eq!(first.len(), second.len());
    for ((pa, a), (pb, b)) in first.iter().zip(&second) {
        assert_eq!(pa, pb);
        assert!(a == b, "{pa} differs between two runs");
    }
    assert_eq!(xtask::fixtures::sources(), xtask::fixtures::sources());
}

#[test]
fn committed_fixtures_match_the_generator() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
    for (rel, bytes) in xtask::fixtures::all() {
        let on_disk = fs::read(root.join(&rel)).unwrap_or_else(|_| {
            panic!("tests/fixtures/{rel} is missing; run `cargo xtask gen-fixtures`")
        });
        assert!(
            on_disk == bytes,
            "tests/fixtures/{rel} is stale; run `cargo xtask gen-fixtures` and commit the result"
        );
    }
}

#[test]
fn story_set_is_37_models_and_49_8_mb() {
    let dir = tempfile::tempdir().unwrap();
    let total = xtask::story::write(dir.path()).unwrap();
    let files: Vec<u64> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .collect();
    assert_eq!(files.len(), xtask::story::MODELS);
    assert_eq!(files.iter().sum::<u64>(), total);
    assert!(
        total.abs_diff(xtask::story::TOTAL_BYTES) < 10_000,
        "story set is {total} bytes"
    );
    assert_eq!((total + 50_000) / 100_000, 498, "rounds to 49.8 MB");
}
