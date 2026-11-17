//! Geometry rules MESH-001…003 (SPEC-RULE-001 FR-002…004, AC-001, AC-002, AC-008 and the §16 edge
//! cases). Fixture folders are checked by `fixtures.rs`; these pin the numbers and the edge cases,
//! with small models written at test time through the fixture generator's GLB writer.

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use meshdiet_core::{Confidence, Config, Finding, Options, Severity, scan};
use serde_json::{Value, json};
use xtask::gltf::glb_from;

const FLOAT: u32 = 5126;
const SHORT: u32 = 5122;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn findings_with(path: &Path, rule: &str, config: Config) -> Vec<Finding> {
    let options = Options {
        config,
        ..Options::default()
    };
    scan(&[path.to_path_buf()], &options)
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.rule == rule)
        .collect()
}

fn findings(path: &Path, rule: &str) -> Vec<Finding> {
    findings_with(path, rule, Config::default())
}

/// A GLB of one mesh: `doc` holds `meshes`, `accessors` and `bufferViews` (and anything else), the
/// binary chunk is `bin_len` zero bytes, and one node draws mesh 0.
fn glb(dir: &Path, name: &str, mut doc: Value, bin_len: usize) -> PathBuf {
    doc["asset"] = json!({ "version": "2.0" });
    doc["scene"] = json!(0);
    doc["scenes"] = json!([{ "nodes": [0] }]);
    doc["nodes"] = json!([{ "mesh": 0 }]);
    doc["buffers"] = json!([{ "byteLength": bin_len }]);
    let path = dir.join(name);
    fs::write(&path, glb_from(&doc, &vec![0; bin_len])).unwrap();
    path
}

fn view(offset: usize, len: usize) -> Value {
    json!({ "buffer": 0, "byteOffset": offset, "byteLength": len })
}

#[test]
fn ac_001_uncompressed_geometry_is_mesh_001() {
    let found = findings(&fixtures().join("MESH-001/bad"), "MESH-001");
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.path, "dense.glb");
    assert_eq!(f.severity, Severity::High);
    assert_eq!(f.confidence, Confidence::High);
    assert!((1_000_000..1_100_000).contains(&f.measured.value), "{f:?}");
    assert!(!f.measured.estimated);
    assert_eq!(f.threshold.as_ref().unwrap().value, 64_000);
    assert_eq!(f.message, "1 primitive, 1.1 MB of uncompressed geometry");
    assert!(f.fix.contains("meshdiet fix") && f.fix.contains("gltf-transform"));
}

#[test]
fn ac_002_meshopt_or_draco_is_not_mesh_001() {
    for file in ["dense-meshopt.glb", "dense-draco.glb"] {
        let path = fixtures().join("MESH-001/good").join(file);
        assert_eq!(findings(&path, "MESH-001"), [], "{file}");
    }
}

#[test]
fn an_original_next_to_its_compressed_copy_is_skipped_unless_the_app_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("public/models");
    fs::create_dir_all(&models).unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), "{}").unwrap();
    let good = fixtures().join("MESH-001/good");
    fs::copy(good.join("dense.glb"), models.join("dense.glb")).unwrap();
    fs::copy(
        good.join("dense-meshopt.glb"),
        models.join("dense-compressed.glb"),
    )
    .unwrap();
    let app = "import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';\n\
               loader.setMeshoptDecoder(MeshoptDecoder);\n";
    let main = dir.path().join("src/main.js");
    fs::write(
        &main,
        format!("{app}loader.load(`models/dense${{suffix}}.glb`);\n"),
    )
    .unwrap();
    let public = dir.path().join("public");
    assert_eq!(findings(&public, "MESH-001"), []);
    let report = scan(std::slice::from_ref(&public), &Options::default()).unwrap();
    let original = report.assets.iter().find(|a| a.path == "models/dense.glb");
    assert_eq!(
        original.unwrap().compressed_copy.as_deref(),
        Some("models/dense-compressed.glb")
    );
    assert_eq!(report.totals.assets, 2, "the original still ships");

    // an easter egg that loads the original by name: the finding stays
    fs::write(&main, format!("{app}loader.load('models/dense.glb');\n")).unwrap();
    let found = findings(&public, "MESH-001");
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].path, "models/dense.glb");
}

#[test]
fn small_geometry_is_not_worth_compressing() {
    let small = fixtures().join("MESH-001/good/box.glb");
    assert_eq!(findings(&small, "MESH-001"), []);
    let strict = Config {
        compress_min: 1,
        ..Config::default()
    };
    assert_eq!(findings_with(&small, "MESH-001", strict).len(), 1);
}

#[test]
fn quantized_geometry_is_mesh_001_medium() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({
        "extensionsUsed": ["KHR_mesh_quantization"],
        "extensionsRequired": ["KHR_mesh_quantization"],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "accessors": [{ "bufferView": 0, "componentType": SHORT, "count": 12_500, "type": "VEC4" }],
        "bufferViews": [view(0, 100_000)],
    });
    let path = glb(dir.path(), "quantized.glb", doc, 100_000);
    let found = findings(&path, "MESH-001");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Medium);
    assert!(found[0].message.contains("KHR_mesh_quantization"));
}

