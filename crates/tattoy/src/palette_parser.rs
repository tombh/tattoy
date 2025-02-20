//! Print out the terminal's palette, take a screenshot and try to parse the true colour values for
//! each member of the palette.
//!
//! The best source for how terminal palettes work is this Stack Overflow answer:
//!   <https://stackoverflow.com/a/27165165/575773>
//!
//! The reason we need this is that the default colours of a terminal are only expressed by their
//! palette index, which doesn't actually give us a colour value that we can use to do things like
//! alpha blending, interpolation etc. So what Tattoy can do is learn to associate each palette
//! index referenced by the PTY with a true colour value. That way the terminal retains the exact
//! palette configured by the user, whilst also being able to do colour maths on the palette.

use std::io::Write as _;

use color_eyre::{eyre::ContextCompat as _, Result};

/// Convenience type for screenshot image.
type Screenshot = xcap::image::ImageBuffer<xcap::image::Rgba<u8>, std::vec::Vec<u8>>;

/// A single palette colour.
type PaletteColour = (u8, u8, u8);

/// Convenience type for the palette hash.
type Palette = std::collections::HashMap<String, PaletteColour>;

/// Reset any OSC colour codes
const RESET_COLOUR: &str = "\x1b[m";

/// OSC code to clear the terminal
const CLEAR_SCREEN: &str = "\x1b[2J";

/// The number of palette colours we put in each row of our "QR code".
const ROW_SIZE: u8 = 16;

/// A pure blue used for signalling in the our "QR Code" of the palette.
const PURE_BLUE: &xcap::image::Rgba<u8> = &xcap::image::Rgba::<u8>([0, 0, 255, 255]);

/// A parser for converting default terminal palette colours to true colours.
pub(crate) struct PaletteParser;

/// A state machine for parsing a QR Code-like grid of colours.
enum ParserState {
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

#[expect(
    clippy::print_stdout,
    reason = "We need to print the terminal's palette"
)]
impl PaletteParser {
    /// Main entrypoint
    pub fn run(maybe_user_screenshot: Option<&String>) -> Result<()> {
        let screenshot = match maybe_user_screenshot {
            Some(path) => {
                Self::print_native_palette()?;
                println!("Parsing screenshot file at: {path}...");

                xcap::image::open(path)?.into_rgba8()
            }
            None => Self::take_screenshot()?,
        };
        let result = Self::parse_screenshot(&screenshot);
        let Ok(palette) = result else {
            if maybe_user_screenshot.is_none() {
                let path = crate::config::Config::temporary_file("screenshot.png")?;
                screenshot.save(path.clone())?;

                color_eyre::eyre::bail!(
                    "\
                    Couldn't parse palette, screenshot saved to: {path:?}. \
                    You may also make your own screenshot and provide it with \
                    `tattoy --parse-palette screenshot.png`.
                    "
                );
            } else {
                color_eyre::eyre::bail!("Palette parsing failed.");
            }
        };
        Self::print_true_colour_palette(&palette)?;
        Self::save(&palette)?;

        Ok(())
    }

    /// Print all the colours of the terminal to STDOUT.
    fn print_native_palette() -> Result<()> {
        println!("These are all the colours in your terminal's palette:");
        Self::print_generic_palette(|palette_index| -> Result<()> {
            let background_colour = palette_index;
            let foreground_colour = palette_index + ROW_SIZE;
            print!("\x1b[48;5;{background_colour}m\x1b[38;5;{foreground_colour}m▄{RESET_COLOUR}");
            Ok(())
        })?;

        Ok(())
    }

    /// Print all the true colour versions of the terminal's palette as found in the screenshot.
    fn print_true_colour_palette(palette: &Palette) -> Result<()> {
        println!();
        println!("These colours should match the colours above:");
        Self::print_generic_palette(|palette_index| -> Result<()> {
            let bg = palette
                .get(&palette_index.to_string())
                .context("Palette colour not found")?;
            let fg = palette
                .get(&(palette_index + ROW_SIZE).to_string())
                .context("Palette colour not found")?;
            Self::print_2_true_colours_in_1((bg.0, bg.1, bg.2), (fg.0, fg.1, fg.2));
            Ok(())
        })
    }

    /// Print out all the colours of a terminal palette in a sqaure, that both looks pretty and
    /// conforms to the QR Code-like requirements of parsing.
    fn print_generic_palette<F: Fn(u8) -> Result<()>>(callback: F) -> Result<()> {
        let pure_blue = (0, 0, 255);
        println!("╭──────────────────╮");
        for y in 0u8..8 {
            print!("│");

            // Print the pure(ish) red that indicates the start of a valid palette row.
            Self::print_2_true_colours_in_1((255, y * 2, 0), (255, (y * 2) + 1, 0));
            // Print the pure blue that helps us avoid false positives.
            Self::print_2_true_colours_in_1(pure_blue, pure_blue);

            for x in 0..ROW_SIZE {
                let palette_index = (y * ROW_SIZE * 2) + x;
                callback(palette_index)?;
            }
            print!("│");
            println!();
        }
        println!("╰──────────────────╯");
        std::io::stdout().flush()?;

        Ok(())
    }

