//! All the variables that can be configured for the simulation

/// All the config for the simulation
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// The gravitational exceleration of the system in metres per second
    pub gravity: (f32, f32),
    /// The velocity of a particle when it is first added
    pub initial_velocity: (f32, f32),
    /// How much bigger a partical is compared to a rendered pixel
    pub scale: f32,
    /// The maximum number of particles in the simulation
    pub max_particles: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gravity: (0.0, -9.81),
            initial_velocity: (0.0, 0.0),
            scale: 0.75,
            max_particles: 3000,
        }
    }
}
