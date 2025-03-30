//! A state machine for parsing a QRcode-like screenshot of the user's terminal colours.

use color_eyre::{eyre::ContextCompat as _, Result};

/// A state machine for parsing a QR Code-like grid of colours.
enum State {
    /// The first colour to look for is a pure(ish) red, which indicates the start of the colour grid.
    /// Reds are also used to indicate the beginnings of new rows of colours by incrementing the
    /// green channel by 1 for every row.
    LookingForRedish,
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
    /// The current row in the palette print out being parsed.
    row_index: u8,
    /// The accumulated confidence that the parser is in a new palette block.
    block_confidence: u8,
    /// The accumulated confidence that the parser is in a new row of the palette.
    row_confidence: u8,
}

impl Machine {
    /// A pure blue used for signalling in the our "QR Code" of the palette.
    const PURE_BLUE: xcap::image::Rgba<u8> = xcap::image::Rgba::<u8>([0, 0, 255, 255]);

    /// The number of times a pixel must occur one after the other to be considered as defining a
    /// new palette block.
    const COLOUR_CONFIDENCE: u8 = 3;

    /// The maximum level of lossy noise that allows 2 colours to be considered the same.
    const MAX_LOSSY_NOISE_LEVEL: u16 = 4;

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
            state: State::LookingForRedish,
            palette: crate::palette::converter::Palette {
                map: std::collections::HashMap::new(),
            },
            current_colour: xcap::image::Rgba::<u8>([0, 0, 0, 0]),
            palette_index: 0,
            row_index: 0,
            block_confidence: 0,
            row_confidence: 0,
        };

        tracing::debug!("Looking for first redish palette row start");
        for pixel in image.enumerate_pixels() {
            let pixel_color = *pixel.2;
            let is_finished = machine.state_transition(pixel_color)?;
            if is_finished {
                return Ok(machine.palette);
            }
        }

        color_eyre::eyre::bail!(
            "Couldn't find all colours in palette, only found: {}.",
            machine.palette_index
        );
    }

    /// Is the parser in known pure red block at the start of a palette row?
    fn is_row_start_redish(&mut self, previous_colour: xcap::image::Rgba<u8>) -> bool {
        let redish_row_start = xcap::image::Rgba::<u8>([255, self.row_index, 0, 255]);
        if !(previous_colour == redish_row_start && self.current_colour == redish_row_start) {
            return false;
        }

        self.row_confidence += 1;
        tracing::debug!(
            "Potential palette row ({}) found, certainty {}/{}",
            self.row_index,
            self.row_confidence,
            Self::COLOUR_CONFIDENCE
        );

        self.row_confidence > Self::COLOUR_CONFIDENCE
    }

    /// Have we moved into a new palette block?
    fn is_new_palette_block(&mut self, previous_colour: xcap::image::Rgba<u8>) -> Result<bool> {
        if self.block_confidence == 0 {
            if !self.is_same_colour(previous_colour)? {
                self.block_confidence += 1;
            }

            return Ok(false);
        }

        if self.is_same_colour(previous_colour)? {
            self.block_confidence += 1;
        }
        Ok(self.block_confidence > Self::COLOUR_CONFIDENCE)
    }

    /// Calculate a crude difference metric for 2 colours
    fn colour_difference(&self, colour: xcap::image::Rgba<u8>) -> Result<u16> {
        let mut difference = 0;
        difference += Self::channel_difference(self.current_colour.0, colour.0, 0)?;
        difference += Self::channel_difference(self.current_colour.0, colour.0, 1)?;
        difference += Self::channel_difference(self.current_colour.0, colour.0, 2)?;
        Ok(difference)
    }

    /// Calculate the difference between a single channel on 2 colours.
    fn channel_difference(left: [u8; 4], right: [u8; 4], channel: usize) -> Result<u16> {
        Ok(i16::from(*left.get(channel).context("")?)
            .abs_diff(i16::from(*right.get(channel).context("")?)))
    }

    /// Within our defined noise levels, is the new colour the same as the current colour?
    fn is_same_colour(&self, colour: xcap::image::Rgba<u8>) -> Result<bool> {
        let difference = self.colour_difference(colour)?;
        if matches!(self.state, State::LookingForBlue) {
            tracing::trace!("{:?}-{:?}={difference}", colour, self.current_colour);
        }
        Ok(difference < Self::MAX_LOSSY_NOISE_LEVEL)
    }

    /// Does the new at the boundary of a state change? Like entering/exit the palette grid,
    /// starting a new palette block etc.
    fn is_transition(&mut self, pixel_colour: xcap::image::Rgba<u8>) -> Result<bool> {
        let previous_colour = self.current_colour;
        self.current_colour = pixel_colour;

        #[expect(
            clippy::collapsible_else_if,
            reason = "
                I think it looks better like this. Besides a trusted member of my Twitch
                community saw me struggling on this minor cosmetic detail kindly
                suggested that I go out and touch grass ❤️.
            "
        )]
        if matches!(self.state, State::LookingForRedish) {
            if !self.is_row_start_redish(previous_colour) {
                return Ok(false);
            }
        } else {
            if !self.is_new_palette_block(previous_colour)? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Transition the state machine.
    fn state_transition(&mut self, pixel_colour: xcap::image::Rgba<u8>) -> Result<bool> {
        if !self.is_transition(pixel_colour)? {
            return Ok(false);
        }

        match self.state {
            State::LookingForRedish => {
                self.state = State::LookingForBlue;
            }
            State::LookingForBlue => {
                if self.current_colour == Self::PURE_BLUE {
                    tracing::debug!("Pure blue row signal found");

                    self.state = State::LookingForFirstColourInRow;
                } else {
                    tracing::debug!(
                        "False positive palette start (found {:?} after pure red), restarting row search",
                        self.current_colour
                    );

                    self.state = State::LookingForRedish;
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
                    self.state = State::LookingForRedish;

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
                        return Ok(true);
                    }
                    self.palette_index += 1;

                    self.state = State::CollectingRow(new_column);
                }
            }
        }

        self.block_confidence = 0;
        self.row_confidence = 0;

        Ok(false)
    }
}

#[expect(clippy::indexing_slicing, reason = "Tests aren't so strict")]
#[cfg(test)]
mod test {
    use super::*;

    fn assert_default_screenshot(path: std::path::PathBuf) {
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
        assert_default_screenshot(path);
    }

    #[test]
    fn parse_palette_hard() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/resources/palette_screenshot_hard.png");

        assert_default_screenshot(path);
    }

    #[test]
    fn parse_palette_lossy() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/resources/palette_screenshot_cmang.png");
        let screenshot = xcap::image::open(path).unwrap();
        let palette = Machine::parse_screenshot(&screenshot.into_rgba8()).unwrap();

        assert_eq!(palette.map["0"], (1, 1, 0));
        assert_eq!(palette.map["128"], (175, 0, 215));
        assert_eq!(palette.map["255"], (238, 237, 238));
    }
}
