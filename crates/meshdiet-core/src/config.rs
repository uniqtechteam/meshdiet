//! Limits the rules compare against, with defaults from ADR-008.
//!
//! `meshdiet.toml` is read here for the `[budgets]` and `[pages]` tables (SPEC-WEIGHT-001). The other
//! ADR-008 keys (`fail_on`, `paths`, `ignore`, `[rules]`) are accepted but not used yet: they come with
//! TASK-CLI-002. Any other key is an error, so a typo never silently drops a budget.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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
    /// SIZE-003: page groups, sorted by name.
    pub pages: Vec<PageGroup>,
    /// The `meshdiet.toml` these values came from, if any. Budget findings point at it.
    pub source: Option<PathBuf>,
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
            pages: Vec::new(),
            source: None,
        }
    }
}

/// One `[pages]` entry: the assets a page loads, and how much it may ship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageGroup {
    /// The page, for example `/shop`.
    pub name: String,
    /// Globs over asset paths (`*` stays inside a folder, `**` crosses folders).
    pub assets: Vec<String>,
    /// Budget for the group, SIZE-003; no budget when `None`.
    pub max: Option<u64>,
}

/// A size in the config: `"5 MB"`, `"64 KB"`, `"812 B"`, or a plain number of bytes.
#[derive(Deserialize)]
#[serde(untagged)]
enum Size {
    Bytes(u64),
    Text(String),
}

impl Size {
    fn bytes(&self, key: &str) -> Result<u64, String> {
        match self {
            Self::Bytes(n) => Ok(*n),
            Self::Text(t) => parse_size(t).ok_or_else(|| {
                format!("{key} = \"{t}\" is not a size; write it like \"5 MB\", \"64 KB\" or \"812 B\" (1 MB = 1,000,000 bytes)")
            }),
        }
    }
}

/// Parse `"5 MB"`, `"1.5MB"`, `"64 KB"`, `"812 B"`. Decimal units only; `MiB` and negatives are
/// rejected.
#[must_use]
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let scale: u64 = match unit.trim() {
        "B" | "" => 1,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        _ => return None,
    };
    let (whole, frac) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut value = whole.checked_mul(scale)?;
    // decimals exactly, digit by digit, so "1.1 MB" is 1,100,000 and not 1,099,999
    let mut place = scale;
    for d in frac.chars() {
        let digit = u64::from(d.to_digit(10)?);
        place /= 10;
        value = value.checked_add(digit * place)?;
    }
    Some(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    budgets: Budgets,
    #[serde(default)]
    pages: BTreeMap<String, Page>,
    // Read by TASK-CLI-002; accepted now so a full ADR-008 file does not fail.
    #[serde(rename = "fail_on")]
    _fail_on: Option<toml::Value>,
    #[serde(rename = "paths")]
    _paths: Option<toml::Value>,
    #[serde(rename = "ignore")]
    _ignore: Option<toml::Value>,
    #[serde(rename = "rules")]
    _rules: Option<toml::Value>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Budgets {
    file_max: Option<Size>,
    build_max: Option<Size>,
    texture_max_px: Option<u32>,
    image_max: Option<Size>,
    triangles_max: Option<u64>,
    compress_min: Option<Size>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    assets: Vec<String>,
    max: Option<Size>,
}

impl Config {
    /// Read the TOML text of a `meshdiet.toml`. Missing keys keep their defaults.
    ///
    /// # Errors
    /// On invalid TOML, an unknown key, a bad size or a bad glob.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: File = toml::from_str(text).map_err(|e| e.to_string().trim().to_owned())?;
        let mut config = Self::default();
        let b = &file.budgets;
        if let Some(s) = &b.file_max {
            config.file_max = s.bytes("budgets.file_max")?;
        }
        if let Some(s) = &b.build_max {
            config.build_max = Some(s.bytes("budgets.build_max")?);
        }
        if let Some(px) = b.texture_max_px {
            config.texture_max_px = px;
        }
        if let Some(s) = &b.image_max {
            config.image_max = s.bytes("budgets.image_max")?;
        }
        if let Some(n) = b.triangles_max {
            config.triangles_max = n;
        }
        if let Some(s) = &b.compress_min {
            config.compress_min = s.bytes("budgets.compress_min")?;
        }
        for (name, page) in file.pages {
            if page.assets.is_empty() {
                return Err(format!("pages.\"{name}\" has no assets globs"));
            }
            for glob in &page.assets {
                crate::weight::glob(glob)
                    .map_err(|e| format!("pages.\"{name}\": bad glob `{glob}`: {e}"))?;
            }
            let max = match &page.max {
                Some(s) => Some(s.bytes(&format!("pages.\"{name}\".max"))?),
                None => None,
            };
            config.pages.push(PageGroup {
                name,
                assets: page.assets,
                max,
            });
        }
        Ok(config)
    }

    /// Read a `meshdiet.toml` from disk and remember where it came from.
    ///
    /// # Errors
    /// When the file cannot be read, or as [`Config::from_toml`].
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut config = Self::from_toml(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        config.source = Some(path.to_path_buf());
        Ok(config)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test that cannot parse should stop")]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_decimal() {
        assert_eq!(parse_size("5 MB"), Some(5_000_000));
        assert_eq!(parse_size("1.1MB"), Some(1_100_000));
        assert_eq!(parse_size("64 KB"), Some(64_000));
        assert_eq!(parse_size("812 B"), Some(812));
        assert_eq!(parse_size("812"), Some(812));
        assert_eq!(parse_size("0.5 GB"), Some(500_000_000));
        assert_eq!(parse_size("5 MiB"), None);
        assert_eq!(parse_size("-1 MB"), None);
        assert_eq!(parse_size("MB"), None);
        assert_eq!(parse_size("1.2.3 MB"), None);
    }

    #[test]
    fn budgets_and_pages_are_read() {
        let c = Config::from_toml(
            r#"
fail_on = "high"
[budgets]
file_max = "6 MB"
build_max = "20 MB"
compress_min = 1000
[pages]
"/shop" = { assets = ["public/models/shop/**/*.glb"], max = "15 MB" }
"/" = { assets = ["public/models/hero/*.glb"] }
"#,
        )
        .unwrap();
        assert_eq!(c.file_max, 6_000_000);
        assert_eq!(c.build_max, Some(20_000_000));
        assert_eq!(c.compress_min, 1000);
        assert_eq!(c.pages.len(), 2);
        assert_eq!(c.pages[0].name, "/");
        assert_eq!(c.pages[0].max, None);
        assert_eq!(c.pages[1].max, Some(15_000_000));
    }

    #[test]
    fn unknown_keys_and_bad_sizes_are_errors() {
        assert!(Config::from_toml("[budgets]\nfile_maxx = \"5 MB\"").is_err());
        assert!(Config::from_toml("[budgets]\nbuild_max = \"5 MiB\"").is_err());
        assert!(Config::from_toml("[pages]\n\"/\" = { assets = [\"[\"] }").is_err());
        assert!(Config::from_toml("colour = true").is_err());
    }
}