    /// Use the UTF-8 half block trick to print 2 colours in one cell.
    fn print_2_true_colours_in_1(top: (u8, u8, u8), bottom: (u8, u8, u8)) {
        print!(
            "\x1b[48;2;{};{};{}m\x1b[38;2;{};{};{}m▄{RESET_COLOUR}",
            top.0, top.1, top.2, bottom.0, bottom.1, bottom.2,
        );
    }

    /// Take a screenshot of the current monitor.
    fn take_screenshot() -> Result<Screenshot> {
        println!("{CLEAR_SCREEN}");

        Self::print_native_palette()?;

        print!(
            "\
            Tattoy will take a screenshot and attempt to find the palette's true colour values. \
            Enter 'y' to continue: "
        );
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        println!();

        if answer != "y\n" {
            println!("Aborted");
            std::process::exit(0);
        }

        let monitors = xcap::Monitor::all()?;
        if monitors.is_empty() {
            color_eyre::eyre::bail!("No monitors found to take screenshot on");
        }

        if monitors.len() > 1 {
            for monitor in monitors.clone() {
                if monitor.is_primary() {
                    return Ok(monitor.capture_image()?);
                }
            }
        }

        if let Some(monitor) = monitors.first() {
            return Ok(monitor.capture_image()?);
        }

        color_eyre::eyre::bail!("No monitors found to take screenshot on");
    }

    // TODO:
    //   Some of the state isn't kept in the enum variants. That's not a big problem, but I'd
    //   be interested if there is a more state machine-like way to do this?
    //
    /// Parse the raw pixels of a screenshot looking for our QR Code-like print out of all the
    /// colours from the current terminal's palette.
    fn parse_screenshot(image: &Screenshot) -> Result<Palette> {
        tracing::debug!(
            "Starting palette parse of image with {} pixels",
            image.pixels().len()
        );

        let mut palette: Palette = std::collections::HashMap::new();

        let mut state = ParserState::LookingForRedish(0);
        tracing::debug!("Looking for first redish palette row start");
        let mut palette_index = 0u8;
        let mut row_index = 0u8;

        let mut current_colour = &xcap::image::Rgba::<u8>([0, 0, 0, 0]);
        for pixel in image.enumerate_pixels() {
            let previous_colour = current_colour;
            current_colour = pixel.2;
            let is_in_palette = !matches!(state, ParserState::LookingForRedish(_));
            if is_in_palette && current_colour == previous_colour {
                continue;
            }

            match state {
                ParserState::LookingForRedish(row_marker) => {
                    let redish_row_start = &xcap::image::Rgba::<u8>([255, row_marker, 0, 255]);
                    if current_colour == redish_row_start {
                        tracing::debug!("Potential palette row found: {row_marker}");

                        state = ParserState::LookingForBlue;
                    }
                }
                ParserState::LookingForBlue => {
                    if current_colour == PURE_BLUE {
                        tracing::debug!("Pure blue row signal found");

                        state = ParserState::LookingForFirstColourInRow;
                    } else {
                        tracing::debug!("False positive palette start, restarting row search");

                        state = ParserState::LookingForRedish(row_index);
                    }
                }
                ParserState::LookingForFirstColourInRow => {
                    tracing::info!("Palette colour found! ID: {palette_index} {current_colour:?}");

                    state = ParserState::CollectingRow(0);

                    // TODO: I feel like there should be a way to get this (inserting of a palette
                    // colour) into the [`ParserState::CollectingRow`] step 🤔
                    palette.insert(
                        palette_index.to_string(),
                        (current_colour[0], current_colour[1], current_colour[2]),
                    );
                    palette_index += 1;
                }
                ParserState::CollectingRow(column_index) => {
                    let new_column = column_index + 1;
                    if new_column >= ROW_SIZE {
                        row_index += 1;
                        state = ParserState::LookingForRedish(row_index);

                        tracing::debug!("Looking for redish palette row start: {row_index}");
                    } else {
                        tracing::info!(
                            "Palette colour found! ID: {palette_index} {current_colour:?}"
                        );

                        palette.insert(
                            palette_index.to_string(),
                            (current_colour[0], current_colour[1], current_colour[2]),
                        );
                        if palette_index == 255 {
                            return Ok(palette);
                        }
                        palette_index += 1;

                        state = ParserState::CollectingRow(new_column);
                    }
                }
            }
        }

        color_eyre::eyre::bail!(
            "Couldn't find all colours in palette, only found: {palette_index}."
        );
    }

    /// Save the parsed palette true colours as TOML in the Tattoy config directory.
    fn save(palette: &Palette) -> Result<()> {
        print!("If the palettes look the same press 'y' to save: ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        println!();

        if answer != "y\n" {
            println!("Aborted");
        }

        let path = crate::config::Config::directory()?.join("palette.toml");

        let data = toml::to_string(&palette)?;
        std::fs::write(path.clone(), data)?;

        println!("Palette saved to: {}", path.display());
        Ok(())
    }
}

#[expect(clippy::indexing_slicing, reason = "Tests aren't so strict")]
#[cfg(test)]
mod test {
    use super::*;

    fn assert_screenshot(path: std::path::PathBuf) {
        let screenshot = xcap::image::open(path).unwrap();
        let palette = PaletteParser::parse_screenshot(&screenshot.into_rgba8()).unwrap();

        assert_eq!(palette["0"], (14, 13, 21));
        assert_eq!(palette["128"], (175, 0, 215));
        assert_eq!(palette["255"], (238, 238, 238));
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
