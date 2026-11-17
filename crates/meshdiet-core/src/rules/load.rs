//! Loading rule LOAD-001 (SPEC-RULE-001 FR-011): the models are compressed, but the app's source
//! never sets up the decoder three.js needs to open them. Unlike the asset rules it looks at the
//! whole scan at once, together with a [`SourceIndex`].

use std::fmt::Write as _;

use crate::rule::RuleInfo;
use crate::source_index::{Decoder, SourceIndex};
use crate::{Asset, Confidence, Finding, Measured, Severity};

/// How many assets a finding names.
const NAMED_MAX: usize = 5;

/// LOAD-001 rule info.
pub static LOAD_001: RuleInfo = RuleInfo {
    id: "LOAD-001",
    title: "Decoder not wired",
    severity: Severity::Medium,
    confidence: Confidence::Medium,
    summary: "models use Draco, meshopt or KTX2, but the app source never sets up that decoder",
};

struct Text {
    name: &'static str,
    looked_for: &'static str,
    why: &'static str,
    fix: &'static str,
}

fn text(decoder: Decoder) -> Text {
    match decoder {
        Decoder::Draco => Text {
            name: "Draco",
            looked_for: "DRACOLoader",
            why: "three.js GLTFLoader stops with `No DRACOLoader instance provided`, so these \
                  models never show up.",
            fix: "Add `const draco = new DRACOLoader(); draco.setDecoderPath('/draco/'); \
                  loader.setDRACOLoader(draco);` (three/addons/loaders/DRACOLoader.js), or load \
                  with drei `useGLTF`. If the app code is outside the scanned folder, pass it with \
                  `--src`.",
        },
        Decoder::Meshopt => Text {
            name: "meshopt",
            looked_for: "MeshoptDecoder",
            why: "three.js GLTFLoader stops with `setMeshoptDecoder must be called before loading \
                  compressed files` (or reads an uncompressed fallback, when the file has one), so \
                  the model breaks or the compression is wasted.",
            fix: "Add `import { MeshoptDecoder } from \
                  'three/addons/libs/meshopt_decoder.module.js'` and \
                  `loader.setMeshoptDecoder(MeshoptDecoder)`, or load with drei `useGLTF`. If the \
                  app code is outside the scanned folder, pass it with `--src`.",
        },
        Decoder::Ktx2 => Text {
            name: "KTX2",
            looked_for: "KTX2Loader",
            why: "three.js GLTFLoader stops with `setKTX2Loader must be called before loading \
                  KTX2 textures`, so these models never show up.",
            fix: "Add `const ktx2 = new KTX2Loader().setTranscoderPath('/basis/')\
                  .detectSupport(renderer); loader.setKTX2Loader(ktx2);` \
                  (three/addons/loaders/KTX2Loader.js); drei `useGLTF` needs it too, through its \
                  `extendLoader` argument. If the app code is outside the scanned folder, pass it \
                  with `--src`.",
        },
    }
}

fn needs(asset: &Asset, decoder: Decoder) -> bool {
    let c = asset.compression;
    match decoder {
        Decoder::Draco => c.draco,
        Decoder::Meshopt => c.meshopt,
        Decoder::Ktx2 => c.ktx2,
    }
}

/// Whether `asset` needs any decoder from the app, so the sources are worth reading.
#[must_use]
pub fn needs_decoder(asset: &Asset) -> bool {
    Decoder::ALL.iter().any(|d| needs(asset, *d))
}

/// One finding per decoder the assets need and the sources never wire. `assets` are sorted by
/// path, so the finding sits on the first asset that needs it.
#[must_use]
pub fn check(assets: &[Asset], sources: &SourceIndex) -> Vec<Finding> {
    if !sources.applies() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for decoder in Decoder::ALL {
        if sources.wires(decoder) {
            continue;
        }
        let needing: Vec<&str> = assets
            .iter()
            .filter(|a| needs(a, decoder))
            .map(|a| a.path.as_str())
            .collect();
        let Some(first) = needing.first() else {
            continue;
        };
        let t = text(decoder);
        let mut named = needing
            .iter()
            .take(NAMED_MAX)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        if needing.len() > NAMED_MAX {
            let _ = write!(named, " and {} more", needing.len() - NAMED_MAX);
        }
        let files = sources.files;
        out.push(Finding {
            rule: LOAD_001.id.to_owned(),
            title: LOAD_001.title.to_owned(),
            severity: LOAD_001.severity,
            confidence: LOAD_001.confidence,
            path: (*first).to_owned(),
            measured: Measured::exact(
                &format!("assets needing the {} decoder", t.name),
                needing.len() as u64,
                "assets",
            ),
            threshold: None,
            message: format!(
                "{} {}-compressed {}, but none of the {files} source {} mention {}: {named}",
                needing.len(),
                t.name,
                if needing.len() == 1 {
                    "model"
                } else {
                    "models"
                },
                if files == 1 { "file" } else { "files" },
                t.looked_for,
            ),
            why: t.why.to_owned(),
            fix: t.fix.to_owned(),
            fingerprint: String::new(),
        });
    }
    out
}
