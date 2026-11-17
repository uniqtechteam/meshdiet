//! Which decoder wiring the app's own JS, TS and HTML mention, for LOAD-001 (SPEC-RULE-001 FR-011).
//!
//! Built once per scan by plain token search (no JS parser): a token in a comment counts too, which
//! can only hide a finding, never invent one.

use std::fs;
use std::path::Path;

use ignore::WalkBuilder;
use memchr::memmem;

use crate::walk::SKIP_DIRS;

/// File types read for wiring.
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "html", "htm", "vue", "svelte", "astro",
];

/// Larger files are skipped; app code is never this big, vendored bundles are.
const SOURCE_MAX_BYTES: u64 = 8_000_000;

/// A decoder an asset can need from the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Decoder {
    /// `KHR_draco_mesh_compression`.
    Draco,
    /// `EXT_meshopt_compression` or `KHR_meshopt_compression`.
    Meshopt,
    /// `KHR_texture_basisu` (KTX2 textures).
    Ktx2,
}

impl Decoder {
    /// Every decoder, in report order.
    pub const ALL: [Self; 3] = [Self::Draco, Self::Meshopt, Self::Ktx2];

    /// Tokens that count as wiring this decoder (SPEC-RULE-001 FR-011). `useGLTF` is drei, which
    /// turns on Draco and meshopt by default but not KTX2.
    #[must_use]
    pub fn tokens(self) -> &'static [&'static str] {
        match self {
            Self::Draco => &[
                "setDRACOLoader",
                "DRACOLoader",
                "useGLTF",
                "<model-viewer",
                "@google/model-viewer",
            ],
            Self::Meshopt => &[
                "setMeshoptDecoder",
                "MeshoptDecoder",
                "useGLTF",
                "<model-viewer",
                "@google/model-viewer",
            ],
            Self::Ktx2 => &[
                "setKTX2Loader",
                "KTX2Loader",
                "<model-viewer",
                "@google/model-viewer",
            ],
        }
    }
}

/// Tokens that show the app loads glTF with three.js (directly, through drei, or through
/// `<model-viewer>`, which is built on three.js). Without one, LOAD-001 cannot tell how the models
/// are loaded and stays quiet.
const LOADER_TOKENS: &[&str] = &[
    "GLTFLoader",
    "useGLTF",
    "<model-viewer",
    "@google/model-viewer",
];

/// Babylon.js fetches its Draco, meshopt and KTX2 decoders from its CDN by itself.
const BABYLON_TOKENS: &[&str] = &["@babylonjs/", "cdn.babylonjs.com"];

/// What the sources under one folder mention.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceIndex {
    /// Source files read.
    pub files: usize,
    /// A three.js-family glTF loader appears somewhere.
    pub loader: bool,
    /// Babylon.js is used.
    pub babylon: bool,
    wired: [bool; 3],
}

impl SourceIndex {
    /// Read every JS/TS/HTML-like file under `root`, with the same walk rules as the asset walk:
    /// `.gitignore` respected, `node_modules`, `.git` and `target` skipped, no symlinks followed.
    #[must_use]
    pub fn build(root: &Path) -> Self {
        let mut index = Self::default();
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .parents(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|e| {
                !(e.depth() > 0
                    && e.file_type().is_some_and(|t| t.is_dir())
                    && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
            })
            .build();
        for entry in walker.flatten() {
            let path = entry.path();
            let is_source = entry.file_type().is_some_and(|t| t.is_file())
                && path
                    .extension()
                    .is_some_and(|e| SOURCE_EXTENSIONS.iter().any(|s| e.eq_ignore_ascii_case(s)));
            if !is_source
                || entry
                    .metadata()
                    .map_or(true, |m| m.len() > SOURCE_MAX_BYTES)
            {
                continue;
            }
            if let Ok(text) = fs::read(path) {
                index.add(&text);
            }
        }
        index
    }

    /// The sources LOAD-001 reads for a scan of `root`: `src` when given. Otherwise the scan root;
    /// when that has no three.js loader (a scan of `public/` or `static/`, which hold no app code),
    /// the nearest folder above it with a `package.json` instead, without leaving the git
    /// repository.
    #[must_use]
    pub fn for_scan(root: &Path, src: Option<&Path>) -> Self {
        if let Some(src) = src {
            return Self::build(src);
        }
        let here = Self::build(root);
        if here.loader {
            return here;
        }
        match project_root(root) {
            Some(project) => Self::build(&project),
            None => here,
        }
    }

    /// Add one source file's text.
    pub fn add(&mut self, text: &[u8]) {
        let has = |tokens: &[&str]| {
            tokens
                .iter()
                .any(|t| memmem::find(text, t.as_bytes()).is_some())
        };
        self.files += 1;
        self.loader |= has(LOADER_TOKENS);
        self.babylon |= has(BABYLON_TOKENS);
        for (slot, decoder) in self.wired.iter_mut().zip(Decoder::ALL) {
            *slot |= has(decoder.tokens());
        }
    }

    /// Whether some source wires `decoder`.
    #[must_use]
    pub fn wires(&self, decoder: Decoder) -> bool {
        Decoder::ALL
            .iter()
            .position(|d| *d == decoder)
            .is_some_and(|i| self.wired[i])
    }

    /// Whether LOAD-001 can judge these sources: there are some, they load glTF with three.js, and
    /// they are not a Babylon.js app.
    #[must_use]
    pub fn applies(&self) -> bool {
        self.files > 0 && self.loader && !self.babylon
    }
}

/// The nearest folder strictly above `dir` that has a `package.json`, stopping at the folder that
/// holds `.git`.
fn project_root(dir: &Path) -> Option<std::path::PathBuf> {
    let dir = dir.canonicalize().ok()?;
    if dir.join(".git").exists() {
        return None;
    }
    for parent in dir.ancestors().skip(1) {
        if parent.join("package.json").is_file() {
            return Some(parent.to_path_buf());
        }
        if parent.join(".git").exists() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(sources: &[&str]) -> SourceIndex {
        let mut index = SourceIndex::default();
        for s in sources {
            index.add(s.as_bytes());
        }
        index
    }

    #[test]
    fn drei_use_gltf_wires_draco_and_meshopt_but_not_ktx2() {
        let i = index(&["import { useGLTF } from '@react-three/drei';\nuseGLTF('/chair.glb');"]);
        assert!(i.applies());
        assert!(i.wires(Decoder::Draco));
        assert!(i.wires(Decoder::Meshopt));
        assert!(!i.wires(Decoder::Ktx2));
    }

    #[test]
    fn model_viewer_wires_everything() {
        let i = index(&["<model-viewer src=\"chair.glb\"></model-viewer>"]);
        assert!(i.applies());
        assert!(Decoder::ALL.iter().all(|d| i.wires(*d)));
    }

    #[test]
    fn wiring_in_another_file_counts() {
        let i = index(&[
            "const loader = new GLTFLoader();",
            "loader.setKTX2Loader(ktx2);",
        ]);
        assert!(i.wires(Decoder::Ktx2));
        assert!(!i.wires(Decoder::Meshopt));
    }

    #[test]
    fn babylon_and_unknown_loaders_are_skipped() {
        assert!(!index(&["import '@babylonjs/loaders/glTF';"]).applies());
        assert!(
            !index(&[
                "import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';",
                "import '@babylonjs/core';"
            ])
            .applies()
        );
        assert!(!index(&["app.assets.loadFromUrl('chair.glb', 'container');"]).applies());
        assert!(!SourceIndex::default().applies());
    }
}
