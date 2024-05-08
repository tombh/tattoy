//! The cursor gives off a gas that floats up and interacts with the history

use std::sync::Arc;

use color_eyre::eyre::Result;
use rand::Rng;

use crate::shared_state::SharedState;

use super::index::Tattoyer;

/// Position of a gas particle
type Position = (f32, f32);
/// Colour of a gas particle
type Colour = (f32, f32, f32);

/// A single particle of gas
#[derive(Default, PartialEq)]
pub struct GasParticle {
    /// Position of a gas particle
    position: Position,
    /// Colour of a gas particle
    colour: Colour,
}

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
    particles: Vec<GasParticle>,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic
)]
impl Tattoyer for SmokeyCursor {
    ///
    fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = state.get_tty_size()?;
        let width = tty_size.0;
        let height = tty_size.1;

        Ok(Self {
            width,
            height,
            state,
            particles: vec![],
        })
    }

    ///
    fn tick(&mut self) -> Result<termwiz::surface::Surface> {
        let mut surface = crate::surface::Surface::new(self.width, self.height);
        let pty = self
            .state
            .shadow_tty
            .read()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        let cursor = pty.cursor_position();
        drop(pty);

        let rng = rand::thread_rng().gen_range(1_i32..=10_i32);
        if rng == 1_i32 {
            self.particles.push(GasParticle {
                position: (cursor.0 as f32, (cursor.1 * 2) as f32),
                colour: (0.2, 0.2, 0.2),
            });
        }

        for particle in &mut self.particles {
            // let x_position = particle.position.0 as usize;
            particle.position.1 -= 0.5;
            surface.add_pixel(
                particle.position.0 as usize,
                particle.position.1 as usize,
                particle.colour.0,
                particle.colour.1,
                particle.colour.2,
            );
        }
        Ok(surface.surface)
    }
}
