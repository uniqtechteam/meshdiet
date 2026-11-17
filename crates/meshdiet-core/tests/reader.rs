//! The GLB / glTF reader: broken files become GLTF-001 findings, never crashes, and the numbers it
//! reads from good files are right.

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use meshdiet_core::{Asset, Options, Report, scan};
use serde_json::{Value, json};
use xtask::gltf::{Builder, ImageStore};
use xtask::texture::{Pixels, Texture};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn run(path: &Path) -> Report {
    scan(&[path.to_path_buf()], &Options::default()).unwrap()
}

fn one(path: &Path) -> Asset {
    let mut r = run(path);
    assert_eq!(r.assets.len(), 1);
    r.assets.remove(0)
}

fn gltf001(r: &Report) -> Vec<String> {
    r.findings
        .iter()
        .filter(|f| f.rule == "GLTF-001")
        .map(|f| f.message.clone())
        .collect()
}

fn cube() -> Builder {
    let mut b = Builder::new();
    let mesh = b.grid_mesh("box", 12, None);
    b.node("box", mesh, 0.0);
    b
}

#[test]
fn ac_005_overrun_json_chunk_is_gltf_001() {
    let r = run(&fixtures().join("GLTF-001/bad/overrun-json.glb"));
    let found = gltf001(&r);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("JSON chunk says it is"), "{}", found[0]);
    assert_eq!(r.findings[0].severity, meshdiet_core::Severity::High);
    assert_eq!(r.findings[0].measured.value, 1476);
}

#[test]
fn broken_files_never_stop_the_scan() {
    let r = run(&fixtures().join("GLTF-001/bad"));
    assert_eq!(r.assets.len(), 7);
    let by_path: Vec<(&str, &str)> = r
        .findings
        .iter()
        .map(|f| (f.path.as_str(), f.message.as_str()))
        .collect();
    let message = |p: &str| by_path.iter().find(|(path, _)| *path == p).unwrap().1;
    assert_eq!(message("empty.glb"), "the file is empty");
    assert!(message("truncated.glb").contains("cut off"));
    assert!(message("bad-magic.glb").contains("`glTX`"));
    assert!(message("broken.gltf").contains("cannot be read"));
    assert!(message("missing-bin.gltf").contains("`missing-bin.bin`, which does not exist"));
    assert!(message("view-past-end.glb").contains("past the end of buffer 0"));
}

#[test]
fn unknown_extensions_are_fine() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc: Value = serde_json::from_slice(&cube().gltf_embedded()).unwrap();
    doc["extensionsUsed"] = json!(["EXT_made_up", "VENDOR_other"]);
    doc["extensions"] = json!({ "EXT_made_up": { "anything": [1, 2, 3] } });
    doc["meshes"][0]["primitives"][0]["extensions"] = json!({ "VENDOR_other": { "x": true } });
    doc["bufferViews"][0]["extensions"] = json!({ "VENDOR_other": {} });
    let path = dir.path().join("ext.gltf");
    fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let r = run(&path);
    assert!(gltf001(&r).is_empty(), "{:?}", gltf001(&r));
    assert_eq!(r.assets[0].triangles, 12);
}

#[test]
fn crlf_and_bom_in_gltf_json_are_fine() {
    let dir = tempfile::tempdir().unwrap();
    let text = String::from_utf8(cube().gltf_embedded()).unwrap();
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    let path = dir.path().join("windows.gltf");
    fs::write(&path, bytes).unwrap();
    let r = run(&path);
    assert!(gltf001(&r).is_empty(), "{:?}", gltf001(&r));
}

#[test]
fn non_utf8_and_escaped_uris_are_resolved_or_reported() {
    let dir = tempfile::tempdir().unwrap();
    let files = cube().gltf_external("model");
    let mut doc: Value = serde_json::from_slice(&files[0].1).unwrap();
    // an escaped name that exists, and an image whose name is not UTF-8
    doc["buffers"][0]["uri"] = json!("my%20model.bin");
    doc["images"] = json!([{ "uri": "%FF%FEtexture.png" }]);
    fs::write(dir.path().join("my model.bin"), &files[1].1).unwrap();
    fs::write(
        dir.path().join("model.gltf"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    let r = run(dir.path());
    let found = gltf001(&r);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("%FF%FEtexture.png"), "{}", found[0]);
    assert_eq!(r.assets[0].files, ["model.gltf", "my model.bin"]);
}

#[test]
fn json_that_is_not_gltf_shaped_is_gltf_001() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.gltf"), r#"{"meshes": "nope"}"#).unwrap();
    fs::write(
        dir.path().join("b.gltf"),
        r#"{"asset": {"version": "1.0"}}"#,
    )
    .unwrap();
    let r = run(dir.path());
    assert_eq!(gltf001(&r).len(), 2);
}

#[test]
fn triangles_count_every_node_that_draws_a_mesh() {
    assert_eq!(
        one(&fixtures().join("MESH-002/bad/crowd.glb")).triangles,
        505_000
    );
    assert_eq!(
        one(&fixtures().join("MESH-002/good/crowd.glb")).triangles,
        495_000
    );
}

