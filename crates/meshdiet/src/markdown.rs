//! Markdown output for a CI job summary or a pull request comment (SPEC-CLI-001 §8): summary line,
//! findings, the weight table (top 20, then "N more"), pages, and what changed since the baseline.
//! Shape first written for vibeguard (ADR-011).

use std::fmt::Write as _;

use meshdiet_core::baseline::tolerance_label;
use meshdiet_core::weight::{asset_vram, distinct_bytes};
use meshdiet_core::{Asset, Report};

use crate::output::{TOP_ROWS, compression, size, summary_line};

/// Text safe inside a table cell.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// A path as inline code; backticks inside become quotes.
fn code(s: &str) -> String {
    format!("`{}`", cell(s).replace('`', "'"))
}

/// The report as Markdown.
pub fn render(report: &Report, all_rows: bool) -> String {
    let mut out = String::from("## meshdiet\n\n");
    let _ = writeln!(out, "**{}**\n", summary_line(report));
    findings(&mut out, report);
    baseline(&mut out, report);
    weight(&mut out, report, all_rows);
    pages(&mut out, report);
    ignored(&mut out, report);
    out
}

fn findings(out: &mut String, report: &Report) {
    if report.findings.is_empty() {
        out.push_str(if report.baseline.is_some() {
            "Nothing new since the baseline.\n\n"
        } else {
            "No problems found.\n\n"
        });
        return;
    }
    out.push_str("| Severity | Rule | File | Problem | Fix |\n|---|---|---|---|---|\n");
    for f in &report.findings {
        let unsure = if f.confidence == meshdiet_core::Confidence::Medium {
            " (check by hand)"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "| {} | {} {} | {} | {}{unsure} | {} |",
            f.severity.label(),
            f.rule,
            cell(&f.title),
            code(&f.path),
            cell(&f.message),
            cell(&f.fix)
        );
    }
    out.push('\n');
}

fn baseline(out: &mut String, report: &Report) {
    let Some(b) = &report.baseline else {
        return;
    };
    let _ = writeln!(
        out,
        "### Since the baseline\n\nCompared with {} (tolerance {}): {} known {} hidden.\n",
        code(&b.file),
        tolerance_label(b.tolerance),
        b.known,
        if b.known == 1 { "finding" } else { "findings" }
    );
    if b.removed.is_empty() && b.shrunk.is_empty() {
        return;
    }
    out.push_str("Improvements:\n\n");
    for c in &b.removed {
        let _ = writeln!(out, "- removed {} ({})", code(&c.path), size(c.before));
    }
    for c in &b.shrunk {
        let _ = writeln!(
            out,
            "- {} shrank from {} to {}",
            code(&c.path),
            size(c.before),
            size(c.after)
        );
    }
    out.push('\n');
}

fn weight(out: &mut String, report: &Report, all_rows: bool) {
    if report.assets.is_empty() {
        return;
    }
    let mut assets: Vec<&Asset> = report.assets.iter().collect();
    assets.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    let shown = if all_rows {
        assets.len()
    } else {
        TOP_ROWS.min(assets.len())
    };
    out.push_str("### Weight, largest first\n\n| File | Total | Geometry | Textures | Compression | GPU (est.) |\n|---|---:|---:|---:|---|---:|\n");
    for a in &assets[..shown] {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            code(&a.path),
            size(a.bytes),
            size(a.geometry_bytes),
            size(a.texture_bytes),
            compression(a),
            size(asset_vram(a))
        );
    }
    if shown < assets.len() {
        let rest = &assets[shown..];
        let _ = writeln!(
            out,
            "\n{} more {}, {}.",
            rest.len(),
            if rest.len() == 1 { "file" } else { "files" },
            size(distinct_bytes(rest.iter().copied()))
        );
    }
    out.push('\n');
}

fn pages(out: &mut String, report: &Report) {
    if report.pages.is_empty() {
        return;
    }
    out.push_str("### Pages\n\n| Page | Assets | Total | Max | |\n|---|---:|---:|---:|---|\n");
    for p in &report.pages {
        let status = match p.max {
            Some(max) if p.bytes > max => "over budget",
            Some(_) => "ok",
            None => "",
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {status} |",
            code(&p.name),
            p.assets.len(),
            size(p.bytes),
            p.max.map(size).unwrap_or_default()
        );
    }
    out.push('\n');
}

fn ignored(out: &mut String, report: &Report) {
    if report.ignored.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "<details><summary>{} ignored by meshdiet.toml</summary>\n\n| Rule | File | Reason |\n|---|---|---|",
        report.ignored.len()
    );
    for i in &report.ignored {
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            i.finding.rule,
            code(&i.finding.path),
            i.reason
                .as_deref()
                .map_or_else(|| "(no reason)".to_owned(), cell)
        );
    }
    out.push_str("\n</details>\n");
}
