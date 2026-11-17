//! Build and page weight (SPEC-WEIGHT-001): totals over every scanned asset, page groups from
//! `[pages]`, and the optional gzip transfer estimate. A file shared by several assets counts once
//! (BR-001).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;

use crate::config::PageGroup;
use crate::rule::Rule;
use crate::rules::mesh::UncompressedGeometry;
use crate::{Asset, Config, Estimate};

/// Totals over the whole scan. A file shared by several assets counts once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Totals {
    /// Assets scanned.
    pub assets: usize,
    /// Distinct files (assets plus their external buffers and images).
    pub files: usize,
    /// Bytes of those files on disk.
    pub bytes: u64,
    /// Geometry bytes; a `.bin` shared by several `.gltf` counts once.
    pub geometry_bytes: u64,
    /// Image bytes; an image file shared by several assets counts once.
    pub texture_bytes: u64,
    /// gzip level 6 size of every file, with `--gzip` only. Servers may use brotli or nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gzip: Option<Estimate>,
    /// Estimated GPU memory of every texture, with mipmaps.
    pub est_vram: Estimate,
    /// Assets MESH-001 flags (geometry with no Draco or meshopt).
    pub uncompressed_assets: usize,
    /// Bytes of those assets, shared files once.
    pub uncompressed_bytes: u64,
}

impl Default for Totals {
    fn default() -> Self {
        Self {
            assets: 0,
            files: 0,
            bytes: 0,
            geometry_bytes: 0,
            texture_bytes: 0,
            gzip: None,
            est_vram: Estimate {
                bytes: 0,
                estimated: true,
            },
            uncompressed_assets: 0,
            uncompressed_bytes: 0,
        }
    }
}

/// One page group from `[pages]`, or `(no page)` for assets in none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PageTotal {
    /// The page name, for example `/shop`.
    pub name: String,
    /// Asset paths in the group, largest first.
    pub assets: Vec<String>,
    /// Bytes of their files, shared files once.
    pub bytes: u64,
    /// gzip level 6 estimate, with `--gzip` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gzip: Option<Estimate>,
    /// The page budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
}

/// Name of the group of assets that no page lists.
pub const NO_PAGE: &str = "(no page)";

/// Bytes in decimal units (1 MB = 1,000,000 bytes), one decimal: `49.8 MB`, `812 B`.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "one decimal of a byte count; f64 is exact far beyond any real file"
)]
pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", b / 1e3)
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.1} GB", b / 1e9)
    }
}

/// A page glob: `*` stays inside one folder, `**` crosses folders.
pub(crate) fn glob(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| e.kind().to_string())
}

/// Distinct files of `assets` and their sizes.
fn distinct_files<'a>(assets: impl IntoIterator<Item = &'a Asset>) -> BTreeMap<&'a str, u64> {
    let mut files = BTreeMap::new();
    for a in assets {
        for (path, bytes) in a.files.iter().zip(&a.file_bytes) {
            files.insert(path.as_str(), *bytes);
        }
    }
    files
}

/// Bytes of `assets`, each distinct file once.
#[must_use]
pub fn distinct_bytes<'a>(assets: impl IntoIterator<Item = &'a Asset>) -> u64 {
    distinct_files(assets).values().sum()
}

/// True when MESH-001 flags the asset: a primitive with no Draco or meshopt, in an asset with at
/// least `compress_min` of such geometry.
#[must_use]
pub fn has_uncompressed_geometry(asset: &Asset, config: &Config) -> bool {
    !UncompressedGeometry.check(asset, config).is_empty()
}

/// Estimated GPU memory of one asset's textures.
#[must_use]
pub fn asset_vram(asset: &Asset) -> u64 {
    asset
        .images
        .iter()
        .filter_map(|i| i.est_vram)
        .map(|e| e.bytes)
        .sum()
}

/// Totals over `assets` (FR-002), shared files once (BR-001). `gzip` is filled in by the scan.
#[must_use]
pub fn totals(assets: &[Asset], config: &Config) -> Totals {
    let files = distinct_files(assets);
    // An image in its own file counts once however many assets use it; embedded ones per asset.
    let mut images: BTreeMap<(&str, usize), (u64, u64)> = BTreeMap::new();
    for a in assets {
        for i in &a.images {
            let key = i
                .file
                .as_deref()
                .map_or((a.path.as_str(), i.index), |f| (f, 0));
            images.insert(key, (i.bytes, i.est_vram.map_or(0, |e| e.bytes)));
        }
    }
    // Geometry lives in the asset's buffers. Assets that read the same external buffers share
    // that geometry, so the group counts its largest figure once.
    let mut geometry: BTreeMap<Vec<&str>, u64> = BTreeMap::new();
    for a in assets {
        let image_files: BTreeSet<&str> =
            a.images.iter().filter_map(|i| i.file.as_deref()).collect();
        let mut buffers: Vec<&str> = a
            .files
            .iter()
            .skip(1)
            .map(String::as_str)
            .filter(|f| !image_files.contains(f))
            .collect();
        if buffers.is_empty() {
            buffers.push(a.path.as_str());
        }
        buffers.sort_unstable();
        let g = geometry.entry(buffers).or_default();
        *g = (*g).max(a.geometry_bytes);
    }
    let uncompressed: Vec<&Asset> = assets
        .iter()
        .filter(|a| has_uncompressed_geometry(a, config))
        .collect();
    let bytes: u64 = files.values().sum();
    let texture_bytes: u64 = images.values().map(|(b, _)| b).sum();
    Totals {
        assets: assets.len(),
        files: files.len(),
        bytes,
        geometry_bytes: geometry
            .values()
            .sum::<u64>()
            .min(bytes.saturating_sub(texture_bytes)),
        texture_bytes,
        gzip: None,
        est_vram: Estimate {
            bytes: images.values().map(|(_, v)| v).sum(),
            estimated: true,
        },
        uncompressed_assets: uncompressed.len(),
        uncompressed_bytes: distinct_bytes(uncompressed),
    }
}