#[test]
fn cyclic_node_graph_does_not_hang() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc: Value = serde_json::from_slice(&cube().gltf_embedded()).unwrap();
    doc["nodes"] = json!([{ "mesh": 0, "children": [1] }, { "mesh": 0, "children": [0] }]);
    let path = dir.path().join("loop.gltf");
    fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert_eq!(one(&path).triangles, 24);
}

#[test]
fn geometry_and_texture_bytes_are_split() {
    let dense = one(&fixtures().join("MESH-001/bad/dense.glb"));
    assert!(dense.geometry_bytes > 1_000_000 && dense.geometry_bytes < dense.bytes);
    assert_eq!(dense.texture_bytes, 0);
    assert_eq!(dense.bytes, dense.geometry_bytes + dense.other_bytes);
    assert!(!dense.compression.meshopt && !dense.compression.draco);

    let poster = one(&fixtures().join("TEX-001/bad/poster.glb"));
    let img = &poster.images[0];
    assert_eq!((img.width, img.height), (Some(4096), Some(4096)));
    assert_eq!(img.mime.as_deref(), Some("image/png"));
    let vram = img.est_vram.unwrap();
    assert_eq!((vram.bytes, vram.estimated), (89_478_485, true));
    assert_eq!(poster.texture_bytes, img.bytes);
}

#[test]
fn compressed_files_from_real_tools_are_recognised() {
    let meshopt = one(&fixtures().join("MESH-001/good/dense-meshopt.glb"));
    assert!(meshopt.compression.meshopt && meshopt.compression.quantized);
    assert_eq!(meshopt.triangles, 48_000);
    assert!(
        meshopt.geometry_bytes < 200_000,
        "{}",
        meshopt.geometry_bytes
    );

    let draco = one(&fixtures().join("MESH-001/good/dense-draco.glb"));
    assert!(draco.compression.draco && !draco.compression.meshopt);
    assert_eq!(draco.triangles, 48_000);
    assert!(draco.geometry_bytes > 0 && draco.geometry_bytes < 50_000);

    let ktx2 = one(&fixtures().join("TEX-002/good/two-textures-ktx2.glb"));
    assert!(ktx2.compression.ktx2);
    for img in &ktx2.images {
        assert_eq!(img.mime.as_deref(), Some("image/ktx2"));
        assert_eq!((img.width, img.height), (Some(2048), Some(2048)));
        assert!(img.supercompression.is_some());
    }
}

#[test]
fn external_and_embedded_images_are_read() {
    let lamp = one(&fixtures().join("demo-site/public/models/lamp.gltf"));
    assert_eq!(lamp.files, ["lamp.gltf", "lamp.bin", "lamp-shade.png"]);
    assert_eq!(lamp.images[0].file.as_deref(), Some("lamp-shade.png"));
    assert_eq!(lamp.images[0].width, Some(300));

    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let tex = Texture::png("tiny", 8, 4, false, Pixels::Solid([1, 2, 3, 255]));
    let m = b.textured_material("m", &tex, ImageStore::BufferView);
    let mesh = b.grid_mesh("q", 2, Some(m));
    b.node("q", mesh, 0.0);
    let path = dir.path().join("inline.gltf");
    fs::write(&path, b.gltf_embedded()).unwrap();
    let a = one(&path);
    assert_eq!((a.images[0].width, a.images[0].height), (Some(8), Some(4)));
}

#[test]
fn shared_files_count_once_in_totals() {
    let dir = tempfile::tempdir().unwrap();
    let files = cube().gltf_external("shared");
    let bin_len = files[1].1.len() as u64;
    let gltf_len = files[0].1.len() as u64;
    fs::write(dir.path().join("shared.bin"), &files[1].1).unwrap();
    fs::write(dir.path().join("a.gltf"), &files[0].1).unwrap();
    fs::write(dir.path().join("b.gltf"), &files[0].1).unwrap();
    let r = run(dir.path());
    assert_eq!(r.assets.len(), 2);
    assert!(r.assets.iter().all(|a| a.bytes == gltf_len + bin_len));
    assert_eq!(r.totals.files, 3);
    assert_eq!(r.totals.bytes, 2 * gltf_len + bin_len);
}

#[test]
fn walker_skips_dependencies_but_scans_an_explicit_ignored_folder() {
    let dir = tempfile::tempdir().unwrap();
    let glb = cube().glb();
    for rel in [
        "public/a.glb",
        "node_modules/pkg/b.glb",
        "dist/c.GLB",
        "ignored/d.glb",
    ] {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &glb).unwrap();
    }
    fs::write(dir.path().join(".gitignore"), "dist/\nignored/\n").unwrap();
    let paths: Vec<String> = run(dir.path()).assets.into_iter().map(|a| a.path).collect();
    assert_eq!(paths, ["public/a.glb"]);
    let dist: Vec<String> = run(&dir.path().join("dist"))
        .assets
        .into_iter()
        .map(|a| a.path)
        .collect();
    assert_eq!(dist, ["c.GLB"]);
}

#[test]
fn missing_path_is_an_error() {
    let err = scan(&[PathBuf::from("/no/such/folder")], &Options::default()).unwrap_err();
    assert!(err.contains("does not exist"));
}
