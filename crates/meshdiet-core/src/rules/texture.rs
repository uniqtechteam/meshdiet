//! SPEC-RULE-001 FR-005…008: the texture rules TEX-001…004.
//!
//! Everything here comes from image headers and the glTF JSON; no pixel is decoded (BR-002). GPU
//! memory figures are estimates and are labelled `est.` (SPEC-CLI-001 BR-002).

use std::collections::BTreeSet;

use crate::images::{self, PngAlpha};
use crate::rule::{Rule, RuleInfo};
use crate::{Asset, Confidence, Config, Finding, ImageInfo, Measured, Severity};

/// TEX-002 fires above this much estimated GPU memory of uncompressed textures per asset.
pub const VRAM_MAX: u64 = 32_000_000;

/// TEX-003 looks only at PNGs larger than this.
pub const PNG_MIN: u64 = 256_000;

/// TEX-001.
pub const TEX_001: RuleInfo = RuleInfo {
    id: "TEX-001",
    title: "Texture dimensions over the limit",
    severity: Severity::Medium,
    confidence: Confidence::High,
    summary: "an image wider or taller than texture_max_px (2048 by default), with its estimated GPU memory",
};

/// TEX-002.
pub const TEX_002: RuleInfo = RuleInfo {
    id: "TEX-002",
    title: "No GPU texture compression",
    severity: Severity::Low,
    confidence: Confidence::High,
    summary: "PNG, JPEG, WebP or AVIF textures whose estimated GPU memory is over 32 MB, where KTX2 would take about a quarter",
};

/// TEX-003.
pub const TEX_003: RuleInfo = RuleInfo {
    id: "TEX-003",
    title: "PNG with no alpha channel",
    severity: Severity::Medium,
    confidence: Confidence::High,
    summary: "a greyscale, RGB or palette PNG over 256 KB that cannot be transparent, so JPEG or WebP would be far smaller",
};

/// TEX-004.
pub const TEX_004: RuleInfo = RuleInfo {
    id: "TEX-004",
    title: "Image bytes over the limit",
    severity: Severity::Medium,
    confidence: Confidence::High,
    summary: "an image stored with more bytes than image_max (1 MB by default)",
};

/// The texture rules, in id order.
#[must_use]
pub fn rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(Dimensions),
        Box::new(GpuCompression),
        Box::new(PngNoAlpha),
        Box::new(ImageBytes),
    ]
}

/// Bytes in decimal units, one decimal: `22.4 MB`, `408.4 KB`, `812 B`.
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
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.1} GB", b / 1e9)
    }
}

/// How an image is named in a message: its name, its file, or its index.
fn label(image: &ImageInfo) -> String {
    image
        .name
        .clone()
        .or_else(|| image.file.clone())
        .unwrap_or_else(|| format!("image {}", image.index))
}

/// `PNG`, `JPEG`, `KTX2`, … from the MIME type.
fn format_name(image: &ImageInfo) -> String {
    image.mime.as_deref().map_or_else(
        || "image".to_owned(),
        |m| m.trim_start_matches("image/").to_ascii_uppercase(),
    )
}

/// Images that are only the PNG/JPEG fallback of a `KHR_texture_basisu` texture: the asset counts
/// as KTX2 for them (SPEC-RULE-001 §16).
fn fallbacks(asset: &Asset) -> BTreeSet<usize> {
    let Some(doc) = &asset.document else {
        return BTreeSet::new();
    };
    doc.textures
        .iter()
        .filter_map(|t| {
            let ktx2 = t.basisu_source()?;
            t.source.filter(|s| *s != ktx2)
        })
        .collect()
}

/// Images used as a normal map by some material.
fn normal_maps(asset: &Asset) -> BTreeSet<usize> {
    let Some(doc) = &asset.document else {
        return BTreeSet::new();
    };
    doc.materials
        .iter()
        .filter_map(|m| m.normal_texture.as_ref())
        .filter_map(|r| doc.textures.get(r.index))
        .flat_map(|t| [t.source, t.basisu_source()])
        .flatten()
        .collect()
}

fn is_ktx2(image: &ImageInfo) -> bool {
    image.mime.as_deref() == Some("image/ktx2")
}

fn estimated(name: &str, value: u64) -> Measured {
    Measured {
        name: name.to_owned(),
        value,
        unit: "bytes".to_owned(),
        estimated: true,
    }
}

/// The words of a finding.
struct Text {
    message: String,
    why: &'static str,
    fix: String,
}

