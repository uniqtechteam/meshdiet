//! Generated textures: solid colours (which compress to almost nothing) and noise (which does not),
//! as PNG or JPEG, at chosen sizes and colour types.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

/// What the pixels look like.
#[derive(Debug, Clone, Copy)]
pub enum Pixels {
    /// One colour everywhere.
    Solid([u8; 4]),
    /// Pseudo-random bytes from a fixed seed: the same every run, but they do not compress.
    Noise(u64),
}

/// How the texture is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// PNG, RGB or RGBA.
    Png,
    /// JPEG at this quality (always RGB).
    Jpeg(u8),
}

/// A texture to generate.
#[derive(Debug, Clone)]
pub struct Texture {
    /// Name used for the glTF image and, for external files, the file name.
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Whether the PNG carries an alpha channel.
    pub alpha: bool,
    /// The pixels.
    pub pixels: Pixels,
    /// The file format.
    pub encoding: Encoding,
}

impl Texture {
    /// A PNG texture.
    #[must_use]
    pub fn png(name: &str, width: u32, height: u32, alpha: bool, pixels: Pixels) -> Self {
        Self {
            name: name.to_owned(),
            width,
            height,
            alpha,
            pixels,
            encoding: Encoding::Png,
        }
    }

    /// A JPEG texture.
    #[must_use]
    pub fn jpeg(name: &str, width: u32, height: u32, quality: u8, pixels: Pixels) -> Self {
        Self {
            name: name.to_owned(),
            width,
            height,
            alpha: false,
            pixels,
            encoding: Encoding::Jpeg(quality),
        }
    }

    /// MIME type for the glTF image.
    #[must_use]
    pub fn mime(&self) -> &'static str {
        match self.encoding {
            Encoding::Png => "image/png",
            Encoding::Jpeg(_) => "image/jpeg",
        }
    }

    /// File extension for an external image.
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self.encoding {
            Encoding::Png => "png",
            Encoding::Jpeg(_) => "jpg",
        }
    }

    fn channels(&self) -> usize {
        if self.alpha && self.encoding == Encoding::Png {
            4
        } else {
            3
        }
    }

    fn raw(&self) -> Vec<u8> {
        let n = self.width as usize * self.height as usize * self.channels();
        match self.pixels {
            Pixels::Solid(rgba) => rgba[..self.channels()].repeat(n / self.channels()),
            Pixels::Noise(seed) => {
                let mut state = seed | 1;
                let mut out = Vec::with_capacity(n + 8);
                while out.len() < n {
                    // xorshift64*: fast, fixed, good enough to defeat deflate
                    state ^= state >> 12;
                    state ^= state << 25;
                    state ^= state >> 27;
                    out.extend_from_slice(&state.wrapping_mul(0x2545_F491_4F6C_DD1D).to_le_bytes());
                }
                out.truncate(n);
                out
            }
        }
    }

    /// The encoded file bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let raw = self.raw();
        let mut out = Cursor::new(Vec::new());
        let result = match self.encoding {
            Encoding::Png => {
                PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive)
                    .write_image(
                        &raw,
                        self.width,
                        self.height,
                        if self.alpha {
                            ExtendedColorType::Rgba8
                        } else {
                            ExtendedColorType::Rgb8
                        },
                    )
            }
            Encoding::Jpeg(q) => JpegEncoder::new_with_quality(&mut out, q).write_image(
                &raw,
                self.width,
                self.height,
                ExtendedColorType::Rgb8,
            ),
        };
        if result.is_err() {
            return Vec::new();
        }
        out.into_inner()
    }
}
