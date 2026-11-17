//! Loading rule LOAD-001 (SPEC-RULE-001 FR-011, AC-006): compressed models against the decoder
//! wiring found in the app's sources. Models are the committed gltfpack fixtures; the sources are
//! written at test time.

#![allow(
    clippy::unwrap_used,
    reason = "test setup; a failure should stop the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use meshdiet_core::{Confidence, Finding, Options, Severity, scan};
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

const MESHOPT: &str = "LOAD-001/good/models/chair.glb";
const DRACO: &str = "MESH-001/good/dense-draco.glb";
const KTX2: &str = "TEX-002/good/two-textures-ktx2.glb";

/// A project folder: `models` copied from the fixtures into `models/`, `sources` as `src/<name>`.
fn project(models: &[&str], sources: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("models")).unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    for m in models {
        let name = Path::new(m).file_name().unwrap();
        fs::copy(fixtures().join(m), dir.path().join("models").join(name)).unwrap();
    }
    for (name, text) in sources {
        fs::write(dir.path().join("src").join(name), text).unwrap();
    }
    dir
}

fn load_001_with(path: &Path, options: &Options) -> Vec<Finding> {
    scan(&[path.to_path_buf()], options)
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.rule == "LOAD-001")
        .collect()
}

fn load_001(path: &Path) -> Vec<Finding> {
    load_001_with(path, &Options::default())
}

const THREE: &str = "import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';\n\
                     const loader = new GLTFLoader();\n";

#[test]
fn ac_006_meshopt_without_decoder_is_load_001() {
    let wired = format!(
        "{THREE}import {{ MeshoptDecoder }} from 'three/addons/libs/meshopt_decoder.module.js';\n\
         loader.setMeshoptDecoder(MeshoptDecoder);\n"
    );
    let dir = project(&[MESHOPT], &[("viewer.js", &wired)]);
    assert_eq!(load_001(dir.path()), []);

    let dir = project(&[MESHOPT], &[("viewer.js", THREE)]);
    let found = load_001(dir.path());
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.path, "models/chair.glb");
    assert_eq!(f.severity, Severity::Medium);
    assert_eq!(f.confidence, Confidence::Medium);
    assert_eq!(f.measured.value, 1);
    assert!(f.message.contains("meshopt"), "{}", f.message);
    assert!(f.fix.contains("setMeshoptDecoder"), "{}", f.fix);
}

#[test]
fn drei_use_gltf_wires_draco_and_meshopt_but_not_ktx2() {
    let drei = "import { useGLTF } from '@react-three/drei';\n\
                export function Chair() { const { scene } = useGLTF('/chair.glb'); return null; }\n";
    let dir = project(&[MESHOPT, DRACO, KTX2], &[("Chair.tsx", drei)]);
    let found = load_001(dir.path());
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].path, "models/two-textures-ktx2.glb");
    assert!(found[0].message.contains("KTX2"), "{}", found[0].message);
    assert!(found[0].fix.contains("extendLoader"), "{}", found[0].fix);
}

#[test]
fn one_finding_per_missing_decoder() {
    let dir = project(&[MESHOPT, DRACO, KTX2], &[("main.js", THREE)]);
    let mut found: Vec<String> = load_001(dir.path())
        .into_iter()
        .map(|f| f.measured.name)
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            "assets needing the Draco decoder",
            "assets needing the KTX2 decoder",
            "assets needing the meshopt decoder"
        ]
    );
}

#[test]
fn wiring_in_html_or_another_file_counts() {
    let html = "<script type=\"module\" src=\"https://unpkg.com/@google/model-viewer/dist/model-viewer.min.js\"></script>\n\
                <model-viewer src=\"models/chair.glb\"></model-viewer>\n";
    let dir = project(&[MESHOPT, DRACO, KTX2], &[("index.html", html)]);
    assert_eq!(load_001(dir.path()), []);

    let split = [
        ("load.js", THREE),
        (
            "decoders.ts",
            "loader.setDRACOLoader(draco);\nloader.setKTX2Loader(ktx2);\n",
        ),
    ];
    let dir = project(&[DRACO, KTX2], &split);
    assert_eq!(load_001(dir.path()), []);
}

#[test]
fn babylon_projects_are_skipped() {
    let babylon = "import '@babylonjs/loaders/glTF';\n\
                   import { SceneLoader } from '@babylonjs/core';\n\
                   SceneLoader.ImportMeshAsync('', '/models/', 'chair.glb', scene);\n";
    let dir = project(&[MESHOPT, DRACO, KTX2], &[("main.ts", babylon)]);
    assert_eq!(load_001(dir.path()), []);
}

#[test]
fn no_sources_or_an_unknown_loader_says_nothing() {
    let dir = project(&[MESHOPT], &[]);
    assert_eq!(load_001(dir.path()), []);
    let other = "app.assets.loadFromUrl('models/chair.glb', 'container', onLoad);\n";
    let dir = project(&[MESHOPT], &[("app.js", other)]);
    assert_eq!(load_001(dir.path()), []);
}

#[test]
fn node_modules_and_ignored_files_are_not_read() {
    let dir = project(&[MESHOPT], &[("main.js", THREE)]);
    let vendored = dir.path().join("node_modules/three/examples/jsm/libs");
    fs::create_dir_all(&vendored).unwrap();
    fs::write(
        vendored.join("meshopt_decoder.module.js"),
        "export const MeshoptDecoder = {};",
    )
    .unwrap();
    fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
    fs::create_dir_all(dir.path().join("dist")).unwrap();
    fs::write(
        dir.path().join("dist/bundle.js"),
        "loader.setMeshoptDecoder(MeshoptDecoder);",
    )
    .unwrap();
    assert_eq!(load_001(dir.path()).len(), 1);
}

#[test]
fn src_points_at_sources_outside_the_scanned_folder() {
    let dir = project(&[MESHOPT], &[("main.js", THREE)]);
    let models = dir.path().join("models");
    // scanning only the models folder finds no sources, so the rule stays quiet
    assert_eq!(load_001(&models), []);
    let options = Options {
        src: Some(dir.path().join("src")),
        ..Options::default()
    };
    let found = load_001_with(&models, &options);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].path, "chair.glb");
}

#[test]
fn up_to_five_assets_are_named() {
    let dir = project(&[], &[("main.js", THREE)]);
    for i in 0..7 {
        fs::copy(
            fixtures().join(MESHOPT),
            dir.path().join(format!("models/chair-{i}.glb")),
        )
        .unwrap();
    }
    let found = load_001(dir.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].measured.value, 7);
    assert!(
        found[0].message.ends_with("models/chair-4.glb and 2 more"),
        "{}",
        found[0].message
    );
}
