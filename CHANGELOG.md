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
- Readable output grouped by file with a summary line, or `--format json` (schema 1) for scripts;
  `--output FILE`, `--quiet`, `--verbose`, and no colour when piped or when `NO_COLOR` is set.
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
  cannot switch a budget off. The other `meshdiet.toml` keys arrive in a later version.
- `--gzip` adds a gzip -6 transfer estimate per file, per page and in total, labelled as an estimate.
  The JSON report gains `pages` and the new `totals` fields.
