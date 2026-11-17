//! The parts of the glTF 2.0 JSON that an audit needs, as plain structs.
//!
//! Deliberately tolerant (ADR-003): every field is optional or defaulted and unknown keys and
//! extensions are kept or ignored, so a half-broken file can still be reported on.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// Extensions object: extension name to its JSON.
pub type Extensions = BTreeMap<String, Value>;

/// Draco geometry compression.
pub const KHR_DRACO: &str = "KHR_draco_mesh_compression";
/// meshopt compression, vendor version.
pub const EXT_MESHOPT: &str = "EXT_meshopt_compression";
/// meshopt compression, Khronos version.
pub const KHR_MESHOPT: &str = "KHR_meshopt_compression";
/// Quantized vertex attributes.
pub const KHR_QUANTIZATION: &str = "KHR_mesh_quantization";
/// KTX2 / Basis Universal textures.
pub const KHR_BASISU: &str = "KHR_texture_basisu";
/// WebP textures.
pub const EXT_WEBP: &str = "EXT_texture_webp";
/// AVIF textures.
pub const EXT_AVIF: &str = "EXT_texture_avif";

/// The glTF document.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    /// `asset.version` and friends.
    pub asset: AssetMeta,
    /// Extensions the file uses.
    pub extensions_used: Vec<String>,
    /// Extensions a loader must support.
    pub extensions_required: Vec<String>,
    /// Default scene.
    pub scene: Option<usize>,
    /// Scenes.
    pub scenes: Vec<Scene>,
    /// Nodes.
    pub nodes: Vec<Node>,
    /// Meshes.
    pub meshes: Vec<Mesh>,
    /// Accessors.
    pub accessors: Vec<Accessor>,
    /// Buffer views.
    pub buffer_views: Vec<BufferView>,
    /// Buffers.
    pub buffers: Vec<Buffer>,
    /// Images.
    pub images: Vec<Image>,
    /// Textures.
    pub textures: Vec<Texture>,
    /// Materials (only counted and named).
    pub materials: Vec<Named>,
}

/// `asset`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AssetMeta {
    /// glTF version, `2.0`.
    pub version: String,
    /// The tool that wrote the file.
    pub generator: Option<String>,
}

/// Anything with only a name we care about.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Named {
    /// Its name.
    pub name: Option<String>,
}

/// A scene: its root nodes.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Scene {
    /// Root node indices.
    pub nodes: Vec<usize>,
}

/// A node.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Node {
    /// Its name.
    pub name: Option<String>,
    /// The mesh it draws.
    pub mesh: Option<usize>,
    /// Child nodes.
    pub children: Vec<usize>,
}

/// A mesh.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Mesh {
    /// Its name.
    pub name: Option<String>,
    /// Its primitives.
    pub primitives: Vec<Primitive>,
}

/// One draw call's worth of geometry.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Primitive {
    /// Attribute name to accessor.
    pub attributes: BTreeMap<String, usize>,
    /// Index accessor.
    pub indices: Option<usize>,
    /// Topology; 4 (triangles) when absent.
    pub mode: Option<u32>,
    /// Material index.
    pub material: Option<usize>,
    /// Morph targets.
    pub targets: Vec<BTreeMap<String, usize>>,
    /// Extensions, such as Draco.
    pub extensions: Extensions,
}

/// An accessor: typed view of a bufferView.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Accessor {
    /// Its bufferView; zero-filled data when absent.
    pub buffer_view: Option<usize>,
    /// Offset inside the bufferView.
    pub byte_offset: u64,
    /// Number of elements.
    pub count: u64,
    /// `5120`…`5126`.
    pub component_type: u32,
    /// `SCALAR`, `VEC3`, …
    #[serde(rename = "type")]
    pub kind: String,
    /// Integer data mapped to 0..1 or -1..1.
    pub normalized: bool,
    /// Sparse storage.
    pub sparse: Option<Sparse>,
}

/// Sparse accessor storage.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Sparse {
    /// Number of replaced elements.
    pub count: u64,
    /// Where the indices are.
    pub indices: SparsePart,
    /// Where the values are.
    pub values: SparsePart,
}

/// One part of a sparse accessor.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SparsePart {
    /// Its bufferView.
    pub buffer_view: Option<usize>,
}

/// A slice of a buffer.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BufferView {
    /// The buffer.
    pub buffer: usize,
    /// Start in the buffer.
    pub byte_offset: u64,
    /// Length.
    pub byte_length: u64,
    /// Vertex stride.
    pub byte_stride: Option<u64>,
    /// Extensions, such as meshopt.
    pub extensions: Extensions,
}

impl BufferView {
    /// The meshopt extension on this view (EXT or KHR), if any.
    #[must_use]
    pub fn meshopt(&self) -> Option<MeshoptView> {
        let ext = self
            .extensions
            .get(EXT_MESHOPT)
            .or_else(|| self.extensions.get(KHR_MESHOPT))?;
        Some(MeshoptView {
            buffer: usize::try_from(ext.get("buffer")?.as_u64()?).ok()?,
            byte_offset: ext.get("byteOffset").and_then(Value::as_u64).unwrap_or(0),
            byte_length: ext.get("byteLength")?.as_u64()?,
        })
    }
}

/// Where meshopt stores the compressed bytes of a bufferView.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshoptView {
    /// Buffer holding the compressed data.
    pub buffer: usize,
    /// Start in that buffer.
    pub byte_offset: u64,
    /// Compressed length.
    pub byte_length: u64,
}

/// A buffer.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Buffer {
    /// File, data URI, or none (GLB BIN chunk or meshopt fallback).
    pub uri: Option<String>,
    /// Declared length.
    pub byte_length: u64,
    /// Extensions, such as a meshopt fallback marker.
    pub extensions: Extensions,
}

impl Buffer {
    /// True for a meshopt fallback buffer, which has no data on purpose.
    #[must_use]
    pub fn is_meshopt_fallback(&self) -> bool {
        [EXT_MESHOPT, KHR_MESHOPT].iter().any(|name| {
            self.extensions
                .get(*name)
                .and_then(|e| e.get("fallback"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
    }
}

/// An image.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Image {
    /// Its name.
    pub name: Option<String>,
    /// File or data URI.
    pub uri: Option<String>,
    /// Declared MIME type.
    pub mime_type: Option<String>,
    /// The bufferView holding it.
    pub buffer_view: Option<usize>,
}

/// A texture.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Texture {
    /// Image index.
    pub source: Option<usize>,
    /// Extensions, such as `KHR_texture_basisu`.
    pub extensions: Extensions,
}

impl Document {
    /// Parse the glTF JSON.
    ///
    /// # Errors
    /// When the bytes are not JSON or a field has the wrong type.
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        let json = json.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(json);
        serde_json::from_slice(json).map_err(|e| format!("the glTF JSON cannot be read: {e}"))
    }

    /// Whether `extensionsUsed` lists `name`.
    #[must_use]
    pub fn uses(&self, name: &str) -> bool {
        self.extensions_used.iter().any(|e| e == name)
    }
}
