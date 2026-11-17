//! Image facts from the first bytes of a file: format, width, height, for PNG whether it can be
//! transparent, and for KTX2 the supercompression scheme. Pixels are never decoded (ADR-003).

use ktx2::SupercompressionScheme;
use serde::Serialize;

/// How many bytes of an image are read to find its header. JPEG size markers can sit behind large
/// EXIF or ICC blocks, so this is generous; it is still a header, not the image.
pub const HEAD_BYTES: u64 = 512 * 1024;

/// What the header says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// MIME type from the magic bytes.
    pub mime: &'static str,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// KTX2 only: `basis-lz`, `zstd`, `zlib` or `none`.
    pub supercompression: Option<&'static str>,
    /// PNG only: what the header says about transparency.
    pub alpha: Option<PngAlpha>,
}

/// What a PNG header says about transparency, from the IHDR colour type and the chunk list before
/// the image data. The pixels are not looked at, so an RGBA file whose alpha is all opaque still
/// counts as `Channel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PngAlpha {
    /// Greyscale or RGB, no `tRNS` chunk: the file cannot be transparent.
    None,
    /// Greyscale with alpha or RGBA.
    Channel,
    /// A `tRNS` chunk adds transparency to a greyscale, RGB or palette image.
    Trns,
    /// Palette colours with no `tRNS` chunk.
    Palette,
}

/// Transparency of a PNG from its first bytes: the IHDR colour type, then the chunk headers up to
/// the first `IDAT` (where `tRNS` must appear). `None` when the header is broken or the chunks before
/// the image data run past `head`, so nothing is claimed that was not read.
#[must_use]
pub fn png_alpha(head: &[u8]) -> Option<PngAlpha> {
    let color_type = *head.get(25)?;
    if head.get(12..16)? != b"IHDR" {
        return None;
    }
    match color_type {
        4 | 6 => return Some(PngAlpha::Channel),
        0 | 2 | 3 => {}
        _ => return None,
    }
    let mut at = 8usize;
    loop {
        let len =
            usize::try_from(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?)).ok()?;
        match head.get(at + 4..at + 8)? {
            b"tRNS" => return Some(PngAlpha::Trns),
            b"IDAT" | b"IEND" => {
                return Some(if color_type == 3 {
                    PngAlpha::Palette
                } else {
                    PngAlpha::None
                });
            }
            _ => at = at.checked_add(len)?.checked_add(12)?,
        }
    }
}

/// The MIME type from the magic bytes, if the format is one glTF allows.
#[must_use]
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if head.starts_with(b"\xABKTX 20\xBB\r\n\x1a\n") {
        Some("image/ktx2")
    } else if head.len() >= 12
        && &head[4..8] == b"ftyp"
        && matches!(&head[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else {
        None
    }
}

/// Read the format and size from the first bytes of an image.
///
/// # Errors
/// When the format is unknown or the header is cut off or broken.
pub fn header(head: &[u8]) -> Result<Header, String> {
    let Some(mime) = sniff(head) else {
        return Err("not a PNG, JPEG, WebP, AVIF or KTX2 image".to_owned());
    };
    if mime == "image/ktx2" {
        let mut fixed = [0u8; ktx2::Header::LENGTH];
        let src = head
            .get(..ktx2::Header::LENGTH)
            .ok_or("the KTX2 header is cut off")?;
        fixed.copy_from_slice(src);
        let h = ktx2::Header::from_bytes(&fixed).map_err(|e| format!("bad KTX2 header: {e:?}"))?;
        let scheme = match h.supercompression_scheme {
            None => "none",
            Some(s) if s == SupercompressionScheme::BasisLZ => "basis-lz",
            Some(s) if s == SupercompressionScheme::Zstandard => "zstd",
            Some(s) if s == SupercompressionScheme::ZLIB => "zlib",
            Some(_) => "other",
        };
        return Ok(Header {
            mime,
            width: h.pixel_width,
            height: h.pixel_height,
            supercompression: Some(scheme),
            alpha: None,
        });
    }
    let size = imagesize::blob_size(head).map_err(|e| format!("bad {mime} header: {e}"))?;
    Ok(Header {
        mime,
        width: u32::try_from(size.width).unwrap_or(u32::MAX),
        height: u32::try_from(size.height).unwrap_or(u32::MAX),
        supercompression: None,
        alpha: if mime == "image/png" {
            png_alpha(head)
        } else {
            None
        },
    })
}

/// Estimated GPU memory for a texture, mipmaps included (× 4/3).
///
/// PNG, JPEG, WebP and AVIF are decoded to RGBA8 on the GPU: 4 bytes per pixel. KTX2 is transcoded
/// to a block format: about 1 byte per pixel (BC7, ASTC 4x4) or less.
#[must_use]
pub fn est_vram(mime: &str, width: u32, height: u32) -> u64 {
    let pixels = u64::from(width) * u64::from(height);
    let per_pixel = if mime == "image/ktx2" { 1 } else { 4 };
    pixels * per_pixel * 4 / 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_003_vram_of_4096_square_png_is_89_5_mb() {
        assert_eq!(est_vram("image/png", 4096, 4096), 89_478_485);
        assert_eq!(est_vram("image/png", 2048, 2048), 22_369_621);
    }

    /// A PNG made of chunk headers only (lengths and CRCs are not checked here).
    fn png(color_type: u8, chunks: &[(&[u8; 4], u32)]) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(13u32.to_be_bytes());
        out.extend(b"IHDR");
        out.extend(64u32.to_be_bytes());
        out.extend(64u32.to_be_bytes());
        out.extend([8, color_type, 0, 0, 0]);
        out.extend([0; 4]);
        for (name, len) in chunks {
            out.extend(len.to_be_bytes());
            out.extend(*name);
            out.extend(vec![0; *len as usize + 4]);
        }
        out
    }

    #[test]
    fn png_alpha_comes_from_the_colour_type_and_trns() {
        let idat = (b"IDAT", 10);
        assert_eq!(png_alpha(&png(2, &[idat])), Some(PngAlpha::None));
        assert_eq!(
            png_alpha(&png(0, &[(b"gAMA", 4), idat])),
            Some(PngAlpha::None)
        );
        assert_eq!(png_alpha(&png(6, &[idat])), Some(PngAlpha::Channel));
        assert_eq!(png_alpha(&png(4, &[idat])), Some(PngAlpha::Channel));
        assert_eq!(
            png_alpha(&png(2, &[(b"tRNS", 6), idat])),
            Some(PngAlpha::Trns)
        );
        assert_eq!(
            png_alpha(&png(3, &[(b"PLTE", 6), idat])),
            Some(PngAlpha::Palette)
        );
        assert_eq!(
            png_alpha(&png(3, &[(b"PLTE", 6), (b"tRNS", 2), idat])),
            Some(PngAlpha::Trns)
        );
    }

    #[test]
    fn png_alpha_claims_nothing_it_did_not_read() {
        // a big text chunk runs past the bytes read, so a tRNS could still follow
        let mut head = png(2, &[(b"iTXt", 100_000), (b"IDAT", 10)]);
        head.truncate(200);
        assert_eq!(png_alpha(&head), None);
        assert_eq!(png_alpha(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(png_alpha(&png(2, &[(b"IDAT", 10)])[..30]), None);
    }

    #[test]
    fn unknown_bytes_are_an_error_not_a_panic() {
        assert!(header(b"").is_err());
        assert!(header(b"GIF89a").is_err());
        assert!(header(b"\x89PNG\r\n\x1a\n").is_err());
        assert!(header(b"\xABKTX 20\xBB\r\n\x1a\n").is_err());
    }
}
