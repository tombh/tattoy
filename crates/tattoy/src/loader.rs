//! The manager of all the fancy Tattoy eye-candy code
//!
//! I want to base the plugin architecture on Nushell's, see:
//! <https://www.nushell.sh/contributor-book/plugin_protocol_reference.html>

use color_eyre::eyre::Result;

use crate::run::{FrameUpdate, Protocol};

/// Start the main loader thread
pub(crate) fn start_tattoys(
    enabled_tattoys: Vec<String>,
    input: tokio::sync::broadcast::Sender<Protocol>,
    output: tokio::sync::mpsc::Sender<FrameUpdate>,
) -> std::thread::JoinHandle<Result<(), color_eyre::eyre::Error>> {
    let tokio_runtime = tokio::runtime::Handle::current();
    std::thread::spawn(move || -> Result<()> {
        tokio_runtime.block_on(async {
            let mut tattoy_futures = tokio::task::JoinSet::new();

            tracing::info!("Starting 'scrollbar' tattoy...");
            tattoy_futures.spawn(crate::tattoys::scrollbar::Scrollbar::start(
                input.clone(),
                output.clone(),
            ));

            if enabled_tattoys.contains(&"random_walker".to_owned()) {
                tracing::info!("Starting 'random_walker' tattoy...");
                tattoy_futures.spawn(crate::tattoys::random_walker::RandomWalker::start(
                    input.clone(),
                    output.clone(),
                ));
            }

            if enabled_tattoys.contains(&"minimap".to_owned()) {
                tracing::info!("Starting 'minimap' tattoy...");
                tattoy_futures.spawn(crate::tattoys::minimap::Minimap::start(
                    input.clone(),
                    output.clone(),
                ));
            }

            if enabled_tattoys.contains(&"smokey_cursor".to_owned()) {
                tracing::info!("Starting 'smokey_cursor' tattoy...");
                tattoy_futures.spawn(crate::tattoys::smokey_cursor::main::SmokeyCursor::start(
                    input.clone(),
                    output.clone(),
                ));
            }

            while let Some(result) = tattoy_futures.join_next().await {
                if let Err(error) = result {
                    tracing::error!("Error running a tattoy: {error:?}");
                }
            }

            Ok(())
        })
    })
}
