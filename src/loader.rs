//! The manager of all the fancy Tattoy eye-candy code

use color_eyre::eyre::Result;

use tokio::sync::mpsc;

use crate::run::TattoySurface;
use crate::tattoys::random_walker::RandomWalker;

/// The number of microseonds in a second
const ONE_MICROSECOND: u64 = 1_000_000;

/// "Compositor" or "Tattoys"?
#[allow(clippy::exhaustive_structs)]
pub struct Loader {
    /// The terminal's width
    width: usize,
    /// The terminal's height
    height: usize,
}

impl Loader {
    /// Create a Compositor/Tattoy
    #[must_use]
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    /// Run the tattoy(s)
    pub fn run(&mut self, tattoy_output: &mpsc::UnboundedSender<TattoySurface>) -> Result<()> {
        let target_frame_rate = 30;

        #[allow(clippy::integer_division)]
        let target_frame_rate_micro =
            std::time::Duration::from_micros(ONE_MICROSECOND / target_frame_rate);

        let mut tattoy = RandomWalker::new(self.width, self.height)?;

        loop {
            let frame_time = std::time::Instant::now();

            tattoy_output.send(TattoySurface {
                kind: crate::run::SurfaceType::BGSurface,
                surface: tattoy.tick()?,
            })?;

            if let Some(i) = target_frame_rate_micro.checked_sub(frame_time.elapsed()) {
                std::thread::sleep(i);
            }
        }
    }
}
