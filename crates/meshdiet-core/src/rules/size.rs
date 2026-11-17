//! Weight budgets (SPEC-WEIGHT-001): SIZE-001 per file, SIZE-002 per build, SIZE-003 per page group,
//! and CFG-PAGE for a page glob that matches nothing (VAL-001).

use std::fmt::Write as _;

use crate::rule::{Rule, RuleInfo};
use crate::weight::{PageTotal, Pages, Totals, human};
use crate::{Asset, Confidence, Config, Finding, Measured, Severity};

/// SIZE-001.
pub const SIZE_001: RuleInfo = RuleInfo {
    id: "SIZE-001",
    title: "File over budget",
    severity: Severity::High,
    confidence: Confidence::High,
    summary: "one asset (with its .bin and images) is bigger than file_max, 5 MB by default",
};

/// SIZE-002.
pub const SIZE_002: RuleInfo = RuleInfo {
    id: "SIZE-002",
    title: "Build over budget",
    severity: Severity::High,
    confidence: Confidence::High,
    summary: "all scanned assets together are bigger than build_max (off unless set)",
};

/// SIZE-003.
pub const SIZE_003: RuleInfo = RuleInfo {
    id: "SIZE-003",
    title: "Page group over budget",
    severity: Severity::High,
    confidence: Confidence::High,
    summary: "the assets of one [pages] group are bigger than its max",
};

/// CFG-PAGE.
pub const CFG_PAGE: RuleInfo = RuleInfo {
    id: "CFG-PAGE",
    title: "Page glob matches nothing",
    severity: Severity::Low,
    confidence: Confidence::High,
    summary: "a [pages] glob matches no scanned asset, so its budget checks nothing",
};

/// The rules that look at the whole scan rather than one asset (run by the scan, not [`Rule`]).
pub const BUILD_RULES: [&RuleInfo; 3] = [&SIZE_002, &SIZE_003, &CFG_PAGE];

/// Why it costs the user, and how to fix it.
struct Advice {
    why: &'static str,
    fix: &'static str,
}

fn finding(
    info: &RuleInfo,
    path: &str,
    measured: Measured,
    threshold: Option<Measured>,
    message: String,
    advice: &Advice,
) -> Finding {
    Finding {
        rule: info.id.to_owned(),
        title: info.title.to_owned(),
        severity: info.severity,
        confidence: info.confidence,
        path: path.to_owned(),
        measured,
        threshold,
        message,
        why: advice.why.to_owned(),
        fix: advice.fix.to_owned(),
    }
}

/// SIZE-001: an asset over `file_max`.
pub struct FileOverBudget;

impl Rule for FileOverBudget {
    fn info(&self) -> &'static RuleInfo {
        &SIZE_001
    }

    fn check(&self, asset: &Asset, config: &Config) -> Vec<Finding> {
        if asset.bytes <= config.file_max {
            return Vec::new();
        }
        let mut message = format!(
            "{}, over the {} file budget: geometry {}, textures {}, other {}",
            human(asset.bytes),
            human(config.file_max),
            human(asset.geometry_bytes),
            human(asset.texture_bytes),
            human(asset.other_bytes),
        );
        if let Some(img) = asset.images.iter().max_by_key(|i| i.bytes) {
            let name = img
                .name
                .clone()
                .or_else(|| img.file.clone())
                .unwrap_or_else(|| format!("image {}", img.index));
            let kind = img
                .mime
                .as_deref()
                .and_then(|m| m.strip_prefix("image/"))
                .unwrap_or("image")
                .to_uppercase();
            let dims = match (img.width, img.height) {
                (Some(w), Some(h)) => format!("{w}x{h} "),
                _ => String::new(),
            };
            let _ = write!(
                message,
                ". Biggest image: {name} ({dims}{kind}, {})",
                human(img.bytes)
            );
        }
        let f = finding(
            &SIZE_001,
            &asset.path,
            Measured::exact("file size", asset.bytes, "bytes"),
            Some(Measured::exact("file_max", config.file_max, "bytes")),
            message,
            &Advice {
                why: "Every byte is downloaded before the model appears, so one heavy file holds up the whole page on a phone connection.",
                fix: "Compress the geometry and textures (`meshdiet fix`, or `gltfpack -cc -tc`), shrink oversized textures, or raise file_max in meshdiet.toml if this size is intended.",
            },
        );
        vec![f]
    }
}

