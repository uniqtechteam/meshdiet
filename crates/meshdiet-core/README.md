# meshdiet-core

The engine behind [meshdiet](https://github.com/uniqtechteam/meshdiet), a command line tool that finds
the 3D models that make a website slow.

It walks a folder for `.glb` and `.gltf` files, reads each container, its glTF JSON, external
buffers and image headers (no pixel or geometry decoding), runs the rules (geometry and texture
compression, texture sizes, unused buffer bytes, size budgets, decoder setup in the app's code) and
returns a report with findings, per-file weight and build totals. The `fix` module runs a gltfpack
binary you installed and checks every output before it is kept.

Nothing here touches the network.

Most people want the command line tool:

```console
cargo install meshdiet
```

The library API follows the tool and may change between 0.x releases.

## Licence

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
