//! The cursor gives off a gas that floats up and interacts with the history

use std::{collections::VecDeque, sync::Arc};

use color_eyre::eyre::Result;

use super::simulation::Simulation;
use crate::{shared_state::SharedState, tattoys::index::Tattoyer};

/// `SmokeyCursor`
#[derive(Default)]
pub(crate) struct SmokeyCursor {
    /// TTY width
    width: u16,
    /// TTY height
    height: u16,
    /// Shared app state
    state: Arc<SharedState>,
    /// All the particles of gas
    simulation: Simulation,
    /// Timestamp of last tick
    durations: VecDeque<f64>,
}

impl Tattoyer for SmokeyCursor {
    /// Instantiate
    fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = state.get_tty_size()?;

        Ok(Self {
            width: tty_size.width,
            height: tty_size.height,
            state,
            simulation: Simulation::new(tty_size.width.into(), (tty_size.height * 2).into()),
            durations: VecDeque::default(),
        })
    }

    fn set_tty_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// One frame of the tattoy
    fn tick(&mut self) -> Result<termwiz::surface::Surface> {
        let start = std::time::Instant::now();

        let mut surface =
            crate::surface::Surface::new(usize::from(self.width), usize::from(self.height));
        let mut pty = self
            .state
            .shadow_tty
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        let cursor = pty.cursor_position();
        let cells = pty.screen_cells();

        self.simulation.tick(cursor, &cells);
        drop(pty);

        for particle in &mut self.simulation.particles {
            let position = particle.position_unscaled();

            #[expect(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "We're just rendering to a terminal grid"
            )]
            surface.add_pixel(position.x as usize, position.y as usize, particle.colour)?;
        }

        let text_coloumn = usize::from(self.width - 20);
        let count = self.simulation.particles.len();
        surface.add_text(text_coloumn, 0, format!("Particles: {count}"));

        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            clippy::default_numeric_fallback,
            reason = "This is just debugging output"
        )]
        {
            let average_tick = self.durations.iter().sum::<f64>() / self.durations.len() as f64;
            let fps = 1.0 / average_tick;
            surface.add_text(text_coloumn, 1, format!("FPS: {fps:.3}"));
        };

        self.durations.push_front(start.elapsed().as_secs_f64());
        if self.durations.len() > 30 {
            self.durations.pop_back();
        }
        Ok(surface.surface)
    }
}
