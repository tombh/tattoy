//! A state machine for parsing a QRcode-like screenshot of the user's terminal colours.

use color_eyre::Result;

/// A state machine for parsing a QR Code-like grid of colours.
enum State {
    /// The first colour to look for is a pure(ish) red, which indicates the start of the colour grid.
    /// Reds are also used to indicate the beginnings of new rows of colours. However, we increment
    /// the green channel of the red by 1 for every new row. Hence the extra `u8` in this enum
    /// variant.
    LookingForRedish(u8),
    /// We then look for a pure blue just as a way to avoid false positive triggerings of the state
    /// machine.
    LookingForBlue,
    /// Once the red and blue have been found, we know that the next colour change will be for an
    /// actual colour in the palette.
    LookingForFirstColourInRow,
    /// Now we are collecting colours. There are 16 per row, hence the `u8` variance.
    CollectingRow(u8),
}

/// The state machine for parsing true colour palettes.
pub struct Machine {
    /// The state as defined by the `State` enum.
    state: State,
    /// The final palette of true colour values.
    palette: crate::palette::converter::Palette,
    /// The current colour being parsed from the screenshot.
    current_colour: xcap::image::Rgba<u8>,
    /// The current terminal palette index being parsed.
    palette_index: u8,
    /// The current colour in the palette print out being parsed.
    row_index: u8,
}

impl Machine {
    /// A pure blue used for signalling in the our "QR Code" of the palette.
    const PURE_BLUE: xcap::image::Rgba<u8> = xcap::image::Rgba::<u8>([0, 0, 255, 255]);

    // TODO:
    //   Some of the state isn't kept in the enum variants. That's not a big problem, but I'd
    //   be interested if there is a more state machine-like way to do this?
    //
    /// Parse the raw pixels of a screenshot looking for our QR Code-like print out of all the
    /// colours from the current terminal's palette.
    pub(crate) fn parse_screenshot(
        image: &super::parser::Screenshot,
    ) -> Result<crate::palette::converter::Palette> {
        tracing::debug!(
            "Starting palette parse of image with {} pixels",
            image.pixels().len()
        );

        let mut machine = Self {
            state: State::LookingForRedish(0),
            palette: crate::palette::converter::Palette {
                map: std::collections::HashMap::new(),
            },
            current_colour: xcap::image::Rgba::<u8>([0, 0, 0, 0]),
            palette_index: 0,
            row_index: 0,
        };

        tracing::debug!("Looking for first redish palette row start");
        for pixel in image.enumerate_pixels() {
            let pixel_color = *pixel.2;
            let is_finished = machine.state_transition(pixel_color);
            if is_finished {
                return Ok(machine.palette);
            }
        }

        color_eyre::eyre::bail!(
            "Couldn't find all colours in palette, only found: {}.",
            machine.palette_index
        );
    }

    /// Is the current pixel a part of the printed palette and has it changed?
    fn is_new_palette_pixel(&self, previous_colour: xcap::image::Rgba<u8>) -> bool {
        let is_in_palette_printout = !matches!(self.state, State::LookingForRedish(_));

        // TODO: I don't feel this condition is very intuitive
        if is_in_palette_printout && self.current_colour == previous_colour {
            return false;
        }

        true
    }

    /// Transition the state machine.
    fn state_transition(&mut self, pixel_colour: xcap::image::Rgba<u8>) -> bool {
        let previous_colour = self.current_colour;
        self.current_colour = pixel_colour;

        if !self.is_new_palette_pixel(previous_colour) {
            return false;
        }

        match self.state {
            State::LookingForRedish(row_marker) => {
                let redish_row_start = xcap::image::Rgba::<u8>([255, row_marker, 0, 255]);
                if self.current_colour == redish_row_start {
                    tracing::debug!("Potential palette row found: {row_marker}");

                    self.state = State::LookingForBlue;
                }
            }
            State::LookingForBlue => {
                if self.current_colour == Self::PURE_BLUE {
                    tracing::debug!("Pure blue row signal found");

                    self.state = State::LookingForFirstColourInRow;
                } else {
                    tracing::debug!("False positive palette start, restarting row search");

                    self.state = State::LookingForRedish(self.row_index);
                }
            }
            State::LookingForFirstColourInRow => {
                tracing::info!(
                    "Palette colour found! ID: {} {:?}",
                    self.palette_index,
                    self.current_colour
                );

                self.state = State::CollectingRow(0);

                // TODO: I feel like there should be a way to get this (inserting of a palette
                // colour) into the [`ParserState::CollectingRow`] step 🤔
                self.palette.map.insert(
                    self.palette_index.to_string(),
                    (
                        self.current_colour[0],
                        self.current_colour[1],
                        self.current_colour[2],
                    ),
                );
                self.palette_index += 1;
            }
            State::CollectingRow(column_index) => {
                let new_column = column_index + 1;
                if new_column >= crate::palette::parser::PALETTE_ROW_SIZE {
                    self.row_index += 1;
                    self.state = State::LookingForRedish(self.row_index);

                    tracing::debug!("Looking for redish palette row start: {}", self.row_index);
                } else {
                    tracing::info!(
                        "Palette colour found! ID: {} {:?}",
                        self.palette_index,
                        self.current_colour
                    );

                    self.palette.map.insert(
                        self.palette_index.to_string(),
                        (
                            self.current_colour[0],
                            self.current_colour[1],
                            self.current_colour[2],
                        ),
                    );
                    if self.palette_index == 255 {
                        return true;
                    }
                    self.palette_index += 1;

                    self.state = State::CollectingRow(new_column);
                }
            }
        }

        false
    }
}

#[expect(clippy::indexing_slicing, reason = "Tests aren't so strict")]
#[cfg(test)]
mod test {
    use super::*;

    fn assert_screenshot(path: std::path::PathBuf) {
        let screenshot = xcap::image::open(path).unwrap();
        let palette = Machine::parse_screenshot(&screenshot.into_rgba8()).unwrap();

        assert_eq!(palette.map["0"], (14, 13, 21));
        assert_eq!(palette.map["128"], (175, 0, 215));
        assert_eq!(palette.map["255"], (238, 238, 238));
    }

    #[test]
    fn parse_palette_easy() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/resources/palette_screenshot_easy.png");
        assert_screenshot(path);
    }

    #[test]
    fn parse_palette_hard() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/resources/palette_screenshot_hard.png");

        assert_screenshot(path);
    }
}
