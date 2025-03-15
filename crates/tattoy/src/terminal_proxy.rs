//! A proxy to a shadow terminal that runs a version of the user's terminal entirely in memory. So
//! that we can use it as a base for compositing tattoys.

use std::sync::Arc;

use color_eyre::eyre::{ContextCompat as _, Result};

use crate::shared_state::SharedState;

/// A proxy for signals and data to and from an in-memory shadow terminal.
pub(crate) struct TerminalProxy {
    /// Shared app state
    pub state: Arc<SharedState>,
    /// A headless Wezterm terminal running entirely in memory.
    shadow_terminal: shadow_terminal::active_terminal::ActiveTerminal,
    /// A channel for output updates from the shadow terminal screen.
    surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    /// The Tattoy protocol
    tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    /// A hash map linking palette indexes to true colour values.
    palette: Option<crate::palette::converter::Palette>,
}

impl TerminalProxy {
    /// Instantiate.
    ///
    /// The `surfaces_tx` channel sends `termwiz::surface::Surface` updates representing the current
    /// content of the shadow terminal.
    async fn new(
        state: &Arc<SharedState>,
        shadow_terminal: shadow_terminal::active_terminal::ActiveTerminal,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<Self> {
        Ok(Self {
            state: Arc::clone(state),
            shadow_terminal,
            surfaces_tx,
            tattoy_protocol,
            palette: crate::config::Config::load_palette(state).await?,
        })
    }

    /// Start the main loop listening for signals and data to and from the shadow terminal.
    pub async fn start(
        state: &Arc<SharedState>,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        config: shadow_terminal::shadow_terminal::Config,
    ) -> Result<()> {
        let shadow_terminal = shadow_terminal::active_terminal::ActiveTerminal::start(config);

        let mut tattoy_protocol_rx = tattoy_protocol.subscribe();
        let mut proxy = Self::new(state, shadow_terminal, surfaces_tx, tattoy_protocol).await?;

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
        self.convert_cells_to_true_colour(&mut output);

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

    /// Convert palette indexes into their true colour values.
    fn convert_cells_to_true_colour(&self, output: &mut shadow_terminal::output::Output) {
        let Some(palette) = &self.palette else {
            return;
        };

        match output {
            shadow_terminal::output::Output::Diff(surface_diff) => {
                let changes = match surface_diff {
                    shadow_terminal::output::SurfaceDiff::Scrollback(diff) => &mut diff.changes,
                    shadow_terminal::output::SurfaceDiff::Screen(diff) => &mut diff.changes,
                    _ => {
                        tracing::error!(
                            "Unrecognised surface diff when converting cells to true colour"
                        );
                        &mut Vec::new()
                    }
                };

                for change in changes {
                    if let termwiz::surface::change::Change::AllAttributes(attributes) = change {
                        palette.cell_attributes_to_true_colour(attributes);
                    }
                }
            }
            shadow_terminal::output::Output::Complete(complete_surface) => {
                let cells = match complete_surface {
                    shadow_terminal::output::CompleteSurface::Scrollback(scrollback) => {
                        scrollback.surface.screen_cells()
                    }
                    shadow_terminal::output::CompleteSurface::Screen(screen) => {
                        screen.surface.screen_cells()
                    }
                    _ => {
                        tracing::error!("Unhandled surface from Shadow Terminal");
                        Vec::new()
                    }
                };
                for line in cells {
                    for cell in line {
                        palette.cell_attributes_to_true_colour(cell.attrs_mut());
                    }
                }
            }
            _ => (),
        }
    }

    /// Handle input from the end user.
    async fn handle_input(&self, input: &crate::input::ParsedInput) -> Result<()> {
        if self.is_tattoy_input_event(&input.event).await {
            tracing::trace!("Tattoy input event: {:?}", input.event);
            self.handle_scrolling_input(&input.event).await?;
        } else if !self.state.get_is_scrolling().await {
            tracing::trace!(
                "Terminal proxy received input bytes: {}",
                String::from_utf8_lossy(&input.bytes)
            );
            for chunk in input.bytes.chunks(128) {
                let mut buffer: crate::input::BytesFromSTDIN = [0; 128];
                for (i, chunk_byte) in chunk.iter().enumerate() {
                    let buffer_byte = buffer.get_mut(i).context("Couldn't get byte from buffer")?;
                    *buffer_byte = *chunk_byte;
                }
                tracing::trace!(
                    "Proxying input to shadow terminal from Tattoy: {}",
                    String::from_utf8_lossy(&buffer)
                );
                let result = self.shadow_terminal.send_input(buffer).await;
                if let Err(error) = result {
                    tracing::error!("Couldn't forward STDIN bytes on PTY input channel: {error:?}");
                }
            }
        } else {
            if let termwiz::input::InputEvent::Key(key_event) = &input.event {
                if key_event.key == termwiz::input::KeyCode::Escape {
                    self.shadow_terminal.scroll_cancel()?;
                }
            }

            tracing::trace!(
                "Not forwarding input because user is scrolling: {:?}",
                input.event
            );
        }

        Ok(())
    }

    /// Is the input event specific to Tattoy (eg toggling tattoys etc)? If it is, then the raw
    /// input bytes shouldn't be passed on to the underlying PTY.
    async fn is_tattoy_input_event(&self, event: &termwiz::input::InputEvent) -> bool {
        match event {
            termwiz::input::InputEvent::Key(_key_event) => {}
            termwiz::input::InputEvent::Mouse(_mouse_event) => {
                if !self.state.get_is_alternate_screen().await {
                    return true;
                }
            }
            termwiz::input::InputEvent::PixelMouse(_pixel_mouse_event) => (),
            termwiz::input::InputEvent::Resized {
                cols: _cols,
                rows: _rows,
            } => (),
            termwiz::input::InputEvent::Paste(_) | termwiz::input::InputEvent::Wake => (),
        }

        false
    }

    /// Because Tattoy is a wrapper around a headless, in-memory terminal, it can't rely on the
    /// user's actual terminal (Kitty, Alacritty, iTerm, etc) to do scrolling. So Tattoy forwards
    /// scrolling events to the shadow terminal and renders its own scrollbars etc.
    async fn handle_scrolling_input(&self, event: &termwiz::input::InputEvent) -> Result<()> {
        if self.state.get_is_alternate_screen().await {
            return Ok(());
        }

        if let termwiz::input::InputEvent::Mouse(mouse) = event {
            let scroll_up = termwiz::input::MouseButtons::VERT_WHEEL
                | termwiz::input::MouseButtons::WHEEL_POSITIVE;

            if mouse.mouse_buttons == scroll_up {
                self.shadow_terminal.scroll_up()?;
            }
            if mouse.mouse_buttons == termwiz::input::MouseButtons::VERT_WHEEL {
                self.shadow_terminal.scroll_down()?;
            }
        }

        Ok(())
    }
}
