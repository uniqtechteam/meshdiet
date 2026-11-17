//! `meshdiet`: find the 3D models that make your website slow.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;

/// Find the 3D models that make your website slow.
#[derive(Parser)]
#[command(name = "meshdiet", version = meshdiet_core::VERSION, about)]
struct Cli {}

fn main() -> ExitCode {
    let Cli {} = Cli::parse();
    ExitCode::SUCCESS
}
