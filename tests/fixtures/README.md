# Test fixtures

Every model and texture here is generated from code by `xtask` (`xtask/src/fixtures.rs`). No
third-party, downloaded or client model is ever committed.

```text
<RULE-ID>/bad/            files that must produce exactly the findings in bad/expected.json
<RULE-ID>/bad/expected.json   [{ "rule": "MESH-001", "path": "dense.glb", "value": 1067784 }]  (value optional)
<RULE-ID>/good/           a near miss that must produce no finding of that rule
demo-site/                a tiny three.js page with planted problems (see its README)
```

`crates/meshdiet-core/tests/fixtures.rs` walks every rule folder, so a new rule adds folders, not
test code. Rules that the engine does not have yet are checked for shape only.

## Regenerate

```bash
cargo xtask gen-fixtures
```

The output is deterministic. `xtask/tests/generator.rs` fails when a committed file differs from what
the generator builds today, so edit the generator, run the command and commit both.

The 37-model / 49.8 MB set from the shop audit story is too big for git; tests build it into a
temporary folder (`cargo xtask story-set DIR` writes it by hand).

## Compressed files (made once with real tools)

Some fixtures need real Draco, meshopt or KTX2 data, so they are made from generated inputs with the
tools users run, then committed. They change only when these commands are run again.

| File | Made by |
|---|---|
| `MESH-001/good/dense-meshopt.glb` | gltfpack, meshopt |
| `MESH-001/good/dense-draco.glb` | glTF Transform, Draco |
| `TEX-002/good/two-textures-ktx2.glb` | gltfpack, KTX2 (BasisU) |
| `LOAD-001/{bad,good}/models/chair.glb`, `demo-site/public/models/chair.glb` | gltfpack, meshopt (same file) |

Tools used on 2026-11-17 (Linux x86_64):

- gltfpack 1.3, `gltfpack-ubuntu.zip` from the meshoptimizer v1.3 release,
  SHA-256 `0666d9dc40d60fe5b9a45f3fc24f8e6ca87112974bd5de9ca57152aa13d06017`
- glTF Transform CLI 4.5.1 (`npm install @gltf-transform/cli@4.5.1`)

```bash
cargo xtask fixture-sources /tmp/meshdiet-src     # chair.glb, dense.glb, two-textures.glb
cd tests/fixtures
gltfpack -i /tmp/meshdiet-src/chair.glb -o LOAD-001/bad/models/chair.glb -cc -kn -km -ke
cp LOAD-001/bad/models/chair.glb LOAD-001/good/models/chair.glb
cp LOAD-001/bad/models/chair.glb demo-site/public/models/chair.glb
gltfpack -i /tmp/meshdiet-src/dense.glb -o MESH-001/good/dense-meshopt.glb -cc -kn -km -ke
gltf-transform draco /tmp/meshdiet-src/dense.glb MESH-001/good/dense-draco.glb
gltfpack -i /tmp/meshdiet-src/two-textures.glb -o TEX-002/good/two-textures-ktx2.glb -tc -kn -km -ke
```

Every generated and tool-made model passed `gltf-transform validate` (Khronos glTF Validator) with no
errors or warnings, except the deliberately broken files in `GLTF-001/bad/`.

## Size

About 8.5 MB of unique file content (identical files, such as `hero.glb`, are stored once by git).
Keep the total under 10 MB.
