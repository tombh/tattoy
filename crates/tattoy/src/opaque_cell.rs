//! This is hopefully a central place to handle all the colour blending needs when compositing the
//! various tattoy frames and PTY screen.

use termwiz::cell::Cell;

/// This is the default colour for when an opaque cell is over a "blank" cell. In Tattoy, a blank cell
/// is any cell that has the default terminal colour. Most terminals use a dark theme, so let's say
/// that, in terms of blending, the default colour is pure black.
///
/// TODO: support light theme terminals.
const DEFAULT_BACKGROUND_TRUE_COLOUR: termwiz::color::SrgbaTuple =
    termwiz::color::SrgbaTuple(0.0, 0.0, 0.0, 1.0);

/// Just a convenience wrapper around Termwiz's `[Cell]`. Compositing cells is a bit tricky, so
/// just having a dedicated module hopefully makes things simpler.
pub(crate) struct OpaqueCell<'cell>(pub &'cell mut Cell);

impl OpaqueCell<'_> {
    /// Convert a simple colour into a cell attribute, because to change the colour of a cell, you must do
    /// so with a wrapping colour atttribute.
    const fn make_true_colour_attribute(
        mut colour: termwiz::color::SrgbaTuple,
    ) -> termwiz::color::ColorAttribute {
        // There's some curious behaviour from `termwiz::BufferedTerminal`. When rendering a colour
        // to the user's actual terminal, it seems to just completely ignore any colour that has a
        // alpha value below 0.0. I may be missing something?
        colour.3 = 1.0;
        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(colour)
    }

    /// Get the colour of a cell from its colour attribute.
    pub const fn extract_colour(
        colour_attribute: termwiz::color::ColorAttribute,
    ) -> Option<termwiz::color::SrgbaTuple> {
        match colour_attribute {
            termwiz::color::ColorAttribute::TrueColorWithPaletteFallback(srgba_tuple, _)
            | termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(srgba_tuple) => {
                Some(srgba_tuple)
            }
            termwiz::color::ColorAttribute::PaletteIndex(_)
            | termwiz::color::ColorAttribute::Default => None,
        }
    }

    /// Blend the background colours of the 2 cells together.
    pub fn blend_backgrounds(&mut self, cell_above_colour: termwiz::color::SrgbaTuple) {
        let this_background_colour = match Self::extract_colour(self.0.attrs().background()) {
            Some(colour) => colour,
            None => DEFAULT_BACKGROUND_TRUE_COLOUR,
        };

        let blended_colour =
            this_background_colour.interpolate(cell_above_colour, cell_above_colour.3.into());

        let attribute = Self::make_true_colour_attribute(blended_colour);

        self.0.attrs_mut().set_background(attribute);
    }

    /// Blend the text colour of the cell below with the background colour of the cell above.
    pub fn blend_foreground(&mut self, cell_above_colour: termwiz::color::SrgbaTuple) {
        if let Some(colour) = Self::extract_colour(self.0.attrs().foreground()) {
            let blended_colour = colour.interpolate(cell_above_colour, cell_above_colour.3.into());
            let attribute = Self::make_true_colour_attribute(blended_colour);

            self.0.attrs_mut().set_foreground(attribute);
        }
    }

    /// Blend the cell colours with the cell above.
    pub fn blend(&mut self, cell_above: &Cell) {
        let maybe_cell_above_colour = Self::extract_colour(cell_above.attrs().background());

        if let Some(cell_above_colour) = maybe_cell_above_colour {
            self.blend_backgrounds(cell_above_colour);
            self.blend_foreground(cell_above_colour);
        }
    }
}
