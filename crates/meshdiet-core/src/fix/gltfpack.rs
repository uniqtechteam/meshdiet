//! Finding gltfpack and checking its version (SPEC-FIX-001 FR-001, ADR-004). meshdiet never
//! downloads it; when it is missing we say how to install it.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Oldest gltfpack `fix` was tested with.
pub const MIN_VERSION: (u32, u32) = (1, 3);

/// Environment variable that names the gltfpack binary.
pub const ENV: &str = "MESHDIET_GLTFPACK";

/// Where the release zips are.
pub const RELEASES: &str = "https://github.com/zeux/meshoptimizer/releases";

/// A gltfpack binary that answered with a version we accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gltfpack {
    /// The binary.
    pub path: PathBuf,
    /// Its version, as it prints it (`1.3`).
    pub version: String,
}

/// Why no usable gltfpack was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateError {
    /// Nothing at the places we looked.
    Missing {
        /// Where we looked, in order.
        tried: Vec<String>,
    },
    /// Found, but older than [`MIN_VERSION`].
    TooOld {
        /// The binary.
        path: PathBuf,
        /// The version it printed.
        version: String,
    },
    /// Found something that did not answer like gltfpack.
    NotGltfpack {
        /// The binary.
        path: PathBuf,
        /// What went wrong.
        detail: String,
    },
}

impl fmt::Display for LocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (major, minor) = MIN_VERSION;
        match self {
            Self::Missing { tried } => write!(
                f,
                "`meshdiet fix` needs gltfpack {major}.{minor} or newer and could not find it (looked at {})",
                tried.join(", ")
            ),
            Self::TooOld { path, version } => write!(
                f,
                "{} is gltfpack {version}; `meshdiet fix` needs {major}.{minor} or newer",
                path.display()
            ),
            Self::NotGltfpack { path, detail } => {
                write!(
                    f,
                    "{} does not look like gltfpack: {detail}",
                    path.display()
                )
            }
        }
    }
}

/// What to print under a [`LocateError`]: one install line per OS.
#[must_use]
pub fn install_help() -> String {
    format!(
        "Install gltfpack (MIT, one file) from the meshoptimizer release page, {RELEASES}/latest :
  macOS:   download gltfpack-macos.zip, unzip it and move gltfpack to /usr/local/bin
  Linux:   download gltfpack-ubuntu.zip, unzip it and move gltfpack to ~/.local/bin
  Windows: download gltfpack-windows.zip, unzip it and put gltfpack.exe in a folder on PATH
Or tell meshdiet where it is with --gltfpack PATH or {ENV}=PATH.
The npm package `gltfpack` is a WebAssembly build that cannot compress textures; use the release zip."
    )
}

/// Read `gltfpack 1.3` (the first line of its usage text) into `(1, 3)` and `"1.3"`.
#[must_use]
pub fn parse_version(text: &str) -> Option<((u32, u32), String)> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("gltfpack"))?;
    let version = line.split_whitespace().nth(1)?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |m| {
        m.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    })?;
    Some(((major, minor), version.to_owned()))
}

/// Run `path` with no arguments and check the version in its usage text.
///
/// # Errors
/// When it cannot run, prints no version, or is too old.
pub fn check(path: &Path) -> Result<Gltfpack, LocateError> {
    // gltfpack prints its usage and exits 1 when run with no arguments; the exit code means nothing.
    let out = Command::new(path)
        .output()
        .map_err(|e| LocateError::NotGltfpack {
            path: path.to_path_buf(),
            detail: format!("it does not run ({e})"),
        })?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let Some((number, version)) = parse_version(&text) else {
        return Err(LocateError::NotGltfpack {
            path: path.to_path_buf(),
            detail: "it printed no `gltfpack <version>` line".to_owned(),
        });
    };
    if number < MIN_VERSION {
        return Err(LocateError::TooOld {
            path: path.to_path_buf(),
            version,
        });
    }
    Ok(Gltfpack {
        path: path.to_path_buf(),
        version,
    })
}

/// The file name gltfpack has on this OS.
fn exe_name() -> String {
    format!("gltfpack{}", std::env::consts::EXE_SUFFIX)
}

/// Find gltfpack: `explicit` (`--gltfpack`), then `env` ([`ENV`]), then each folder of `path_var`
/// (`PATH`). The first one found is checked; a wrong explicit path is an error, not a fallback.
///
/// # Errors
/// [`LocateError`] when nothing usable is found.
pub fn locate_with(
    explicit: Option<&Path>,
    env: Option<OsString>,
    path_var: Option<&OsStr>,
) -> Result<Gltfpack, LocateError> {
    let chosen = explicit
        .map(|p| (p.to_path_buf(), "--gltfpack"))
        .or_else(|| {
            env.filter(|v| !v.is_empty())
                .map(|v| (PathBuf::from(v), ENV))
        });
    if let Some((path, from)) = chosen {
        if !path.is_file() {
            return Err(LocateError::Missing {
                tried: vec![format!("{} ({from})", path.display())],
            });
        }
        return check(&path);
    }
    let name = exe_name();
    let found = path_var
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| dir.join(&name))
        .find(|p| p.is_file());
    match found {
        Some(path) => check(&path),
        None => Err(LocateError::Missing {
            tried: vec![
                "--gltfpack".to_owned(),
                ENV.to_owned(),
                format!("{name} on PATH"),
            ],
        }),
    }
}

/// [`locate_with`] using the real `MESHDIET_GLTFPACK` and `PATH`.
///
/// # Errors
/// [`LocateError`] when nothing usable is found.
pub fn locate(explicit: Option<&Path>) -> Result<Gltfpack, LocateError> {
    locate_with(
        explicit,
        std::env::var_os(ENV),
        std::env::var_os("PATH").as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_from_the_usage_text() {
        let usage = "gltfpack 1.3\nUsage: gltfpack [options] -i input -o output\n";
        assert_eq!(parse_version(usage), Some(((1, 3), "1.3".to_owned())));
        assert_eq!(parse_version("gltfpack 0.21\n").map(|v| v.0), Some((0, 21)));
        assert_eq!(
            parse_version("gltfpack 1.10-dev\n").map(|v| v.0),
            Some((1, 10))
        );
        assert_eq!(parse_version("usage: something else"), None);
    }

    #[test]
    fn nothing_found_lists_where_it_looked() {
        let err = locate_with(None, None, Some(OsStr::new(""))).err();
        assert!(
            matches!(&err, Some(LocateError::Missing { tried }) if tried.len() == 3),
            "{err:?}"
        );
    }

    #[test]
    fn a_wrong_explicit_path_does_not_fall_back() {
        let err = locate_with(
            Some(Path::new("/no/such/gltfpack")),
            None,
            std::env::var_os("PATH").as_deref(),
        );
        assert!(matches!(err, Err(LocateError::Missing { .. })));
    }

    #[test]
    fn help_has_a_line_per_os() {
        let help = install_help();
        for os in ["macOS:", "Linux:", "Windows:"] {
            assert!(help.contains(os), "{os} missing");
        }
        assert!(help.contains("npm"));
    }
}
