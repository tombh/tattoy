//! The manager of all the fancy Tattoy eye-candy code

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::run::{FrameUpdate, Protocol};
use crate::shared_state::SharedState;
use crate::tattoys::index::{create_instance, Tattoyer};

/// The number of microseonds in a second
const ONE_MICROSECOND: u64 = 1_000_000;

/// Rename to "Compositor" or "Tattoys"?
pub(crate) struct Loader {
    /// All the enabled tattoys that will be run
    tattoys: Vec<Box<dyn Tattoyer + Send>>,
}

impl Loader {
    /// Create a Compositor/Tattoy
    pub async fn new(state: &Arc<SharedState>, mut requested_tattoys: Vec<String>) -> Result<Self> {
        // The scrollbar should always be enabled
        requested_tattoys.push("scrollbar".to_owned());

        let mut tattoys: Vec<Box<dyn Tattoyer + Send>> = vec![];
        for tattoy in requested_tattoys {
            let instance = create_instance(&tattoy, state).await?;
            tattoys.push(instance);
        }
        if tattoys.is_empty() {
            return Err(color_eyre::eyre::eyre!("No tattoys to run"));
        }
        Ok(Self { tattoys })
    }

    /// Start the main loader thread
    pub fn start(
        enabled_tattoys: Vec<String>,
        state: Arc<SharedState>,
        protocol_tx: tokio::sync::broadcast::Sender<Protocol>,
        tattoy_frame_tx: tokio::sync::mpsc::Sender<FrameUpdate>,
    ) -> std::thread::JoinHandle<Result<(), color_eyre::eyre::Error>> {
        let tokio_runtime = tokio::runtime::Handle::current();
        std::thread::spawn(move || -> Result<()> {
            tokio_runtime.block_on(async {
                if let Err(error) = Self::start_without_concurrency(
                    enabled_tattoys,
                    state,
                    protocol_tx.clone(),
                    tattoy_frame_tx,
                )
                .await
                {
                    crate::run::broadcast_protocol_end(&protocol_tx);
                    return Err(error);
                }

                Ok(())
            })
        })
    }

    /// Just a convenience wrapper to catch all the magic `?` errors in one place.
    async fn start_without_concurrency(
        enabled_tattoys: Vec<String>,
        state: Arc<SharedState>,
        protocol_tx: tokio::sync::broadcast::Sender<Protocol>,
        tattoy_frame_tx: tokio::sync::mpsc::Sender<FrameUpdate>,
    ) -> Result<()> {
        let protocol_rx = protocol_tx.subscribe();
        let mut tattoys = Self::new(&state, enabled_tattoys).await?;
        tattoys.run(&tattoy_frame_tx, protocol_rx).await?;
        Ok(())
    }

    /// Run the tattoy(s)
    pub async fn run(
        &mut self,
        tattoy_output: &tokio::sync::mpsc::Sender<FrameUpdate>,
        mut protocol: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        let target_frame_rate = 30;

        let target = ONE_MICROSECOND.wrapping_div(target_frame_rate);
        let target_frame_rate_micro = std::time::Duration::from_micros(target);

        tracing::debug!("Starting tattoys loop...");
        loop {
            let frame_time = std::time::Instant::now();

            // TODO: Use `tokio::select!`
            if let Ok(message) = protocol.try_recv() {
                #[expect(clippy::wildcard_enum_match_arm, reason = "It's our internal protocol")]
                match message {
                    Protocol::End => {
                        tracing::trace!("Tattoys loader loop received message: {message:?}");
                        break;
                    }
                    Protocol::Resize { width, height } => {
                        tracing::trace!("Tattoys loader loop received message: {message:?}");
                        for tattoy in &mut self.tattoys {
                            tattoy.set_tty_size(width, height);
                        }
                    }
                    Protocol::Output(output) => {
                        tracing::trace!(
                            "Tattoys loader loop received message for new output from PTY"
                        );
                        for tattoy in &mut self.tattoys {
                            tattoy.handle_pty_output(output.clone());
                        }
                    }
                    _ => (),
                }
            }

            for tattoy in &mut self.tattoys {
                let maybe_surface = tattoy.tick().await?;
                if let Some(surface) = maybe_surface {
                    let result = tattoy_output.try_send(FrameUpdate::TattoySurface(surface));
                    if let Err(error) = result {
                        tracing::error!("Sending output for tattoy {error:?}");
                        break;
                    }
                }
            }

            if let Some(i) = target_frame_rate_micro.checked_sub(frame_time.elapsed()) {
                std::thread::sleep(i);
            }
        }

        tracing::debug!("Tattoys loop finished");
        Ok(())
    }
}
