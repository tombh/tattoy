//! Just `main()`. Keep as small as possible.

// TODO: Consider using `mod.rs`. As pointed out by @Justus_Fluegel, the disadvantage of
// this approach is that when moving files/modules, you _also_ have to move these module
// definitions.

pub mod cli_args;
pub mod loader;
pub mod pty;
pub mod renderer;
pub mod run;
pub mod shadow_tty;
pub mod shared_state;
pub mod surface;

/// This is where all the various tattoys are kept
pub mod tattoys {
    pub mod index;
    pub mod random_walker;
    pub mod utils;

    /// The smokey cursor Tattoy
    pub mod smokey_cursor {
        pub mod config;
        pub mod main;
        pub mod particle;
        pub mod particles;
        pub mod simulation;
    }
}

use color_eyre::eyre::Result;

#[expect(clippy::non_ascii_literal, reason = "It's just for debugging")]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    run::run().await?;
    tracing::debug!("Tattoy is exiting 🙇");
    Ok(())
}
