//! A particle of smoke

use std::f32::consts::PI;

use glam::Vec2;

/// "Size", or "area of influence" of a particle
pub const PARTICLE_SIZE: f32 = 16.0;
/// The grvitational constant
const GRAVITY: Vec2 = Vec2::from_array([0.0, -9.81]);

/// The radius around a particle within which its density is calculated
const DENSITY_RADIUS: f32 = PARTICLE_SIZE * PARTICLE_SIZE;
/// Mass of the particle
const MASS: f32 = 2.5;
/// ?
const GAS_CONST: f32 = 2000.0;
/// ?
const REST_DENSITY: f32 = 300.0;
/// Viscosity of the gas/liquid
const VISCOSITY: f32 = 50.0; // Liquid default was 200.0

/// Timestep, therefore how detailed to make the simulation
const TIMESTEP: f32 = 0.0007;
/// How quickly to bring a particle's velocity back into bounds
const BOUND_DAMPING: f32 = -0.5;

// Manually write out powers since `f32::powf` is not yet a `const fn`
/// ?
static POLY6: f32 = 4.0
    / (PI
        * PARTICLE_SIZE
        * PARTICLE_SIZE
        * PARTICLE_SIZE
        * PARTICLE_SIZE
        * PARTICLE_SIZE
        * PARTICLE_SIZE
        * PARTICLE_SIZE
        * PARTICLE_SIZE);
/// ?
static SPIKY_GRAD: f32 =
    -10.0 / (PI * PARTICLE_SIZE * PARTICLE_SIZE * PARTICLE_SIZE * PARTICLE_SIZE * PARTICLE_SIZE);
/// ?
static VISC_LAP: f32 =
    40.0 / (PI * PARTICLE_SIZE * PARTICLE_SIZE * PARTICLE_SIZE * PARTICLE_SIZE * PARTICLE_SIZE);

/// Colour of a gas particle
type Colour = (f32, f32, f32);

/// A single particle of gas
#[derive(Clone)]
#[non_exhaustive]
pub struct Particle {
    /// Position of a particle
    pub created_at: std::time::Instant,
    /// Position of a particle
    pub position: Vec2,
    /// Velocity of a  particle
    pub velocity: Vec2,
    /// Force of a  particle
    pub force: Vec2,
    /// Density of the particle
    pub density: f32,
    /// Density of the particle
    pub pressure: f32,
    /// Colour of a gas particle
    pub colour: Colour,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::indexing_slicing
)]
impl Particle {
    /// Add the density generated by another particle
    pub fn accumulate_density(&mut self, other: &Self) {
        let delta = other.position - self.position;
        let distance_squared = delta.length_squared();
        if distance_squared >= DENSITY_RADIUS {
            return;
        }

        self.density += MASS * POLY6 * f32::powf(DENSITY_RADIUS - distance_squared, 3.0);
    }

    /// Calculate forces on the particle
    pub fn calculate_forces(&mut self, other: &Self) -> Option<(Vec2, Vec2)> {
        let delta = other.position - self.position;
        let distance = delta.length();
        if distance >= PARTICLE_SIZE {
            return None;
        }

        let force_from_pressure = -delta.normalize() * MASS * (self.pressure + other.pressure)
            / (2.0 * other.density)
            * SPIKY_GRAD
            * f32::powf(PARTICLE_SIZE - distance, 3.0);

        let force_from_viscosity = VISCOSITY * MASS * (other.velocity - self.velocity)
            / other.density
            * VISC_LAP
            * (PARTICLE_SIZE - distance);

        Some((force_from_pressure, force_from_viscosity))
    }

    /// Given the acummulated density of a particle and its neighbours, calculate its presssure
    pub fn update_pressure(&mut self) {
        self.pressure = GAS_CONST * (self.density - REST_DENSITY);
    }

    /// The force from gravity
    pub fn force_from_gravity(&mut self) -> Vec2 {
        GRAVITY * MASS / self.density
    }

    /// Apply the forces to the velocity and then actually move the particle
    pub fn integrate(&mut self) {
        self.velocity += TIMESTEP * self.force / self.density;
        self.position += TIMESTEP * self.velocity;
    }

    /// Keep the particles in the container
    pub fn boundaries(&mut self, width: f32, height: f32) {
        if self.position.x < 0.0 {
            self.velocity.x *= BOUND_DAMPING;
            self.position.x = 0.0;
        }
        if self.position.x > width - 1.0 {
            self.velocity.x *= BOUND_DAMPING;
            self.position.x = width - 1.0;
        }
        if self.position.y < 0.0 {
            self.velocity.y *= BOUND_DAMPING;
            self.position.y = 0.0;
        }
        if self.position.y > height - 1.0 {
            self.velocity.y *= BOUND_DAMPING;
            self.position.y = height - 1.0;
        }
    }
}
