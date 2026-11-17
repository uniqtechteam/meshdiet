//! The committed fixture set under `tests/fixtures/`, built from code.
//!
//! Each rule folder has `bad/` (must produce exactly the findings in `bad/expected.json`) and
//! `good/` (a near miss that must produce none of that rule). Files made by gltfpack or glTF
//! Transform are not built here; [`sources`] writes their inputs and `tests/fixtures/README.md` has the
//! commands.

use serde_json::json;

use crate::gltf::{Builder, File, ImageStore};
use crate::texture::{Pixels, Texture};

const GREY: Pixels = Pixels::Solid([180, 180, 180, 255]);
const WOOD: Pixels = Pixels::Solid([150, 110, 70, 255]);

/// Every generated fixture file, as a path under `tests/fixtures/` and its bytes.
#[must_use]
pub fn all() -> Vec<File> {
    let mut out = Vec::new();
    gltf_001(&mut out);
    gltf_002(&mut out);
    mesh_001(&mut out);
    mesh_002(&mut out);
    mesh_003(&mut out);
    tex_001(&mut out);
    tex_002(&mut out);
    tex_003(&mut out);
    tex_004(&mut out);
    load_001(&mut out);
    size_001(&mut out);
    demo_site(&mut out);
    out
}

/// Inputs for the compressed fixtures, which gltfpack and glTF Transform make from these files.
#[must_use]
pub fn sources() -> Vec<File> {
    vec![
        ("chair.glb".into(), chair().glb()),
        ("dense.glb".into(), dense().glb()),
        ("two-textures.glb".into(), two_textures().glb()),
    ]
}

fn add(out: &mut Vec<File>, dir: &str, files: Vec<File>) {
    out.extend(files.into_iter().map(|(p, b)| (format!("{dir}/{p}"), b)));
}

fn expected(out: &mut Vec<File>, dir: &str, rule: &str, paths: &[&str]) {
    let mut list: Vec<_> = paths
        .iter()
        .map(|p| json!({ "rule": rule, "path": p }))
        .collect();
    list.sort_by_key(ToString::to_string);
    let mut text = serde_json::to_vec_pretty(&list).unwrap_or_default();
    text.push(b'\n');
    out.push((format!("{dir}/expected.json"), text));
}

/// A 12-triangle box with a plain named material: the smallest sensible model.
fn cube() -> Builder {
    let mut b = Builder::new();
    let m = b.plain_material("paint");
    let mesh = b.grid_mesh("box", 12, Some(m));
    b.node("box", mesh, 0.0);
    b
}

fn textured(mesh_name: &str, triangles: u32, texture: &Texture, store: ImageStore) -> Builder {
    let mut b = Builder::new();
    let m = b.textured_material(&format!("{mesh_name}-material"), texture, store);
    let mesh = b.grid_mesh(mesh_name, triangles, Some(m));
    b.node(mesh_name, mesh, 0.0);
    b
}

/// About 1 MB of float32 geometry, no compression (SPEC-RULE-001 AC-001).
fn dense() -> Builder {
    let mut b = Builder::new();
    let m = b.plain_material("clay");
    let mesh = b.grid_mesh("dense", 48_000, Some(m));
    b.node("dense", mesh, 0.0);
    b
}

/// A small textured model with named nodes, the input for the meshopt chair.
fn chair() -> Builder {
    let tex = Texture::png("chair-wood", 256, 256, false, WOOD);
    let mut b = Builder::new();
    let m = b.textured_material("chair-wood", &tex, ImageStore::BufferView);
    for (i, part) in ["seat", "back", "legs"].into_iter().enumerate() {
        let mesh = b.grid_mesh(part, 4_000, Some(m));
        #[allow(clippy::cast_precision_loss, reason = "three parts")]
        b.node(part, mesh, i as f32);
    }
    b
}

/// Two 2048x2048 PNGs: about 44.7 MB of estimated GPU memory, no single one over the size limit.
fn two_textures() -> Builder {
    let mut b = Builder::new();
    for (i, name) in ["albedo", "roughness"].into_iter().enumerate() {
        let tex = Texture::png(name, 2048, 2048, true, Pixels::Solid([90, 120, 160, 255]));
        let m = b.textured_material(name, &tex, ImageStore::BufferView);
        let mesh = b.grid_mesh(name, 12, Some(m));
        #[allow(clippy::cast_precision_loss, reason = "two parts")]
        b.node(name, mesh, i as f32);
    }
    b
}

