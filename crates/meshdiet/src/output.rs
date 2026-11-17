//! Report formats: pretty for people, JSON for scripts. Shape first written for vibeguard (ADR-011).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use meshdiet_core::{Asset, Finding, Report, Severity};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

fn severity_color(s: Severity) -> &'static str {
    match s {
        Severity::High => "\x1b[31m",
        Severity::Medium => "\x1b[33m",
        Severity::Low => "\x1b[36m",
    }
}

/// Bytes in decimal units (1 MB = 1,000,000 bytes), one decimal: `49.8 MB`, `812 B`.
pub fn size(bytes: u64) -> String {
    meshdiet_core::weight::human(bytes)
}

/// Rows of the weight table shown without `--all`.
const TOP_ROWS: usize = 20;

/// How pretty output looks.
pub struct Style {
    /// ANSI colours.
    pub color: bool,
    /// Only the summary line.
    pub quiet: bool,
    /// Every row of the weight table.
    pub all_rows: bool,
}

/// The one-line summary:
/// `37 files · 49.8 MB · 37 uncompressed (49.8 MB) · 3 high · 12 medium · 4 low · scanned in 0.62s`.
pub fn summary_line(report: &Report) -> String {
    let s = report.summary;
    let t = &report.totals;
    let n = t.assets;
    let mut line = format!(
        "{n} {} · {}",
        if n == 1 { "file" } else { "files" },
        size(t.bytes)
    );
    if t.uncompressed_assets > 0 {
        let _ = write!(
            line,
            " · {} uncompressed ({})",
            t.uncompressed_assets,
            size(t.uncompressed_bytes)
        );
    }
    if let Some(gz) = t.gzip {
        let _ = write!(line, " · {} gzip -6 est.", size(gz.bytes));
    }
    let _ = write!(
        line,
        " · {} high · {} medium · {} low · scanned in {:.2}s",
        s.high,
        s.medium,
        s.low,
        report.duration().as_secs_f64()
    );
    line
}

/// Readable terminal output: findings grouped by file, the weight table, the page table, then the
/// summary line.
pub fn pretty(report: &Report, style: &Style) -> String {
    let color = style.color;
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_owned()
        }
    };
    let mut out = String::new();
    if !style.quiet {
        if report.findings.is_empty() && !report.assets.is_empty() {
            let _ = writeln!(out, "No problems found.\n");
        }
        let mut last_path = None;
        for f in &report.findings {
            if last_path != Some(f.path.as_str()) {
                if last_path.is_some() {
                    out.push('\n');
                }
                let bytes = report
                    .assets
                    .iter()
                    .find(|a| a.path == f.path)
                    .map(|a| format!("  {}", size(a.bytes)))
                    .unwrap_or_default();
                let _ = writeln!(out, "{}{bytes}", paint(BOLD, &f.path));
                last_path = Some(f.path.as_str());
            }
            write_finding(&mut out, f, &paint);
        }
        if !report.findings.is_empty() {
            out.push('\n');
        }
        weight_table(&mut out, report, style.all_rows, &paint);
        page_table(&mut out, report, &paint);
    }
    let _ = writeln!(out, "{}", summary_line(report));
    out
}

/// `draco+ktx2`, or `none`.
fn compression(a: &Asset) -> String {
    let c = a.compression;
    let used: Vec<&str> = [
        (c.draco, "draco"),
        (c.meshopt, "meshopt"),
        (c.ktx2, "ktx2"),
        (c.webp, "webp"),
        (c.avif, "avif"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| *name)
    .collect();
    if used.is_empty() {
        "none".to_owned()
    } else {
        used.join("+")
    }
}

/// Lay out rows as columns: the first left-aligned, the rest right-aligned except `left` ones.
fn columns(rows: &[Vec<String>], left: &[usize]) -> Vec<String> {
    let cols = rows.first().map_or(0, Vec::len);
    let widths: Vec<usize> = (0..cols)
        .map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|r| {
            let cells: Vec<String> = r
                .iter()
                .enumerate()
                .map(|(c, cell)| {
                    if c == 0 || left.contains(&c) {
                        format!("{cell:<w$}", w = widths[c])
                    } else {
                        format!("{cell:>w$}", w = widths[c])
                    }
                })
                .collect();
            format!("  {}", cells.join("  ").trim_end())
        })
        .collect()
}

