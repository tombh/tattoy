//! The manager of all the fancy Tattoy eye-candy code

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::run::FrameUpdate;

/// Start the main loader thread
pub(crate) fn start_tattoys(
    enabled_tattoys: Vec<String>,
    output: tokio::sync::mpsc::Sender<FrameUpdate>,
    state: Arc<crate::shared_state::SharedState>,
) -> std::thread::JoinHandle<Result<(), color_eyre::eyre::Error>> {
    let tokio_runtime = tokio::runtime::Handle::current();
    std::thread::spawn(move || -> Result<()> {
        tokio_runtime.block_on(async {
            crate::run::wait_for_system(state.protocol_tx.subscribe(), "renderer").await;

            let palette = crate::config::main::Config::load_palette(Arc::clone(&state)).await?;
            let mut tattoy_futures = tokio::task::JoinSet::new();

            if enabled_tattoys.contains(&"notifications".to_owned())
                || state.config.read().await.notifications.enabled
            {
                tracing::info!("Starting 'notifications' tattoy...");
                tattoy_futures.spawn(crate::tattoys::notifications::main::Notifications::start(
                    output.clone(),
                    Arc::clone(&state),
                ));
                crate::run::wait_for_system(state.protocol_tx.subscribe(), "notifications").await;
            }

            tracing::info!("Starting 'scrollbar' tattoy...");
            tattoy_futures.spawn(crate::tattoys::scrollbar::Scrollbar::start(
                output.clone(),
                Arc::clone(&state),
            ));

            if enabled_tattoys.contains(&"random_walker".to_owned()) {
                tracing::info!("Starting 'random_walker' tattoy...");
                tattoy_futures.spawn(crate::tattoys::random_walker::RandomWalker::start(
                    output.clone(),
                    Arc::clone(&state),
                ));
            }

            if enabled_tattoys.contains(&"minimap".to_owned())
                || state.config.read().await.minimap.enabled
            {
                tracing::info!("Starting 'minimap' tattoy...");
                tattoy_futures.spawn(crate::tattoys::minimap::Minimap::start(
                    output.clone(),
                    Arc::clone(&state),
                ));
            }

            if enabled_tattoys.contains(&"shaders".to_owned())
                || state.config.read().await.shader.enabled
            {
                tracing::info!("Starting 'shaders' tattoy...");
                tattoy_futures.spawn(crate::tattoys::shaders::main::Shaders::start(
                    output.clone(),
                    Arc::clone(&state),
                ));
            }

            if enabled_tattoys.contains(&"bg_command".to_owned())
                || state.config.read().await.bg_command.enabled
            {
                tracing::info!("Starting 'bg_command' tattoy...");
                tattoy_futures.spawn(crate::tattoys::bg_command::BGCommand::start(
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
                    output.clone(),
                ));
            }

            while let Some(completes) = tattoy_futures.join_next().await {
                match completes {
                    Ok(result) => match result {
                        Ok(()) => tracing::debug!("A tattoy succesfully exited"),
                        Err(error) => {
                            let title = "Unhandled tattoy error";
                            let message = format!("{title}: {error:?}");
                            tracing::warn!(message);
                            state
                                .send_notification(
                                    title,
                                    crate::tattoys::notifications::message::Level::Error,
                                    Some(error.root_cause().to_string()),
                                    true,
                                )
                                .await;
                        }
                    },
                    Err(error) => tracing::error!("Tattoy task join error: {error:?}"),
                }
            }

            Ok(())
        })
    })
}
