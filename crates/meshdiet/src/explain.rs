//! `meshdiet rules` and `meshdiet explain RULE` (SPEC-CLI-001 FR-003): why each rule matters, how
//! to fix it with gltfpack and glTF Transform, and what the loader needs after the fix.

use std::fmt::Write as _;

use meshdiet_core::rule::{RuleInfo, catalogue};
use meshdiet_core::{Confidence, Severity};

/// The long text of one rule.
pub struct Text {
    /// Why it costs the user.
    pub why: &'static str,
    /// How to fix it, with the commands.
    pub fix: &'static str,
    /// What the page's loader needs once the file is fixed.
    pub loader: &'static str,
}

const MESHOPT_LOADER: &str = "three.js: `import { MeshoptDecoder } from 'three/addons/libs/meshopt_decoder.module.js'` and \
`loader.setMeshoptDecoder(MeshoptDecoder)` before loading. Draco instead: `const draco = new DRACOLoader(); \
draco.setDecoderPath('/draco/'); loader.setDRACOLoader(draco)`, and serve the decoder files. \
drei `useGLTF` and `<model-viewer>` set both up for you. LOAD-001 checks this in your sources.";

const KTX2_LOADER: &str = "three.js: `const ktx2 = new KTX2Loader().setTranscoderPath('/basis/').detectSupport(renderer); \
loader.setKTX2Loader(ktx2)`, and serve the Basis transcoder files. drei `useGLTF` does not do this for \
you; `<model-viewer>` does. WebP needs no loader change (every current browser reads it).";

const NO_LOADER: &str = "None: the fixed file loads with the same loader setup as before.";

