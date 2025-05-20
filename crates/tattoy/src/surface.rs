//! Add pixels and or characters to a tattoy surface

use color_eyre::eyre::bail;
use color_eyre::eyre::ContextCompat as _;
use color_eyre::eyre::Result;
use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;

/// An RGB colour
pub(crate) type Colour = (f32, f32, f32, f32);

/// A default pure white.
pub const WHITE: Colour = (1.0, 1.0, 1.0, 1.0);

/// A default pure black.
pub const BLACK: Colour = (0.0, 0.0, 0.0, 1.0);

/// A default pure red.
pub const RED: Colour = (1.0, 0.0, 0.0, 1.0);

/// `Surface`
#[derive(Clone)]
pub(crate) struct Surface {
    /// The unique ID of the tattoy to which this surface belongs.
    pub id: String,
    /// The terminal's width
    pub width: usize,
    /// The terminal's height
    pub height: usize,
    /// The order in which the tattoy should be rendered. The PTY is always layer 0, so any
    /// layering value below 0 will make the tattoy appear below the user's terminal content,
    /// and any value above 0 will make it appear above the user's terminal content.
    pub layer: i16,
    /// The transparency of the surface.
    pub opacity: f32,
    /// A surface of terminal cells
    pub surface: termwiz::surface::Surface,
}

impl Surface {
    /// Create a Compositor/Tattoy
    #[must_use]
    pub fn new(id: String, width: usize, height: usize, layer: i16, opacity: f32) -> Self {
        Self {
            id,
            width,
            height,
            layer,
            opacity,
            surface: termwiz::surface::Surface::new(width, height),
        }
    }

    /// Add a pixel ("▀", "▄") to a tattoy surface.
    ///
    /// The rule is that we default to rendering any pair of colours using the upper half block.
    /// Therefore that the upper "pixel" is rendered with the cell's foreground and the lower
    /// "pixel" is rendered with the cell's background colour.
    ///
    /// However, there is one edge case that requires this to be inverted: when an empty cell
    /// needs a pixel in the lower half. It is impossible to do this with an upper half block
    /// *whilst retaining the ANSI-coded default background colour*.
    pub fn add_pixel(&mut self, x: usize, y: usize, colour: Colour) -> Result<()> {
        let (col, row) = self.coords_to_tty(x, y)?;
        self.surface.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(col),
            y: TermwizPosition::Absolute(row),
        });

        let cell = self.get_cell_at(col, row)?;
        let is_empty_upper = cell.str() != "▀";
        let is_upper_half = y.rem_euclid(2) == 0;
        let is_lower_half = !is_upper_half;
        let is_adding_to_bottom_of_empty_upper = is_empty_upper && is_lower_half;

        let mut fg_colour = if is_upper_half {
            Self::make_fg_colour(colour)
        } else {
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(
                cell.attrs().foreground(),
            ))
        };

        #[expect(
            clippy::useless_let_if_seq,
            reason = "I think the verbosity is useful here"
        )]
        let mut bg_colour = if is_upper_half {
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                cell.attrs().background(),
            ))
        } else {
            Self::make_bg_colour(colour)
        };

        if is_adding_to_bottom_of_empty_upper {
            fg_colour = Self::make_fg_colour(colour);
            bg_colour = TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                cell.attrs().background(),
            ));
        }

        // This is when we add a pixel to a cell that only has a lower-half colour.
        let is_converting_lower_to_full = is_upper_half && cell.str() == "▄";
        if is_converting_lower_to_full {
            fg_colour = Self::make_fg_colour(colour);
            bg_colour = TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                cell.attrs().foreground(),
            ));
        }

        self.surface.add_changes(vec![fg_colour, bg_colour]);
        if is_adding_to_bottom_of_empty_upper {
            self.surface.add_change("▄");
        } else {
            self.surface.add_change("▀");
        }

        Ok(())
    }

    /// Overlay text at a given coord with the given colours.
    pub fn add_text(
        &mut self,
        x: usize,
        y: usize,
        text: String,
        maybe_background_colour: Option<Colour>,
        maybe_foreground_colour: Option<Colour>,
    ) {
        let bg_colour = maybe_background_colour
            .map_or_else(Self::make_default_bg_colour, |colour| {
                Self::make_bg_colour(colour)
            });

        let fg_colour = maybe_foreground_colour
            .map_or_else(|| Self::make_fg_colour(WHITE), Self::make_fg_colour);

        self.surface.add_changes(vec![
            TermwizChange::CursorPosition {
                x: TermwizPosition::Absolute(x),
                y: TermwizPosition::Absolute(y),
            },
            bg_colour,
            fg_colour,
        ]);
        self.surface.add_change(text);
    }

    /// Make a Termwiz colour attribute
    #[must_use]
    pub const fn make_colour_attribute(colour: Colour) -> termwiz::color::ColorAttribute {
        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(termwiz::color::SrgbaTuple(
            colour.0, colour.1, colour.2, colour.3,
        ))
    }

    /// Make a Termwiz background colour
    #[must_use]
    pub const fn make_bg_colour(colour: Colour) -> TermwizChange {
        let colour_attribute = Self::make_colour_attribute(colour);
        TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(colour_attribute))
    }

    /// Make the default Termwiz background colour. This is the non-colour, usually black, that a
    /// terminal displays when nothing else has been set. It's often what's used on a GUI terminal
    /// to make it's background transparent.
    #[must_use]
    pub const fn make_default_bg_colour() -> TermwizChange {
        let colour_attribute = termwiz::color::ColorAttribute::Default;
        TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(colour_attribute))
    }

    /// Make a Termwiz background colour
    #[must_use]
    pub const fn make_fg_colour(colour: Colour) -> TermwizChange {
        let colour_attribute = Self::make_colour_attribute(colour);
        TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(colour_attribute))
    }

    /// Safely convert pixel coordinates to TTY col/row
    fn coords_to_tty(&self, x: usize, y: usize) -> Result<(usize, usize)> {
        let col = x;
        let row = y.div_euclid(2);
        if col >= self.width {
            bail!("Tried to add pixel to column: {col}")
        }
        if row >= self.height {
            bail!("Tried to add pixel to row: {row}")
        }
        Ok((col, row))
    }

    /// Get thell at the given column and row.
    fn get_cell_at(&mut self, col: usize, row: usize) -> Result<termwiz::cell::Cell> {
        let cells = self.surface.screen_cells();
        let cell = cells
            .get(row)
            .context("No cell row")?
            .get(col)
            .context("No cell column")?;
        // TODO: avoid this clone!
        Ok(cell.clone())
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::shadow_unrelated,
    reason = "Tests aren't so strict"
)]
mod test {
    use super::*;

