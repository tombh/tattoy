//! Handle parsed input events

use color_eyre::eyre::{ContextCompat as _, Result};

impl crate::terminal_proxy::proxy::Proxy {
    /// Handle input from the end user.
    pub async fn handle_input(&self, input: &crate::raw_input::ParsedInput) -> Result<()> {
        if self.handle_tattoy_input_event(&input.event).await? {
            tracing::trace!(
                "Not forwarding input because Tattoy received a known input event: {:?}",
                input.event
            );
            return Ok(());
        }

        self.forward_input_to_pty(input).await
    }

    /// Forward raw input bytes to the underlying PTY.
    async fn forward_input_to_pty(&self, input: &crate::raw_input::ParsedInput) -> Result<()> {
        tracing::trace!(
            "Terminal proxy received input bytes: {}",
            String::from_utf8_lossy(&input.bytes)
        );
        for chunk in input.bytes.chunks(128) {
            let mut buffer: crate::raw_input::BytesFromSTDIN = [0; 128];
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

        Ok(())
    }

    /// Is the input event specific to Tattoy (eg toggling tattoys etc)?
    async fn handle_tattoy_input_event(&self, event: &termwiz::input::InputEvent) -> Result<bool> {
        let is_input_event = match event {
            termwiz::input::InputEvent::Key(key_event) => {
                self.handle_tattoy_key_event(key_event).await?
            }
            termwiz::input::InputEvent::Mouse(mouse_event) => {
                self.handle_mouse_scrolling_input(mouse_event).await?
            }
            termwiz::input::InputEvent::PixelMouse(_pixel_mouse_event) => false,
            termwiz::input::InputEvent::Resized {
                cols: _cols,
                rows: _rows,
            } => false,
            termwiz::input::InputEvent::Paste(_) | termwiz::input::InputEvent::Wake => false,
        };

        Ok(is_input_event || self.state.get_is_scrolling().await)
    }

    /// Handle a key event that we have a keybinding for.
    async fn handle_tattoy_key_event(&self, key_event: &termwiz::input::KeyEvent) -> Result<bool> {
        // TODO: may turn out to be better to cache this.
        let keybindings = self.state.keybindings.read().await;
        let maybe_match = keybindings
            .iter()
            .find_map(|(action, binding)| (binding == key_event).then_some(action.clone()));
        let Some(trigger) = maybe_match else {
            return Ok(false);
        };
        drop(keybindings);

        match trigger {
            crate::config::input::KeybindingAction::ToggleTattoy => {
                let existing = *self.state.is_rendering_enabled.read().await;
                tracing::debug!("Toggling Tattoy renderer to: {}", !existing);
                *self.state.is_rendering_enabled.write().await = !existing;
                Ok(true)
            }
            crate::config::input::KeybindingAction::ToggleScrolling => {
                if self.state.get_is_scrolling().await {
                    self.shadow_terminal.scroll_cancel()?;
                } else {
                    self.shadow_terminal.scroll_up()?;
                }
                Ok(true)
            }
            crate::config::input::KeybindingAction::ScrollUp => {
                if self.state.get_is_scrolling().await {
                    self.shadow_terminal.scroll_up()?;
                    return Ok(true);
                }
                Ok(false)
            }
            crate::config::input::KeybindingAction::ScrollDown => {
                if self.state.get_is_scrolling().await {
                    self.shadow_terminal.scroll_down()?;
                    return Ok(true);
                }
                Ok(false)
            }
            crate::config::input::KeybindingAction::ScrollExit => {
                if self.state.get_is_scrolling().await {
                    self.shadow_terminal.scroll_cancel()?;
                    return Ok(true);
                }
                Ok(false)
            }
        }
    }

    /// Because Tattoy is a wrapper around a headless, in-memory terminal, it can't rely on the
    /// user's actual terminal (Kitty, Alacritty, iTerm, etc) to do scrolling. So Tattoy forwards
    /// scrolling events to the shadow terminal and renders its own scrollbars etc.
    async fn handle_mouse_scrolling_input(
        &self,
        event: &termwiz::input::MouseEvent,
    ) -> Result<bool> {
        if self.state.get_is_alternate_screen().await {
            return Ok(false);
        }

        let scroll_up =
            termwiz::input::MouseButtons::VERT_WHEEL | termwiz::input::MouseButtons::WHEEL_POSITIVE;
        if event.mouse_buttons == scroll_up {
            self.shadow_terminal.scroll_up()?;
        }

        let scroll_down = termwiz::input::MouseButtons::VERT_WHEEL;
        if self.state.get_is_scrolling().await && event.mouse_buttons == scroll_down {
            self.shadow_terminal.scroll_down()?;
        }

        Ok(true)
    }
}
