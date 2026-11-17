//! Models that ship next to their own compressed copy: `chair.glb` with `chair-compressed.glb`,
//! `chair.draco.glb` or gltfjsx's `chair-transformed.glb`. Build scripts keep the original as the
//! source and the app loads the copy, so the original's per-file findings would only repeat what
//! was already done (found in TASK-QA-001: four of six MESH-001 findings on one public site).

use std::collections::{BTreeMap, BTreeSet};

use crate::Asset;

/// Words build scripts and tools put after the name of a compressed copy.
const TAGS: &[&str] = &[
    "compressed",
    "draco",
    "meshopt",
    "ktx",
    "ktx2",
    "transformed",
    "optimized",
    "optimised",
    "opt",
    "packed",
    "min",
];

/// What goes between the name and the tag.
const SEPARATORS: &[char] = &['-', '_', '.'];

/// For each asset that has a compressed copy in the same folder, the copy's path.
#[must_use]
pub fn copies(assets: &[Asset]) -> BTreeMap<String, String> {
    let paths: BTreeSet<&str> = assets.iter().map(|a| a.path.as_str()).collect();
    let mut out = BTreeMap::new();
    for asset in assets {
        let Some((stem, _)) = asset.path.rsplit_once('.') else {
            continue;
        };
        let copy = SEPARATORS.iter().find_map(|sep| {
            TAGS.iter().find_map(|tag| {
                ["glb", "gltf"]
                    .iter()
                    .map(|ext| format!("{stem}{sep}{tag}.{ext}"))
                    .find(|p| paths.contains(p.as_str()))
            })
        });
        if let Some(copy) = copy {
            out.insert(asset.path.clone(), copy);
        }
    }
    out
}

/// The file name of a scan path (`models/chair.glb` → `chair.glb`).
#[must_use]
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetKind, Compression};

    fn asset(path: &str) -> Asset {
        Asset {
            path: path.to_owned(),
            kind: AssetKind::Glb,
            files: vec![path.to_owned()],
            bytes: 0,
            geometry_bytes: 0,
            texture_bytes: 0,
            other_bytes: 0,
            triangles: 0,
            primitives: 0,
            compression: Compression::default(),
            images: Vec::new(),
            compressed_copy: None,
            problems: Vec::new(),
            file_bytes: Vec::new(),
            document: None,
        }
    }

    #[test]
    fn copies_are_found_by_name_in_the_same_folder() {
        let assets: Vec<Asset> = [
            "areas/areas.glb",
            "areas/areas-compressed.glb",
            "chair.glb",
            "chair-transformed.glb",
            "lamp.gltf",
            "lamp.draco.glb",
            "table.glb",
            "other/table-compressed.glb",
            "tablet-compressed.glb",
        ]
        .into_iter()
        .map(asset)
        .collect();
        let found = copies(&assets);
        assert_eq!(
            found.into_iter().collect::<Vec<_>>(),
            vec![
                (
                    "areas/areas.glb".into(),
                    "areas/areas-compressed.glb".into()
                ),
                ("chair.glb".into(), "chair-transformed.glb".into()),
                ("lamp.gltf".into(), "lamp.draco.glb".into()),
            ]
        );
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("vehicle/default.glb"), "default.glb");
        assert_eq!(file_name("default.glb"), "default.glb");
    }
}
