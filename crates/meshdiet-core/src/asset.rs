//! One glTF asset as the rules see it: its files, its size split, its compression and its images
//! (SPEC-RULE-001 FR-001 and §8). Reading never decodes buffers or pixels.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Serialize;

use crate::glb::{self, Span};
use crate::gltf::{self, Document};
use crate::images;

/// GLB or `.gltf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    /// Binary glTF, one file.
    Glb,
    /// JSON glTF, with external or embedded buffers and images.
    Gltf,
}

/// Compression and format extensions the asset uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent flags, one per extension, as in the JSON report"
)]
pub struct Compression {
    /// `KHR_draco_mesh_compression`.
    pub draco: bool,
    /// `EXT_meshopt_compression` or `KHR_meshopt_compression`.
    pub meshopt: bool,
    /// `KHR_mesh_quantization`.
    pub quantized: bool,
    /// KTX2 textures (`KHR_texture_basisu`).
    pub ktx2: bool,
    /// WebP textures.
    pub webp: bool,
    /// AVIF textures.
    pub avif: bool,
}

/// A number that is an estimate, never read from the file (SPEC-CLI-001 BR-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Estimate {
    /// The estimated bytes.
    pub bytes: u64,
    /// Always true.
    pub estimated: bool,
}

/// Where an image's bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageSource {
    /// In a buffer, through a bufferView.
    BufferView,
    /// A base64 data URI in the JSON.
    DataUri,
    /// A file next to the `.gltf`.
    File,
    /// An `http(s)` URI; not read.
    Remote,
}

/// One image of the asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageInfo {
    /// Index in the glTF `images` array.
    pub index: usize,
    /// Its name, if it has one.
    pub name: Option<String>,
    /// MIME type from the bytes (or as declared when the bytes were not read).
    pub mime: Option<String>,
    /// Where it is stored.
    pub source: ImageSource,
    /// The file, for external images, relative to the scan root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Stored bytes.
    pub bytes: u64,
    /// Width in pixels, from the header.
    pub width: Option<u32>,
    /// Height in pixels, from the header.
    pub height: Option<u32>,
    /// Estimated GPU memory with mipmaps.
    pub est_vram: Option<Estimate>,
    /// KTX2 supercompression: `basis-lz`, `zstd`, `zlib` or `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supercompression: Option<String>,
}

/// One `.glb` or `.gltf` and everything it pulls in.
#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    /// Path relative to the scan root, forward slashes.
    pub path: String,
    /// GLB or glTF.
    pub kind: AssetKind,
    /// The asset file and every external file it references, relative to the scan root.
    pub files: Vec<String>,
    /// Sum of all its files on disk.
    pub bytes: u64,
    /// Bytes of bufferViews used by mesh accessors (compressed size when compressed).
    pub geometry_bytes: u64,
    /// Bytes of images.
    pub texture_bytes: u64,
    /// The rest: JSON, animation, padding, unused bytes.
    pub other_bytes: u64,
    /// Triangles drawn by the default scene, each node instance counted.
    pub triangles: u64,
    /// Mesh primitives in the file.
    pub primitives: u64,
    /// Compression in use.
    pub compression: Compression,
    /// Images.
    pub images: Vec<ImageInfo>,
    /// What could not be read; each becomes a GLTF-001 finding.
    #[serde(skip)]
    pub problems: Vec<String>,
    /// Size of each entry of `files`.
    #[serde(skip)]
    pub file_bytes: Vec<u64>,
    /// The parsed JSON, when it could be read.
    #[serde(skip)]
    pub document: Option<Document>,
}

/// Where a buffer's bytes are.
enum Data {
    /// The BIN chunk of the GLB file.
    Chunk(PathBuf, Span),
    /// An external file.
    File(PathBuf, u64),
    /// A decoded data URI.
    Inline(Vec<u8>),
    /// No data here on purpose (meshopt fallback, remote URI) or none found.
    Absent,
}

