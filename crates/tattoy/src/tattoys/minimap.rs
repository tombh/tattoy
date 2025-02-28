//! Display a scrollbar when scrolling

use std::sync::Arc;

use color_eyre::eyre::{ContextCompat as _, Result};

use crate::shared_state::SharedState;

use super::index::Tattoyer;

/// The max width of the minimap (in units of terminal columns). The image resizer may choose a
/// slimmer minimap in order to maintain the original ratio.
const MAX_WIDTH: u16 = 10;

/// `RandomWalker`
#[derive(Default)]
pub struct Minimap {
    /// Global Tattoy state
    state: Arc<SharedState>,
    /// TTY width
    width: u16,
    /// TTY height
    height: u16,
    /// Our own copy of the scrollback. Saves taking costly read locks.
    scrollback: shadow_terminal::output::CompleteScrollback,
    /// Whether the user is scolling, primarily used to detect when the shared scrolling state changes.
    is_scrolling: bool,
    /// Keep track of the underlying PTY sequence counter
    pty_sequence: usize,
}

#[async_trait::async_trait]
impl Tattoyer for Minimap {
    fn id() -> String {
        "minimap".into()
    }

    /// Instantiate
    async fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = state.get_tty_size().await;
        let width = tty_size.width;
        let height = tty_size.height;

        Ok(Self {
            state,
            width,
            height,
            ..Default::default()
        })
    }

    fn handle_pty_output(&mut self, output: shadow_terminal::output::Output) {
        match output {
            shadow_terminal::output::Output::Diff(
                shadow_terminal::output::SurfaceDiff::Scrollback(scrollback_diff),
            ) => {
                self.scrollback
                    .surface
                    .resize(scrollback_diff.size.0, scrollback_diff.height);
                self.scrollback.surface.add_changes(scrollback_diff.changes);
            }
            shadow_terminal::output::Output::Complete(
                shadow_terminal::output::CompleteSurface::Scrollback(scrollback),
            ) => self.scrollback = scrollback,

            shadow_terminal::output::Output::Diff(_)
            | shadow_terminal::output::Output::Complete(_)
            | _ => (),
        }
    }

    fn set_tty_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Tick the render
    async fn tick(&mut self) -> Result<Option<crate::surface::Surface>> {
        let pty_sequence = self.state.pty_sequence.read().await;

        if *pty_sequence == self.pty_sequence {
            return Ok(None);
        }

        self.pty_sequence = *pty_sequence;
        drop(pty_sequence);

        let scrollback_width = self.scrollback.surface.dimensions().0;
        let scrollback_height = self.scrollback.surface.dimensions().1;
        let current_is_scrolling = self.state.get_is_scrolling().await;

        if scrollback_width == 0 || scrollback_height == 0 {
            return Ok(None);
        }

        let mut surface =
            crate::surface::Surface::new(Self::id(), self.width.into(), self.height.into(), 100);

        if current_is_scrolling != self.is_scrolling {
            self.is_scrolling = current_is_scrolling;
            if !self.is_scrolling {
                return Ok(Some(surface));
            }
        }

        if !self.is_scrolling {
            return Ok(None);
        }

        let mut scrollback_image = image::DynamicImage::new_rgba8(
            scrollback_width.try_into()?,
            (scrollback_height * 2).try_into()?,
        );
        let image_buffer = scrollback_image
            .as_mut_rgba8()
            .context("Couldn't get mutable reference to scrollback image")?;

        for (x, y, pixel) in image_buffer.enumerate_pixels_mut() {
            let cells = self.scrollback.surface.screen_cells();
            let line = cells
                .get(usize::try_from(y)?.div_euclid(2))
                .context("Couldn't get surface line")?;
            let cell = &line
                .get(usize::try_from(x)?)
                .context("Couldn't surface cell from line")?;

            let cell_colour = if cell.str() == " " {
                crate::opaque_cell::OpaqueCell::extract_colour(cell.attrs().background()).map_or(
                    crate::opaque_cell::DEFAULT_BACKGROUND_TRUE_COLOUR,
                    |background_colour| background_colour,
                )
            } else {
                let maybe_colour =
                    crate::opaque_cell::OpaqueCell::extract_colour(cell.attrs().foreground());

                if let Some(colour) = maybe_colour {
                    colour
                } else {
                    tracing::warn!("Using Minimap without a parsed palette");
                    return Ok(None);
                }
            };

            *pixel = image::Rgba(cell_colour.to_srgb_u8().into());
        }

        let minimap_as_rgb255 = scrollback_image.resize(
            MAX_WIDTH.into(),
            (self.height * 2).into(),
            image::imageops::Lanczos3,
        );

        let minimap = minimap_as_rgb255.to_rgba32f();
        let dimensions = minimap.dimensions();
        for y_pixel in 0..dimensions.1 {
            for x_pixel in 0..dimensions.0 {
                let pixel = minimap
                    .get_pixel_checked(x_pixel, y_pixel)
                    .context(format!("Couldn't get pixel: {x_pixel}x{y_pixel}"))?
                    .0;

                let x_cell: usize = (u32::from(self.width) - dimensions.0 + x_pixel).try_into()?;
                let y_cell = (u32::from(self.height * 2) - dimensions.1 + y_pixel).try_into()?;
                surface.add_pixel(x_cell, y_cell, pixel.into())?;
            }
        }

        Ok(Some(surface))
    }
}

impl Minimap {}
