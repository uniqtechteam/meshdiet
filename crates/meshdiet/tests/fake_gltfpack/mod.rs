//! A fake gltfpack for the error paths of `meshdiet fix` (SPEC-FIX-001 AC-004 to AC-006).
//!
//! The `fix` test binary runs this instead of its tests when `FAKE_GLTFPACK_MODE` is set:
//!
//! * no arguments: prints `gltfpack <FAKE_GLTFPACK_VERSION or 1.3>` and usage, exits 1 like gltfpack
//! * `copy`: writes the input unchanged
//! * `larger`: writes the input with its JSON chunk padded, so the output is bigger
//! * `drop-names`: writes the input with every node name removed
//! * `fail`: prints 30 error lines on stderr and exits 1
//!
//! Every call with arguments is appended to `FAKE_GLTFPACK_LOG`, one argument per line.

use std::fs;
use std::io::Write as _;
use std::process::ExitCode;

use serde_json::Value;

/// Environment variable that selects the behaviour.
pub const MODE: &str = "FAKE_GLTFPACK_MODE";
/// Environment variable with the version to print.
pub const VERSION: &str = "FAKE_GLTFPACK_VERSION";
/// Environment variable naming the call log.
pub const LOG: &str = "FAKE_GLTFPACK_LOG";

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).cloned()
}

/// Rewrite the JSON chunk of a GLB.
fn with_json(glb: &[u8], edit: impl FnOnce(&mut Value), pad: usize) -> Option<Vec<u8>> {
    let len = usize::try_from(u32::from_le_bytes(glb.get(12..16)?.try_into().ok()?)).ok()?;
    let mut doc: Value = serde_json::from_slice(glb.get(20..20 + len)?).ok()?;
    edit(&mut doc);
    let rest = glb.get(20 + len..)?;
    let mut json = serde_json::to_vec(&doc).ok()?;
    json.resize(json.len() + pad, b' ');
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = u32::try_from(12 + 8 + json.len() + rest.len()).ok()?;
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&u32::try_from(json.len()).ok()?.to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(rest);
    Some(out)
}

/// Act as gltfpack.
pub fn run(mode: &str) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        let version = std::env::var(VERSION).unwrap_or_else(|_| "1.3".to_owned());
        println!("gltfpack {version}\nUsage: gltfpack [options] -i input -o output");
        return ExitCode::FAILURE;
    }
    if let Ok(log) = std::env::var(LOG)
        && let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(log)
    {
        let _ = writeln!(f, "{}", args.join("\n"));
    }
    if mode == "fail" {
        for i in 1..=30 {
            eprintln!("error line {i}");
        }
        return ExitCode::FAILURE;
    }
    let (Some(input), Some(output)) = (arg_after(&args, "-i"), arg_after(&args, "-o")) else {
        eprintln!("fake gltfpack: -i and -o are required");
        return ExitCode::FAILURE;
    };
    let Ok(glb) = fs::read(&input) else {
        eprintln!("fake gltfpack: cannot read {input}");
        return ExitCode::FAILURE;
    };
    let written = match mode {
        "copy" => Some(glb),
        "larger" => with_json(&glb, |_| {}, 4096),
        "drop-names" => with_json(
            &glb,
            |doc| {
                for node in doc["nodes"].as_array_mut().into_iter().flatten() {
                    if let Some(n) = node.as_object_mut() {
                        n.remove("name");
                    }
                }
            },
            0,
        ),
        _ => None,
    };
    if let Some(Ok(())) = written.map(|bytes| fs::write(&output, bytes)) {
        ExitCode::SUCCESS
    } else {
        eprintln!("fake gltfpack: mode {mode} failed on {input}");
        ExitCode::FAILURE
    }
}
