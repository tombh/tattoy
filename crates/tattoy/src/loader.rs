//! The manager of all the fancy Tattoy eye-candy code
//!
//! I want to base the plugin architecture on Nushell's, see:
//! <https://www.nushell.sh/contributor-book/plugin_protocol_reference.html>

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::run::{FrameUpdate, Protocol};

/// Start the main loader thread
pub(crate) fn start_tattoys(
    enabled_tattoys: Vec<String>,
    input: tokio::sync::broadcast::Sender<Protocol>,
    output: tokio::sync::mpsc::Sender<FrameUpdate>,
    state: Arc<crate::shared_state::SharedState>,
) -> std::thread::JoinHandle<Result<(), color_eyre::eyre::Error>> {
    let tokio_runtime = tokio::runtime::Handle::current();
    std::thread::spawn(move || -> Result<()> {
        tokio_runtime.block_on(async {
            let maybe_palette =
                crate::config::main::Config::load_palette(Arc::clone(&state)).await?;
            let Some(palette) = maybe_palette.as_ref() else {
                color_eyre::eyre::bail!("You must first parse your terminal's palette.");
            };

            let mut tattoy_futures = tokio::task::JoinSet::new();

            tracing::info!("Starting 'scrollbar' tattoy...");
            tattoy_futures.spawn(crate::tattoys::scrollbar::Scrollbar::start(
                input.clone(),
                output.clone(),
                Arc::clone(&state),
            ));

            if enabled_tattoys.contains(&"random_walker".to_owned()) {
                tracing::info!("Starting 'random_walker' tattoy...");
                tattoy_futures.spawn(crate::tattoys::random_walker::RandomWalker::start(
                    input.clone(),
                    output.clone(),
                    Arc::clone(&state),
                ));
            }

            if enabled_tattoys.contains(&"minimap".to_owned())
                || state.config.read().await.minimap.enabled
            {
                tracing::info!("Starting 'minimap' tattoy...");
                tattoy_futures.spawn(crate::tattoys::minimap::Minimap::start(
                    input.clone(),
                    output.clone(),
                    Arc::clone(&state),
                ));
            }

            if enabled_tattoys.contains(&"shaders".to_owned())
                || state.config.read().await.shader.enabled
            {
                tracing::info!("Starting 'shaders' tattoy...");
                tattoy_futures.spawn(crate::tattoys::shaders::main::Shaders::start(
                    input.clone(),
                    output.clone(),
                    Arc::clone(&state),
                ));
            }

            if enabled_tattoys.contains(&"bg_command".to_owned())
                || state.config.read().await.bg_command.enabled
            {
                tracing::info!("Starting 'bg_command' tattoy...");
                tattoy_futures.spawn(crate::tattoys::bg_command::BGCommand::start(
                    input.clone(),
                    output.clone(),
                    Arc::clone(&state),
                    palette.clone(),
                ));
            }

            for plugin_config in &state.config.read().await.plugins {
                if let Some(is_enabled) = plugin_config.enabled {
                    if !is_enabled {
                        continue;
                    }
                }

                tattoy_futures.spawn(crate::tattoys::plugins::Plugin::start(
                    plugin_config.clone(),
                    palette.clone(),
                    Arc::clone(&state),
                    input.clone(),
                    output.clone(),
                ));
            }

            while let Some(completes) = tattoy_futures.join_next().await {
                match completes {
                    Ok(result) => match result {
                        Ok(()) => tracing::debug!("A tattoy succesfully exited"),
                        Err(error) => tracing::debug!("A tattoy exited with an error: {error}"),
                    },
                    Err(error) => tracing::error!("Tattoy task join error: {error:?}"),
                }
            }

            Ok(())
        })
    })
}