fn finding(
    info: &RuleInfo,
    asset: &Asset,
    measured: Measured,
    threshold: Measured,
    text: Text,
) -> Finding {
    Finding {
        rule: info.id.to_owned(),
        title: info.title.to_owned(),
        severity: info.severity,
        confidence: info.confidence,
        path: asset.path.clone(),
        measured,
        threshold: Some(threshold),
        message: text.message,
        why: text.why.to_owned(),
        fix: text.fix,
        fingerprint: String::new(),
    }
}

/// TEX-001 (FR-005): an image wider or taller than `texture_max_px`.
struct Dimensions;

impl Rule for Dimensions {
    fn info(&self) -> &'static RuleInfo {
        &TEX_001
    }

    fn check(&self, asset: &Asset, config: &Config) -> Vec<Finding> {
        let limit = config.texture_max_px;
        let skip = fallbacks(asset);
        let mut out = Vec::new();
        for image in asset.images.iter().filter(|i| !skip.contains(&i.index)) {
            let (Some(w), Some(h)) = (image.width, image.height) else {
                continue;
            };
            let side = w.max(h);
            if side <= limit {
                continue;
            }
            let mime = image.mime.as_deref().unwrap_or("image/png");
            let (sw, sh) = scaled(w, h, limit);
            let now = images::est_vram(mime, w, h);
            let then = images::est_vram(mime, sw, sh);
            let message = format!(
                "{}: {w}×{h} {}, over the {limit} px limit; est. {} GPU memory, est. {} at {sw}×{sh}",
                label(image),
                format_name(image),
                size(now),
                size(then)
            );
            out.push(finding(
                &TEX_001,
                asset,
                Measured::exact("longest side", u64::from(side), "pixels"),
                Measured::exact("texture_max_px", u64::from(limit), "pixels"),
                Text {
                    message,
                    why: "Every texture is uploaded to the GPU at full size, whatever its file size, and on phones that memory runs out first.",
                    fix: format!(
                        "Resize to {limit} px: `meshdiet fix` (gltfpack -tl {limit}) or `gltf-transform resize in.glb out.glb --width {limit} --height {limit}`."
                    ),
                },
            ));
        }
        out
    }
}

/// `w`×`h` scaled down so the longest side is `limit`, keeping the aspect ratio.
fn scaled(w: u32, h: u32, limit: u32) -> (u32, u32) {
    let side = u64::from(w.max(h));
    let fit = |v: u32| {
        u32::try_from((u64::from(v) * u64::from(limit) + side / 2) / side)
            .unwrap_or(limit)
            .max(1)
    };
    (fit(w), fit(h))
}

/// TEX-002 (FR-006): uncompressed textures over 32 MB of estimated GPU memory in one asset.
struct GpuCompression;

impl Rule for GpuCompression {
    fn info(&self) -> &'static RuleInfo {
        &TEX_002
    }

    fn check(&self, asset: &Asset, _config: &Config) -> Vec<Finding> {
        let skip = fallbacks(asset);
        let plain: Vec<&ImageInfo> = asset
            .images
            .iter()
            .filter(|i| !is_ktx2(i) && !skip.contains(&i.index))
            .filter(|i| i.width.is_some() && i.height.is_some())
            .collect();
        let vram: u64 = plain
            .iter()
            .filter_map(|i| i.est_vram)
            .map(|e| e.bytes)
            .sum();
        if vram <= VRAM_MAX {
            return Vec::new();
        }
        let as_ktx2: u64 = plain
            .iter()
            .map(|i| images::est_vram("image/ktx2", i.width.unwrap_or(0), i.height.unwrap_or(0)))
            .sum();
        let mut formats: Vec<String> = plain.iter().map(|i| format_name(i)).collect();
        formats.sort();
        formats.dedup();
        let message = format!(
            "{} {} {} est. {} of GPU memory, over 32.0 MB; as KTX2 about est. {}",
            plain.len(),
            formats.join("/"),
            if plain.len() == 1 {
                "texture takes"
            } else {
                "textures take"
            },
            size(vram),
            size(as_ktx2)
        );
        vec![finding(
            &TEX_002,
            asset,
            estimated("texture GPU memory", vram),
            Measured::exact("GPU memory limit", VRAM_MAX, "bytes"),
            Text {
                message,
                why: "PNG, JPEG and WebP are unpacked to 4 bytes per pixel on the GPU; KTX2 stays compressed there, so the page uses less memory and uploads faster.",
                fix: "Compress textures to KTX2: `meshdiet fix --textures ktx2` (gltfpack -tc) or `gltf-transform etc1s in.glb out.glb` (needs KTX-Software), then add KTX2Loader to the app.".to_owned(),
            },
        )]
    }
}

/// TEX-003 (FR-007): a PNG over 256 KB that cannot be transparent.
struct PngNoAlpha;

