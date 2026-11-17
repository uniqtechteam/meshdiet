//! A small glTF 2.0 writer: enough to build test models with chosen geometry, textures, names and
//! layout (GLB, `.gltf` with external files, or `.gltf` with data URIs).

use serde_json::{Value, json};

use crate::texture::Texture;

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;

/// Where an image's bytes live once the model is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageStore {
    /// Inside the binary buffer, through a bufferView (the usual GLB layout).
    BufferView,
    /// A file next to the `.gltf`, named after the texture.
    File,
}

/// Builds one glTF asset in memory.
#[derive(Debug, Default)]
pub struct Builder {
    bin: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    materials: Vec<Value>,
    textures: Vec<Value>,
    images: Vec<Value>,
    files: Vec<(String, Vec<u8>)>,
}

/// One written file: a path relative to where the asset goes, and its bytes.
pub type File = (String, Vec<u8>);

impl Builder {
    /// An empty asset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn align(&mut self) {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
    }

    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.align();
        let mut view =
            json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.bin.extend_from_slice(bytes);
        self.buffer_views.push(view);
        self.buffer_views.len() - 1
    }

    fn accessor(&mut self, value: Value) -> usize {
        self.accessors.push(value);
        self.accessors.len() - 1
    }

    /// Add a mesh made of one grid primitive with exactly `triangles` triangles (float positions,
    /// normals and UVs, 16- or 32-bit indices). Returns the mesh index.
    pub fn grid_mesh(&mut self, name: &str, triangles: u32, material: Option<usize>) -> usize {
        let grid = Grid::new(triangles);
        let (positions, min, max) = grid.positions();
        let pos_view = self.view(&positions, Some(ARRAY_BUFFER));
        let position = self.accessor(json!({
            "bufferView": pos_view, "componentType": FLOAT, "count": grid.vertices(),
            "type": "VEC3", "min": min, "max": max
        }));
        let normal_view = self.view(&grid.normals(), Some(ARRAY_BUFFER));
        let normal = self.accessor(json!({
            "bufferView": normal_view, "componentType": FLOAT, "count": grid.vertices(), "type": "VEC3"
        }));
        let uv_view = self.view(&grid.uvs(), Some(ARRAY_BUFFER));
        let uv = self.accessor(json!({
            "bufferView": uv_view, "componentType": FLOAT, "count": grid.vertices(), "type": "VEC2"
        }));
        let (indices, wide) = grid.indices(triangles);
        let index_view = self.view(&indices, Some(ELEMENT_ARRAY_BUFFER));
        let index = self.accessor(json!({
            "bufferView": index_view,
            "componentType": if wide { UNSIGNED_INT } else { UNSIGNED_SHORT },
            "count": triangles * 3, "type": "SCALAR"
        }));
        let mut primitive = json!({
            "attributes": { "POSITION": position, "NORMAL": normal, "TEXCOORD_0": uv },
            "indices": index, "mode": 4
        });
        if let Some(m) = material {
            primitive["material"] = json!(m);
        }
        self.meshes
            .push(json!({ "name": name, "primitives": [primitive] }));
        self.meshes.len() - 1
    }

    /// Add a material whose base colour is `texture`. Returns the material index.
    pub fn textured_material(&mut self, name: &str, texture: &Texture, store: ImageStore) -> usize {
        let bytes = texture.encode();
        let image = match store {
            ImageStore::BufferView => {
                let view = self.view(&bytes, None);
                json!({ "name": texture.name, "bufferView": view, "mimeType": texture.mime() })
            }
            ImageStore::File => {
                let file = format!("{}.{}", texture.name, texture.extension());
                self.files.push((file.clone(), bytes));
                json!({ "name": texture.name, "uri": file })
            }
        };
        self.images.push(image);
        self.textures
            .push(json!({ "source": self.images.len() - 1 }));
        self.materials.push(json!({
            "name": name,
            "pbrMetallicRoughness": { "baseColorTexture": { "index": self.textures.len() - 1 },
                                      "metallicFactor": 0.0 }
        }));
        self.materials.len() - 1
    }

    /// Add a plain material with no texture. Returns the material index.
    pub fn plain_material(&mut self, name: &str) -> usize {
        self.materials.push(json!({
            "name": name, "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.8, 0.8, 1.0] }
        }));
        self.materials.len() - 1
    }

    /// Add a node that draws `mesh`, placed `x` units along the X axis.
    pub fn node(&mut self, name: &str, mesh: usize, x: f32) {
        self.nodes
            .push(json!({ "name": name, "mesh": mesh, "translation": [x, 0.0, 0.0] }));
    }

    /// Append `n` bytes to the buffer that no bufferView covers (dead weight).
    pub fn dead_bytes(&mut self, n: usize) {
        self.align();
        self.bin.resize(self.bin.len() + n, 0);
    }

    /// Change a bufferView after the fact, to plant broken references.
    pub fn patch_view(&mut self, index: usize, key: &str, value: Value) {
        if let Some(view) = self.buffer_views.get_mut(index) {
            view[key] = value;
        }
    }

    fn document(&self, buffer: &Value) -> Value {
        let mut doc = json!({
            "asset": { "version": "2.0", "generator": "meshdiet xtask" },
            "scene": 0,
            "scenes": [{ "nodes": (0..self.nodes.len()).collect::<Vec<_>>() }],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "accessors": self.accessors,
            "bufferViews": self.buffer_views,
            "buffers": [buffer],
        });
        for (key, list) in [
            ("materials", &self.materials),
            ("textures", &self.textures),
            ("images", &self.images),
        ] {
            if !list.is_empty() {
                doc[key] = json!(list);
            }
        }
        doc
    }

    fn padded_bin(&self) -> Vec<u8> {
        let mut bin = self.bin.clone();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        bin
    }

    /// The asset as one GLB file.
    #[must_use]
    pub fn glb(&self) -> Vec<u8> {
        let bin = self.padded_bin();
        let doc = self.document(&json!({ "byteLength": bin.len() }));
        glb_from(&doc, &bin)
    }

    /// The asset as `<base>.gltf` + `<base>.bin` + any image files.
    #[must_use]
    pub fn gltf_external(&self, base: &str) -> Vec<File> {
        let bin = self.padded_bin();
        let doc = self.document(&json!({ "uri": format!("{base}.bin"), "byteLength": bin.len() }));
        let mut files = vec![
            (format!("{base}.gltf"), pretty(&doc)),
            (format!("{base}.bin"), bin),
        ];
        files.extend(self.files.iter().cloned());
        files
    }

    /// The asset as one `.gltf` whose buffer is a base64 data URI.
    #[must_use]
    pub fn gltf_embedded(&self) -> Vec<u8> {
        let bin = self.padded_bin();
        let uri = format!("data:application/octet-stream;base64,{}", base64(&bin));
        pretty(&self.document(&json!({ "uri": uri, "byteLength": bin.len() })))
    }
}

