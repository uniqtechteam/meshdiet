# meshdiet

Find the 3D models that make your website slow. meshdiet is a command line tool that reads the
`.glb` and `.gltf` files a site ships and tells you which ones are heavy and why: geometry with no
Draco or meshopt compression, textures that are too big, and a total nobody added up.

It is early work. Today `meshdiet scan` reads every model under a folder (container, JSON, external
files and image headers, nothing decoded), checks geometry, textures and size budgets, and adds up
what every model weighs. The first release will come with install steps.

```console
$ meshdiet scan tests/fixtures/GLTF-001/bad --quiet
7 files · 7.1 KB · 7 high · 0 medium · 0 low · scanned in 0.00s
```

### Budgets

`meshdiet scan` ends with a weight table, largest file first, and can hold a site to a budget. Put a
`meshdiet.toml` next to where you run it (or pass `--config`):

```toml
[budgets]
file_max = "5 MB"     # SIZE-001, the default
build_max = "20 MB"   # SIZE-002, off unless set

[pages]
"/shop" = { assets = ["public/models/shop/**/*.glb"], max = "15 MB" }   # SIZE-003
```

Sizes are decimal (1 MB = 1,000,000 bytes). `--gzip` adds a gzip -6 transfer estimate per file; your
server may use brotli or nothing, so treat it as a rough guide.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Made by [UniqTech](https://uniqtech.dev).