/// §16: an accessor with no bufferView is zero-filled; its declared size counts, and the sparse
/// parts count on top.
#[test]
fn sparse_accessor_counts_its_declared_size() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "accessors": [{
            "componentType": FLOAT, "count": 10_000, "type": "VEC3",
            "sparse": { "count": 100, "indices": { "bufferView": 0, "componentType": 5123 },
                        "values": { "bufferView": 1 } }
        }],
        "bufferViews": [view(0, 200), view(200, 1_200)],
    });
    let path = glb(dir.path(), "sparse.glb", doc, 1_400);
    let found = findings(&path, "MESH-001");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measured.value, 120_000 + 200 + 1_200);
}

/// §16: morph targets add to the geometry bytes.
#[test]
fn morph_targets_add_to_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = json!({
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "accessors": [
            { "bufferView": 0, "componentType": FLOAT, "count": 3_000, "type": "VEC3" },
            { "bufferView": 1, "componentType": FLOAT, "count": 3_000, "type": "VEC3" }
        ],
        "bufferViews": [view(0, 36_000), view(36_000, 36_000)],
    });
    let base = glb(dir.path(), "base.glb", doc.clone(), 72_000);
    assert_eq!(
        findings(&base, "MESH-001"),
        [],
        "36 KB alone is under 64 KB"
    );
    doc["meshes"][0]["primitives"][0]["targets"] = json!([{ "POSITION": 1 }]);
    let morphed = glb(dir.path(), "morphed.glb", doc, 72_000);
    let found = findings(&morphed, "MESH-001");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measured.value, 72_000);
}

/// A bufferView past the end of its buffer is GLTF-001 only; its length is not geometry.
#[test]
fn broken_buffer_view_is_not_counted() {
    let path = fixtures().join("GLTF-001/bad/view-past-end.glb");
    assert_eq!(findings(&path, "MESH-001"), []);
    assert_eq!(findings(&path, "GLTF-001").len(), 1);
}

#[test]
fn triangles_over_budget_is_mesh_002() {
    let found = findings(&fixtures().join("MESH-002/bad"), "MESH-002");
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.severity, Severity::Medium);
    assert_eq!(f.measured.value, 505_000);
    assert_eq!(f.threshold.as_ref().unwrap().value, 500_000);
    assert_eq!(
        f.message,
        "505,000 triangles drawn by the default scene, over the budget of 500,000"
    );
    let tight = Config {
        triangles_max: 400_000,
        ..Config::default()
    };
    let good = fixtures().join("MESH-002/good");
    assert_eq!(findings_with(&good, "MESH-002", tight).len(), 1);
}

/// Strips and fans give n − 2 triangles; points and lines give none.
#[test]
fn strips_and_fans_count_lines_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let primitive = |mode: u32| json!({ "attributes": { "POSITION": 0 }, "mode": mode });
    let doc = json!({
        "meshes": [{ "primitives": [primitive(5), primitive(6), primitive(1), primitive(0), primitive(4)] }],
        "accessors": [{ "bufferView": 0, "componentType": FLOAT, "count": 300, "type": "VEC3" }],
        "bufferViews": [view(0, 3_600)],
    });
    let path = glb(dir.path(), "modes.glb", doc, 3_600);
    let budget = Config {
        triangles_max: 1,
        ..Config::default()
    };
    let found = findings_with(&path, "MESH-002", budget);
    assert_eq!(found[0].measured.value, 298 + 298 + 100);
}

#[test]
fn ac_008_dead_buffer_bytes_is_mesh_003() {
    let found = findings(&fixtures().join("MESH-003/bad"), "MESH-003");
    assert_eq!(found.len(), 1);
    let f = &found[0];
    assert_eq!(f.path, "leftovers.glb");
    assert_eq!(f.severity, Severity::Low);
    assert_eq!(f.confidence, Confidence::Medium);
    assert_eq!(f.measured.value, 200 * 1024);
    assert!(
        f.message.contains("not used by any bufferView"),
        "{}",
        f.message
    );
}

/// A view that only an extension names (here Draco's) is in use.
#[test]
fn buffer_view_named_by_an_extension_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "meshes": [{ "primitives": [{
            "attributes": { "POSITION": 0 },
            "extensions": { "KHR_draco_mesh_compression": { "bufferView": 0, "attributes": { "POSITION": 0 } } }
        }] }],
        "accessors": [{ "componentType": FLOAT, "count": 30_000, "type": "VEC3" }],
        "bufferViews": [view(0, 100_000)],
    });
    let path = glb(dir.path(), "draco.glb", doc, 100_000);
    assert_eq!(findings(&path, "MESH-003"), []);
    assert_eq!(findings(&path, "MESH-001"), []);
}

/// §16: the meshopt fallback buffer has no bytes on purpose and is not dead weight.
#[test]
fn meshopt_fallback_buffer_is_not_dead() {
    let path = fixtures().join("MESH-001/good/dense-meshopt.glb");
    let report = scan(std::slice::from_ref(&path), &Options::default()).unwrap();
    let doc = report.assets[0].document.as_ref().unwrap();
    assert!(
        doc.buffers
            .iter()
            .any(meshdiet_core::gltf::Buffer::is_meshopt_fallback)
    );
    assert_eq!(findings(&path, "MESH-003"), []);
}

#[test]
fn rules_are_listed() {
    for id in ["MESH-001", "MESH-002", "MESH-003"] {
        assert!(meshdiet_core::rule::find(id).is_some(), "{id}");
    }
}
