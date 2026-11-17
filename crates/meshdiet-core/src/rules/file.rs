//! File rule GLTF-002 (SPEC-RULE-001 FR-010): buffers and images stored as base64 `data:` URIs
//! inside a `.gltf`. Sizes come from the JSON text; nothing is decoded (ADR-003).

use super::mesh::{plural, size};
use crate::rule::{Rule, RuleInfo};
use crate::{Asset, AssetKind, Confidence, Config, Finding, Measured, Severity};

/// A data URI is reported when its text is longer than this (16 KB).
pub const EMBEDDED_MAX: u64 = 16_000;

/// GLTF-002: base64 data embedded in a `.gltf`.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbeddedData;

static GLTF_002: RuleInfo = RuleInfo {
    id: "GLTF-002",
    title: "Embedded base64 data",
    severity: Severity::Medium,
    confidence: Confidence::High,
    summary: "a .gltf that stores a buffer or image as a base64 data URI over 16 KB",
};

/// Length of `uri` when it is a base64 data URI.
fn base64_len(uri: Option<&str>) -> Option<u64> {
    let uri = uri?;
    let (meta, _) = uri.strip_prefix("data:")?.split_once(',')?;
    meta.ends_with(";base64").then_some(uri.len() as u64)
}

impl Rule for EmbeddedData {
    fn info(&self) -> &'static RuleInfo {
        &GLTF_002
    }

    fn check(&self, asset: &Asset, _config: &Config) -> Vec<Finding> {
        let Some(doc) = &asset.document else {
            return Vec::new();
        };
        if asset.kind != AssetKind::Gltf {
            return Vec::new();
        }
        // (text bytes, data bytes) of every data URI over the limit
        let buffers: Vec<(u64, u64)> = doc
            .buffers
            .iter()
            .filter_map(|b| Some((base64_len(b.uri.as_deref())?, b.byte_length)))
            .filter(|(text, _)| *text > EMBEDDED_MAX)
            .collect();
        let images: Vec<(u64, u64)> = doc
            .images
            .iter()
            .enumerate()
            .filter_map(|(i, img)| {
                let text = base64_len(img.uri.as_deref())?;
                Some((text, asset.images.get(i).map_or(0, |info| info.bytes)))
            })
            .filter(|(text, _)| *text > EMBEDDED_MAX)
            .collect();
        if buffers.is_empty() && images.is_empty() {
            return Vec::new();
        }
        let text: u64 = buffers.iter().chain(&images).map(|(t, _)| t).sum();
        let data: u64 = buffers.iter().chain(&images).map(|(_, d)| d).sum();
        let what = match (buffers.len() as u64, images.len() as u64) {
            (b, 0) => plural(b, "buffer", "buffers"),
            (0, i) => plural(i, "image", "images"),
            (b, i) => format!(
                "{} and {}",
                plural(b, "buffer", "buffers"),
                plural(i, "image", "images")
            ),
        };
        vec![Finding {
            rule: GLTF_002.id.to_owned(),
            title: GLTF_002.title.to_owned(),
            severity: GLTF_002.severity,
            confidence: GLTF_002.confidence,
            path: asset.path.clone(),
            measured: Measured::exact("base64 bytes", text, "bytes"),
            threshold: Some(Measured::exact("data URI size", EMBEDDED_MAX, "bytes")),
            message: format!(
                "{what} embedded as base64: {} of text for {} of data, {} extra",
                size(text),
                size(data),
                size(text.saturating_sub(data))
            ),
            why: "Base64 makes the data about a third bigger, and the loader has to download and \
                  parse the whole JSON before it can use any of it."
                .to_owned(),
            fix: "Convert it to one binary file: `meshdiet fix` (gltfpack writes .glb), \
                  `gltf-transform copy in.gltf out.glb`, or `gltfpack -i in.gltf -o out.glb`."
                .to_owned(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_base64_data_uris_count() {
        assert_eq!(
            base64_len(Some("data:application/gltf-buffer;base64,AAAA")),
            Some(40)
        );
        assert_eq!(base64_len(Some("data:text/plain,hello")), None);
        assert_eq!(base64_len(Some("model.bin")), None);
        assert_eq!(base64_len(None), None);
    }
}
