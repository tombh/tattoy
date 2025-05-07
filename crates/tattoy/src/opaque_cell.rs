//! This is hopefully a central place to handle all the colour blending needs when compositing the
//! various tattoy frames and PTY screen.

use palette::{
    color_difference::Wcag21RelativeContrast as _, DarkenAssign as _, IntoColor as _,
    LightenAssign as _,
};
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
    /// The opacity of the cell above.
    cell_above_opacity: f32,
}

impl<'cell> OpaqueCell<'cell> {
    /// Instantiate
    pub const fn new(
        cell: &'cell mut Cell,
        maybe_default_bg_colour: Option<termwiz::color::SrgbaTuple>,
        cell_above_opacity: f32,
    ) -> Self {
        let default_bg_colour = match maybe_default_bg_colour {
            Some(colour) => colour,
            None => DEFAULT_COLOUR,
        };

        Self {
            cell,
            default_colour: default_bg_colour,
            cell_above_opacity,
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
        let this_colour_attribute = match kind {
            Kind::Foreground => self.cell.attrs().foreground(),
            Kind::Background => self.cell.attrs().background(),
        };

        let colour = match Self::extract_colour(this_colour_attribute) {
            Some(raw_colour) => raw_colour,
            None => self.default_colour,
        };

        let blended_colour = colour.interpolate(
            incoming_colour,
            f64::from(incoming_colour.3 * self.cell_above_opacity),
        );
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

    /// Ensure that the colour difference between the background and foreground is sufficient
    /// enough to be readable.
    pub fn ensure_readable_contrast(
        &mut self,
        target_contrast: f32,
        apply_to_readable_text_only: bool,
    ) {
        // TODO:
        // * Check that the colour is from the terminal palette.
        if apply_to_readable_text_only && !self.cell.str().chars().all(char::is_alphanumeric) {
            return;
        }

        if self.cell.str() == "▀" || self.cell.str() == "▄" {
            return;
        }

        // I think these default colours are only assigned for the very first composited layer?
        let fg_raw =
            Self::extract_colour(self.cell.attrs().foreground()).unwrap_or(self.default_colour);
        let bg_raw =
            Self::extract_colour(self.cell.attrs().background()).unwrap_or(self.default_colour);

        let fg_original = palette::rgb::Rgba::new(fg_raw.0, fg_raw.1, fg_raw.2, fg_raw.3);
        let bg = palette::rgb::Rgb::new(bg_raw.0, bg_raw.1, bg_raw.2);

        let contrast = fg_original.relative_contrast(bg);
        if contrast >= target_contrast {
            return;
        }

        let maybe_maxed_out_lightness =
            self.find_and_set_min_contrast(fg_original, bg, target_contrast, true);
        if let Some(lightest) = maybe_maxed_out_lightness {
            let maybe_maxed_out_darkness =
                self.find_and_set_min_contrast(fg_original, bg, target_contrast, false);
            if let Some(darkest) = maybe_maxed_out_darkness {
                let lightest_contrast = bg.relative_contrast(lightest.into_color());
                let darkest_contrast = bg.relative_contrast(darkest.into_color());
                if lightest_contrast >= darkest_contrast {
                    self.set_colour_from_rgba(lightest);
                    tracing::trace!(
                        "Contrast for {} not reached, setting to max contrast +{lightest_contrast}",
                        self.cell.str()
                    );
                } else {
                    self.set_colour_from_rgba(darkest);
                    tracing::trace!(
                        "Contrast for {} not reached, setting to max contrast -{darkest_contrast}",
                        self.cell.str()
                    );
                }
            }
        }
    }

    /// Find the foreground colour that achieves the target contrast.
    fn find_and_set_min_contrast(
        &mut self,
        mut fg: palette::rgb::Rgba,
        bg: palette::rgb::Rgb,
        target_contrast: f32,
        is_lighten: bool,
    ) -> Option<palette::Srgba> {
        let step = 0.005;

        #[expect(
            clippy::as_conversions,
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "
                I don't want to install a whole crate just to get fallible float to integer
                conversions 🙄
            "
        )]
        let max_attempts = (1.0 / step) as u16;

        for _ in 0..max_attempts {
            if is_lighten {
                fg.lighten_fixed_assign(step);
            } else {
                fg.darken_fixed_assign(step);
            }

            let contrast = fg.relative_contrast(bg);
            if contrast >= target_contrast {
                self.set_colour_from_rgba(fg);
                return None;
            }
        }

        Some(fg)
    }

    /// Sets the cell's colour from a `palette` crate colour.
    fn set_colour_from_rgba(&mut self, colour: palette::rgb::Rgba) {
        let color_attribute = Self::make_true_colour_attribute(termwiz::color::SrgbaTuple(
            colour.red,
            colour.green,
            colour.blue,
            colour.alpha,
        ));
        self.cell.attrs_mut().set_foreground(color_attribute);
    }
}
