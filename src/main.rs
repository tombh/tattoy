//! Just `main()`. Keep as small as possible.
#![allow(clippy::cargo)]
#![allow(clippy::blanket_clippy_restriction_lints)]
#![allow(clippy::restriction)]

use color_eyre::eyre::Result;
use tattoy::run;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    run::run().await?;
    tracing::debug!("Tattoy is exiting 🙇");
    Ok(())
}
