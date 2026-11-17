//! Report formats: pretty for people, JSON for scripts. Shape first written for vibeguard (ADR-011).

use std::fmt::Write as _;

use meshdiet_core::{Finding, Report, Severity};

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
#[allow(
    clippy::cast_precision_loss,
    reason = "one decimal of a byte count; f64 is exact far beyond any real file"
)]
pub fn size(bytes: u64) -> String {
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

/// The one-line summary: `37 files · 49.8 MB · 3 high · 12 medium · 4 low · scanned in 0.62s`.
pub fn summary_line(report: &Report) -> String {
    let s = report.summary;
    let n = report.totals.assets;
    format!(
        "{n} {} · {} · {} high · {} medium · {} low · scanned in {:.2}s",
        if n == 1 { "file" } else { "files" },
        size(report.totals.bytes),
        s.high,
        s.medium,
        s.low,
        report.duration().as_secs_f64()
    )
}

/// Readable terminal output: findings grouped by file, then the summary line.
pub fn pretty(report: &Report, color: bool, quiet: bool) -> String {
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_owned()
        }
    };
    let mut out = String::new();
    if !quiet {
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
                    .map_or(0, |a| a.bytes);
                let _ = writeln!(out, "{}  {}", paint(BOLD, &f.path), size(bytes));
                last_path = Some(f.path.as_str());
            }
            write_finding(&mut out, f, &paint);
        }
        if !report.findings.is_empty() {
            out.push('\n');
        }
    }
    let _ = writeln!(out, "{}", summary_line(report));
    out
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
