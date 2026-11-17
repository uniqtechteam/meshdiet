//! The engine behind the `meshdiet` command line tool.
//!
//! It reads GLB and glTF files from disk and reports what makes them heavy. Nothing here touches
//! the network.

#![forbid(unsafe_code)]

/// Version of the engine, shown by `meshdiet --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
