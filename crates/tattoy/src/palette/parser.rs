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

use color_eyre::Result;

/// Convenience type for screenshot image.
pub type Screenshot = xcap::image::ImageBuffer<xcap::image::Rgba<u8>, std::vec::Vec<u8>>;

/// The number of palette colours we put in each row of our "QR code".
pub const PALETTE_ROW_SIZE: u8 = 16;

/// A parser for converting default terminal palette colours to true colours.
pub(crate) struct Parser;

#[expect(
    clippy::print_stdout,
    reason = "We need to print the terminal's palette"
)]
impl Parser {
    /// Main entrypoint
    pub async fn run(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
        maybe_user_screenshot: Option<&String>,
    ) -> Result<()> {
        let screenshot = match maybe_user_screenshot {
            Some(path) => {
                Self::print_native_palette()?;
                println!("Parsing screenshot file at: {path}...");

                xcap::image::open(path)?.into_rgba8()
            }
            None => Self::take_screenshot()?,
        };
        let result = super::state_machine::Machine::parse_screenshot(&screenshot);
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
        palette.print_true_colour_palette()?;
        Self::save(state, &palette).await?;

        Ok(())
    }

    /// Canonical path to the palette config file.
    pub async fn palette_config_path(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> std::path::PathBuf {
        crate::config::Config::directory(state)
            .await
            .join("palette.toml")
    }

    /// Print all the colours of the terminal to STDOUT.
    fn print_native_palette() -> Result<()> {
        println!("These are all the colours in your terminal's palette:");
        Self::print_generic_palette(|palette_index| -> Result<()> {
            let background_colour = palette_index;
            let foreground_colour = palette_index + PALETTE_ROW_SIZE;
            print!(
                "\x1b[48;5;{background_colour}m\x1b[38;5;{foreground_colour}m▄{}",
                crate::utils::RESET_COLOUR
            );
            Ok(())
        })?;

        Ok(())
    }

    /// Print out all the colours of a terminal palette in a sqaure, that both looks pretty and
    /// conforms to the QR Code-like requirements of parsing.
    pub fn print_generic_palette<F: Fn(u8) -> Result<()>>(callback: F) -> Result<()> {
        let pure_blue = (0, 0, 255);
        println!("╭──────────────────╮");
        for y in 0u8..8 {
            print!("│");

            // Print the pure(ish) red that indicates the start of a valid palette row.
            Self::print_2_true_colours_in_1((255, y * 2, 0), (255, (y * 2) + 1, 0));
            // Print the pure blue that helps us avoid false positives.
            Self::print_2_true_colours_in_1(pure_blue, pure_blue);

            for x in 0..PALETTE_ROW_SIZE {
                let palette_index = (y * PALETTE_ROW_SIZE * 2) + x;
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
    pub fn print_2_true_colours_in_1(top: (u8, u8, u8), bottom: (u8, u8, u8)) {
        print!(
            "\x1b[48;2;{};{};{}m\x1b[38;2;{};{};{}m▄{}",
            top.0,
            top.1,
            top.2,
            bottom.0,
            bottom.1,
            bottom.2,
            crate::utils::RESET_COLOUR
        );
    }

    /// Take a screenshot of the current monitor.
    fn take_screenshot() -> Result<Screenshot> {
        println!("{}", crate::utils::CLEAR_SCREEN);

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

        if answer != format!("y{}", crate::utils::NEWLINE) {
            println!("'y' not selected, exiting...");
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

    /// Save the parsed palette true colours as TOML in the Tattoy config directory.
    async fn save(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
        palette: &crate::palette::converter::Palette,
    ) -> Result<()> {
        print!("If the palettes look the same press 'y' to save: ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        println!();

        if answer != format!("y{}", crate::utils::NEWLINE) {
            println!("'y' not selected, exiting...");
            return Ok(());
        }

        let path = Self::palette_config_path(state).await;
        let data = toml::to_string(&palette.map)?;
        std::fs::write(path.clone(), data)?;

        println!("Palette saved to: {}", path.display());
        Ok(())
    }
}
