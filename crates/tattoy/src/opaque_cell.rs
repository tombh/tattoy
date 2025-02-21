//! This is hopefully a central place to handle all the colour blending needs when compositing the
//! various tattoy frames and PTY screen.

use termwiz::cell::Cell;

use crate::palette_parser::PaletteParser;

/// This is the default colour for when an opaque cell is over a "blank" cell. In Tattoy, a blank cell
/// is any cell that has the default terminal colour. Most terminals use a dark theme, so let's say
/// that, when alpha blending, the default colour is pure black.
///
/// TODO: support light theme terminals.
const DEFAULT_BACKGROUND_TRUE_COLOUR: termwiz::color::SrgbaTuple =
    termwiz::color::SrgbaTuple(0.0, 0.0, 0.0, 1.0);

/// This might be a big assumption, but I think the convention is that text uses this colour from
/// the palette when no other index or true colour is specified.
const DEFAULT_TEXT_PALETTE_INDEX: u8 = 15;

/// Just a convenience wrapper around Termwiz's `[Cell]`. Compositing cells is a bit tricky, so
/// having a dedicated module hopefully makes things a bit simpler.
pub(crate) struct OpaqueCell<'cell> {
    /// The normal underlying cell
    cell: &'cell mut Cell,
    /// The true colour value to use when the cell doesn't have a colour.
    default_bg_colour: termwiz::color::SrgbaTuple,
}

impl<'cell> OpaqueCell<'cell> {
    /// Instantiate
    pub const fn new(
        cell: &'cell mut Cell,
        maybe_default_bg_colour: Option<termwiz::color::SrgbaTuple>,
    ) -> Self {
        let default_bg_colour = match maybe_default_bg_colour {
            Some(colour) => colour,
            None => DEFAULT_BACKGROUND_TRUE_COLOUR,
        };

        Self {
            cell,
            default_bg_colour,
        }
    }

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
        let this_background_colour = match Self::extract_colour(self.cell.attrs().background()) {
            Some(colour) => colour,
            None => self.default_bg_colour,
        };

        let blended_colour =
            this_background_colour.interpolate(cell_above_colour, cell_above_colour.3.into());

        let attribute = Self::make_true_colour_attribute(blended_colour);

        self.cell.attrs_mut().set_background(attribute);
    }

    /// Blend the text colour of the cell _below_ with the background colour of the cell _above_.
    pub fn blend_foreground(&mut self, cell_above_colour: termwiz::color::SrgbaTuple) {
        if let Some(colour) = Self::extract_colour(self.cell.attrs().foreground()) {
            let blended_colour = colour.interpolate(cell_above_colour, cell_above_colour.3.into());
            let attribute = Self::make_true_colour_attribute(blended_colour);

            self.cell.attrs_mut().set_foreground(attribute);
        }
    }

    /// Blend the cell's colours with the cell above.
    pub fn blend(&mut self, cell_above: &Cell) {
        let maybe_cell_above_colour = Self::extract_colour(cell_above.attrs().background());

        if let Some(cell_above_colour) = maybe_cell_above_colour {
            self.blend_backgrounds(cell_above_colour);
            self.blend_foreground(cell_above_colour);
        }
    }

    /// Convert any palette index-defined cells to their true colour values.
    pub fn convert_to_true_colour(&mut self, palette: &crate::palette_parser::Palette) {
        self.convert_fg_to_true_colour(palette);
        self.convert_bg_to_true_colour(palette);
    }

    /// Convert text palette indexes to treu colour values.
    fn convert_fg_to_true_colour(&mut self, palette: &crate::palette_parser::Palette) {
        if matches!(
            self.cell.attrs().foreground(),
            termwiz::color::ColorAttribute::Default
        ) {
            let colour_attribute =
                PaletteParser::true_colour_from_index(palette, DEFAULT_TEXT_PALETTE_INDEX);
            self.cell.attrs_mut().set_foreground(colour_attribute);
            return;
        }

        let termwiz::color::ColorAttribute::PaletteIndex(index) = self.cell.attrs().foreground()
        else {
            return;
        };

        let colour_attribute = PaletteParser::true_colour_from_index(palette, index);
        self.cell.attrs_mut().set_foreground(colour_attribute);
    }

    /// Convert the background palette index to a true colour. Note that we don't handle the
    /// default colour variant because that's currently used to help with the compositing of render
    /// layers, namely knowing when to let a lower layer's content pass through to higher layers.
    /// But it might turn out to be a better idea to also make transparent cells use true colour,
    /// because they could easily be defined with a `0.0` alpha channel.
    fn convert_bg_to_true_colour(&mut self, palette: &crate::palette_parser::Palette) {
        let termwiz::color::ColorAttribute::PaletteIndex(index) = self.cell.attrs().background()
        else {
            return;
        };

        let colour_attribute = PaletteParser::true_colour_from_index(palette, index);
        self.cell.attrs_mut().set_background(colour_attribute);
    }
}
