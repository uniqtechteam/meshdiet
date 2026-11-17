#!/usr/bin/env python3
"""Draw docs/img/qa-fix.svg from docs/data/qa-fix.json.

Usage: python3 scripts/qa_chart.py

The numbers are the per-project results of `meshdiet fix --out` in docs/qa.md (12 public projects,
2026-11-17); this script only draws them. Each project gets two bars, the size of every scanned model
before and after, with the values written beside them.
"""
import html
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
data = json.loads((ROOT / "docs" / "data" / "qa-fix.json").read_text())
repos = data["repos"]

label_w, bar_x, bar_max = 330, 342, 330
bar_h, gap, group = 11, 2, 40
top, width = 92, 940
height = top + len(repos) * group + 46
scale = bar_max / max(r["before"] for r in repos)
before = sum(r["before"] for r in repos)
after = sum(r["after"] for r in repos)
models = sum(r["models"] for r in repos)


def mb(v: float) -> str:
    return f"{v:.2f} MB"


rows = []
for i, r in enumerate(repos):
    y = top + i * group
    name = html.escape(r["name"])
    rows.append(f'<text x="{label_w}" y="{y + bar_h + 2}" text-anchor="end" class="axis">{name}</text>')
    rows.append(f'<text x="{label_w}" y="{y + 2 * bar_h + 9}" text-anchor="end" class="note">{r["models"]} model{"s" if r["models"] != 1 else ""}</text>')
    for j, (key, cls) in enumerate((("before", "before"), ("after", "after"))):
        by = y + j * (bar_h + gap)
        w = max(r[key] * scale, 1.5)
        rows.append(f'<rect x="{bar_x}" y="{by}" width="{w:.1f}" height="{bar_h}" rx="2" class="{cls}"/>')
    change = (1 - r["after"] / r["before"]) * 100 if r["before"] else 0
    text = f'{mb(r["before"])} → {mb(r["after"])}'
    text += f"  (−{change:.0f}%)" if change >= 0.5 else "  (nothing to fix)"
    end = bar_x + r["before"] * scale + 8
    rows.append(f'<text x="{end:.1f}" y="{y + bar_h + 5}" class="value">{html.escape(text)}</text>')

title = f"meshdiet fix on {len(repos)} public projects: {before:.1f} MB → {after:.1f} MB"
svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-label="{html.escape(title)}">
<style>
  .title {{ font: 600 16px system-ui, -apple-system, "Segoe UI", Helvetica, Arial, sans-serif; fill: #0b0b0b; }}
  .axis, .value {{ font: 13px system-ui, -apple-system, "Segoe UI", Helvetica, Arial, sans-serif; fill: #0b0b0b; }}
  .note {{ font: 11px system-ui, -apple-system, "Segoe UI", Helvetica, Arial, sans-serif; fill: #52514e; }}
  .before {{ fill: #2a78d6; }} .after {{ fill: #eb6834; }}
  @media (prefers-color-scheme: dark) {{
    .title, .axis, .value {{ fill: #f0efec; }} .note {{ fill: #c3c2b7; }}
    .before {{ fill: #3987e5; }} .after {{ fill: #d95926; }}
  }}
</style>
<text x="20" y="30" class="title">{html.escape(title)}</text>
<text x="20" y="52" class="note">{models} models, sizes of every scanned model before and after (skipped and kept models count at their original size)</text>
<rect x="{bar_x}" y="64" width="11" height="11" rx="2" class="before"/><text x="{bar_x + 16}" y="74" class="value">before</text>
<rect x="{bar_x + 80}" y="64" width="11" height="11" rx="2" class="after"/><text x="{bar_x + 96}" y="74" class="value">after meshdiet fix</text>
{chr(10).join(rows)}
<text x="20" y="{height - 16}" class="note">{html.escape(data["chart_note"])}</text>
</svg>
"""
(ROOT / "docs" / "img" / "qa-fix.svg").write_text(svg)
print(f"wrote docs/img/qa-fix.svg ({title})")
