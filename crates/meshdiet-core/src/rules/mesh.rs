//! Geometry rules MESH-001…003 (SPEC-RULE-001 FR-002…004). They read accessor and bufferView
//! lengths from the glTF JSON and never decode a buffer (ADR-003).

use std::collections::{BTreeMap, BTreeSet};

use crate::gltf::{self, Accessor, Document, Primitive};
use crate::rule::{Rule, RuleInfo};
use crate::{Asset, Confidence, Config, Finding, Measured, Severity};

/// MESH-003 reports a buffer when more than this many bytes are unused…
const DEAD_BYTES_MAX: u64 = 64_000;
/// …or when more than this share of it (in percent) is unused.
const DEAD_PERCENT_MAX: u64 = 10;

/// MESH-001: geometry stored without Draco or meshopt.
#[derive(Debug, Clone, Copy, Default)]
pub struct UncompressedGeometry;

/// MESH-002: more triangles drawn than the budget.
#[derive(Debug, Clone, Copy, Default)]
pub struct TriangleBudget;

/// MESH-003: buffer bytes that no bufferView uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnusedBufferBytes;

static MESH_001: RuleInfo = RuleInfo {
    id: "MESH-001",
    title: "Geometry not compressed",
    severity: Severity::High,
    confidence: Confidence::High,
    summary: "mesh data stored without Draco or meshopt compression",
};

static MESH_002: RuleInfo = RuleInfo {
    id: "MESH-002",
    title: "Too many triangles",
    severity: Severity::Medium,
    confidence: Confidence::High,
    summary: "the default scene draws more triangles than the budget",
};

static MESH_003: RuleInfo = RuleInfo {
    id: "MESH-003",
    title: "Unused buffer bytes",
    severity: Severity::Low,
    confidence: Confidence::Medium,
    summary: "bytes in a buffer that no bufferView, and so nothing in the model, uses",
};

/// Why it costs the user, and how to fix it: one sentence each (SPEC-RULE-001 BR-001, BR-003).
struct Advice {
    why: &'static str,
    fix: &'static str,
}

const MESH_001_ADVICE: Advice = Advice {
    why: "Geometry is downloaded in full before the model shows up; meshopt or Draco usually makes \
          it several times smaller.",
    fix: "Run `meshdiet fix` on this file (gltfpack, meshopt), or `gltf-transform draco in.glb \
          out.glb` if you need Draco; then make sure the app loads the decoder (LOAD-001).",
};

const MESH_002_ADVICE: Advice = Advice {
    why: "The GPU draws every triangle each frame, so phones and laptops drop frames; compression \
          makes the download smaller but not this number.",
    fix: "Lower the detail in your modelling tool, or simplify with `gltfpack -i in.glb -o out.glb \
          -si 0.5` or `gltf-transform simplify in.glb out.glb --ratio 0.5` and check the result by \
          eye (`meshdiet fix` keeps every triangle). If the budget is wrong for this page, raise \
          `triangles_max`.",
};

const MESH_003_ADVICE: Advice = Advice {
    why: "Users download these bytes and nothing reads them; usually left over from an exporter or \
          an edited file.",
    fix: "Write the file again: `meshdiet fix` (gltfpack rebuilds the buffers) or `gltf-transform \
          prune in.glb out.glb`.",
};

fn finding(
    info: &RuleInfo,
    asset: &Asset,
    measured: Measured,
    threshold: Measured,
    message: String,
    advice: &Advice,
) -> Finding {
    Finding {
        rule: info.id.to_owned(),
        title: info.title.to_owned(),
        severity: info.severity,
        confidence: info.confidence,
        path: asset.path.clone(),
        measured,
        threshold: Some(threshold),
        message,
        why: advice.why.to_owned(),
        fix: advice.fix.to_owned(),
    }
}

/// Bytes in decimal units with one decimal (1 MB = 1,000,000 bytes), as in the CLI output.
#[allow(
    clippy::cast_precision_loss,
    reason = "one decimal of a byte count; f64 is exact far beyond any real file"
)]
fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", b / 1e3)
    } else {
        format!("{:.1} MB", b / 1e6)
    }
}