fn pretty(doc: &Value) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(doc).unwrap_or_default();
    text.push(b'\n');
    text
}

/// Wrap a glTF JSON document and a binary chunk into a GLB container.
#[must_use]
pub fn glb_from(doc: &Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(doc).unwrap_or_default();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32_len(total).to_le_bytes());
    out.extend_from_slice(&u32_len(json.len()).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    if !bin.is_empty() {
        out.extend_from_slice(&u32_len(bin.len()).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bin);
    }
    out
}

fn u32_len(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ABC[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A flat-ish grid of quads, cut so it has an exact triangle count.
struct Grid {
    columns: u32,
    rows: u32,
}

impl Grid {
    fn new(triangles: u32) -> Self {
        let quads = triangles.max(1).div_ceil(2);
        let columns = quads.isqrt().max(1);
        let rows = quads.div_ceil(columns);
        Self { columns, rows }
    }

    fn vertices(&self) -> u32 {
        (self.columns + 1) * (self.rows + 1)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "grid coordinates stay far below 2^24"
    )]
    fn positions(&self) -> (Vec<u8>, [f32; 3], [f32; 3]) {
        let mut out = Vec::with_capacity(self.vertices() as usize * 12);
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for j in 0..=self.rows {
            for i in 0..=self.columns {
                let p = [
                    i as f32 / self.columns as f32,
                    ((i * 7 + j * 13) % 17) as f32 / 170.0,
                    j as f32 / self.rows as f32,
                ];
                for k in 0..3 {
                    min[k] = min[k].min(p[k]);
                    max[k] = max[k].max(p[k]);
                    out.extend_from_slice(&p[k].to_le_bytes());
                }
            }
        }
        (out, min, max)
    }

    fn normals(&self) -> Vec<u8> {
        let one: Vec<u8> = [0.0f32, 1.0, 0.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        one.repeat(self.vertices() as usize)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "grid coordinates stay far below 2^24"
    )]
    fn uvs(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.vertices() as usize * 8);
        for j in 0..=self.rows {
            for i in 0..=self.columns {
                out.extend_from_slice(&(i as f32 / self.columns as f32).to_le_bytes());
                out.extend_from_slice(&(j as f32 / self.rows as f32).to_le_bytes());
            }
        }
        out
    }

    /// Index bytes for exactly `triangles` triangles, and whether they are 32-bit.
    fn indices(&self, triangles: u32) -> (Vec<u8>, bool) {
        let wide = self.vertices() > u32::from(u16::MAX);
        let mut list = Vec::with_capacity(triangles as usize * 3);
        'quads: for j in 0..self.rows {
            for i in 0..self.columns {
                let a = j * (self.columns + 1) + i;
                let b = a + 1;
                let c = a + self.columns + 1;
                let d = c + 1;
                for tri in [[a, c, b], [b, c, d]] {
                    if list.len() >= triangles as usize * 3 {
                        break 'quads;
                    }
                    list.extend_from_slice(&tri);
                }
            }
        }
        let bytes = if wide {
            list.iter().flat_map(|v| v.to_le_bytes()).collect()
        } else {
            list.iter()
                .flat_map(|v| u16::try_from(*v).unwrap_or(u16::MAX).to_le_bytes())
                .collect()
        };
        (bytes, wide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn grid_has_the_exact_triangle_count() {
        for t in [1, 2, 3, 12, 999, 10_000] {
            let g = Grid::new(t);
            let (idx, wide) = g.indices(t);
            assert_eq!(idx.len(), t as usize * 3 * if wide { 4 } else { 2 });
        }
    }
}
