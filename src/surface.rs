//! Add pixels and or characters to a tattoy surface

use color_eyre::eyre::bail;
use color_eyre::eyre::Result;
use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;

/// An RGB colour
type Colour = (f32, f32, f32);

/// "Compositor" or "Tattoys"?
#[allow(clippy::exhaustive_structs)]
pub struct Surface {
    /// The terminal's width
    pub width: usize,
    /// The terminal's height
    pub height: usize,
    /// A surface of terminal cells
    pub surface: termwiz::surface::Surface,
}

impl Surface {
    /// Create a Compositor/Tattoy
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            surface: termwiz::surface::Surface::new(width, height),
        }
    }

    /// Add a pixel ("▀", "▄") to a tattoy surface
    #[allow(clippy::non_ascii_literal)]
    pub fn add_pixel(&mut self, x: usize, y: usize, colour: Colour) -> Result<()> {
        let (col, row) = self.coords_to_tty(x, y)?;
        self.surface.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(col),
            y: TermwizPosition::Absolute(row),
        });
        let block = match y.rem_euclid(2) {
            0 => "▀", // even
            _ => "▄", // odd
        };
        let bg_default = Self::make_default_bg_colour();
        let bg = Self::make_bg_colour(colour);
        let fg = Self::make_fg_colour(colour);

        let existing = self.get_existing_cell_string(col, row);
        let mut content = existing.clone();
        match (existing.as_str(), block) {
            ("▄", "▀") | ("▀", "▄") => {
                self.surface.add_change(bg);
            }
            ("▄", "▄") | ("▀", "▀") => {
                self.surface.add_change(fg);
            }
            _ => {
                self.surface.add_changes(vec![bg_default, fg]);
                block.clone_into(&mut content);
            }
        }

        self.surface.add_change(content);

        Ok(())
    }

    /// Overlay white text at a given coord
    pub fn add_text(&mut self, x: usize, y: usize, text: String) {
        self.surface.add_changes(vec![
            TermwizChange::CursorPosition {
                x: TermwizPosition::Absolute(x),
                y: TermwizPosition::Absolute(y),
            },
            Self::make_default_bg_colour(),
            Self::make_fg_colour((1.0, 1.0, 1.0)),
        ]);
        self.surface.add_change(text);
    }

    /// Make a Termwiz colour attribute
    #[must_use]
    pub const fn make_colour_attribute(colour: Colour) -> termwiz::color::ColorAttribute {
        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(termwiz::color::SrgbaTuple(
            colour.0, colour.1, colour.2, 1.0,
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
    #[allow(clippy::arithmetic_side_effects)]
    fn coords_to_tty(&self, x: usize, y: usize) -> Result<(usize, usize)> {
        let col = x;
        let row = (y + 1).div_ceil(2) - 1;
        if col + 1 > self.width {
            bail!("Tried to add particle to column: {col}")
        }
        if row > self.height {
            bail!("Tried to add particle to row: {row}")
        }
        Ok((col, row))
    }

    /// Get the string contents of the existing TTY cell where we want to put the new pixel
    fn get_existing_cell_string(&mut self, col: usize, row: usize) -> String {
        let cells = self.surface.screen_cells();
        #[allow(clippy::indexing_slicing)]
        let cell = &cells[row][col];
        cell.str().to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::non_ascii_literal)]
mod tests {
    use super::*;

    const WHITE: Colour = (1.0, 1.0, 1.0);
    const GREY: Colour = (0.5, 0.5, 0.5);

    fn add_pixel_on_fresh_surface(x: usize, y: usize) -> Vec<Vec<wezterm_term::Cell>> {
        let mut cells_copy: Vec<Vec<wezterm_term::Cell>> = Vec::default();
        let mut surface = Surface::new(2, 1);
        surface.add_pixel(x, y, WHITE).unwrap();
        let cells = surface.surface.screen_cells();
        for (i, line) in cells.iter().enumerate() {
            cells_copy.push(Vec::default());
            for cell in line.iter() {
                cells_copy[i].push(cell.clone());
            }
        }
        cells_copy
    }

    #[test]
    fn add_new_pixels() {
        let cells1 = add_pixel_on_fresh_surface(0, 0);
        assert_eq!(cells1[0][0].str(), "▀");

        let cells2 = add_pixel_on_fresh_surface(0, 1);
        assert_eq!(cells2[0][0].str(), "▄");

        let cells3 = add_pixel_on_fresh_surface(1, 0);
        assert_eq!(cells3[0][1].str(), "▀");

        let cells4 = add_pixel_on_fresh_surface(1, 1);
        assert_eq!(cells4[0][1].str(), "▄");
    }

    #[test]
    fn add_pixels_on_or_near_other_pixels() {
        let mut surface = Surface::new(2, 1);
        surface.add_pixel(0, 0, WHITE).unwrap();

        let bg = Surface::make_colour_attribute(GREY);
        let fg = Surface::make_colour_attribute(WHITE);

        surface.add_pixel(0, 1, GREY).unwrap();
        let cells = surface.surface.screen_cells();
        let first_cell = cells[0][0].clone();
        assert_eq!(first_cell.str(), "▀");
        assert_eq!(first_cell.attrs().background(), bg);
        assert_eq!(first_cell.attrs().foreground(), fg);
    }
}