fn gltf_001(out: &mut Vec<File>) {
    let dir = "GLTF-001/bad";
    let mut overrun = cube().glb();
    let json_len = u32::from_le_bytes([overrun[12], overrun[13], overrun[14], overrun[15]]);
    overrun[12..16].copy_from_slice(&(json_len + 4096).to_le_bytes());
    let mut magic = cube().glb();
    magic[0..4].copy_from_slice(b"glTX");
    let mut truncated = cube().glb();
    truncated.truncate(truncated.len() / 2);
    let mut past_end = cube();
    past_end.patch_view(0, "byteLength", json!(1_000_000));
    let missing: Vec<File> = cube()
        .gltf_external("missing-bin")
        .into_iter()
        .filter(|(p, _)| p.as_str() == "missing-bin.gltf")
        .collect();
    add(
        out,
        dir,
        vec![
            ("overrun-json.glb".into(), overrun),
            ("bad-magic.glb".into(), magic),
            ("truncated.glb".into(), truncated),
            ("empty.glb".into(), Vec::new()),
            (
                "broken.gltf".into(),
                b"{\"asset\": {\"version\": \"2.0\"}, \"meshes\": [\n".to_vec(),
            ),
            ("view-past-end.glb".into(), past_end.glb()),
        ],
    );
    add(out, dir, missing);
    expected(
        out,
        dir,
        "GLTF-001",
        &[
            "bad-magic.glb",
            "broken.gltf",
            "empty.glb",
            "missing-bin.gltf",
            "overrun-json.glb",
            "truncated.glb",
            "view-past-end.glb",
        ],
    );
    let dir = "GLTF-001/good";
    let tex = Texture::png("crate", 64, 64, false, WOOD);
    add(
        out,
        dir,
        vec![(
            "crate.glb".into(),
            textured("crate", 12, &tex, ImageStore::BufferView).glb(),
        )],
    );
    add(
        out,
        dir,
        textured(
            "barrel",
            12,
            &Texture::png("barrel", 64, 64, true, GREY),
            ImageStore::File,
        )
        .gltf_external("barrel"),
    );
    add(out, dir, vec![("box.gltf".into(), cube().gltf_embedded())]);
}

fn gltf_002(out: &mut Vec<File>) {
    // ~100 KB of buffer as base64 (SPEC-RULE-001 AC-007), and a near miss under 16 KB
    let mut big = Builder::new();
    let mesh = big.grid_mesh("rock", 3_300, None);
    big.node("rock", mesh, 0.0);
    add(
        out,
        "GLTF-002/bad",
        vec![("rock.gltf".into(), big.gltf_embedded())],
    );
    expected(out, "GLTF-002/bad", "GLTF-002", &["rock.gltf"]);
    let mut small = Builder::new();
    let mesh = small.grid_mesh("pebble", 300, None);
    small.node("pebble", mesh, 0.0);
    add(
        out,
        "GLTF-002/good",
        vec![("pebble.gltf".into(), small.gltf_embedded())],
    );
}

fn mesh_001(out: &mut Vec<File>) {
    add(
        out,
        "MESH-001/bad",
        vec![("dense.glb".into(), dense().glb())],
    );
    expected(out, "MESH-001/bad", "MESH-001", &["dense.glb"]);
    // dense-meshopt.glb and dense-draco.glb are made by tools (README)
    add(out, "MESH-001/good", vec![("box.glb".into(), cube().glb())]);
}

fn crowd(instances: u16) -> Vec<u8> {
    let mut b = Builder::new();
    let m = b.plain_material("figure");
    let mesh = b.grid_mesh("figure", 5_000, Some(m));
    for i in 0..instances {
        b.node(&format!("figure-{i:02}"), mesh, f32::from(i));
    }
    b.glb()
}

fn mesh_002(out: &mut Vec<File>) {
    // one 5,000-triangle mesh drawn by 101 nodes = 505,000 triangles; 99 nodes = 495,000
    add(out, "MESH-002/bad", vec![("crowd.glb".into(), crowd(101))]);
    expected(out, "MESH-002/bad", "MESH-002", &["crowd.glb"]);
    add(out, "MESH-002/good", vec![("crowd.glb".into(), crowd(99))]);
}