/// `505000` as `505,000`.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Every accessor a primitive reads: attributes, indices and morph targets.
fn primitive_accessors(p: &Primitive) -> impl Iterator<Item = usize> + '_ {
    p.attributes
        .values()
        .copied()
        .chain(p.indices)
        .chain(p.targets.iter().flat_map(|t| t.values().copied()))
}

/// Draco on the primitive, or meshopt on any bufferView it reads.
fn is_compressed(doc: &Document, p: &Primitive) -> bool {
    p.extensions.contains_key(gltf::KHR_DRACO)
        || primitive_accessors(p)
            .filter_map(|a| doc.accessors.get(a))
            .flat_map(accessor_views)
            .filter_map(|v| doc.buffer_views.get(v))
            .any(|view| view.meshopt().is_some())
}

/// The bufferViews an accessor reads, sparse parts included.
fn accessor_views(a: &Accessor) -> impl Iterator<Item = usize> + '_ {
    a.buffer_view
        .into_iter()
        .chain(a.sparse.iter().flat_map(|s| {
            s.indices
                .buffer_view
                .into_iter()
                .chain(s.values.buffer_view)
        }))
}

/// Declared size of an accessor's elements, for accessors with no bufferView (zero-filled).
fn declared_bytes(a: &Accessor) -> u64 {
    let component = match a.component_type {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        _ => 4,
    };
    let components = match a.kind.as_str() {
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        _ => 1,
    };
    a.count.saturating_mul(component * components)
}

impl Rule for UncompressedGeometry {
    fn info(&self) -> &'static RuleInfo {
        &MESH_001
    }

    fn check(&self, asset: &Asset, config: &Config) -> Vec<Finding> {
        let Some(doc) = &asset.document else {
            return Vec::new();
        };
        let mut primitives = 0u64;
        let mut accessors = BTreeSet::new();
        for p in doc.meshes.iter().flat_map(|m| &m.primitives) {
            if !is_compressed(doc, p) {
                primitives += 1;
                accessors.extend(primitive_accessors(p));
            }
        }
        let mut views = BTreeSet::new();
        let mut zero_filled = 0u64;
        for a in accessors.iter().filter_map(|i| doc.accessors.get(*i)) {
            if a.buffer_view.is_none() {
                zero_filled += declared_bytes(a);
            }
            views.extend(accessor_views(a));
        }
        // a view past the end of its buffer is already GLTF-001; its length means nothing
        let bytes = views
            .iter()
            .filter_map(|v| doc.buffer_views.get(*v))
            .filter(|v| {
                doc.buffers
                    .get(v.buffer)
                    .is_some_and(|b| v.byte_offset.saturating_add(v.byte_length) <= b.byte_length)
            })
            .map(|v| v.byte_length)
            .sum::<u64>()
            + zero_filled;
        if primitives == 0 || bytes < config.compress_min {
            return Vec::new();
        }
        let mut message = format!(
            "{}, {} of uncompressed geometry",
            plural(primitives, "primitive", "primitives"),
            size(bytes)
        );
        let quantized = asset.compression.quantized;
        if quantized {
            message.push_str(" (already quantized with KHR_mesh_quantization, so less to gain)");
        }
        let mut f = finding(
            &MESH_001,
            asset,
            Measured::exact("uncompressed geometry", bytes, "bytes"),
            Measured::exact("compress_min", config.compress_min, "bytes"),
            message,
            &MESH_001_ADVICE,
        );
        if quantized {
            f.severity = Severity::Medium;
        }
        vec![f]
    }
}

impl Rule for TriangleBudget {
    fn info(&self) -> &'static RuleInfo {
        &MESH_002
    }

    fn check(&self, asset: &Asset, config: &Config) -> Vec<Finding> {
        if asset.document.is_none() || asset.triangles <= config.triangles_max {
            return Vec::new();
        }
        let message = format!(
            "{} triangles drawn by the default scene, over the budget of {}",
            thousands(asset.triangles),
            thousands(config.triangles_max)
        );
        let f = finding(
            &MESH_002,
            asset,
            Measured::exact("triangles", asset.triangles, "triangles"),
            Measured::exact("triangles_max", config.triangles_max, "triangles"),
            message,
            &MESH_002_ADVICE,
        );
        vec![f]
    }
}

