//! A proxy to a shadow terminal that runs a version of the user's terminal entirely in memory. So
//! that we can use it as a base for compositing tattoys.

use std::sync::Arc;

use color_eyre::eyre::Result;

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
}

impl TerminalProxy {
    /// Instantiate.
    /// The `surfaces_tx` channel sends `termwiz::surface::Surface` updates representing the current
    /// content of the shadow terminal.
    const fn new(
        state: Arc<SharedState>,
        shadow_terminal: shadow_terminal::active_terminal::ActiveTerminal,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Self {
        Self {
            state,
            shadow_terminal,
            surfaces_tx,
            tattoy_protocol,
        }
    }

    /// Start the main loop listening for signals and data to and from the shadow terminal.
    pub async fn start(
        state: Arc<SharedState>,
        surfaces_tx: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        config: shadow_terminal::shadow_terminal::Config,
    ) -> Result<()> {
        let shadow_terminal = shadow_terminal::active_terminal::ActiveTerminal::start(config);

        let mut shadow_terminal_events = shadow_terminal.control_tx.subscribe();
        let mut tattoy_protocol_rx = tattoy_protocol.subscribe();
        let mut proxy = Self::new(state, shadow_terminal, surfaces_tx, tattoy_protocol);

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
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
                Some(surface) = proxy.shadow_terminal.surface_output_rx.recv() => {
                    tracing::trace!("Received surface from Shadow Terminal");
                    proxy.update_shared_state_with_new_surface(surface).await?;
                    proxy.send_pty_surface_notification().await;
                }
                Ok(event) = shadow_terminal_events.recv() => {
                    proxy.handle_shadow_terminal_event(&event).await;
                }
            }
        }

        Ok(())
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

    /// Update the shared state with a new surface from the shadow terminal. Could the scrollback
    /// contents or the currently visible screen.
    async fn update_shared_state_with_new_surface(
        &self,
        surface: shadow_terminal::shadow_terminal::Surface,
    ) -> Result<()> {
        match surface {
            shadow_terminal::shadow_terminal::Surface::Scrollback(scrollback) => {
                // TODO: could most of this live in its own function?
                let mut shadow_tty_scrollback = self.state.shadow_tty_scrollback.write().await;
                *shadow_tty_scrollback = scrollback;

                let current_scrolling_state = self.state.get_is_scrolling().await;
                let new_scrolling_state = shadow_tty_scrollback.position != 0;
                drop(shadow_tty_scrollback);

                if current_scrolling_state != new_scrolling_state {
                    self.state.set_is_scrolling(new_scrolling_state).await;
                    self.tattoy_protocol
                        .send(crate::run::Protocol::CursorVisibility(!new_scrolling_state))?;
                }
            }
            shadow_terminal::shadow_terminal::Surface::Screen(screen) => {
                let mut shadow_tty = self.state.shadow_tty_screen.write().await;
                *shadow_tty = screen;
                drop(shadow_tty);
            }
            _ => (),
        }

        Ok(())
    }

    /// Handle signals from the Wezterm shadow terminal.
    async fn handle_shadow_terminal_event(&self, event: &shadow_terminal::Protocol) {
        tracing::debug!("Shadow Terminal event: {event:?}");
        if let shadow_terminal::Protocol::IsAlternateScreen(state) = event {
            self.state.set_is_alternate_screen(*state).await;
        }
    }

    /// Handle input from the end user.
    async fn handle_input(&self, input: &crate::input::ParsedInput) -> Result<()> {
        if self.is_tattoy_input_event(&input.event).await {
            tracing::trace!("Tattoy input event: {:?}", input.event);
            self.handle_scrolling(&input.event).await?;
        } else if !self.state.get_is_scrolling().await {
            let result = self.shadow_terminal.send_input(input.bytes).await;
            if let Err(error) = result {
                tracing::error!("Couldn't forward STDIN bytes on PTY input channel: {error:?}");
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
    async fn handle_scrolling(&self, event: &termwiz::input::InputEvent) -> Result<()> {
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
