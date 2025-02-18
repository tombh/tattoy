//! Display a scrollbar when scrolling

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::shared_state::SharedState;

use super::index::Tattoyer;

/// `RandomWalker`
#[derive(Default)]
pub struct Scrollbar {
    /// Global Tattoy state
    state: Arc<SharedState>,
    /// TTY width
    width: u16,
    /// TTY height
    height: u16,
    /// Whether the user is scolling, primarily used to detect when the shared scrolling state changes.
    is_scrolling: bool,
}

#[async_trait::async_trait]
impl Tattoyer for Scrollbar {
    fn id() -> String {
        "scrollbar".into()
    }

    /// Instatiate
    async fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = state.get_tty_size().await;
        let width = tty_size.width;
        let height = tty_size.height;

        Ok(Self {
            state,
            width,
            height,
            is_scrolling: false,
        })
    }

    fn set_tty_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Tick the render
    async fn tick(&mut self) -> Result<Option<crate::surface::Surface>> {
        let current_is_scrolling = self.state.get_is_scrolling().await;
        if !current_is_scrolling {
            // Cleanup
            if self.is_scrolling && !current_is_scrolling {
                let surface = crate::surface::Surface::new(
                    Self::id(),
                    self.width.into(),
                    self.height.into(),
                    100,
                );
                self.is_scrolling = false;
                return Ok(Some(surface));
            }

            // Nothing to do here
            return Ok(None);
        }
        self.is_scrolling = true;

        let scrollback = self.state.shadow_tty_scrollback.read().await;
        let scrollback_position = scrollback.position;
        let scrollback_height = scrollback.surface.dimensions().1;
        drop(scrollback);

        let (start, end) = self.get_start_end(scrollback_position, scrollback_height);
        if start > end {
            tracing::error!("Bad scrollbar dimensions: {start:?} {end:?}");
            return Ok(None);
        }

        let mut surface =
            crate::surface::Surface::new(Self::id(), self.width.into(), self.height.into(), 100);

        for y in start..end {
            surface.add_text(
                (self.width - 1).into(),
                y,
                " ".into(),
                Some((1.0, 1.0, 1.0, 0.5)),
                None,
            );
        }
        Ok(Some(surface))
    }
}

impl Scrollbar {
    /// Get the start and end y coordinates of the scrollbar
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_lossless,
        clippy::cast_possible_truncation,
        reason = "It's just a scrollbar"
    )]
    fn get_start_end(
        &self,
        scrollback_position: usize,
        scrollback_height: usize,
    ) -> (usize, usize) {
        let top_of_terminal_position =
            scrollback_height - scrollback_position - self.height as usize;
        let top_of_terminal_fraction = top_of_terminal_position as f32 / scrollback_height as f32;
        let mut scrollbar_start = (top_of_terminal_fraction * self.height as f32) as usize;

        let bottom_of_terminal_position = scrollback_height - scrollback_position;
        let bottom_of_terminal_fraction =
            bottom_of_terminal_position as f32 / scrollback_height as f32;
        let mut scrollbar_end = (bottom_of_terminal_fraction * self.height as f32) as usize;

        scrollbar_start = scrollbar_start.clamp(0, (self.height - 1).into());
        scrollbar_end = scrollbar_end.clamp(0, (self.height - 1).into());

        (scrollbar_start, scrollbar_end)
    }
}