impl Data {
    fn len(&self) -> Option<u64> {
        match self {
            Self::Chunk(_, span) => Some(span.len),
            Self::File(_, len) => Some(*len),
            Self::Inline(bytes) => Some(bytes.len() as u64),
            Self::Absent => None,
        }
    }

    /// Up to `max` bytes starting at `offset`.
    fn head(&self, offset: u64, max: u64) -> std::io::Result<Vec<u8>> {
        let (path, start) = match self {
            Self::Chunk(path, span) => (path, span.offset + offset),
            Self::File(path, _) => (path, offset),
            Self::Inline(bytes) => {
                let from = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let to = from.saturating_add(usize::try_from(max).unwrap_or(usize::MAX));
                return Ok(bytes[from..to.min(bytes.len())].to_vec());
            }
            Self::Absent => return Ok(Vec::new()),
        };
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut out = Vec::new();
        file.take(max).read_to_end(&mut out)?;
        Ok(out)
    }
}

/// Decode `%XX` escapes. Invalid UTF-8 is replaced, which then simply names a missing file.
fn percent_decode(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = uri.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Join a relative URI to the folder of `rel` and fold `.` and `..`, with forward slashes.
fn join_rel(rel: &str, uri: &str) -> String {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    for piece in uri.split(['/', '\\']) {
        match piece {
            "" | "." => {}
            ".." => {
                if parts.last().is_none_or(|p| *p == "..") {
                    parts.push("..");
                } else {
                    parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Bytes of a `data:` URI, decoded. `Err` when the payload is not valid base64.
fn data_uri(uri: &str) -> Option<Result<Vec<u8>, String>> {
    let rest = uri.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',').unwrap_or((rest, ""));
    if meta.ends_with(";base64") {
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(payload.trim())
                .map_err(|e| format!("the data URI is not valid base64 ({e})")),
        )
    } else {
        Some(Ok(percent_decode(payload).into_bytes()))
    }
}

struct Reader<'a> {
    rel: &'a str,
    abs: &'a Path,
    asset: Asset,
}

/// Read the asset at `abs`, shown as `rel`. Never fails: what cannot be read lands in
/// [`Asset::problems`] and the rest is still filled in.
#[must_use]
pub fn read(rel: &str, abs: &Path) -> Asset {
    let kind = if abs
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gltf"))
    {
        AssetKind::Gltf
    } else {
        AssetKind::Glb
    };
    let mut reader = Reader {
        rel,
        abs,
        asset: Asset {
            path: rel.to_owned(),
            kind,
            files: Vec::new(),
            bytes: 0,
            geometry_bytes: 0,
            texture_bytes: 0,
            other_bytes: 0,
            triangles: 0,
            primitives: 0,
            compression: Compression::default(),
            images: Vec::new(),
            problems: Vec::new(),
            file_bytes: Vec::new(),
            document: None,
        },
    };
    if let Err(problem) = reader.run() {
        reader.asset.problems.push(problem);
    }
    let a = &mut reader.asset;
    a.bytes = a.file_bytes.iter().sum();
    a.other_bytes = a.bytes.saturating_sub(a.geometry_bytes + a.texture_bytes);
    reader.asset
}

impl Reader<'_> {
    fn add_file(&mut self, rel: String, bytes: u64) {
        if !self.asset.files.contains(&rel) {
            self.asset.files.push(rel);
            self.asset.file_bytes.push(bytes);
        }
    }

    /// Read everything; `Err` for a problem that stops the read.
    fn run(&mut self) -> Result<(), String> {
        let size = fs::metadata(self.abs)
            .map_err(|e| format!("cannot open the file: {e}"))?
            .len();
        self.add_file(self.rel.to_owned(), size);
        if size == 0 {
            return Err("the file is empty".to_owned());
        }
        let (json, bin) = match self.asset.kind {
            AssetKind::Glb => {
                let mut file =
                    File::open(self.abs).map_err(|e| format!("cannot open the file: {e}"))?;
                let c = glb::read(&mut file, size)?;
                (c.json, c.bin)
            }
            AssetKind::Gltf => (
                fs::read(self.abs).map_err(|e| format!("cannot read the file: {e}"))?,
                None,
            ),
        };
        let doc = Document::parse(&json)?;
        if doc.asset.version.starts_with('1') {
            return Err(format!(
                "glTF {}; only glTF 2.0 is supported",
                doc.asset.version
            ));
        }
        let buffers = self.buffers(&doc, bin);
        let bad_views = self.check_views(&doc, &buffers);
        self.images(&doc, &buffers, &bad_views);
        self.geometry(&doc);
        self.asset.compression = compression(&doc, &self.asset.images);
        self.asset.document = Some(doc);
        Ok(())
    }

    /// Resolve an external URI to (path for output, absolute path), or a problem when missing.
    fn external(&mut self, what: &str, uri: &str) -> Result<(String, PathBuf, u64), String> {
        let decoded = percent_decode(uri);
        let rel = join_rel(self.rel, &decoded);
        let abs = self
            .abs
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&decoded);
        match fs::metadata(&abs) {
            Ok(m) if m.is_file() => {
                self.add_file(rel.clone(), m.len());
                Ok((rel, abs, m.len()))
            }
            _ => Err(format!("{what} points to `{uri}`, which does not exist")),
        }
    }

    fn buffers(&mut self, doc: &Document, bin: Option<Span>) -> Vec<Data> {
        let mut out = Vec::with_capacity(doc.buffers.len());
        for (i, buffer) in doc.buffers.iter().enumerate() {
            let data = match buffer.uri.as_deref() {
                None if i == 0 && self.asset.kind == AssetKind::Glb => {
                    if let Some(span) = bin {
                        Data::Chunk(self.abs.to_path_buf(), span)
                    } else {
                        self.asset
                            .problems
                            .push("buffer 0 has no data: the GLB has no BIN chunk".to_owned());
                        Data::Absent
                    }
                }
                None if buffer.is_meshopt_fallback() => Data::Absent,
                None => {
                    self.asset
                        .problems
                        .push(format!("buffer {i} has no uri and no data"));
                    Data::Absent
                }
                Some(uri) if uri.contains("://") => Data::Absent,
                Some(uri) => match data_uri(uri) {
                    Some(Ok(bytes)) => Data::Inline(bytes),
                    Some(Err(e)) => {
                        self.asset.problems.push(format!("buffer {i}: {e}"));
                        Data::Absent
                    }
                    None => match self.external(&format!("buffer {i}"), uri) {
                        Ok((_, abs, len)) => Data::File(abs, len),
                        Err(e) => {
                            self.asset.problems.push(e);
                            Data::Absent
                        }
                    },
                },
            };
            if let Some(len) = data.len()
                && len < buffer.byte_length
            {
                self.asset.problems.push(format!(
                    "buffer {i} says it is {} bytes but only {len} are there",
                    buffer.byte_length
                ));
            }
            out.push(data);
        }
        out
    }

    /// Report bufferViews that point past their buffer; returns their indices.
    fn check_views(&mut self, doc: &Document, buffers: &[Data]) -> BTreeSet<usize> {
        let mut bad = BTreeSet::new();
        for (v, view) in doc.buffer_views.iter().enumerate() {
            let mut ranges = vec![(view.buffer, view.byte_offset, view.byte_length)];
            if let Some(m) = view.meshopt() {
                ranges.push((m.buffer, m.byte_offset, m.byte_length));
            }
            for (b, offset, len) in ranges {
                let Some(data) = buffers.get(b) else {
                    self.asset.problems.push(format!(
                        "bufferView {v} points to buffer {b}, which does not exist"
                    ));
                    bad.insert(v);
                    continue;
                };
                let end = offset.saturating_add(len);
                if let Some(have) = data.len()
                    && end > have
                {
                    self.asset.problems.push(format!(
                        "bufferView {v} ends at byte {end}, past the end of buffer {b} ({have} bytes)"
                    ));
                    bad.insert(v);
                }
            }
        }
        bad
    }

    fn images(&mut self, doc: &Document, buffers: &[Data], bad_views: &BTreeSet<usize>) {
        for (index, image) in doc.images.iter().enumerate() {
            let mut info = ImageInfo {
                index,
                name: image.name.clone(),
                mime: image.mime_type.clone(),
                source: ImageSource::BufferView,
                file: None,
                bytes: 0,
                width: None,
                height: None,
                est_vram: None,
                supercompression: None,
            };
            let what = format!("image {index}");
            let head = match (image.buffer_view, image.uri.as_deref()) {
                (Some(v), _) => match doc.buffer_views.get(v) {
                    Some(view) if !bad_views.contains(&v) => {
                        info.bytes = view.byte_length;
                        buffers.get(view.buffer).map(|data| {
                            data.head(view.byte_offset, view.byte_length.min(images::HEAD_BYTES))
                        })
                    }
                    Some(_) => None,
                    None => {
                        self.asset.problems.push(format!(
                            "{what} points to bufferView {v}, which does not exist"
                        ));
                        None
                    }
                },
                (None, Some(uri)) if uri.contains("://") => {
                    info.source = ImageSource::Remote;
                    None
                }
                (None, Some(uri)) => {
                    if let Some(decoded) = data_uri(uri) {
                        info.source = ImageSource::DataUri;
                        Some(
                            decoded
                                .inspect(|bytes| info.bytes = bytes.len() as u64)
                                .map_err(std::io::Error::other),
                        )
                    } else {
                        info.source = ImageSource::File;
                        match self.external(&what, uri) {
                            Ok((rel, abs, len)) => {
                                info.file = Some(rel);
                                info.bytes = len;
                                Some(Data::File(abs, len).head(0, images::HEAD_BYTES))
                            }
                            Err(e) => {
                                self.asset.problems.push(e);
                                None
                            }
                        }
                    }
                }
                (None, None) => {
                    self.asset
                        .problems
                        .push(format!("{what} has neither a uri nor a bufferView"));
                    None
                }
            };
            match head {
                Some(Ok(head)) if !head.is_empty() => match images::header(&head) {
                    Ok(h) => {
                        info.mime = Some(h.mime.to_owned());
                        info.width = Some(h.width);
                        info.height = Some(h.height);
                        info.est_vram = Some(Estimate {
                            bytes: images::est_vram(h.mime, h.width, h.height),
                            estimated: true,
                        });
                        info.supercompression = h.supercompression.map(str::to_owned);
                    }
                    Err(e) => self.asset.problems.push(format!("{what}: {e}")),
                },
                Some(Err(e)) => self.asset.problems.push(format!("{what}: {e}")),
                _ => {}
            }
            self.asset.texture_bytes += info.bytes;
            self.asset.images.push(info);
        }
    }

    fn geometry(&mut self, doc: &Document) {
        let mut views = BTreeSet::new();
        let mut accessors: BTreeSet<usize> = BTreeSet::new();
        for p in doc.meshes.iter().flat_map(|m| &m.primitives) {
            accessors.extend(p.attributes.values().copied());
            accessors.extend(p.indices);
            accessors.extend(p.targets.iter().flat_map(|t| t.values().copied()));
            if let Some(v) = p
                .extensions
                .get(gltf::KHR_DRACO)
                .and_then(|d| d.get("bufferView"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
            {
                views.insert(v);
            }
        }
        for a in accessors.iter().filter_map(|i| doc.accessors.get(*i)) {
            views.extend(a.buffer_view);
            if let Some(s) = &a.sparse {
                views.extend(s.indices.buffer_view);
                views.extend(s.values.buffer_view);
            }
        }
        self.asset.geometry_bytes = views
            .iter()
            .filter_map(|v| doc.buffer_views.get(*v))
            .map(|view| view.meshopt().map_or(view.byte_length, |m| m.byte_length))
            .sum();
        self.asset.primitives = doc.meshes.iter().map(|m| m.primitives.len() as u64).sum();
        self.asset.triangles = triangles(doc);
    }
}

/// Triangles in one mesh.
fn mesh_triangles(doc: &Document, mesh: usize) -> u64 {
    let Some(mesh) = doc.meshes.get(mesh) else {
        return 0;
    };
    mesh.primitives
        .iter()
        .map(|p| {
            let count = p
                .indices
                .or_else(|| p.attributes.get("POSITION").copied())
                .and_then(|a| doc.accessors.get(a))
                .map_or(0, |a| a.count);
            match p.mode.unwrap_or(4) {
                4 => count / 3,
                5 | 6 => count.saturating_sub(2),
                _ => 0,
            }
        })
        .sum()
}

/// Triangles drawn by the default scene, every node that draws a mesh counted once. Without
/// scenes, every mesh once. Cycles and absurdly deep or wide graphs are cut off.
fn triangles(doc: &Document) -> u64 {
    const MAX_DEPTH: usize = 64;
    const MAX_VISITS: usize = 1_000_000;
    let Some(scene) = doc.scenes.get(doc.scene.unwrap_or(0)) else {
        return (0..doc.meshes.len()).map(|m| mesh_triangles(doc, m)).sum();
    };
    let per_mesh: Vec<u64> = (0..doc.meshes.len())
        .map(|m| mesh_triangles(doc, m))
        .collect();
    let mut total = 0u64;
    let mut visits = 0usize;
    let mut stack: Vec<(usize, Vec<usize>)> =
        scene.nodes.iter().map(|n| (*n, Vec::new())).collect();
    while let Some((node, mut path)) = stack.pop() {
        visits += 1;
        if visits > MAX_VISITS || path.len() > MAX_DEPTH || path.contains(&node) {
            continue;
        }
        let Some(n) = doc.nodes.get(node) else {
            continue;
        };
        if let Some(m) = n.mesh {
            total += per_mesh.get(m).copied().unwrap_or(0);
        }
        path.push(node);
        stack.extend(n.children.iter().map(|c| (*c, path.clone())));
    }
    total
}

fn compression(doc: &Document, images: &[ImageInfo]) -> Compression {
    let mime = |m: &str| images.iter().any(|i| i.mime.as_deref() == Some(m));
    Compression {
        draco: doc.uses(gltf::KHR_DRACO)
            || doc
                .meshes
                .iter()
                .flat_map(|m| &m.primitives)
                .any(|p| p.extensions.contains_key(gltf::KHR_DRACO)),
        meshopt: doc.uses(gltf::EXT_MESHOPT)
            || doc.uses(gltf::KHR_MESHOPT)
            || doc.buffer_views.iter().any(|v| v.meshopt().is_some()),
        quantized: doc.uses(gltf::KHR_QUANTIZATION),
        ktx2: doc.uses(gltf::KHR_BASISU) || mime("image/ktx2"),
        webp: doc.uses(gltf::EXT_WEBP) || mime("image/webp"),
        avif: doc.uses(gltf::EXT_AVIF) || mime("image/avif"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_are_percent_decoded() {
        assert_eq!(percent_decode("my%20model.bin"), "my model.bin");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%FF%FE.bin"), "\u{FFFD}\u{FFFD}.bin");
    }

    #[test]
    fn relative_uris_join_the_asset_folder() {
        assert_eq!(join_rel("models/a.gltf", "a.bin"), "models/a.bin");
        assert_eq!(join_rel("models/a.gltf", "../tex/b.png"), "tex/b.png");
        assert_eq!(join_rel("a.gltf", "../b.png"), "../b.png");
        assert_eq!(join_rel("a.gltf", "./sub\\c.png"), "sub/c.png");
    }

    #[test]
    fn data_uris_decode() {
        assert_eq!(
            data_uri("data:application/octet-stream;base64,AAEC"),
            Some(Ok(vec![0, 1, 2]))
        );
        assert!(matches!(data_uri("data:;base64,@@@"), Some(Err(_))));
        assert_eq!(data_uri("model.bin"), None);
    }
}
