//! The manager of all the fancy Tattoy eye-candy code

use std::sync::Arc;

use color_eyre::eyre::Result;

use tokio::sync::mpsc;

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
    pub fn new(state: &Arc<SharedState>, requested_tattoys: Vec<String>) -> Result<Self> {
        let mut tattoys: Vec<Box<dyn Tattoyer + Send>> = vec![];
        for tattoy in requested_tattoys {
            let instance = create_instance(&tattoy, state)?;
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
        tattoy_frame_tx: mpsc::Sender<FrameUpdate>,
    ) -> std::thread::JoinHandle<Result<(), color_eyre::eyre::Error>> {
        let protocol_rx = protocol_tx.subscribe();
        std::thread::spawn(move || -> Result<()> {
            let closure = || {
                let mut tattoys = Self::new(&state, enabled_tattoys)?;
                tattoys.run(&tattoy_frame_tx, protocol_rx)?;
                Ok(())
            };

            if let Err(error) = closure() {
                crate::run::broadcast_protocol_end(&protocol_tx);
                return Err(error);
            }

            Ok(())
        })
    }

    /// Run the tattoy(s)
    pub fn run(
        &mut self,
        tattoy_output: &mpsc::Sender<FrameUpdate>,
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
                match message {
                    Protocol::End => {
                        break;
                    }
                    Protocol::Resize { width, height } => {
                        for tattoy in &mut self.tattoys {
                            tattoy.set_tty_size(width, height);
                        }
                    }
                };
            }

            for tattoy in &mut self.tattoys {
                let surface = tattoy.tick()?;
                tattoy_output.try_send(FrameUpdate::TattoySurface(surface))?;
            }

            if let Some(i) = target_frame_rate_micro.checked_sub(frame_time.elapsed()) {
                std::thread::sleep(i);
            }
        }

        tracing::debug!("Tattoys loop finished");
        Ok(())
    }
}
