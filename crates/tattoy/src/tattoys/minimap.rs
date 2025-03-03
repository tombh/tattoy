//! Display a minimap of the scrollback history.

use color_eyre::eyre::{ContextCompat as _, Result};

use super::tattoyer::Tattoyer;

/// The max width of the minimap (in units of terminal columns). The image resizer may choose a
/// slimmer minimap in order to maintain the original ratio.
const MAX_WIDTH: u16 = 10;

/// `Minimap`
pub struct Minimap {
    /// The base Tattoy struct
    tattoy: Tattoyer,
    /// If the PTY output has changed.
    output_changed: bool,
    /// The current state of any UI transitions; fading, sliding, etc.
    animation: bool,
}

impl Minimap {
    /// Instantiate
    fn new(output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>) -> Self {
        let tattoy = Tattoyer::new("minimap".to_owned(), 90, output_channel);
        Self {
            tattoy,
            output_changed: true,
            animation: false,
        }
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Result<()> {
        let mut minimap = Self::new(output);
        let mut protocol = protocol_tx.subscribe();

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                () = minimap.tattoy.sleep_until_next_frame_tick(), if minimap.needs_rendering() => {
                    minimap.render().await?;
                },
                result = protocol.recv() => {
                    if matches!(result, Ok(crate::run::Protocol::End)) {
                        break;
                    }
                    minimap.handle_protocol_message(result)?;
                }
            }
        }

        Ok(())
    }

    /// Handle messages from the main Tattoy app.
    fn handle_protocol_message(
        &mut self,
        result: std::result::Result<crate::run::Protocol, tokio::sync::broadcast::error::RecvError>,
    ) -> Result<()> {
        match result {
            Ok(message) => {
                self.check_if_scrollback_has_changed(&message);
                self.tattoy.handle_common_protocol_messages(message)?;
            }
            Err(error) => tracing::error!("Receiving protocol message: {error:?}"),
        }

        Ok(())
    }

    /// Whether the minimap needs re-rendering.
    const fn needs_rendering(&self) -> bool {
        self.output_changed || self.animation
    }

    /// Check if the scrollback output has changed such that we need to trigger a re-render.
    fn check_if_scrollback_has_changed(&mut self, message: &crate::run::Protocol) {
        if self.tattoy.is_scrolling_end() || Tattoyer::is_scrollback_output_changed(message) {
            self.output_changed = true;
        }
    }

    /// Tick the render
    async fn render(&mut self) -> Result<()> {
        if self.tattoy.is_scrolling_end() {
            self.tattoy.send_blank_output().await?;
            return Ok(());
        }

        if !self.tattoy.is_ready() || !self.tattoy.is_scrolling() {
            return Ok(());
        }

        self.tattoy.initialise_surface();

        let scrollback_width = self.tattoy.scrollback.surface.dimensions().0;
        let scrollback_height = self.tattoy.scrollback.surface.dimensions().1;

        let mut scrollback_image = image::DynamicImage::new_rgba8(
            scrollback_width.try_into()?,
            (scrollback_height * 2).try_into()?,
        );
        let image_buffer = scrollback_image
            .as_mut_rgba8()
            .context("Couldn't get mutable reference to scrollback image")?;

        for (x, y, pixel) in image_buffer.enumerate_pixels_mut() {
            let cells = self.tattoy.scrollback.surface.screen_cells();
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
                    return Ok(());
                }
            };

            *pixel = image::Rgba(cell_colour.to_srgb_u8().into());
        }

        let minimap_as_rgb255 = scrollback_image.resize(
            MAX_WIDTH.into(),
            (self.tattoy.height * 2).into(),
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

                let x_cell: usize =
                    (u32::from(self.tattoy.width) - dimensions.0 + x_pixel).try_into()?;
                let y_cell =
                    (u32::from(self.tattoy.height * 2) - dimensions.1 + y_pixel).try_into()?;
                self.tattoy
                    .surface
                    .add_pixel(x_cell, y_cell, pixel.into())?;
            }
        }

        self.tattoy.send_output().await?;
        self.output_changed = false;

        Ok(())
    }
}
