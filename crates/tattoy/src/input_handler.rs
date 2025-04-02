//! Handle parsed input events

use color_eyre::eyre::{ContextCompat as _, Result};

impl crate::terminal_proxy::TerminalProxy {
    /// Handle input from the end user.
    pub async fn handle_input(&self, input: &crate::raw_input::ParsedInput) -> Result<()> {
        if self.is_tattoy_input_event(&input.event).await {
            tracing::trace!("Tattoy input event: {:?}", input.event);
            self.handle_scrolling_input(&input.event).await?;
        } else if !self.state.get_is_scrolling().await {
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
