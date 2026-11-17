//! `cargo xtask <command>`: maintenance tasks for the meshdiet repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  cargo xtask gen-fixtures            rebuild tests/fixtures from code
  cargo xtask fixture-sources DIR     write the inputs for the compressed fixtures (see tests/fixtures/README.md)
  cargo xtask story-set DIR           write the 37-model / 49.8 MB set";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["gen-fixtures"] => write_all(&fixtures_dir(), xtask::fixtures::all()),
        ["fixture-sources", dir] => write_all(Path::new(dir), xtask::fixtures::sources()),
        ["story-set", dir] => xtask::story::write(Path::new(dir)).map(|total| {
            println!("{} models, {total} bytes in {dir}", xtask::story::MODELS);
        }),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}

fn write_all(root: &Path, files: Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
    let count = files.len();
    for (rel, bytes) in files {
        let path = root.join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
    }
    println!("wrote {count} files under {}", root.display());
    Ok(())
}
