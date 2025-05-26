//! A proxy to a shadow terminal that runs a version of the user's terminal entirely in memory. So
//! that we can use it as a base for compositing tattoys.

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::shared_state::SharedState;

/// A proxy for signals and data to and from an in-memory shadow terminal.
pub(crate) struct Proxy {
    /// Shared app state
    pub state: Arc<SharedState>,
    /// A headless Wezterm terminal running entirely in memory.
    pub shadow_terminal: shadow_terminal::active_terminal::ActiveTerminal,
    /// A channel for output updates from the shadow terminal screen.
    surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    /// The Tattoy protocol
    pub tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    /// A hash map linking palette indexes to true colour values.
    palette: crate::palette::converter::Palette,
}

impl Proxy {
    /// Instantiate.
    ///
    /// The `surfaces_tx` channel sends `termwiz::surface::Surface` updates representing the current
    /// content of the shadow terminal.
    async fn new(
        state: Arc<SharedState>,
        shadow_terminal: shadow_terminal::active_terminal::ActiveTerminal,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<Self> {
        Ok(Self {
            state: Arc::clone(&state),
            shadow_terminal,
            surfaces_tx,
            tattoy_protocol,
            palette: crate::config::main::Config::load_palette(state).await?,
        })
    }

    /// Start the main loop listening for signals and data to and from the shadow terminal.
    pub async fn start(
        state: Arc<SharedState>,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        config: shadow_terminal::shadow_terminal::Config,
    ) -> Result<()> {
        let shadow_terminal = shadow_terminal::active_terminal::ActiveTerminal::start(config);

        let mut tattoy_protocol_rx = tattoy_protocol.subscribe();
        let mut proxy =
            Self::new(state, shadow_terminal, surfaces_tx, tattoy_protocol.clone()).await?;
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            // TODO: Be 100% sure that everything is cancellation safe here.
            tokio::select! {
                Ok(message) = tattoy_protocol_rx.recv() => {
                    proxy.handle_tattoy_protocol_message(message).await?;
                }
                result = &mut proxy.shadow_terminal.task_handle => {
                    if let Err(error) = result {
                        tracing::error!("{error:?}");
                    }
                    break;
                }
                Some(output) = proxy.shadow_terminal.surface_output_rx.recv() => {
                    proxy.handle_output(output).await?;
                }
            }
        }

        Ok(())
    }

    /// Handle output from the Shadow Terminal.
    async fn handle_output(&self, mut output: shadow_terminal::output::Output) -> Result<()> {
        tracing::trace!("Received output from Shadow Terminal: {output:?}");
        self.palette.convert_cells_to_true_colour(&mut output);

        match output.clone() {
            shadow_terminal::output::Output::Diff(diff) => {
                self.reconstruct_surface_from_diff(diff).await?;
            }
            shadow_terminal::output::Output::Complete(complete_surface) => match complete_surface {
                shadow_terminal::output::CompleteSurface::Scrollback(scrollback) => {
                    let mut shadow_tty_scrollback = self.state.shadow_tty_scrollback.write().await;
                    *shadow_tty_scrollback = scrollback;
                }
                shadow_terminal::output::CompleteSurface::Screen(screen) => {
                    let mut shadow_tty_screen = self.state.shadow_tty_screen.write().await;
                    *shadow_tty_screen = screen.surface;
                    drop(shadow_tty_screen);

                    let is_alternate_screen =
                        matches!(screen.mode, shadow_terminal::output::ScreenMode::Alternate);
                    self.state
                        .set_is_alternate_screen(is_alternate_screen)
                        .await;
                }
                _ => (),
            },
            _ => (),
        }

        self.send_pty_surface_notifications(output).await;

        let mut pty_sequence = self.state.pty_sequence.write().await;
        *pty_sequence += 1;
        drop(pty_sequence);

        Ok(())
    }

