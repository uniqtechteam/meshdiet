//! The "37 uncompressed models, 49.8 MB" set from the shop audit story (SPEC-WEIGHT-001 AC-001).
//!
//! Too big for git, so tests build it into a temporary folder. Only the two numbers come from the
//! audit; the models are plain generated grids.

use std::fs;
use std::io;
use std::path::Path;

use crate::gltf::Builder;

/// How many models the set has.
pub const MODELS: usize = 37;
/// Total bytes the set adds up to (49.8 MB, 1 MB = 1,000,000 bytes).
pub const TOTAL_BYTES: u64 = 49_800_000;

fn model(index: usize, triangles: u32) -> Vec<u8> {
    let mut b = Builder::new();
    let m = b.plain_material("product");
    let mesh = b.grid_mesh(&format!("product-{index:02}"), triangles, Some(m));
    b.node(&format!("product-{index:02}"), mesh, 0.0);
    b.glb()
}

/// Triangle counts for the first 36 models: a spread from small props to big pieces.
fn spread(index: usize) -> u32 {
    let step = u32::try_from((index * 11) % 36).unwrap_or(0);
    22_000 + step * 2_000
}

/// Write the set into `dir` as `product-01.glb` … `product-37.glb`. Returns the total bytes, which
/// is within 10 KB of [`TOTAL_BYTES`]: the last model is sized to land on it.
///
/// # Errors
/// When a file cannot be written.
pub fn write(dir: &Path) -> io::Result<u64> {
    fs::create_dir_all(dir)?;
    let mut total = 0u64;
    for i in 1..MODELS {
        let bytes = model(i, spread(i));
        total += bytes.len() as u64;
        fs::write(dir.join(format!("product-{i:02}.glb")), bytes)?;
    }
    let target = TOTAL_BYTES.saturating_sub(total);
    // binary search the triangle count of the last model so the total lands on the target
    let (mut lo, mut hi) = (1u32, 400_000u32);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if (model(MODELS, mid).len() as u64) < target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let last = model(MODELS, lo);
    total += last.len() as u64;
    fs::write(dir.join(format!("product-{MODELS:02}.glb")), last)?;
    Ok(total)
}