    const GREY: Colour = (0.5, 0.5, 0.5, 1.0);

    #[test]
    fn add_new_pixels() {
        let mut surface = Surface::new("test".into(), 2, 2, -1, 1.0);

        let cell = &surface.surface.screen_cells()[0][0];
        assert_eq!(cell.str(), " ");
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::Default
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );

        surface.add_pixel(0, 0, WHITE).unwrap();
        let cell = &surface.surface.screen_cells()[0][0];

        assert_eq!(cell.str(), "▀");
        assert_eq!(
            cell.attrs().foreground(),
            Surface::make_colour_attribute(WHITE)
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );

        surface.add_pixel(1, 0, WHITE).unwrap();
        let cell = &surface.surface.screen_cells()[0][1];
        assert_eq!(cell.str(), "▀");

        surface.add_pixel(1, 2, WHITE).unwrap();
        let cell = &surface.surface.screen_cells()[1][1];
        assert_eq!(cell.str(), "▀");

        surface.add_pixel(1, 3, WHITE).unwrap();
        let cell = &surface.surface.screen_cells()[1][1];
        assert_eq!(cell.str(), "▀");

        let result = surface.add_pixel(1, 4, WHITE).unwrap_err();
        assert_eq!(
            format!("{}", result.root_cause()),
            "Tried to add pixel to row: 2"
        );
    }

    #[test]
    fn add_pixel_at_bottom_of_empty_cell() {
        let mut surface = Surface::new("test".into(), 1, 1, -1, 1.0);

        surface.add_pixel(0, 1, WHITE).unwrap();
        let cell = &surface.surface.screen_cells()[0][0];
        assert_eq!(cell.str(), "▄");
        assert_eq!(
            cell.attrs().foreground(),
            Surface::make_colour_attribute(WHITE)
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );
    }

    #[test]
    fn overwrite_pixel_at_bottom_of_empty_cell() {
        let mut surface = Surface::new("test".into(), 1, 1, -1, 1.0);

        surface.add_pixel(0, 1, RED).unwrap();
        surface.add_pixel(0, 1, WHITE).unwrap();
        let cell = &surface.surface.screen_cells()[0][0];
        assert_eq!(cell.str(), "▄");
        assert_eq!(
            cell.attrs().foreground(),
            Surface::make_colour_attribute(WHITE)
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );
    }

    #[test]
    fn convert_cell_from_bottom_to_full() {
        let mut surface = Surface::new("test".into(), 1, 1, -1, 1.0);

        surface.add_pixel(0, 1, WHITE).unwrap();
        surface.add_pixel(0, 0, RED).unwrap();
        let cell = &surface.surface.screen_cells()[0][0];
        assert_eq!(cell.str(), "▀");
        assert_eq!(
            cell.attrs().foreground(),
            Surface::make_colour_attribute(RED)
        );
        assert_eq!(
            cell.attrs().background(),
            Surface::make_colour_attribute(WHITE)
        );
    }

    #[test]
    fn add_pixels_on_or_near_other_pixels() {
        let mut surface = Surface::new("test".into(), 2, 1, -1, 1.0);
        surface.add_pixel(0, 0, WHITE).unwrap();

        let fg = Surface::make_colour_attribute(WHITE);
        let bg = Surface::make_colour_attribute(GREY);

        surface.add_pixel(0, 1, GREY).unwrap();
        let cells = surface.surface.screen_cells();
        let first_cell = cells[0][0].clone();
        assert_eq!(first_cell.str(), "▀");
        assert_eq!(first_cell.attrs().foreground(), fg);
        assert_eq!(first_cell.attrs().background(), bg);

        let fg = Surface::make_colour_attribute(RED);
        surface.add_pixel(0, 0, RED).unwrap();
        let cells = surface.surface.screen_cells();
        let first_cell = cells[0][0].clone();
        assert_eq!(first_cell.attrs().foreground(), fg);
        assert_eq!(first_cell.attrs().background(), bg);
    }
}
