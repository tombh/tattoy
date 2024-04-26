//! Just main(). Keep as small as possible.
#![allow(clippy::cargo)]
#![allow(clippy::blanket_clippy_restriction_lints)]
#![allow(clippy::restriction)]

use color_eyre::eyre::Result;
use tattoy::run;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    run::run()?;
    Ok(())
}
