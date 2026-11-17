//! The engine behind the `meshdiet` command line tool.
//!
//! It reads GLB and glTF files from disk and reports what makes them heavy. Only headers and the
//! glTF JSON are read; buffers and pixels are never decoded, and nothing here touches the network.

#![forbid(unsafe_code)]

pub mod asset;
pub mod glb;
pub mod gltf;
pub mod images;
pub mod walk;

pub use asset::{Asset, AssetKind, Compression, Estimate, ImageInfo, ImageSource};

/// Version of the engine, shown by `meshdiet --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