    /// Reconstruct full surfaces from diffs.
    async fn reconstruct_surface_from_diff(
        &self,
        diff: shadow_terminal::output::SurfaceDiff,
    ) -> Result<()> {
        match diff {
            shadow_terminal::output::SurfaceDiff::Scrollback(scrollback_diff) => {
                self.handle_scrolling_output(&scrollback_diff).await?;
                self.reconstruct_scrollback_diff(scrollback_diff).await?;
            }
            shadow_terminal::output::SurfaceDiff::Screen(screen_diff) => {
                let is_alternate_screen = matches!(
                    screen_diff.mode,
                    shadow_terminal::output::ScreenMode::Alternate
                );
                self.state
                    .set_is_alternate_screen(is_alternate_screen)
                    .await;
                self.reconstruct_screen_diff(screen_diff).await;
            }
            _ => (),
        }

        Ok(())
    }

    /// Reconstruct the scrollback surface from a diff of changes.
    async fn reconstruct_scrollback_diff(
        &self,
        diff: shadow_terminal::output::ScrollbackDiff,
    ) -> Result<()> {
        let mut shadow_tty_scrollback = self.state.shadow_tty_scrollback.write().await;

        if shadow_tty_scrollback.surface.dimensions() != diff.size {
            shadow_tty_scrollback
                .surface
                .resize(diff.size.0, diff.height);
        }

        shadow_tty_scrollback.surface.add_changes(diff.changes);
        shadow_tty_scrollback.position = diff.position;

        drop(shadow_tty_scrollback);

        Ok(())
    }

    /// Handle new scrolling state from Shadow Terminal.
    async fn handle_scrolling_output(
        &self,
        diff: &shadow_terminal::output::ScrollbackDiff,
    ) -> Result<()> {
        let current_scrolling_state = self.state.get_is_scrolling().await;
        let new_is_scrolling_state = diff.position != 0;
        if current_scrolling_state != new_is_scrolling_state {
            self.state.set_is_scrolling(new_is_scrolling_state).await;
            self.tattoy_protocol
                .send(crate::run::Protocol::CursorVisibility(
                    !new_is_scrolling_state,
                ))?;
        }

        Ok(())
    }

    /// Reconstruct the alternate screen surface from a diff of changes.
    async fn reconstruct_screen_diff(&self, diff: shadow_terminal::output::ScreenDiff) {
        let mut shadow_tty_screen = self.state.shadow_tty_screen.write().await;
        let size = self.state.get_tty_size().await;

        if shadow_tty_screen.dimensions() != diff.size {
            shadow_tty_screen.resize(size.width.into(), size.height.into());
        }
        shadow_tty_screen.add_changes(diff.changes);
    }

    /// Handle protocol messages from Tattoy.
    async fn handle_tattoy_protocol_message(&self, message: crate::run::Protocol) -> Result<()> {
        #[expect(clippy::wildcard_enum_match_arm, reason = "It's our internal protocol")]
        match message {
            crate::run::Protocol::End => {
                self.shadow_terminal.kill()?;
            }
            crate::run::Protocol::Resize { width, height } => {
                self.shadow_terminal.resize(width, height)?;
            }
            crate::run::Protocol::Input(input) => {
                self.handle_input(&input).await?;
            }
            _ => (),
        }

        Ok(())
    }

    // TODO:
    // It is a bit odd that we send 3 notifications about new PTY output. I'm sure the
    // receiver of the `Protocol::Output` message could do everything that the receiver of the
    // `FrameUpdate::PTYSurface` message does. But we also use the `FrameUpdate::PTYSurface`
    // channel for tattoy frame updates, so let's keep the `FrameUpdate` channel for now.
    //
    /// Notify the Tattoy renderer and individial tattous that there's new frame data from the
    /// shadow terminal.
    async fn send_pty_surface_notifications(&self, output: shadow_terminal::output::Output) {
        let frame_update_result = self
            .surfaces_tx
            .send(crate::run::FrameUpdate::PTYSurface)
            .await;
        if let Err(err) = frame_update_result {
            tracing::error!("Couldn't notify frame update channel about new PTY surface: {err:?}");
        }

        let output_update_result = self
            .tattoy_protocol
            .send(crate::run::Protocol::Output(output));
        if let Err(err) = output_update_result {
            tracing::error!("Couldn't notify protocol channel about new PTY output: {err:?}");
        }
    }
}