/// The long text of every rule, by id.
const TEXTS: &[(&str, Text)] = &[
    (
        "MESH-001",
        Text {
            why: "Geometry (positions, normals, UVs, indices) is downloaded in full before the model shows up. \
Stored plain, it is often most of a model's bytes; meshopt or Draco usually makes it several times smaller, \
and quantization alone (KHR_mesh_quantization) roughly halves it.",
            fix: "`meshdiet fix in.glb` (gltfpack -cc: meshopt plus quantization), or by hand \
`gltfpack -i in.glb -o out.glb -cc`. With glTF Transform: `gltf-transform meshopt in.glb out.glb`, or \
`gltf-transform draco in.glb out.glb` when you need Draco. Geometry under compress_min (64 KB) is not \
reported; raise it in meshdiet.toml if small files are fine as they are.",
            loader: MESHOPT_LOADER,
        },
    ),
    (
        "MESH-002",
        Text {
            why: "The GPU draws every triangle each frame. Past a few hundred thousand, phones and older laptops \
drop frames and heat up. Compression makes the download smaller but does not change this number.",
            fix: "Lower the detail in your modelling tool (decimate, bake detail into a normal map), or simplify: \
`gltfpack -i in.glb -o out.glb -si 0.5` or `gltf-transform simplify in.glb out.glb --ratio 0.5 --error 0.001`, \
and check the result by eye. `meshdiet fix` never removes triangles. If the page really needs the detail, \
raise triangles_max.",
            loader: NO_LOADER,
        },
    ),
    (
        "MESH-003",
        Text {
            why: "Buffer bytes that no bufferView points at are downloaded and never read. Exporters and \
hand-edited files leave them behind.",
            fix: "Write the file again so the buffers are rebuilt: `meshdiet fix in.glb` (gltfpack) or \
`gltf-transform prune in.glb out.glb`. This is a heuristic (some extensions read buffers in ways the \
JSON does not show), so check the result loads.",
            loader: NO_LOADER,
        },
    ),
    (
        "TEX-001",
        Text {
            why: "A texture is uploaded to the GPU at full size whatever its file size: a 4096×4096 RGBA image \
takes about 89 MB of GPU memory with mipmaps (est.), and phones run out of that first. A model on a web page \
rarely fills enough screen to show more than 2048 pixels.",
            fix: "Resize: `gltfpack -i in.glb -o out.glb -tl 2048` or `gltf-transform resize in.glb out.glb \
--width 2048 --height 2048`. Or raise texture_max_px if the texture is shown up close.",
            loader: NO_LOADER,
        },
    ),
    (
        "TEX-002",
        Text {
            why: "PNG, JPEG, WebP and AVIF are unpacked to 4 bytes per pixel on the GPU. KTX2 (Basis Universal) \
stays compressed there, at about a quarter of the memory, and uploads without a decode on the main thread.",
            fix: "`meshdiet fix --textures ktx2 in.glb` (gltfpack -tc), or `gltf-transform etc1s in.glb out.glb` \
for colour maps and `gltf-transform uastc in.glb out.glb` for normal maps (both need KTX-Software's `ktx` \
on the PATH).",
            loader: KTX2_LOADER,
        },
    ),
    (
        "TEX-003",
        Text {
            why: "A PNG whose header says it cannot be transparent is stored lossless for nothing: as JPEG or \
WebP the same texture is usually several times smaller, and nobody sees the difference on a 3D surface.",
            fix: "`meshdiet fix in.glb` (gltfpack -tw, WebP) or `gltf-transform webp in.glb out.glb`. JPEG is \
fine for colour maps but not for normal maps, where its blocks show up as lighting artefacts; use WebP there.",
            loader: "None for JPEG or WebP. KTX2 instead needs the KTX2Loader (see TEX-002).",
        },
    ),
    (
        "TEX-004",
        Text {
            why: "The browser downloads the whole image before the model can show, and images are often most \
of a model's weight.",
            fix: "Recompress: `meshdiet fix in.glb` (gltfpack -tw, WebP) or `gltf-transform webp in.glb out.glb`; \
resize too if it is larger than it is ever shown (`gltf-transform resize`). Or raise image_max.",
            loader: "None for WebP. KTX2 instead needs the KTX2Loader (see TEX-002).",
        },
    ),
    (
        "GLTF-001",
        Text {
            why: "Loaders stop at the first broken part, so the model, or part of it, never shows up on the \
page. The usual causes are a cut-off upload, a `.bin` or texture that was not copied next to the `.gltf`, or \
a hand edit.",
            fix: "Export again, preferably as one `.glb` so no file can go missing (`gltf-transform copy \
in.gltf out.glb`), and run the Khronos glTF Validator (https://github.khronos.org/glTF-Validator/) for the \
exact error.",
            loader: NO_LOADER,
        },
    ),
    (
        "GLTF-002",
        Text {
            why: "Base64 makes binary data about a third bigger, the browser has to decode it on the main \
thread, and nothing can start until the whole JSON has arrived.",
            fix: "Convert to a binary GLB: `gltf-transform copy in.gltf out.glb` or \
`gltfpack -i in.gltf -o out.glb`. To keep a `.gltf`, write the buffers and images as separate files.",
            loader: NO_LOADER,
        },
    ),
    (
        "LOAD-001",
        Text {
            why: "Compressed models need their decoder set up in the app. Without it three.js GLTFLoader \
stops with an error such as `No DRACOLoader instance provided`, and the model never shows up, often only \
in production.",
            fix: "Set up the decoder the models use (below). If the app code is not inside the scanned \
folder, point meshdiet at it with `--src`. This check reads source text, so it can miss a decoder set up \
in an unusual way; check by hand before acting.",
            loader: "Draco: `loader.setDRACOLoader(draco)`. meshopt: `loader.setMeshoptDecoder(MeshoptDecoder)`. \
KTX2: `loader.setKTX2Loader(ktx2)`. drei `useGLTF` covers Draco and meshopt but not KTX2.",
        },
    ),
    (
        "SIZE-001",
        Text {
            why: "Every byte of the file (with its `.bin` and images) is downloaded before the model appears, \
so one heavy file holds up the whole page on a phone connection.",
            fix: "Start with the biggest part the message names: `meshdiet fix in.glb` compresses geometry and \
textures (gltfpack -cc -tw), or with glTF Transform `gltf-transform optimize in.glb out.glb`. Shrink \
oversized textures (TEX-001). Or raise file_max in meshdiet.toml if this size is intended.",
            loader: MESHOPT_LOADER,
        },
    ),
    (
        "SIZE-002",
        Text {
            why: "A build only gets heavier one model at a time. Past the budget, a first visit on mobile waits \
for megabytes of 3D, and hosting egress grows with it.",
            fix: "Compress the largest assets first (the weight table lists them; `meshdiet fix`), drop \
models no page loads, or raise build_max if the growth is intended. To stop growth without a fixed number, \
use a baseline (`--write-baseline`, then `--baseline`).",
            loader: MESHOPT_LOADER,
        },
    ),
    (
        "SIZE-003",
        Text {
            why: "Everything in a page group loads with that page, so the visitor waits for the whole sum \
before the scene is complete.",
            fix: "Compress or shrink the largest assets in the group (`meshdiet fix`), load some later (on \
interaction or scroll), or raise the page's max if intended.",
            loader: MESHOPT_LOADER,
        },
    ),
    (
        "CFG-PAGE",
        Text {
            why: "A [pages] glob that matches nothing turns that page's budget off without a word.",
            fix: "Fix the glob. Globs match asset paths relative to the scan root, or with the scanned \
folder in front; `*` stays inside a folder, `**` crosses folders.",
            loader: NO_LOADER,
        },
    ),
    (
        "CFG-IGN",
        Text {
            why: "An ignore outlives the reason it was added for. Without the reason written down, nobody can \
tell later whether it still holds, and it quietly hides a real problem.",
            fix: "Add `reason = \"...\"` to the entry: `{ rule = \"TEX-001\", path = \"public/hero/*.glb\", \
reason = \"hero model, shown full screen\" }`. The entry applies either way.",
            loader: NO_LOADER,
        },
    ),
    (
        "BASE-GROW",
        Text {
            why: "The baseline is the weight the team accepted. Anything past it is new weight users download, \
and weight only goes one way unless something stops it.",
            fix: "Compress or shrink what grew (`meshdiet fix`). If the growth is intended, write the baseline \
again (`meshdiet scan --write-baseline FILE`) and commit it, so the review shows the new weight. \
`--tolerance` sets how much growth is noise (1% by default).",
            loader: NO_LOADER,
        },
    ),
];

