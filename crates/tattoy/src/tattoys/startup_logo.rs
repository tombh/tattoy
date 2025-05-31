//! Display the logo at startup

use color_eyre::eyre::Result;
use palette::blend::Blend as _;
use rand::{Rng as _, SeedableRng as _};

/// The ASCII logo.
const LOGO: &str = include_str!("../../logo.txt");

/// `StartupLogo`
pub(crate) struct StartupLogo {
    /// The base Tattoy struct
    tattoy: super::tattoyer::Tattoyer,
    /// The logo text itself
    logo: String,
    /// The width of the logo
    width: u16,
    /// The height of the logo
    height: u16,
    /// The terminal palette
    palette: crate::palette::converter::Palette,
    /// Time at which the logo appeared
    started_at: tokio::time::Instant,
    /// Has the rendering finished?
    is_finished: bool,
}

impl StartupLogo {
    /// Instantiate
    async fn new(
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Self {
        let tattoy = super::tattoyer::Tattoyer::new(
            "startup_logo".to_owned(),
            state,
            200,
            1.0,
            output_channel,
        )
        .await;
        let (width, height) = Self::get_width_and_height();

        Self {
            tattoy,
            logo: Self::make_logo(),
            width,
            height,
            palette,
            started_at: tokio::time::Instant::now(),
            is_finished: false,
        }
    }

    /// Make the logo.
    fn make_logo() -> String {
        let (width, _) = Self::get_width_and_height();
        let version = format!("v{}", std::env!("CARGO_PKG_VERSION"));
        let padding = usize::from(width) - version.len();
        format!("{}{}{}", LOGO, " ".repeat(padding), version)
    }

    /// Get the dimensions of the logo.
    #[expect(clippy::unwrap_used, reason = "The logo is a static asset")]
    fn get_width_and_height() -> (u16, u16) {
        let width = u16::try_from(
            LOGO.lines()
                .max_by(|line_a, line_b| line_a.len().cmp(&line_b.len()))
                .unwrap()
                .len(),
        )
        .unwrap();
        let height = u16::try_from(LOGO.lines().count()).unwrap();

        (width, height)
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Result<()> {
        let tty_size = *state.tty_size.read().await;
        let (logo_width, logo_height) = Self::get_width_and_height();
        if tty_size.height <= logo_height || tty_size.width <= logo_width {
            return Ok(());
        }

        let mut protocol = state.protocol_tx.subscribe();
        let mut runner = Self::new(output, state, palette).await;

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                () = runner.tattoy.sleep_until_next_frame_tick(), if !runner.is_finished => {
                    runner.render().await?;
                },
                result = protocol.recv() => {
                    if matches!(result, Ok(crate::run::Protocol::End)) {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Tick the render
    async fn render(&mut self) -> Result<()> {
        if self.fade_out(16) == 0.0 {
            self.tattoy.send_blank_output().await?;
            self.is_finished = true;
            return Ok(());
        }

        self.tattoy.initialise_surface();

        let tty_centre_x = self.tattoy.width.div_euclid(2) - 1;
        let tty_centre_y = self.tattoy.height.div_euclid(2) - 1;
        let logo_centre_x = self.width.div_euclid(2);
        let logo_centre_y = self.height.div_euclid(2);

        let mut y = tty_centre_y - logo_centre_y;
        for (logo_y, line) in self.logo.lines().enumerate() {
            let mut x = tty_centre_x - logo_centre_x;
            for (logo_x, character) in line.chars().enumerate() {
                let colour = self.get_colour(logo_x.try_into()?, logo_y.try_into()?)?;
                self.tattoy.surface.add_text(
                    x.into(),
                    y.into(),
                    character.into(),
                    None,
                    Some(colour),
                );
                x += 1;
            }
            y += 1;
        }

        self.tattoy.send_output().await
    }

    /// Get the colour of an individual character in the logo.
    fn get_colour(&self, x: u16, y: u16) -> Result<crate::surface::Colour> {
        let mut seeded = rand::rngs::StdRng::seed_from_u64((x * y).into());

        let mut index: u8 = y.try_into()?;
        let mut main_colour = self.colour_from_palette_index(index);

        if seeded.gen_range(0..3u8) == 0 {
            if seeded.gen_range(0..1u8) == 0 {
                index -= 1;
            } else {
                index += 1;
            }
            index = index.clamp(1, 16);
            let mut blendable_colour = self.colour_from_palette_index(index);
            blendable_colour.alpha = 0.5;
            main_colour = main_colour.multiply(blendable_colour);
        }

        let fade_out = self.fade_out(index);
        Ok((
            main_colour.red * fade_out,
            main_colour.green * fade_out,
            main_colour.blue * fade_out,
            fade_out,
        ))
    }

    /// Get a palette-crate colour from a Tattoy-palette index.
    fn colour_from_palette_index(&self, y: u8) -> palette::Alpha<palette::rgb::Rgb, f32> {
        let palette_colour = self.palette.true_colour_tuple_from_index(y);
        palette::Srgba::from_components(palette_colour.into())
    }

    /// Calculate the fade out opacity.
    pub fn fade_out(&self, index: u8) -> f32 {
        let start_fade_after = 1.5;
        let max = 1.0;
        let min = 0.0;

        let age = tokio::time::Instant::now() - self.started_at;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::as_conversions,
            reason = "We're working within very known limits"
        )]
        if age < tokio::time::Duration::from_millis((start_fade_after * 1000.0) as u64) {
            return max;
        }

        let from_bottom = f32::from(index) / 32.0;
        let x = age.as_secs_f32() - start_fade_after + from_bottom;

        crate::utils::smoothstep(max, min, x)
    }
}
