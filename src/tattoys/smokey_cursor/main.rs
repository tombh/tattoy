//! The cursor gives off a gas that floats up and interacts with the history

use std::{collections::VecDeque, sync::Arc};

use color_eyre::eyre::Result;

use super::simulation::Simulation;
use crate::{shared_state::SharedState, tattoys::index::Tattoyer};

///
#[derive(Default)]
pub struct SmokeyCursor {
    /// TTY width
    width: usize,
    /// TTY height
    height: usize,
    /// Shared app state
    state: Arc<SharedState>,
    /// All the particles of gas
    simulation: Simulation,
    /// Timestamp of last tick
    durations: VecDeque<f64>,
}

impl Tattoyer for SmokeyCursor {
    ///
    fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = state.get_tty_size()?;

        Ok(Self {
            width: tty_size.0,
            height: tty_size.1,
            state,
            #[allow(clippy::arithmetic_side_effects)]
            simulation: Simulation::new(tty_size.0, tty_size.1 * 2),
            durations: VecDeque::default(),
        })
    }

    /// One frame of the tattoy
    #[allow(
        clippy::float_arithmetic,
        clippy::arithmetic_side_effects,
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::default_numeric_fallback
    )]
    fn tick(&mut self) -> Result<termwiz::surface::Surface> {
        let start = std::time::Instant::now();

        let mut surface = crate::surface::Surface::new(self.width, self.height);
        let mut pty = self
            .state
            .shadow_tty
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        let cursor = pty.cursor_position();
        let cells = pty.screen_cells();

        self.simulation.tick(cursor, &cells);
        drop(pty);

        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            clippy::as_conversions
        )]
        for particle in &mut self.simulation.particles {
            let position = particle.position_unscaled();
            surface.add_pixel(position.x as usize, position.y as usize, particle.colour)?;
        }

        let text_coloumn = self.width - 20;
        let count = self.simulation.particles.len();
        surface.add_text(text_coloumn, 0, format!("Particles: {count}"));

        let average_tick = self.durations.iter().sum::<f64>() / self.durations.len() as f64;
        let fps = 1.0 / average_tick;
        surface.add_text(text_coloumn, 1, format!("FPS: {fps:.3}"));

        self.durations.push_front(start.elapsed().as_secs_f64());
        if self.durations.len() > 30 {
            self.durations.pop_back();
        }
        Ok(surface.surface)
    }
}
