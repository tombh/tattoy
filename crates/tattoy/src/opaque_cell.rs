//! This is hopefully a central place to handle all the colour blending needs when compositing the
//! various tattoy frames and PTY screen.

use termwiz::cell::Cell;

/// This is the default colour for when an opaque cell is over a "blank" cell.
///
/// In Tattoy, a blank cell is any cell that has the default terminal colour. Most terminals use a
/// dark theme, so let's say that, when alpha blending, the default colour is pure black.
/// TODO: support light theme terminals.
pub const DEFAULT_COLOUR: termwiz::color::SrgbaTuple =
    termwiz::color::SrgbaTuple(0.0, 0.0, 0.0, 1.0);

/// Whether we're acting on a foreground or background attribute.
enum Kind {
    /// A foreground attribute.
    Foreground,
    /// A background attribute.
    Background,
}

/// Just a convenience wrapper around Termwiz's `[Cell]`. Compositing cells is a bit tricky, so
/// having a dedicated module hopefully makes things a bit simpler.
pub(crate) struct OpaqueCell<'cell> {
    /// The normal underlying cell
    cell: &'cell mut Cell,
    /// The true colour value to use when the cell doesn't have a colour.
    default_colour: termwiz::color::SrgbaTuple,
}

impl<'cell> OpaqueCell<'cell> {
    /// Instantiate
    pub const fn new(
        cell: &'cell mut Cell,
        maybe_default_bg_colour: Option<termwiz::color::SrgbaTuple>,
    ) -> Self {
        let default_bg_colour = match maybe_default_bg_colour {
            Some(colour) => colour,
            None => DEFAULT_COLOUR,
        };

        Self {
            cell,
            default_colour: default_bg_colour,
        }
    }

    /// Convert a simple colour into a cell attribute, because to change the colour of a cell, you must do
    /// so with a wrapping colour atttribute.
    pub const fn make_true_colour_attribute(
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

    /// Blend this cell's foreground colour with a new colour.
    fn blend(&mut self, kind: &Kind, incoming_colour: termwiz::color::SrgbaTuple) {
        let this_colour = match kind {
            Kind::Foreground => self.cell.attrs().foreground(),
            Kind::Background => self.cell.attrs().background(),
        };

        let maybe_colour = match Self::extract_colour(this_colour) {
            Some(colour) => colour,
            None => self.default_colour,
        };
        let blended_colour = maybe_colour.interpolate(incoming_colour, incoming_colour.3.into());
        let attribute = Self::make_true_colour_attribute(blended_colour);

        match kind {
            Kind::Foreground => self.cell.attrs_mut().set_foreground(attribute),
            Kind::Background => self.cell.attrs_mut().set_background(attribute),
        };
    }

    /// Blend the cell's colours with the cell above.
    pub fn blend_all(&mut self, cell_above: &Cell) {
        let character_above = cell_above.str();
        let character_above_is_empty = character_above.is_empty() || character_above == " ";
        if character_above_is_empty {
            if let Some(colour) = Self::extract_colour(cell_above.attrs().background()) {
                self.blend(&Kind::Background, colour);
                self.blend(&Kind::Foreground, colour);
            }
        } else {
            if let Some(colour) = Self::extract_colour(self.cell.attrs().foreground()) {
                let is_blending_2_pixels = self.cell.str() == "▀" && cell_above.str() == "▀";
                if !is_blending_2_pixels {
                    // Blend this cell's own foreground colour with this cell's own background colour.
                    self.blend(&Kind::Background, colour);
                }
            }
            if let Some(colour) = Self::extract_colour(cell_above.attrs().foreground()) {
                self.blend(&Kind::Foreground, colour);
            }
            if let Some(colour) = Self::extract_colour(cell_above.attrs().background()) {
                self.blend(&Kind::Background, colour);
            }
        }
    }
}