/// The ways a glob may name `path`: as shown, and with the trailing folders of the scan `root` in
/// front (`public/models/a.glb` and `repo/public/models/a.glb` for root `/home/me/repo/public`).
fn spellings(root: &str, path: &str) -> Vec<String> {
    let folders: Vec<&str> = root
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    let mut out = vec![path.to_owned()];
    for start in (0..folders.len()).rev() {
        out.push(format!("{}/{path}", folders[start..].join("/")));
    }
    out
}

/// What page grouping found: the groups, and the globs that matched nothing (VAL-001).
#[derive(Debug, Default)]
pub struct Pages {
    /// One entry per page, then `(no page)` when some asset is in none.
    pub totals: Vec<PageTotal>,
    /// `(page, glob)` pairs that matched no asset.
    pub unmatched: Vec<(String, String)>,
}

/// Group `assets` into the configured pages (FR-005). A glob matches an asset's path as shown
/// (relative to the scan root) or with folders of the scan `root` in front, so both `models/*.glb`
/// and `public/models/*.glb` work for `meshdiet scan public`.
#[must_use]
pub fn pages(assets: &[Asset], groups: &[PageGroup], root: &str) -> Pages {
    let mut out = Pages::default();
    if groups.is_empty() {
        return out;
    }
    let names: Vec<Vec<String>> = assets.iter().map(|a| spellings(root, &a.path)).collect();
    let mut placed = vec![false; assets.len()];
    for group in groups {
        let mut members = BTreeSet::new();
        for pattern in &group.assets {
            let Ok(m) = glob(pattern) else {
                continue;
            };
            let mut hit = false;
            for (i, spelled) in names.iter().enumerate() {
                if spelled.iter().any(|n| m.is_match(n)) {
                    members.insert(i);
                    hit = true;
                }
            }
            if !hit {
                out.unmatched.push((group.name.clone(), pattern.clone()));
            }
        }
        for i in &members {
            placed[*i] = true;
        }
        out.totals.push(page_total(
            &group.name,
            members.iter().map(|i| &assets[*i]),
            group.max,
        ));
    }
    if placed.contains(&false) {
        let rest = assets
            .iter()
            .zip(&placed)
            .filter(|(_, p)| !**p)
            .map(|(a, _)| a);
        out.totals.push(page_total(NO_PAGE, rest, None));
    }
    out
}

fn page_total<'a>(
    name: &str,
    members: impl Iterator<Item = &'a Asset>,
    max: Option<u64>,
) -> PageTotal {
    let mut members: Vec<&Asset> = members.collect();
    members.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    PageTotal {
        name: name.to_owned(),
        bytes: distinct_bytes(members.iter().copied()),
        assets: members.iter().map(|a| a.path.clone()).collect(),
        gzip: None,
        max,
    }
}

/// Counts bytes written, keeps none.
struct Counter(u64);

impl Write for Counter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// gzip level 6 size of one file, streamed so huge files never sit in memory.
///
/// # Errors
/// When the file cannot be read.
pub fn gzip_size(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut gz = flate2::write::GzEncoder::new(Counter(0), flate2::Compression::new(6));
    io::copy(&mut file, &mut gz)?;
    Ok(gz.finish()?.0)
}

/// gzip size of every distinct file of `assets`, keyed by path relative to the scan `root`. Files run
/// in parallel on every core. A file that cannot be read is left out (the reader already reported it).
#[must_use]
pub fn gzip_files(assets: &[Asset], root: &Path) -> BTreeMap<String, u64> {
    let files: Vec<&str> = distinct_files(assets).into_keys().collect();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(BTreeMap::new());
    std::thread::scope(|s| {
        for _ in 0..threads.min(files.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(rel) = files.get(i) else {
                        break;
                    };
                    if let Ok(size) = gzip_size(&root.join(rel))
                        && let Ok(mut map) = results.lock()
                    {
                        map.insert((*rel).to_owned(), size);
                    }
                }
            });
        }
    });
    results.into_inner().unwrap_or_default()
}

/// Sum of the gzip sizes of `assets`' distinct files.
#[must_use]
pub fn gzip_of<'a>(
    assets: impl IntoIterator<Item = &'a Asset>,
    gzip: &BTreeMap<String, u64>,
) -> Estimate {
    Estimate {
        bytes: distinct_files(assets)
            .keys()
            .filter_map(|f| gzip.get(*f))
            .sum(),
        estimated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes_use_decimal_units() {
        assert_eq!(human(812), "812 B");
        assert_eq!(human(64_000), "64.0 KB");
        assert_eq!(human(49_807_324), "49.8 MB");
        assert_eq!(human(1_250_000_000), "1.2 GB");
    }

    #[test]
    fn globs_keep_star_inside_a_folder() {
        let m = glob("models/*.glb").map(|g| g.is_match("models/shop/a.glb"));
        assert_eq!(m, Ok(false));
        let m = glob("models/**/*.glb").map(|g| g.is_match("models/shop/a.glb"));
        assert_eq!(m, Ok(true));
    }

    #[test]
    fn globs_may_include_the_scanned_folder() {
        assert_eq!(spellings(".", "a.glb"), ["a.glb"]);
        assert_eq!(spellings("./public/", "a.glb"), ["a.glb", "public/a.glb"]);
        assert_eq!(
            spellings("/srv/site/public", "a.glb"),
            [
                "a.glb",
                "public/a.glb",
                "site/public/a.glb",
                "srv/site/public/a.glb"
            ]
        );
    }
}
