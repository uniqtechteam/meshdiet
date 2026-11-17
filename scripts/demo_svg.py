#!/usr/bin/env python3
"""Render docs/img/demo.svg from a real run of meshdiet on tests/fixtures/demo-site.

Usage: cargo build --release && python3 scripts/demo_svg.py [path/to/meshdiet]

It copies the demo site to a temporary folder and runs, in order:

    meshdiet scan demo-site
    meshdiet fix demo-site --out demo-site-fixed
    meshdiet scan demo-site-fixed --quiet

gltfpack must be on PATH (or in MESHDIET_GLTFPACK). The text is the binary's own output. The script
only adds colour, wraps lines longer than the window, keeps the first "Fix:" hint and drops the
others (each finding prints one), and shows the home folder as "~". Lines appear one after another
with CSS, so the image plays once like a terminal and then stays still; a viewer that ignores
animation, or asks for reduced motion, sees the finished screen.
"""
import html
import os
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target" / "release" / "meshdiet"
WRAP = 112
SEVERITIES = ("high", "medium", "low")


def run(cwd: Path, *args: str) -> list[str]:
    out = subprocess.run([BIN, *args], cwd=cwd, capture_output=True, text=True, check=False)
    text = out.stdout.replace(str(Path.home()), "~")
    return text.rstrip("\n").splitlines()


def classify(line: str) -> str:
    body = line.strip()
    if not body:
        return "blank"
    if body.split(" ")[0] in SEVERITIES and line.startswith("  "):
        return body.split(" ")[0]
    if body.startswith("Fix:"):
        return "hint"
    if body[0].isdigit():
        return "summary"
    if line.startswith(("public/", "Weight")):
        return "file"
    if body.startswith(("file ", "note:", "no MESH")) or "gltfpack 1." in body:
        return "dim"
    if body.startswith(("import ", "loader.")):
        return "code"
    return "text"


def wrap(line: str) -> list[str]:
    if len(line) <= WRAP:
        return [line]
    indent = len(line) - len(line.lstrip())
    if line.lstrip().split(" ")[0] in SEVERITIES:
        indent = 18  # under the message, past severity and rule id
    elif line.lstrip().startswith("Fix:"):
        indent += 5
    first, *rest = textwrap.wrap(line, WRAP, subsequent_indent=" " * indent, drop_whitespace=True)
    return [" " * (len(line) - len(line.lstrip())) + first.lstrip(), *rest]


def build() -> list[tuple[str, str, float]]:
    """Every row as (kind, text, seconds after the start when it appears)."""
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        shutil.copytree(ROOT / "tests" / "fixtures" / "demo-site", work / "demo-site")
        steps = [
            ("meshdiet scan demo-site", run(work, "scan", "demo-site"), 0.04),
            (
                "meshdiet fix demo-site --out demo-site-fixed",
                run(work, "fix", "demo-site", "--out", "demo-site-fixed"),
                0.18,
            ),
            ("meshdiet scan demo-site-fixed --quiet", run(work, "scan", "demo-site-fixed", "--quiet"), 0.04),
        ]
    rows: list[tuple[str, str, float]] = []
    t = 0.4
    for n, (command, output, step) in enumerate(steps):
        if n:
            rows.append(("blank", "", t))
        rows.append(("prompt", f"$ {command}", t))
        t += 0.9
        hints = 0
        for line in output:
            kind = classify(line)
            if kind == "hint":
                hints += 1
                if hints > 1:
                    continue
            for j, part in enumerate(wrap(line)):
                rows.append((kind if j == 0 else "text", part, t))
            t += step
        t += 1.2
    return rows


COLORS = {
    "prompt": "#e6edf3", "high": "#f0883e", "medium": "#d29922", "low": "#79c0ff",
    "file": "#e6edf3", "hint": "#8b949e", "dim": "#8b949e", "code": "#a5d6ff",
    "summary": "#e6edf3", "text": "#c9d1d9", "blank": "#c9d1d9",
}
BOLD = {"prompt", "file", "summary"}


def row_svg(kind: str, text: str, x: int, y: int) -> str:
    weight = ' font-weight="700"' if kind in BOLD else ""
    if kind in SEVERITIES:
        # "  high    MESH-001  message": colour the severity, bold the rule id
        match = re.match(r"(\s*\S+\s+)(\S+\s+)(.*)", text)
        sev, rule, msg = match.groups() if match else (text, "", "")
        return (
            f'<tspan fill="{COLORS[kind]}" font-weight="700">{html.escape(sev)}</tspan>'
            f'<tspan fill="#e6edf3" font-weight="700">{html.escape(rule)}</tspan>'
            f'<tspan fill="#c9d1d9">{html.escape(msg)}</tspan>'
        )
    if kind == "prompt":
        return f'<tspan fill="#3fb950">$</tspan><tspan fill="{COLORS[kind]}"{weight}>{html.escape(text[1:])}</tspan>'
    return f'<tspan fill="{COLORS[kind]}"{weight}>{html.escape(text)}</tspan>'


def main() -> None:
    rows = build()
    line_h, pad, top = 19, 20, 44
    width = 1000
    height = top + len(rows) * line_h + pad
    body = []
    for i, (kind, text, at) in enumerate(rows):
        y = top + i * line_h + 14
        body.append(
            f'<text x="{pad}" y="{y}" xml:space="preserve" style="animation-delay:{at:.2f}s">'
            f"{row_svg(kind, text, pad, y)}</text>"
        )
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-label="meshdiet scanning a demo site, fixing three models with gltfpack, and scanning the fixed copy">
<style>
  text {{ animation: show 0.01s steps(1) backwards; }}
  @keyframes show {{ from {{ opacity: 0; }} to {{ opacity: 1; }} }}
  @media (prefers-reduced-motion: reduce) {{ text {{ animation: none; }} }}
</style>
<rect width="{width}" height="{height}" rx="10" fill="#0d1117"/>
<circle cx="22" cy="20" r="6" fill="#ff5f57"/><circle cx="42" cy="20" r="6" fill="#febc2e"/><circle cx="62" cy="20" r="6" fill="#28c840"/>
<g font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" font-size="12.5">
{chr(10).join(body)}
</g>
</svg>
"""
    out = ROOT / "docs" / "img" / "demo.svg"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(svg)
    print(f"wrote docs/img/demo.svg ({len(rows)} lines, {rows[-1][2]:.1f} s)")


if __name__ == "__main__":
    if not BIN.is_file():
        sys.exit(f"no meshdiet binary at {BIN}; run cargo build --release or pass its path")
    if not (shutil.which("gltfpack") or os.environ.get("MESHDIET_GLTFPACK")):
        sys.exit("gltfpack is not on PATH; meshdiet fix needs it (see README)")
    main()
