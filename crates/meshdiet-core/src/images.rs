//! Image facts from the first bytes of a file: format, width, height and, for KTX2, the
//! supercompression scheme. Pixels are never decoded (ADR-003).

use ktx2::SupercompressionScheme;

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
        });
    }
    let size = imagesize::blob_size(head).map_err(|e| format!("bad {mime} header: {e}"))?;
    Ok(Header {
        mime,
        width: u32::try_from(size.width).unwrap_or(u32::MAX),
        height: u32::try_from(size.height).unwrap_or(u32::MAX),
        supercompression: None,
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

    #[test]
    fn unknown_bytes_are_an_error_not_a_panic() {
        assert!(header(b"").is_err());
        assert!(header(b"GIF89a").is_err());
        assert!(header(b"\x89PNG\r\n\x1a\n").is_err());
        assert!(header(b"\xABKTX 20\xBB\r\n\x1a\n").is_err());
    }
}