/// Byte ranges of each buffer that a referenced bufferView covers, meshopt data included.
fn covered_ranges(doc: &Document) -> BTreeMap<usize, Vec<(u64, u64)>> {
    let mut ranges: BTreeMap<usize, Vec<(u64, u64)>> = BTreeMap::new();
    for view in doc
        .view_refs
        .iter()
        .filter_map(|v| doc.buffer_views.get(*v))
    {
        let mut add = |buffer: usize, offset: u64, len: u64| {
            ranges
                .entry(buffer)
                .or_default()
                .push((offset, offset.saturating_add(len)));
        };
        add(view.buffer, view.byte_offset, view.byte_length);
        if let Some(m) = view.meshopt() {
            add(m.buffer, m.byte_offset, m.byte_length);
        }
    }
    ranges
}

/// Bytes of `0..len` that no range covers.
fn uncovered(len: u64, ranges: &mut [(u64, u64)]) -> u64 {
    ranges.sort_unstable();
    let mut covered = 0u64;
    let mut reach = 0u64;
    for &(start, end) in ranges.iter() {
        let (start, end) = (start.max(reach).min(len), end.min(len));
        if end > start {
            covered += end - start;
            reach = end;
        }
    }
    len - covered
}

impl Rule for UnusedBufferBytes {
    fn info(&self) -> &'static RuleInfo {
        &MESH_003
    }

    fn check(&self, asset: &Asset, _config: &Config) -> Vec<Finding> {
        let Some(doc) = &asset.document else {
            return Vec::new();
        };
        let mut ranges = covered_ranges(doc);
        let mut dead = 0u64;
        let mut total = 0u64;
        let mut buffers = 0u64;
        for (i, buffer) in doc.buffers.iter().enumerate() {
            // a meshopt fallback has no bytes on purpose; a remote buffer is not read
            if buffer.is_meshopt_fallback()
                || buffer.uri.as_deref().is_some_and(|u| u.contains("://"))
            {
                continue;
            }
            let unused = uncovered(
                buffer.byte_length,
                ranges.get_mut(&i).map_or(&mut [], Vec::as_mut_slice),
            );
            if unused > DEAD_BYTES_MAX || unused * 100 > buffer.byte_length * DEAD_PERCENT_MAX {
                dead += unused;
                total += buffer.byte_length;
                buffers += 1;
            }
        }
        if buffers == 0 {
            return Vec::new();
        }
        let message = format!(
            "{} of {} ({}%) in {} is not used by any bufferView",
            size(dead),
            size(total),
            dead * 100 / total.max(1),
            plural(buffers, "buffer", "buffers")
        );
        let f = finding(
            &MESH_003,
            asset,
            Measured::exact("unused buffer bytes", dead, "bytes"),
            Measured::exact("unused bytes", DEAD_BYTES_MAX, "bytes"),
            message,
            &MESH_003_ADVICE,
        );
        vec![f]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncovered_merges_overlaps_and_clamps() {
        assert_eq!(uncovered(100, &mut []), 100);
        assert_eq!(uncovered(100, &mut [(0, 50), (40, 60)]), 40);
        assert_eq!(uncovered(100, &mut [(60, 80), (0, 10)]), 70);
        assert_eq!(uncovered(100, &mut [(0, 30), (5, 10), (90, 400)]), 60);
    }

    #[test]
    fn numbers_read_well() {
        assert_eq!(thousands(505_000), "505,000");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(size(1_067_784), "1.1 MB");
        assert_eq!(size(204_800), "204.8 KB");
    }

    #[test]
    fn declared_size_follows_type() {
        let a = Accessor {
            count: 10,
            component_type: 5126,
            kind: "VEC3".into(),
            ..Accessor::default()
        };
        assert_eq!(declared_bytes(&a), 120);
    }
}
