//! Map all tattoys to CLI-callable strings

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::shared_state::SharedState;

use super::{random_walker::RandomWalker, smokey_cursor::SmokeyCursor};

/// The trait that all tattoys must follow
// #[enum_dispatch::enum_dispatch]
pub trait Tattoyer {
    ///
    fn new(state: Arc<SharedState>) -> Result<Self>
    where
        Self: Sized;

    /// Run one frame of the tattoy
    fn tick(&mut self) -> Result<termwiz::surface::Surface>;
}

/// How to map from a CLI arg to a tattoy implementation
pub fn create_instance(tattoy: &str, state: &Arc<SharedState>) -> Result<Box<dyn Tattoyer + Send>> {
    let state_clone = Arc::clone(state);
    match tattoy {
        "random_walker" => Ok(Box::new(RandomWalker::new(state_clone)?)),
        "smokey_cursor" => Ok(Box::new(SmokeyCursor::new(state_clone)?)),
        _ => Err(color_eyre::eyre::eyre!(
            "The tattoy, `{tattoy}` was not found"
        )),
    }
}
