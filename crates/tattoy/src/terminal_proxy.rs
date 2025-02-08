//! A proxy to a shadow terminal that runs a version of the user's terminal entirely in memory. So
//! that we can use it as a base for compositing tattoys.

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::shared_state::SharedState;

/// A proxy for signals and data to and from an in-memory shadow terminal.
pub(crate) struct TerminalProxy {
    /// Shared app state
    pub state: Arc<SharedState>,
    /// A channel for output updates from the shadow terminal screen.
    surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
}

impl TerminalProxy {
    /// Instantiate.
    /// The `surfaces_tx` channel sends `termwiz::surface::Surface` updates representing the current
    /// content of the shadow terminal.
    const fn new(
        state: Arc<SharedState>,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Self {
        Self { state, surfaces_tx }
    }

    /// Start the main loop listening for signals and data to and from the shadow terminal.
    pub async fn start(
        state: Arc<SharedState>,
        mut input_rx: tokio::sync::mpsc::Receiver<crate::input::BytesFromSTDIN>,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        config: shadow_terminal::shadow_terminal::Config,
    ) -> Result<()> {
        tracing::debug!("Starting shadow terminal...");
        let mut tattoy_protocol_rx = tattoy_protocol_tx.subscribe();
        let proxy = Self::new(state, surfaces_tx);
        let mut shadow_terminal = shadow_terminal::active_terminal::ActiveTerminal::start(config);
        tracing::debug!("Shadow terminal started.");

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                Some(input) = input_rx.recv() => {
                    shadow_terminal.send_input(input).await?;
                }
                Ok(message) = tattoy_protocol_rx.recv() => {
                    Self::handle_protocol_message(&message, &shadow_terminal)?;
                }
                result = &mut shadow_terminal.task_handle => {
                    if let Err(error) = result {
                        tracing::error!("{error:?}");
                    }
                    break;
                }
                Some(surface) = shadow_terminal.surface_output_rx.recv() => {
                    tracing::trace!("Received surface from Shadow Terminal");
                    proxy.update_state_surface(surface)?;
                    proxy.send_pty_surface_notification().await;
                }
            }
        }

        Ok(())
    }

    /// Handle protocol messages from Tattoy.
    fn handle_protocol_message(
        message: &crate::run::Protocol,
        terminal: &shadow_terminal::active_terminal::ActiveTerminal,
    ) -> Result<()> {
        match message {
            crate::run::Protocol::End => terminal.kill()?,
            crate::run::Protocol::Resize { width, height } => terminal.resize(*width, *height)?,
        };

        Ok(())
    }

    /// Notify the Tattoy renderer that there's a new frame of data from the shadow terminal.
    async fn send_pty_surface_notification(&self) {
        let result = self
            .surfaces_tx
            .send(crate::run::FrameUpdate::PTYSurface)
            .await;
        if let Err(err) = result {
            tracing::error!("Couldn't notify frame update channel about new PTY surface: {err:?}");
        }
    }

    /// Send the current PTY surface to the shared state.
    /// Needs to be in its own non-async function like this because of the error:
    ///   'future created by async block is not `Send`'
    fn update_state_surface(&self, surface: termwiz::surface::Surface) -> Result<()> {
        let mut shadow_tty = self
            .state
            .shadow_tty
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        *shadow_tty = surface;
        drop(shadow_tty);
        Ok(())
    }
}
