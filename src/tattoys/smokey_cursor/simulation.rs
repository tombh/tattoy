//! All the maths to do a smoke simulation
//! Heavily inspired by [mueller-sph-rs](https://github.com/lucas-schuermann/mueller-sph-rs)

use glam::Vec2;
use rand::Rng;

use super::{
    config::Config,
    particle::{Particle, PARTICLE_SIZE},
};
use crate::tattoys::utils::is_random_trigger;

/// The number of attempts allowed to try to find a safe place to add a new particle
const ATTEMPTS_TO_FIND_SAFE_PLACE: usize = 30;
/// Number of times to iterate the simulation per graphical frame
const NUMBER_OF_SIMULATION_STEPS_PER_TICK: usize = 10;
/// The number of seconds a particle can live before being removed
const MAX_AGE_OF_PARTICLE: u64 = 60;

///
#[derive(Default)]
#[non_exhaustive]
pub struct Simulation {
    /// Width of the simulation
    pub width: f32,
    /// Height of the simulation (double the rows of the TTY)
    pub height: f32,
    /// All the particles
    pub particles: Vec<Particle>,
    /// The configurable settings for the simulation
    pub config: Config,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::indexing_slicing
)]
impl Simulation {
    /// Initialise a new simulation
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let config = Config {
            initial_velocity: Vec2::new(0.01, -0.1),
            ..Default::default()
        };
        Self {
            width: width as f32 * config.scale,
            height: height as f32 * config.scale,
            particles: Vec::default(),
            config,
        }
    }

    /// A tick of a graphical frame render
    pub fn tick(&mut self, cursor: (usize, usize)) {
        if is_random_trigger(1) {
            self.add_particle(cursor.0 as f32, (cursor.1 * 2) as f32);
        }

        self.remove_old_particles();

        for _ in 0..NUMBER_OF_SIMULATION_STEPS_PER_TICK {
            self.evolve();
        }
    }

    /// Remove particles over a certain age
    pub fn remove_old_particles(&mut self) {
        self.particles.retain(|particle| {
            particle.created_at.elapsed() < std::time::Duration::from_secs(MAX_AGE_OF_PARTICLE)
        });
    }

    /// Safely add a particle without creating "explosions"
    pub fn add_particle(&mut self, mut x: f32, mut y: f32) {
        x *= self.config.scale;
        y *= self.config.scale;

        let ish_range = 0.01;
        let colour_ish = rand::thread_rng().gen_range(-ish_range..ish_range);

        if let Some((x_safe, y_safe)) = self.find_safe_place(x, y) {
            let particle = Particle {
                created_at: std::time::Instant::now(),
                scale: self.config.scale,
                position: Vec2::new(x_safe, y_safe),
                density: 1.0,
                colour: (0.15 + colour_ish, 0.15 + colour_ish, 0.15 + colour_ish),
                velocity: self.config.initial_velocity,
                force: Vec2::ZERO,
                pressure: 0.0,
            };
            self.particles.push(particle);
        }
    }

    /// Based on the requested location of the new particle find a position near it, but also a
    /// safe distance from other particles, so as not to create unrealistic "explosive" responses.
    fn find_safe_place(&self, mut x: f32, mut y: f32) -> Option<(f32, f32)> {
        let mut too_close;
        for _ in 0_usize..ATTEMPTS_TO_FIND_SAFE_PLACE {
            too_close = false;
            for particle in &self.particles {
                let delta = particle.position - Vec2::new(x, y);
                let distance = delta.length();
                if distance < PARTICLE_SIZE {
                    too_close = true;
                    x += rand::thread_rng().gen_range(-PARTICLE_SIZE..PARTICLE_SIZE);
                    y += rand::thread_rng().gen_range(-PARTICLE_SIZE..PARTICLE_SIZE);
                    break;
                }
            }
            if !too_close {
                return Some((x, y));
            }
        }

        None
    }

    /// Step through the simulation
    fn evolve(&mut self) {
        self.compute_density_pressure();
        self.compute_forces();
        self.integrate();
    }

    /// Calculate the next position of the particles
    fn integrate(&mut self) {
        for particle in &mut self.particles {
            particle.integrate();
            particle.boundaries(self.width, self.height);
        }
    }

    /// Calculate the density and pressure affecting the particles
    fn compute_density_pressure(&mut self) {
        for i in 0..self.particles.len() {
            let mut particle = self.particles[i].clone();
            particle.density = 0.0;
            for other in &self.particles {
                particle.accumulate_density(other);
            }
            particle.update_pressure();
            self.particles[i] = particle;
        }
    }

    /// Compute forces on the particles, from density, pressure and gravity
    fn compute_forces(&mut self) {
        for i in 0..self.particles.len() {
            let mut particle = self.particles[i].clone();
            let mut force_from_pressure = Vec2::ZERO;
            let mut force_from_viscosity = Vec2::ZERO;

            for other in &self.particles {
                if particle.position == other.position {
                    continue;
                }
                if let Some(forces) = particle.calculate_forces(other) {
                    force_from_pressure += forces.0;
                    force_from_viscosity += forces.1;
                }
            }
            particle.force = force_from_pressure
                + force_from_viscosity
                + particle.force_from_gravity(self.config.gravity);

            self.particles[i] = particle;
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn make_sim() -> Simulation {
        let mut sim = Simulation::new(100, 100);
        sim.config.gravity = Vec2::ZERO;
        sim.config.initial_velocity = Vec2::ZERO;
        sim.config.scale = 1.0; // So we don't have to scale/unscale
        sim
    }

    fn add_particle(sim: &mut Simulation, position: Vec2) {
        let particle = Particle {
            position,
            ..Default::default()
        };
        sim.particles.push(particle);
    }

    #[test]
    fn basic() {
        let mut sim = Simulation::new(100, 100);
        for _ in 0_usize..100 {
            sim.tick((50, 50));
        }
        assert!(sim.particles.len() > 5);
    }

    #[test]
    fn distant_particles_dont_interact() {
        let mut sim = make_sim();
        add_particle(&mut sim, Vec2::new(0.0, 0.0));
        add_particle(&mut sim, Vec2::new(99.0, 99.0));
        for _ in 0_usize..100 {
            sim.evolve();
        }
        assert_eq!(sim.particles[0].position, Vec2::new(0.0, 0.0));
        assert_eq!(sim.particles[1].position_unscaled(), Vec2::new(99.0, 99.0));
    }

    #[test]
    fn extremely_close_particles_repel() {
        let mut sim = make_sim();
        add_particle(&mut sim, Vec2::new(50.0, 50.0));
        add_particle(&mut sim, Vec2::new(55.0, 55.0));

        let distance_before = sim.particles[0]
            .position
            .distance(sim.particles[1].position);
        for _ in 0_usize..100 {
            sim.evolve();
        }
        let distance_after = sim.particles[0]
            .position
            .distance(sim.particles[1].position);

        assert!(
            distance_before < distance_after,
            "before/after: {distance_before:?}/{distance_after:?}"
        );
    }

    #[test]
    fn gravity_moves_particle() {
        let mut sim = make_sim();
        sim.config.gravity = Vec2::new(0.0, -1.0);
        add_particle(&mut sim, Vec2::new(50.0, 50.0));

        for _ in 0_usize..10 {
            sim.evolve();
        }

        let x = sim.particles[0].position.x;
        let y = sim.particles[0].position.y;
        assert!(y < 50.0, "y: {y}");
        assert!(y > 40.0, "y: {y}");
        assert_eq!(x, 50.0);
    }
}