fn mesh_003(out: &mut Vec<File>) {
    let mut bad = cube();
    bad.dead_bytes(200 * 1024);
    add(
        out,
        "MESH-003/bad",
        vec![("leftovers.glb".into(), bad.glb())],
    );
    expected(out, "MESH-003/bad", "MESH-003", &["leftovers.glb"]);
    // 6 KB dead in a ~76 KB buffer: under 64 KB and under 10%
    let mut good = Builder::new();
    let mesh = good.grid_mesh("table", 3_000, None);
    good.node("table", mesh, 0.0);
    good.dead_bytes(6 * 1024);
    add(out, "MESH-003/good", vec![("table.glb".into(), good.glb())]);
}

fn tex_001(out: &mut Vec<File>) {
    let big = Texture::png("poster", 4096, 4096, true, GREY);
    add(
        out,
        "TEX-001/bad",
        vec![(
            "poster.glb".into(),
            textured("poster", 12, &big, ImageStore::BufferView).glb(),
        )],
    );
    expected(out, "TEX-001/bad", "TEX-001", &["poster.glb"]);
    let ok = Texture::png("poster", 2048, 2048, true, GREY);
    add(
        out,
        "TEX-001/good",
        vec![(
            "poster.glb".into(),
            textured("poster", 12, &ok, ImageStore::BufferView).glb(),
        )],
    );
}

fn tex_002(out: &mut Vec<File>) {
    add(
        out,
        "TEX-002/bad",
        vec![("two-textures.glb".into(), two_textures().glb())],
    );
    expected(out, "TEX-002/bad", "TEX-002", &["two-textures.glb"]);
    // two-textures-ktx2.glb is made by gltfpack -tc (README); this one is 22.4 MB est., under 32 MB
    let one = Texture::png(
        "albedo",
        2048,
        2048,
        true,
        Pixels::Solid([90, 120, 160, 255]),
    );
    add(
        out,
        "TEX-002/good",
        vec![(
            "one-texture.glb".into(),
            textured("panel", 12, &one, ImageStore::BufferView).glb(),
        )],
    );
}

fn tex_003(out: &mut Vec<File>) {
    // ~400 KB PNGs of noise: RGB (should be JPEG or WebP) vs RGBA (alpha is used)
    let rgb = Texture::png("marble", 368, 368, false, Pixels::Noise(3));
    add(
        out,
        "TEX-003/bad",
        vec![(
            "marble.glb".into(),
            textured("marble", 12, &rgb, ImageStore::BufferView).glb(),
        )],
    );
    expected(out, "TEX-003/bad", "TEX-003", &["marble.glb"]);
    let rgba = Texture::png("glass", 320, 320, true, Pixels::Noise(4));
    add(
        out,
        "TEX-003/good",
        vec![(
            "glass.glb".into(),
            textured("glass", 12, &rgba, ImageStore::BufferView).glb(),
        )],
    );
}

fn tex_004(out: &mut Vec<File>) {
    // the hero's 1400x1400 JPEG is over 1 MB; the glass texture (~410 KB) is under
    add(out, "TEX-004/bad", vec![("hero.glb".into(), hero())]);
    expected(out, "TEX-004/bad", "TEX-004", &["hero.glb"]);
    let light = Texture::png("glass", 320, 320, true, Pixels::Noise(4));
    add(
        out,
        "TEX-004/good",
        vec![(
            "glass.glb".into(),
            textured("glass", 12, &light, ImageStore::BufferView).glb(),
        )],
    );
}

const VIEWER_DRACO_ONLY: &str = "import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { DRACOLoader } from 'three/addons/loaders/DRACOLoader.js';

const scene = new THREE.Scene();
const draco = new DRACOLoader();
draco.setDecoderPath('/draco/');

const loader = new GLTFLoader();
loader.setDRACOLoader(draco);
loader.load('/models/chair.glb', (gltf) => scene.add(gltf.scene));
";

const VIEWER_WITH_MESHOPT: &str = "import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { MeshoptDecoder } from 'three/addons/libs/meshopt_decoder.module.js';

const scene = new THREE.Scene();
const loader = new GLTFLoader();
loader.setMeshoptDecoder(MeshoptDecoder);
loader.load('/models/chair.glb', (gltf) => scene.add(gltf.scene));
";