impl Rule for PngNoAlpha {
    fn info(&self) -> &'static RuleInfo {
        &TEX_003
    }

    fn check(&self, asset: &Asset, _config: &Config) -> Vec<Finding> {
        let skip = fallbacks(asset);
        let normals = normal_maps(asset);
        asset
            .images
            .iter()
            .filter(|i| !skip.contains(&i.index))
            .filter(|i| i.mime.as_deref() == Some("image/png"))
            .filter(|i| {
                matches!(i.alpha, Some(PngAlpha::None | PngAlpha::Palette)) && i.bytes > PNG_MIN
            })
            .map(|image| {
                let normal = normals.contains(&image.index);
                let message = format!(
                    "{}: {} {}PNG{} with no alpha channel",
                    label(image),
                    size(image.bytes),
                    if image.alpha == Some(PngAlpha::Palette) {
                        "palette "
                    } else {
                        ""
                    },
                    if normal { " normal map" } else { "" }
                );
                let (why, fix) = if normal {
                    (
                        "PNG is lossless and this one has no transparency to keep; WebP stores a normal map far smaller (JPEG blocks would show up as bumps in the lighting).",
                        "Store it as WebP: `meshdiet fix` (gltfpack -tw) or `gltf-transform webp in.glb out.glb`.",
                    )
                } else {
                    (
                        "PNG is lossless and this one has no transparency to keep; as JPEG or WebP the same texture is usually several times smaller.",
                        "Store it as WebP or JPEG: `meshdiet fix` (gltfpack -tw) or `gltf-transform webp in.glb out.glb` (`gltf-transform jpeg` for JPEG).",
                    )
                };
                finding(
                    &TEX_003,
                    asset,
                    Measured::exact("image bytes", image.bytes, "bytes"),
                    Measured::exact("PNG size limit", PNG_MIN, "bytes"),
                    Text {
                        message,
                        why,
                        fix: fix.to_owned(),
                    },
                )
            })
            .collect()
    }
}

/// TEX-004 (FR-008): an image stored with more bytes than `image_max`.
struct ImageBytes;