/// "a.glb 3.1 MB, b.glb 2.0 MB, …": the five largest of `assets` (already sorted, largest first).
fn largest(assets: &[String], all: &[Asset]) -> String {
    let mut parts: Vec<String> = assets
        .iter()
        .take(5)
        .map(|p| {
            let bytes = all.iter().find(|a| &a.path == p).map_or(0, |a| a.bytes);
            format!("{p} {}", human(bytes))
        })
        .collect();
    if assets.len() > 5 {
        parts.push(format!("and {} more", assets.len() - 5));
    }
    parts.join(", ")
}

/// Findings about the whole scan: SIZE-002, SIZE-003 and CFG-PAGE, located at the config file
/// shown as `config_path`.
#[must_use]
pub fn build_findings(
    assets: &[Asset],
    totals: &Totals,
    pages: &Pages,
    config: &Config,
    config_path: &str,
) -> Vec<Finding> {
    let mut out = Vec::new();
    if let Some(max) = config.build_max
        && totals.bytes > max
    {
        let mut by_size: Vec<&Asset> = assets.iter().collect();
        by_size.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
        let names: Vec<String> = by_size.iter().map(|a| a.path.clone()).collect();
        let f = finding(
            &SIZE_002,
            config_path,
            Measured::exact("build bytes", totals.bytes, "bytes"),
            Some(Measured::exact("build_max", max, "bytes")),
            format!(
                "{} assets ship {}, over the {} build budget. Largest: {}",
                totals.assets,
                human(totals.bytes),
                human(max),
                largest(&names, assets)
            ),
            &Advice {
                why: "The build only gets heavier one model at a time; past the budget, first visits on mobile wait for megabytes of 3D.",
                fix: "Compress the largest assets first (`meshdiet fix`), drop models no page loads, or raise build_max if the growth is intended.",
            },
        );
        out.push(f);
    }
    for page in &pages.totals {
        out.extend(page_over_budget(page, assets, config_path));
    }
    for (page, pattern) in &pages.unmatched {
        let f = finding(
            &CFG_PAGE,
            config_path,
            Measured::exact("matching assets", 0, "assets"),
            None,
            format!("page \"{page}\": `{pattern}` matches no scanned asset"),
            &Advice {
                why: "A typo in a page glob turns its budget off without a word.",
                fix: "Fix the glob, or check that the folder it points at is scanned (globs match paths relative to the scan root, or with the scanned folder in front).",
            },
        );
        out.push(f);
    }
    out
}

fn page_over_budget(page: &PageTotal, assets: &[Asset], config_path: &str) -> Option<Finding> {
    let max = page.max?;
    if page.bytes <= max {
        return None;
    }
    let f = finding(
        &SIZE_003,
        config_path,
        Measured::exact(&format!("page {} bytes", page.name), page.bytes, "bytes"),
        Some(Measured::exact("max", max, "bytes")),
        format!(
            "page \"{}\" ships {} in {} assets, over its {} budget. Largest: {}",
            page.name,
            human(page.bytes),
            page.assets.len(),
            human(max),
            largest(&page.assets, assets)
        ),
        &Advice {
            why: "Everything in the group loads with the page, so the visitor waits for the whole sum before the scene is complete.",
            fix: "Compress or shrink the largest assets in the group, load some of them later (on interaction or scroll), or raise the page max if intended.",
        },
    );
    Some(f)
}
