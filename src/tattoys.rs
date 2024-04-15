//! The manager of all the fancy Tattoy eye-candy code

use color_eyre::eyre::Result;
use rand::Rng;

use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;
use tokio::sync::mpsc;

use crate::run::TattoySurface;

/// The number of microseonds in a second
const ONE_MICROSECOND: u64 = 1_000_000;

/// "Compositor" or "Tattoys"?
#[allow(clippy::exhaustive_structs)]
pub struct Tattoys {
    /// The terminal's width
    width: usize,
    /// The terminal's height
    height: usize,
}

impl Tattoys {
    /// Create a Compositor/Tattoy
    #[must_use]
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    /// Run the tattoy(s)
    #[allow(clippy::arithmetic_side_effects)]
    pub fn run(&self, tattoy_output: &mpsc::UnboundedSender<TattoySurface>) -> Result<()> {
        let target_frame_rate = 30;

        #[allow(clippy::integer_division)]
        let target_frame_rate_micro =
            std::time::Duration::from_micros(ONE_MICROSECOND / target_frame_rate);

        let mut x1 = rand::thread_rng().gen_range(1..self.width);
        let mut y1 = rand::thread_rng().gen_range(1..self.height);
        loop {
            let frame_time = std::time::Instant::now();
            let mut block = termwiz::surface::Surface::new(self.width, self.height);

            x1 = x1 + rand::thread_rng().gen_range(0..=2) - 1;
            x1 = x1.clamp(1, self.width);

            y1 = y1 + rand::thread_rng().gen_range(0..=2) - 1;
            y1 = y1.clamp(1, self.height);

            block.add_change(TermwizChange::CursorPosition {
                x: TermwizPosition::Absolute(x1),
                y: TermwizPosition::Absolute(y1),
            });
            block.add_changes(vec![TermwizChange::Attribute(
                termwiz::cell::AttributeChange::Foreground(
                    termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                        termwiz::color::SrgbaTuple(
                            rand::thread_rng().gen_range(0.0..1.0),
                            rand::thread_rng().gen_range(0.0..1.0),
                            rand::thread_rng().gen_range(0.0..1.0),
                            1.0,
                        ),
                    ),
                ),
            )]);

            #[allow(clippy::non_ascii_literal)]
            block.add_change("▄");

            tattoy_output.send(TattoySurface {
                kind: crate::run::SurfaceType::BGSurface,
                surface: block,
            })?;

            if let Some(i) = target_frame_rate_micro.checked_sub(frame_time.elapsed()) {
                std::thread::sleep(i);
            }
        }
    }
}