impl Rule for ImageBytes {
    fn info(&self) -> &'static RuleInfo {
        &TEX_004
    }

    fn check(&self, asset: &Asset, config: &Config) -> Vec<Finding> {
        asset
            .images
            .iter()
            .filter(|i| i.bytes > config.image_max)
            .map(|image| {
                let dims = match (image.width, image.height) {
                    (Some(w), Some(h)) => format!(" {w}×{h}"),
                    _ => String::new(),
                };
                let message = format!(
                    "{}: {}{dims} {}, over the {} limit",
                    label(image),
                    size(image.bytes),
                    format_name(image),
                    size(config.image_max)
                );
                finding(
                    &TEX_004,
                    asset,
                    Measured::exact("image bytes", image.bytes, "bytes"),
                    Measured::exact("image_max", config.image_max, "bytes"),
                    Text {
                        message,
                        why: "The browser downloads the whole image before the model can show, and images are often most of a model's weight.",
                        fix: "Recompress it: `meshdiet fix` (gltfpack -tw, WebP) or `gltf-transform webp in.glb out.glb`; resize it too if it is larger than it is ever shown (`gltf-transform resize`).".to_owned(),
                    },
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf::Document;
    use crate::{AssetKind, Compression, Estimate, ImageSource};

    fn image(index: usize, mime: &str, side: u32, bytes: u64) -> ImageInfo {
        ImageInfo {
            index,
            name: None,
            mime: Some(mime.to_owned()),
            source: ImageSource::BufferView,
            file: None,
            bytes,
            width: Some(side),
            height: Some(side),
            est_vram: Some(Estimate {
                bytes: images::est_vram(mime, side, side),
                estimated: true,
            }),
            supercompression: None,
            alpha: (mime == "image/png").then_some(PngAlpha::None),
        }
    }

    fn asset(json: &str, images: Vec<ImageInfo>) -> Asset {
        Asset {
            path: "a.glb".to_owned(),
            kind: AssetKind::Glb,
            files: vec!["a.glb".to_owned()],
            bytes: images.iter().map(|i| i.bytes).sum(),
            geometry_bytes: 0,
            texture_bytes: 0,
            other_bytes: 0,
            triangles: 0,
            primitives: 0,
            compression: Compression::default(),
            images,
            problems: Vec::new(),
            file_bytes: Vec::new(),
            document: Document::parse(json.as_bytes()).ok(),
        }
    }

    fn run(rule: &dyn Rule, asset: &Asset) -> Vec<Finding> {
        rule.check(asset, &Config::default())
    }

    #[test]
    fn ktx2_images_never_trigger_tex_002() {
        let a = asset(
            r#"{"asset":{"version":"2.0"}}"#,
            vec![
                image(0, "image/ktx2", 8192, 9_000_000),
                image(1, "image/ktx2", 8192, 9_000_000),
            ],
        );
        assert_eq!(run(&GpuCompression, &a), Vec::<Finding>::new());
        let mixed = asset(
            r#"{"asset":{"version":"2.0"}}"#,
            vec![
                image(0, "image/ktx2", 8192, 9_000_000),
                image(1, "image/png", 1024, 900_000),
            ],
        );
        assert_eq!(run(&GpuCompression, &mixed), Vec::<Finding>::new());
    }

    #[test]
    fn basisu_with_png_fallback_is_treated_as_ktx2() {
        let json = r#"{"asset":{"version":"2.0"},
            "extensionsUsed":["KHR_texture_basisu"],
            "textures":[{"source":0,"extensions":{"KHR_texture_basisu":{"source":1}}}]}"#;
        let a = asset(
            json,
            vec![
                image(0, "image/png", 4096, 2_000_000),
                image(1, "image/ktx2", 4096, 3_000_000),
            ],
        );
        assert_eq!(run(&GpuCompression, &a), Vec::<Finding>::new());
        assert_eq!(run(&PngNoAlpha, &a), Vec::<Finding>::new());
        // the KTX2 image is the one that is too big; the fallback is not reported twice
        let tex_001 = run(&Dimensions, &a);
        assert_eq!(tex_001.len(), 1);
        assert!(
            tex_001[0].message.contains("KTX2"),
            "{}",
            tex_001[0].message
        );
        // the fallback bytes still count
        assert_eq!(run(&ImageBytes, &a).len(), 2);
    }

    #[test]
    fn normal_maps_get_webp_not_jpeg() {
        let json = r#"{"asset":{"version":"2.0"},
            "textures":[{"source":0},{"source":1}],
            "materials":[{"normalTexture":{"index":1},"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}]}"#;
        let a = asset(
            json,
            vec![
                image(0, "image/png", 512, 400_000),
                image(1, "image/png", 512, 400_000),
            ],
        );
        let found = run(&PngNoAlpha, &a);
        assert_eq!(found.len(), 2);
        assert!(found[0].fix.contains("JPEG"));
        assert!(found[1].message.contains("normal map"));
        assert!(found[1].fix.contains("WebP") && !found[1].fix.contains("JPEG"));
    }

    #[test]
    fn tex_003_needs_a_png_with_no_alpha_over_256_kb() {
        let doc = r#"{"asset":{"version":"2.0"}}"#;
        let mut small = image(0, "image/png", 512, PNG_MIN);
        assert_eq!(
            run(&PngNoAlpha, &asset(doc, vec![small.clone()])),
            Vec::<Finding>::new()
        );
        small.bytes += 1;
        assert_eq!(run(&PngNoAlpha, &asset(doc, vec![small.clone()])).len(), 1);
        // an indexed PNG with no tRNS cannot be transparent either (QA-001: 2048² anisotropy and
        // metal/roughness maps of 0.6 to 2.8 MB)
        small.alpha = Some(PngAlpha::Palette);
        let found = run(&PngNoAlpha, &asset(doc, vec![small.clone()]));
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("palette PNG with no alpha"));
        for alpha in [Some(PngAlpha::Channel), Some(PngAlpha::Trns), None] {
            small.alpha = alpha;
            assert_eq!(
                run(&PngNoAlpha, &asset(doc, vec![small.clone()])),
                Vec::<Finding>::new()
            );
        }
    }

    #[test]
    fn tex_001_scales_the_estimate_keeping_the_aspect_ratio() {
        assert_eq!(scaled(4096, 4096, 2048), (2048, 2048));
        assert_eq!(scaled(8192, 2048, 2048), (2048, 512));
        assert_eq!(scaled(3000, 1, 2048), (2048, 1));
        let mut wide = image(0, "image/jpeg", 2048, 500_000);
        wide.width = Some(4096);
        let found = run(
            &Dimensions,
            &asset(r#"{"asset":{"version":"2.0"}}"#, vec![wide]),
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("4096×2048 JPEG"));
        assert!(
            found[0].message.ends_with("at 2048×1024"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn tex_002_measures_an_estimate() {
        let a = asset(
            r#"{"asset":{"version":"2.0"}}"#,
            vec![image(0, "image/webp", 4096, 900_000)],
        );
        let found = run(&GpuCompression, &a);
        assert_eq!(found.len(), 1);
        assert!(found[0].measured.estimated);
        assert_eq!(found[0].measured.value, 89_478_485);
        assert!(
            found[0]
                .message
                .contains("1 WEBP texture takes est. 89.5 MB")
        );
    }
}