fn load_001(out: &mut Vec<File>) {
    // models/chair.glb in both folders is the gltfpack output (README)
    add(
        out,
        "LOAD-001/bad",
        vec![("src/viewer.js".into(), VIEWER_DRACO_ONLY.into())],
    );
    expected(out, "LOAD-001/bad", "LOAD-001", &["models/chair.glb"]);
    add(
        out,
        "LOAD-001/good",
        vec![("src/viewer.js".into(), VIEWER_WITH_MESHOPT.into())],
    );
}

/// About 5.1 MB: uncompressed geometry and a large JPEG. Shared with the demo site.
fn hero() -> Vec<u8> {
    let photo = Texture::jpeg("hero-photo", 1400, 1400, 91, Pixels::Noise(7));
    let mut b = Builder::new();
    let m = b.textured_material("hero-paint", &photo, ImageStore::BufferView);
    let mesh = b.grid_mesh("hero", 60_000, Some(m));
    b.node("hero", mesh, 0.0);
    b.glb()
}

fn size_001(out: &mut Vec<File>) {
    add(out, "SIZE-001/bad", vec![("hero.glb".into(), hero())]);
    expected(out, "SIZE-001/bad", "SIZE-001", &["hero.glb"]);
    add(
        out,
        "SIZE-001/good",
        vec![("dense.glb".into(), dense().glb())],
    );
}

const DEMO_HTML: &str = "<!doctype html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <title>Demo shop</title>
    <script type=\"importmap\">
      { \"imports\": { \"three\": \"https://cdn.jsdelivr.net/npm/three@0.181.0/build/three.module.js\",
                       \"three/addons/\": \"https://cdn.jsdelivr.net/npm/three@0.181.0/examples/jsm/\" } }
    </script>
  </head>
  <body>
    <canvas id=\"view\"></canvas>
    <script type=\"module\" src=\"/src/main.js\"></script>
  </body>
</html>
";

const DEMO_JS: &str = "import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { DRACOLoader } from 'three/addons/loaders/DRACOLoader.js';

const renderer = new THREE.WebGLRenderer({ canvas: document.getElementById('view') });
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(50, innerWidth / innerHeight, 0.1, 100);
camera.position.set(0, 1.5, 4);

const draco = new DRACOLoader();
draco.setDecoderPath('/draco/');
const loader = new GLTFLoader();
loader.setDRACOLoader(draco);

for (const name of ['hero.glb', 'chair.glb', 'sofa.glb', 'lamp.gltf']) {
  loader.load(`/models/${name}`, (gltf) => scene.add(gltf.scene));
}

renderer.setAnimationLoop(() => renderer.render(scene, camera));
";

const DEMO_PLANTED: &str = "# demo-site

A tiny three.js shop page with generated models (`cargo xtask gen-fixtures`). Problems planted on purpose:

| File | Problem | Rule |
|---|---|---|
| `public/models/hero.glb` | 5.1 MB: uncompressed geometry and a 3.8 MB JPEG | SIZE-001, MESH-001 |
| `public/models/sofa.glb` | 4096x4096 PNG texture | TEX-001, TEX-002 |
| `public/models/lamp.gltf` | external `.bin` and a 270 KB RGB PNG that has no alpha | TEX-003 |
| `public/models/chair.glb` | meshopt-compressed, but `src/main.js` never calls `setMeshoptDecoder` | LOAD-001 |
";

fn demo_site(out: &mut Vec<File>) {
    let dir = "demo-site";
    let sofa_tex = Texture::png(
        "sofa-fabric",
        4096,
        4096,
        false,
        Pixels::Solid([120, 60, 60, 255]),
    );
    let lamp_tex = Texture::png("lamp-shade", 300, 300, false, Pixels::Noise(8));
    add(
        out,
        dir,
        vec![
            ("index.html".into(), DEMO_HTML.into()),
            ("src/main.js".into(), DEMO_JS.into()),
            ("README.md".into(), DEMO_PLANTED.into()),
            ("public/models/hero.glb".into(), hero()),
            (
                "public/models/sofa.glb".into(),
                textured("sofa", 2_000, &sofa_tex, ImageStore::BufferView).glb(),
            ),
        ],
    );
    add(
        out,
        &format!("{dir}/public/models"),
        textured("lamp", 1_500, &lamp_tex, ImageStore::File).gltf_external("lamp"),
    );
    // public/models/chair.glb is the gltfpack output (README)
}
