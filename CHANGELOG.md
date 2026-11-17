# Changelog

All notable changes to meshdiet are written down here, for people who use it.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Project setup: the `meshdiet` command line tool and the `meshdiet-core` engine.
- `meshdiet scan [PATH...]` finds every `.glb` and `.gltf` (respecting `.gitignore`, skipping
  `node_modules`, `.git` and `target`; a folder you name, such as `dist`, is always scanned) and reads
  each one without decoding anything: the GLB container and glTF JSON, external `.bin` files and images,
  data URIs, image sizes from their headers (PNG, JPEG, WebP, AVIF, KTX2), triangles drawn by the
  default scene, and the compression in use (Draco, meshopt, quantization, KTX2, WebP, AVIF).
- Each asset's size is split into geometry, textures and the rest, with an estimated GPU memory figure per
  image (always labelled as an estimate). Files shared by several assets count once in the totals.
- GLTF-001: a file that cannot be read (empty, cut off, wrong magic, JSON chunk past the end of the file,
  invalid JSON, a missing `.bin` or image, a bufferView past its buffer) is reported as a high finding and
  the scan carries on with the other files.
- MESH-001: models whose geometry has no Draco or meshopt compression, with the number of primitives
  and their bytes (from 64 KB of geometry; medium instead of high when the attributes are already
  quantized). MESH-002: more than 500,000 triangles drawn by the default scene, each node instance
  counted, strips and fans counted right. MESH-003: buffer bytes that no bufferView uses (over 64 KB or
  10% of the buffer); meshopt fallback buffers and views that only an extension names are not counted.
- Texture checks. TEX-001: an image wider or taller than 2048 px, with its estimated GPU memory now
  and at the limit. TEX-002: PNG, JPEG, WebP or AVIF textures that take more than an estimated 32 MB
  of GPU memory in one model, where KTX2 would take about a quarter. TEX-003: a PNG over 256 KB whose
  header says it cannot be transparent, so JPEG or WebP would be far smaller (normal maps are only
  pointed to WebP). TEX-004: an image over 1 MB. A `KHR_texture_basisu` texture counts as KTX2 even
  with a PNG fallback. Each image in the JSON report now says what its PNG header declares about
  transparency (`alpha`).
- GLTF-002: a `.gltf` that stores a buffer or image as a base64 data URI over 16 KB, with the bytes base64
  adds; the fix is to convert it to `.glb`.
- LOAD-001: models compressed with Draco, meshopt or KTX2 while the app's JS, TS or HTML never sets up
  that decoder (`DRACOLoader`, `MeshoptDecoder`, `KTX2Loader`, drei `useGLTF` for Draco and meshopt,
  `<model-viewer>`). One finding per missing decoder, naming up to five models. It reads the scanned folder,
  or the folder given with `--src`, by plain text search; it stays quiet when it finds no sources, no
  three.js glTF loader, or a Babylon.js app (Babylon fetches its decoders itself). Medium confidence, so it
  fails a build only with `--strict`.
- Readable output grouped by file with a summary line, or `--format json` (schema 1) for scripts;
  `--output FILE`, `--quiet`, `--verbose`, and no colour when piped or when `NO_COLOR` is set.
- `meshdiet fix [PATH...]` compresses the models with MESH-001 or TEX findings by running gltfpack
  1.3 or newer (found through `--gltfpack`, `MESHDIET_GLTFPACK` or `PATH`; meshdiet never downloads
  it and says how to install it when it is missing). Geometry gets meshopt (`-cc`), textures WebP by
  default (`--textures ktx2|keep`, `--max-texture`), and gltfpack always runs with `-kn -km -ke` and
  never simplifies, so named nodes, named materials and their extras survive. Every output is read
  back and kept only when it has the same scenes, the same named nodes under the same named parents,
  the same named materials and extras, and is smaller. Results go to `meshdiet-out/` (or `--out`),
  `--in-place` only replaces files git has a clean copy of (or with `--force`), and `--dry-run`
  writes nothing. The report shows sizes measured on disk and the three.js decoder lines the app
  needs. `scan --fix` runs `fix` after the scan.
- Exit codes for CI: 0 when nothing reaches `--fail-on` (high by default), 1 when something does, 2 for
  usage errors. Medium-confidence findings count only with `--strict`.
- A weight table after the findings: every asset with its total, geometry, textures, compression and
  estimated GPU memory, largest first. The top 20 are shown; `--all` shows every row. Files shared by
  several assets are marked `(shared)` and counted once in every total.
- The summary line says how many models ship without geometry compression and what they weigh, for
  example `37 files · 49.8 MB · 37 uncompressed (49.8 MB)`.
- Budgets in `meshdiet.toml` (found in the scanned folder or the current one, or `--config FILE`):
  SIZE-001 flags a file over `file_max` (5 MB by default) with its size split and biggest image,
  SIZE-002 flags the whole build over `build_max`, and SIZE-003 flags a `[pages]` group over its `max`
  with its five largest assets. A page glob that matches nothing is reported as CFG-PAGE, so a typo
  cannot switch a budget off.
- `--gzip` adds a gzip -6 transfer estimate per file, per page and in total, labelled as an estimate.
  The JSON report gains `pages` and the new `totals` fields.
- The rest of `meshdiet.toml`: `fail_on`, `paths` (what to scan when you name no folder), `ignore` and
  `[rules] skip`. Unknown keys, unknown rule ids and sizes such as `"5 MiB"` stop with exit code 2 and
  the valid choices. `--rules` and `--skip` choose rules for one run.
- Ignores live in `meshdiet.toml`, since a binary model cannot carry a comment:
  `{ rule = "TEX-001", path = "public/hero/*.glb", reason = "..." }`. An entry without a reason still
  applies and is reported as a low `CFG-IGN` finding. `--show-ignored` lists what was left out.
- A baseline for sites that are already heavy: `--write-baseline FILE` records every finding and every
  file size, and `--baseline FILE` then reports only what is new, plus any file or build that grew by
  more than `--tolerance` (1% by default) as `BASE-GROW`. Removed and smaller files are listed as
  improvements. Every finding now has a `fingerprint` in JSON.
- `--format sarif` (SARIF 2.1.0, for GitHub code scanning) and `--format markdown` (for job summaries
  and pull request comments, with the weight and page tables).
- `meshdiet rules` lists every rule with its severity and confidence; `meshdiet explain RULE` says why
  it matters, how to fix it with gltfpack or glTF Transform, and what the loader needs after the fix.
- A GitHub Action (`uses: uniqtechteam/meshdiet@v0`): installs the meshdiet release for the runner and
  checks its SHA-256, writes the Markdown report to the job summary, uploads SARIF to code scanning and
  fails the job at `fail-on`. Outputs carry the finding counts and total bytes. `install-gltfpack`
  adds gltfpack 1.3, pinned and checksum-checked, so a later step can run `meshdiet fix`.