/// The long text for `id`, or `None` for a rule this build does not know how to explain yet.
#[must_use]
pub fn text(id: &str) -> Option<&'static Text> {
    TEXTS.iter().find(|(i, _)| *i == id).map(|(_, t)| t)
}

fn severity(s: Severity) -> &'static str {
    s.label()
}

fn confidence(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high",
        Confidence::Medium => "medium",
    }
}

/// `meshdiet rules`: one line per rule.
pub fn rule_list() -> String {
    let list = catalogue();
    let width = list.iter().map(|i| i.id.len()).max().unwrap_or(8);
    let mut out = format!(
        "{:<width$}  {:<8}  {:<10}  WHAT IT FINDS\n",
        "RULE", "SEVERITY", "CONFIDENCE"
    );
    for i in list {
        let _ = writeln!(
            out,
            "{:<width$}  {:<8}  {:<10}  {}",
            i.id,
            severity(i.severity),
            confidence(i.confidence),
            i.summary
        );
    }
    out.push_str("\n`meshdiet explain RULE` tells why one matters and how to fix it.\n");
    out
}

/// `text` in lines of at most `width` characters, each indented by two spaces.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width - 2 {
            let _ = writeln!(out, "  {line}");
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        let _ = writeln!(out, "  {line}");
    }
    out
}

/// `meshdiet explain RULE`.
pub fn explain(i: &RuleInfo) -> String {
    let unsure = match i.confidence {
        Confidence::High => "high",
        Confidence::Medium => {
            "medium (a heuristic; check before acting, fails CI only with --strict)"
        }
    };
    let mut out = format!(
        "{} {}\nSeverity: {} · Confidence: {unsure}\n\nFinds: {}\n",
        i.id,
        i.title,
        severity(i.severity),
        i.summary
    );
    if let Some(t) = text(i.id) {
        for (head, body) in [
            ("Why it matters", t.why),
            ("How to fix it", t.fix),
            ("Loader change", t.loader),
        ] {
            let _ = write!(out, "\n{head}\n{}", wrap(body, 96));
        }
    }
    let _ = write!(
        out,
        "\nTo accept it for some files, add to meshdiet.toml:\n  [[ignore]]\n  rule = \"{}\"\n  path = \"public/models/hero/*.glb\"\n  reason = \"why this is fine\"\n",
        i.id
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_has_a_long_text_with_both_tools_where_it_fixes_a_file() {
        for i in catalogue() {
            assert!(text(i.id).is_some(), "{} has no explanation", i.id);
            let Some(t) = text(i.id) else { continue };
            assert!(!t.why.is_empty() && !t.fix.is_empty() && !t.loader.is_empty());
            if i.id.starts_with("MESH-0") || i.id.starts_with("TEX-") {
                assert!(
                    t.fix.contains("gltfpack") && t.fix.contains("gltf-transform"),
                    "{} should name both tools",
                    i.id
                );
            }
        }
    }
}
