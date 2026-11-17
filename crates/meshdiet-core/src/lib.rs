//! The engine behind the `meshdiet` command line tool.
//!
//! It reads GLB and glTF files from disk and reports what makes them heavy. Only headers and the
//! glTF JSON are read; buffers and pixels are never decoded, and nothing here touches the network.

#![forbid(unsafe_code)]

pub mod asset;
pub mod baseline;
mod config;
mod finding;
pub mod fix;
pub mod glb;
pub mod gltf;
pub mod images;
mod report;
pub mod rule;
pub mod rules;
pub mod source_index;
pub mod walk;
pub mod weight;

pub use asset::{Asset, AssetKind, Compression, Estimate, ImageInfo, ImageSource};
pub use baseline::{Baseline, BaselineDiff};
pub use config::{CFG_IGN, Config, Ignore, PageGroup, parse_fail_on, parse_size, rule_id};
pub use finding::{Confidence, Finding, Measured, Severity};
pub use report::{FailOn, Ignored, Options, Report, SCHEMA, Summary, scan, scan_with};
pub use source_index::{Decoder, SourceIndex};
pub use weight::{PageTotal, Totals};

/// Version of the engine, shown by `meshdiet --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
