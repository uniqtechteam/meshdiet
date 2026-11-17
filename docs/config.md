# Configuration

meshdiet works with no configuration. A `meshdiet.toml` lets a project set its own gate and budgets,
list which models each page loads, and accept findings it has decided to live with. It is found in
the scanned folder or the current one; `--config FILE` points somewhere else. Flags on the command
line win over the file.

## Every key

```toml
fail_on = "high"            # high | medium | low | never; --fail-on wins
paths = ["public"]          # scanned when no PATH is given, relative to this file

ignore = [
  "**/fixtures/**",          # a plain glob: never scanned
  { rule = "TEX-001", path = "public/models/sofa.glb", reason = "fabric close-up, shown full screen" },
]

[budgets]
file_max = "5 MB"           # SIZE-001
build_max = "20 MB"         # SIZE-002, off unless set
image_max = "1 MB"          # TEX-004
texture_max_px = 2048       # TEX-001
triangles_max = 500000      # MESH-002
compress_min = "64 KB"      # MESH-001 ignores geometry smaller than this

[pages]
"/" = { assets = ["public/models/hero.glb"], max = "3 MB" }               # SIZE-003
"/shop" = { assets = ["public/models/**/*.glb", "public/models/*.gltf"] }  # no max: listed, not judged

[rules]
skip = ["TEX-002"]
```

The values under `[budgets]` above are the defaults, except `build_max`, which is off until you set
it. Keys at the top (`fail_on`, `paths`, `ignore`) must come before the first `[table]`, as TOML
requires.

Sizes are decimal: `"5 MB"` is 5,000,000 bytes, and a plain number is bytes. `"5 MiB"` is refused
rather than guessed. Unknown keys, unknown rule ids and bad globs stop the scan with exit code 2 and
say what is valid, so a typo never switches a check off quietly:

```console
$ meshdiet scan . --config meshdiet.toml --quiet
meshdiet: meshdiet.toml: budgets.file_max = "5 MiB" is not a size; write it like "5 MB", "64 KB" or "812 B" (1 MB = 1,000,000 bytes)
```

## Pages

Each `[pages]` entry names the models one page loads, as globs over the scanned paths (`*` stays in
one folder, `**` crosses folders). The scan prints a table of pages, and a page over its `max` is a
SIZE-003 finding with its five largest models. A glob that matches nothing is reported as CFG-PAGE,
so a renamed folder cannot switch a budget off.

With the file above in the demo site:

```console
$ meshdiet scan
[findings and weight table]

Pages
  page   assets   total     max
  /           1  5.1 MB  3.0 MB  over budget
  /shop       4  5.6 MB

4 files · 5.6 MB · 1 uncompressed (5.1 MB) · 3 high · 2 medium · 0 low · 1 ignored · scanned in 0.00s
```

## Ignoring a finding

A binary model cannot carry a comment, so accepted findings live in `meshdiet.toml`. An entry needs a
`rule`; `path` is a glob (every file when it is left out). An entry without a `reason` still applies,
but is reported as a low CFG-IGN finding, because nobody can tell later whether it still holds. The
`[[ignore]]` table form that `meshdiet explain` prints works the same way.

`--show-ignored` lists what the entries keep out:

```console
$ meshdiet scan --show-ignored
[...]
Ignored by meshdiet.toml
  TEX-001  models/sofa.glb  sofa-fabric: 4096×4096 PNG, over the 2048 px limit; est. 89.5 MB GPU memory, est. 22.4 MB at 2048×2048  (fabric close-up, shown full screen)
```

For one run, `--rules MESH-001,SIZE-001` runs only those rules and `--skip TEX-002` leaves one out,
on top of `[rules] skip`.

## Compressed copies

If a build keeps each source model next to the compressed copy the app loads (`chair.glb` with
`chair-compressed.glb`, `chair.draco.glb`, or gltfjsx's `chair-transformed.glb`), the source still
counts in the weight, marked `(source of ...)`, but its own findings are left out and `fix` leaves it
alone. If the app's code names the original file, nothing is skipped.

## Decoders and `--src`

LOAD-001 needs to see the app's code to know whether it sets up the Draco, meshopt or KTX2 decoder.
It searches the JS, TS and HTML in the scanned folder. When that folder has no app code, as with
`public/`, it reads the nearest folder above with a `package.json`, inside the same git repository.
`--src DIR` points it somewhere else. It is a text search, so it is marked "check by hand" and fails a
build only with `--strict`. Babylon.js apps are skipped, since Babylon fetches its decoders itself.

## Transfer size

`--gzip` adds a gzip -6 estimate per file, per page and in total. It reads every byte, so it is
slower, and your server may use brotli or nothing; treat it as a rough guide.

```console
$ meshdiet scan --gzip --quiet
4 files · 5.6 MB · 1 uncompressed (5.1 MB) · 4.5 MB gzip -6 est. · 3 high · 2 medium · 0 low · 1 ignored · scanned in 0.12s
```
