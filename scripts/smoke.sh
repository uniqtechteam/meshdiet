#!/usr/bin/env bash
# Runs a built meshdiet against the demo-site fixture the way a user would.
#
#   scripts/smoke.sh path/to/meshdiet [path/to/gltfpack]
#
# With a gltfpack (the second argument, or $MESHDIET_GLTFPACK), it also runs `meshdiet fix` and
# checks that the fixed copy passes `scan --fail-on high`. Used by the release workflow on every
# OS and by the GitLab test job.
set -euo pipefail

bin=${1:?usage: scripts/smoke.sh MESHDIET [GLTFPACK]}
gltfpack=${2:-${MESHDIET_GLTFPACK:-}}
site="$(cd "$(dirname "$0")/.." && pwd)/tests/fixtures/demo-site"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
  echo "smoke: $*" >&2
  exit 1
}

"$bin" --version

# The demo site has planted high findings: the default gate fails, `never` passes.
code=0
"$bin" scan "$site" || code=$?
[ "$code" = 1 ] || fail "scan of demo-site exited $code, expected 1 (high findings)"
"$bin" scan "$site" --fail-on never --quiet || fail "scan --fail-on never did not exit 0"

# Every report format writes something that looks like itself.
"$bin" scan "$site" --fail-on never --format json --output "$work/report.json"
grep -q '"schema": 1' "$work/report.json" || fail "JSON report has no schema field"
"$bin" scan "$site" --fail-on never --format sarif --output "$work/report.sarif"
grep -q '"version": "2.1.0"' "$work/report.sarif" || fail "SARIF report is not 2.1.0"
"$bin" scan "$site" --fail-on never --format markdown --output "$work/report.md"
grep -q '^## meshdiet' "$work/report.md" || fail "Markdown report has no heading"

"$bin" rules > /dev/null
"$bin" explain MESH-001 > /dev/null

if [ -z "$gltfpack" ]; then
  echo "smoke: no gltfpack given, skipping meshdiet fix"
  exit 0
fi

# fix writes a compressed copy; the copy must pass the gate the original failed.
"$bin" fix "$site" --gltfpack "$gltfpack" --out "$work/fixed" --format json > "$work/fix.json"
grep -q '"status": "fixed"' "$work/fix.json" || fail "meshdiet fix fixed nothing"
"$bin" fix "$site" --gltfpack "$gltfpack" --dry-run
"$bin" scan "$work/fixed" --fail-on high --quiet || fail "the fixed demo-site still fails at high"
echo "smoke: ok"
