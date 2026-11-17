# demo-site

A tiny three.js shop page with generated models (`cargo xtask gen-fixtures`). Problems planted on purpose:

| File | Problem | Rule |
|---|---|---|
| `public/models/hero.glb` | 5.1 MB: uncompressed geometry and a 3.8 MB JPEG | SIZE-001, MESH-001 |
| `public/models/sofa.glb` | 4096x4096 PNG texture | TEX-001, TEX-002 |
| `public/models/lamp.gltf` | external `.bin` and a 270 KB RGB PNG that has no alpha | TEX-003 |
| `public/models/chair.glb` | meshopt-compressed, but `src/main.js` never calls `setMeshoptDecoder` | LOAD-001 |
