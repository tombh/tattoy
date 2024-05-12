//! Just main(). Keep as small as possible.
#![allow(clippy::cargo)]
#![allow(clippy::blanket_clippy_restriction_lints)]
#![allow(clippy::restriction)]

use color_eyre::eyre::Result;
use tattoy::run;

#[tokio::main(
    // I wanted this to maybe be able to put the tattoy work in their own threads, but it doesn't
    // seem make any new threads.
    flavor = "multi_thread"
)]
async fn main() -> Result<()> {
    color_eyre::install()?;
    run::run().await?;
    tracing::debug!("Tattoy is exiting 🙇");
    // TODO: something is still running in the background that prevents the app from exiting
    // by itself.
    std::process::exit(0);
}
