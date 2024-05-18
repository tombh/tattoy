//! The cursor gives off a gas that floats up and interacts with the history

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::{shared_state::SharedState, tattoys::index::Tattoyer};

use super::simulation::Simulation;

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
        })
    }

    /// One frame of the tattoy
    #[allow(clippy::float_arithmetic)]
    fn tick(&mut self) -> Result<termwiz::surface::Surface> {
        let mut surface = crate::surface::Surface::new(self.width, self.height);
        let pty = self
            .state
            .shadow_tty
            .read()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        let cursor = pty.cursor_position();
        drop(pty);

        self.simulation.tick(cursor);

        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            clippy::as_conversions
        )]
        for particle in &mut self.simulation.particles {
            let position = particle.position_unscaled();
            surface.add_pixel(
                (position.x) as usize,
                (position.y) as usize,
                particle.colour,
            )?;
        }
        Ok(surface.surface)
    }
}
