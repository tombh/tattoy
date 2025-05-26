//! Functions that add and remove particles

use std::ops::Div as _;

use super::{particle::Particle, simulation::Simulation};

#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    reason = "We're just prototyping for now"
)]
impl Simulation {
    /// Add particles that represent the character from the user's current TTY. These particles are
    /// immovable, yet they still interact with the simulation.
    ///
    /// # Panics
    /// When it can't safely cast the cursor usize to i32
    pub fn add_pty_particles(
        &mut self,
        cursor: (u16, u16),
        pty: &Vec<tattoy_protocol::Cell>,
    ) -> usize {
        let scale = self.config.scale * super::particle::PARTICLE_SIZE;
        let mut count: usize = 0;

        for cell in pty {
            if !cell.character.is_whitespace() {
                let x_f32 = cell.coordinates.0 as f32;
                let y_f32 = cell.coordinates.1 as f32;

                let pty_particl1 = Particle::default_immovable(scale, x_f32, y_f32);
                self.particles.push_front(pty_particl1.clone());
                count += 1;

                let pty_particle2 = Particle::default_immovable(scale, x_f32, y_f32 + 1.0);
                self.particles.push_front(pty_particle2.clone());
                count += 1;
            }
        }

        // Surround the cursor with particles so that the cursor can interact with the other
        // particles.
        let radius: i32 = 8;
        for y in 0i32..radius {
            for x in 0i32..radius {
                let cursor_x: i32 = cursor.0.into();
                let cursor_y: i32 = cursor.1.into();
                let x_f32 = (cursor_x + x - (radius.div(2i32))) as f32;
                let y_f32 = (cursor_y * 2i32 + y - (radius.div(2i32))) as f32;
                let mut cursor_particle = Particle::default_immovable(scale, x_f32, y_f32 + 1.0);
                cursor_particle.is_immovable = false;
                self.particles.push_front(cursor_particle.clone());
                count += 1;
            }
        }
        count
    }

    /// Remove first-in particles from FILO queue
    pub fn remove_old_particles(&mut self) {
        if self.particles.len() < self.config.max_particles {
            return;
        }
        self.particles.pop_back();
    }

    /// Safely add a particle without creating "explosions"
    pub fn add_particle(&mut self, x: f32, y: f32) {
        let particle = Particle::default_movable(
            self.config.scale * super::particle::PARTICLE_SIZE,
            self.config.initial_velocity.into(),
            x,
            y,
        );
        self.particles.push_front(particle);
    }
}
