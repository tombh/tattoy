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

            let maybe_palette = crate::config::main::Config::load_palette(&state).await?;
            let Some(palette) = maybe_palette.as_ref() else {
                color_eyre::eyre::bail!("A palette is needed for running plugins");
            };

            for plugin_config in &state.config.read().await.plugins {
                if let Some(is_enabled) = plugin_config.enabled {
                    if !is_enabled {
                        continue;
                    }
                }

                tattoy_futures.spawn(crate::tattoys::plugins::Plugin::start(
                    plugin_config.clone(),
                    palette.clone(),
                    input.clone(),
                    output.clone(),
                ));
            }

            while let Some(starting) = tattoy_futures.join_next().await {
                match starting {
                    Ok(result) => match result {
                        Ok(()) => tracing::error!("A tattoy exited without error"),
                        Err(error) => tracing::error!("A tattoy exited: {error:?}"),
                    },
                    Err(spawn_error) => tracing::error!("Error spawning a tattoy: {spawn_error:?}"),
                }
            }

            Ok(())
        })
    })
}
