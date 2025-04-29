//! Display a scrollbar when scrolling

use color_eyre::eyre::Result;

/// `Scrollbar`
pub(crate) struct Scrollbar {
    /// The base Tattoy struct
    tattoy: super::tattoyer::Tattoyer,
}

impl Scrollbar {
    /// Instantiate
    fn new(output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>) -> Self {
        let tattoy =
            super::tattoyer::Tattoyer::new("scrollbar".to_owned(), 100, 1.0, output_channel);
        Self { tattoy }
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Result<()> {
        let mut scrollbar = Self::new(output);
        let mut protocol = protocol_tx.subscribe();

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                result = protocol.recv() => {
                    if matches!(result, Ok(crate::run::Protocol::End)) {
                        break;
                    }
                    scrollbar.handle_protocol_message(result).await?;
                }
            }
        }

        Ok(())
    }

    /// Handle messages from the main Tattoy app.
    async fn handle_protocol_message(
        &mut self,
        result: std::result::Result<crate::run::Protocol, tokio::sync::broadcast::error::RecvError>,
    ) -> Result<()> {
        match result {
            Ok(message) => {
                self.tattoy.handle_common_protocol_messages(message)?;
                if self.tattoy.last_scroll_position != self.tattoy.scrollback.position {
                    self.render().await?;
                }
            }
            Err(error) => tracing::error!("Receiving protocol message: {error:?}"),
        }

        Ok(())
    }

    /// Tick the render
    async fn render(&mut self) -> Result<()> {
        if self.tattoy.is_scrolling_end() {
            tracing::debug!("Scrolling finished.");
            self.tattoy.send_blank_output().await?;
            return Ok(());
        }

        if !self.tattoy.is_ready() {
            tracing::debug!("Scrolling tattoy not ready.");
            return Ok(());
        }

        if !self.tattoy.is_scrolling() {
            tracing::trace!("Not rendering scrollbar because we're not scrolling yet.");
            return Ok(());
        }

        // TODO: only render on scroll position change.

        let (start, end) = self.get_start_end();
        if start > end {
            tracing::error!("Bad scrollbar dimensions: {start:?} {end:?}");
            return Ok(());
        }

        self.tattoy.initialise_surface();

        for y in start..end {
            self.tattoy.surface.add_text(
                (self.tattoy.width - 1).into(),
                y,
                " ".into(),
                Some((1.0, 1.0, 1.0, 0.5)),
                None,
            );
        }

        self.tattoy.send_output().await
    }

    /// Get the start and end y coordinates of the scrollbar
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_lossless,
        clippy::cast_possible_truncation,
        reason = "It's just a scrollbar"
    )]
    fn get_start_end(&self) -> (usize, usize) {
        let scrollback_height = self.tattoy.scrollback.surface.dimensions().1;

        let top_of_terminal_position =
            scrollback_height - self.tattoy.scrollback.position - self.tattoy.height as usize;
        let top_of_terminal_fraction = top_of_terminal_position as f32 / scrollback_height as f32;
        let mut scrollbar_start = (top_of_terminal_fraction * self.tattoy.height as f32) as usize;

        let bottom_of_terminal_position = scrollback_height - self.tattoy.scrollback.position;
        let bottom_of_terminal_fraction =
            bottom_of_terminal_position as f32 / scrollback_height as f32;
        let mut scrollbar_end = (bottom_of_terminal_fraction * self.tattoy.height as f32) as usize;

        scrollbar_start = scrollbar_start.clamp(0, (self.tattoy.height - 1).into());
        scrollbar_end = scrollbar_end.clamp(0, (self.tattoy.height - 1).into());

        (scrollbar_start, scrollbar_end)
    }
}
