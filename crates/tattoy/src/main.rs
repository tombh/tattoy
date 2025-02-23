//! Just `main()`. Keep as small as possible.

// TODO: Consider using `mod.rs`. As pointed out by @Justus_Fluegel, the disadvantage of
// this approach is that when moving files/modules, you _also_ have to move these module
// definitions.

pub mod cli_args;
pub mod config;
pub mod input;
pub mod loader;
pub mod opaque_cell;
pub mod palette_parser;
pub mod renderer;
pub mod run;
pub mod shared_state;
pub mod surface;
pub mod terminal_proxy;

/// This is where all the various tattoys are kept
pub mod tattoys {
    pub mod index;
    pub mod minimap;
    pub mod random_walker;
    pub mod scrollbar;
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
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _, Layer as _};

#[expect(clippy::non_ascii_literal, reason = "It's just for debugging")]
#[expect(
    clippy::print_stderr,
    reason = "It's our central place for reporting errors to the user"
)]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    setup_logging()?;
    color_eyre::install()?;
    tracing::debug!(
        "Tokio runtime flavour: {:?}",
        tokio::runtime::Handle::current().runtime_flavor()
    );
    let state_arc = shared_state::SharedState::init().await?;
    let result = run::run(&std::sync::Arc::clone(&state_arc)).await;
    tracing::debug!("Tattoy is exiting 🙇");
    if let Err(error) = result {
        tracing::error!("{error:?}");
        eprintln!("Error: {error}");
        eprintln!("See `./tattoy.log` for more details");
    }
    Ok(())
}

// TODO: don't log by default.
/// Setup logging
fn setup_logging() -> Result<()> {
    let log_file = "tattoy.log";
    let file = std::fs::File::create(log_file)?;
    let logfile_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                // We don't want any of the trace lines that make the `tokio-console` possible
                .add_directive("tokio=debug".parse()?)
                .add_directive("runtime=debug".parse()?),
        );

    let tracing_setup = tracing_subscriber::registry().with(logfile_layer);

    if std::env::var_os("ENABLE_TOKIO_CONSOLE") == Some("1".into()) {
        let console_layer = console_subscriber::spawn();
        tracing_setup.with(console_layer).init();
    } else {
        tracing_setup.init();
    }

    Ok(())
}