/// Every asset, largest first: total, geometry, textures, compression, estimated GPU memory, and
/// the gzip estimate with `--gzip` (SPEC-WEIGHT-001 FR-003).
fn weight_table(
    out: &mut String,
    report: &Report,
    all_rows: bool,
    paint: &dyn Fn(&str, &str) -> String,
) {
    if report.assets.is_empty() {
        return;
    }
    let gzip = !report.gzip_estimate.is_empty();
    let mut owners: BTreeMap<&str, usize> = BTreeMap::new();
    for a in &report.assets {
        for f in &a.files {
            *owners.entry(f.as_str()).or_default() += 1;
        }
    }
    let mut assets: Vec<&Asset> = report.assets.iter().collect();
    assets.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    let shown = if all_rows {
        assets.len()
    } else {
        TOP_ROWS.min(assets.len())
    };
    let mut header = vec![
        "file",
        "total",
        "geometry",
        "textures",
        "compression",
        "GPU (est.)",
    ];
    if gzip {
        header.push("gzip -6 est.");
    }
    let mut rows = vec![header.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>()];
    for a in &assets[..shown] {
        let shared = a.files.iter().any(|f| owners.get(f.as_str()) > Some(&1));
        let mut row = vec![
            if shared {
                format!("{} (shared)", a.path)
            } else {
                a.path.clone()
            },
            size(a.bytes),
            size(a.geometry_bytes),
            size(a.texture_bytes),
            compression(a),
            size(meshdiet_core::weight::asset_vram(a)),
        ];
        if gzip {
            row.push(size(
                a.files
                    .iter()
                    .filter_map(|f| report.gzip_estimate.get(f))
                    .sum(),
            ));
        }
        rows.push(row);
    }
    let _ = writeln!(out, "{}", paint(BOLD, "Weight, largest first"));
    let mut lines = columns(&rows, &[4]);
    if let Some(head) = lines.first_mut() {
        *head = paint(DIM, head);
    }
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    if shown < assets.len() {
        let rest = &assets[shown..];
        let _ = writeln!(
            out,
            "  {} more {}, {} (--all shows every row)",
            rest.len(),
            if rest.len() == 1 { "file" } else { "files" },
            size(meshdiet_core::weight::distinct_bytes(rest.iter().copied()))
        );
    }
    out.push('\n');
}

/// One row per `[pages]` group, then `(no page)` (SPEC-WEIGHT-001 FR-005).
fn page_table(out: &mut String, report: &Report, paint: &dyn Fn(&str, &str) -> String) {
    if report.pages.is_empty() {
        return;
    }
    let gzip = report.pages.iter().any(|p| p.gzip.is_some());
    let mut header = vec!["page", "assets", "total", "max", ""];
    if gzip {
        header.insert(3, "gzip -6 est.");
    }
    let mut rows = vec![header.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>()];
    for p in &report.pages {
        let status = match p.max {
            Some(max) if p.bytes > max => "over budget",
            Some(_) => "ok",
            None => "",
        };
        let mut row = vec![
            p.name.clone(),
            p.assets.len().to_string(),
            size(p.bytes),
            p.max.map(size).unwrap_or_default(),
            status.to_owned(),
        ];
        if gzip {
            row.insert(3, p.gzip.map(|g| size(g.bytes)).unwrap_or_default());
        }
        rows.push(row);
    }
    let _ = writeln!(out, "{}", paint(BOLD, "Pages"));
    let last = rows[0].len() - 1;
    let mut lines = columns(&rows, &[last]);
    if let Some(head) = lines.first_mut() {
        *head = paint(DIM, head);
    }
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    out.push('\n');
}

fn write_finding(out: &mut String, f: &Finding, paint: &dyn Fn(&str, &str) -> String) {
    let label = format!("{:<6}", f.severity.label());
    let unsure = if f.confidence == meshdiet_core::Confidence::Medium {
        "  (check by hand)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "  {}  {}  {}{unsure}",
        paint(severity_color(f.severity), &label),
        f.rule,
        f.message
    );
    let _ = writeln!(out, "{}", paint(DIM, &format!("          Fix: {}", f.fix)));
}

/// The full report as pretty-printed JSON.
pub fn json(report: &Report) -> String {
    let mut text =
        serde_json::to_string_pretty(report).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(size(812), "812 B");
        assert_eq!(size(64_000), "64.0 KB");
        assert_eq!(size(49_807_324), "49.8 MB");
        assert_eq!(size(1_250_000_000), "1.2 GB");
    }
}
