//! Just main(). Keep as small as possible.
// The `main.rs` file is special in Rust.
// So attributes here have no affect on the main codebase. If the file remains minimal we can just
// blanket allow lint groups.
#![allow(clippy::cargo)]
#![allow(clippy::blanket_clippy_restriction_lints)]
#![allow(clippy::restriction)]

// use std::error::Error;

use color_eyre::eyre::Result;
use tattoy::run;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    run::run().await?;
    Ok(())
}
