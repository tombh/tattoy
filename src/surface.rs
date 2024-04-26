//! Add pixels and or characters to a tattoy surface

use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;

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

    /// Add a pixel ("▄" or "▀") to a tattoy surface
    pub fn add_pixel(&mut self, x: usize, y: usize, red: f32, green: f32, blue: f32) {
        self.surface.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(x),
            y: TermwizPosition::Absolute(y.div_ceil(2)),
        });

        self.surface.add_changes(vec![TermwizChange::Attribute(
            termwiz::cell::AttributeChange::Foreground(
                termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                    termwiz::color::SrgbaTuple(red, green, blue, 1.0),
                ),
            ),
        )]);

        #[allow(clippy::non_ascii_literal)]
        let block = match y % 2 {
            0 => "▄", // even
            _ => "▀", // odd
        };

        self.surface.add_change(block);
    }
}
