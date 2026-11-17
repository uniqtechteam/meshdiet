//! The engine behind the `meshdiet` command line tool.
//!
//! It reads GLB and glTF files from disk and reports what makes them heavy. Only headers and the
//! glTF JSON are read; buffers and pixels are never decoded, and nothing here touches the network.

#![forbid(unsafe_code)]

pub mod asset;
mod config;
mod finding;
pub mod glb;
pub mod gltf;
pub mod images;
mod report;
pub mod rule;
pub mod rules;
pub mod walk;
pub mod weight;

pub use asset::{Asset, AssetKind, Compression, Estimate, ImageInfo, ImageSource};
pub use config::{Config, PageGroup};
pub use finding::{Confidence, Finding, Measured, Severity};
pub use report::{FailOn, Options, Report, SCHEMA, Summary, scan, scan_with};
pub use weight::{PageTotal, Totals};

/// Version of the engine, shown by `meshdiet --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
