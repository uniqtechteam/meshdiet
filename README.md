# meshdiet

Find the 3D models that make your website slow. meshdiet is a command line tool that reads the
`.glb` and `.gltf` files a site ships and tells you which ones are heavy and why: geometry with no
Draco or meshopt compression, textures that are too big, and a total nobody added up.

It is early work. Today `meshdiet scan` reads every model under a folder (container, JSON, external
files and image headers, nothing decoded), reports files that cannot be read, and prints the measured
sizes as JSON. The weight checks come next; the first release will come with install steps.

```console
$ meshdiet scan tests/fixtures/GLTF-001/bad --quiet
7 files · 7.1 KB · 7 high · 0 medium · 0 low · scanned in 0.00s
```

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

Made by [UniqTech](https://uniqtech.dev).
