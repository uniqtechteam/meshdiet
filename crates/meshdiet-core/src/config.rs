//! Limits the rules compare against. Defaults from ADR-008; reading `meshdiet.toml` comes with
//! TASK-CLI-002.

/// Budgets and limits, in bytes, pixels or triangles. 1 MB = 1,000,000 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// SIZE-001: largest asset.
    pub file_max: u64,
    /// SIZE-002: whole build, off when `None`.
    pub build_max: Option<u64>,
    /// TEX-001: widest or tallest texture.
    pub texture_max_px: u32,
    /// TEX-004: largest stored image.
    pub image_max: u64,
    /// MESH-002: triangles drawn per asset.
    pub triangles_max: u64,
    /// MESH-001: geometry smaller than this is not worth compressing.
    pub compress_min: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            file_max: 5_000_000,
            build_max: None,
            texture_max_px: 2048,
            image_max: 1_000_000,
            triangles_max: 500_000,
            compress_min: 64_000,
        }
    }
}
