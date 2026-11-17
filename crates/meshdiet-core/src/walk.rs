//! Finds the `.glb` and `.gltf` files to scan (SPEC-CLI-001 FR-001). Walker settings first written
//! for vibeguard (ADR-011).

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// Folders never worth walking, even when a project forgot to ignore them.
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "target"];

/// One asset file found by the walk.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Found {
    /// Path relative to the scan root, forward slashes.
    pub rel: String,
    /// Path on disk.
    pub abs: PathBuf,
}

/// What a walk found.
#[derive(Debug)]
pub struct Walk {
    /// The folder paths are shown relative to.
    pub root: PathBuf,
    /// Asset files, sorted by path.
    pub files: Vec<Found>,
}

fn is_asset(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("glb") || e.eq_ignore_ascii_case("gltf"))
}

/// `path` relative to `root` with forward slashes, or `path` as given when it is elsewhere.
fn relative(root: &Path, path: &Path) -> String {
    let shown = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<String> = shown
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// Walk `paths` (a folder, files, or several of both). `.gitignore` is respected below each given
/// path, `node_modules`, `.git` and `target` are skipped, and symlinks are not followed. A folder
/// given explicitly is always walked, even when a parent `.gitignore` lists it (`dist/`).
///
/// # Errors
/// When a path does not exist, or a file given explicitly is not `.glb` or `.gltf`.
pub fn walk(paths: &[PathBuf]) -> Result<Walk, String> {
    let default = [PathBuf::from(".")];
    let paths = if paths.is_empty() {
        &default[..]
    } else {
        paths
    };
    for p in paths {
        if !p.exists() {
            return Err(format!("{} does not exist", p.display()));
        }
        if p.is_file() && !is_asset(p) {
            return Err(format!("{} is not a .glb or .gltf file", p.display()));
        }
    }
    let root = match paths {
        [one] if one.is_dir() => one.clone(),
        [one] => one
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        _ => PathBuf::from("."),
    };
    let mut files = Vec::new();
    for p in paths {
        if p.is_file() {
            files.push(Found {
                rel: relative(&root, p),
                abs: p.clone(),
            });
            continue;
        }
        let walker = WalkBuilder::new(p)
            .hidden(false)
            .parents(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true)
            .require_git(false)
            .follow_links(false)
            .filter_entry(|e| {
                !(e.depth() > 0
                    && e.file_type().is_some_and(|t| t.is_dir())
                    && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
            })
            .build();
        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|t| t.is_file()) && is_asset(entry.path()) {
                files.push(Found {
                    rel: relative(&root, entry.path()),
                    abs: entry.path().to_path_buf(),
                });
            }
        }
    }
    files.sort();
    files.dedup_by(|a, b| a.rel == b.rel);
    Ok(Walk { root, files })
}
