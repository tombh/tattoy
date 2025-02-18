//! Map all tattoys to CLI-callable strings

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::shared_state::SharedState;

use super::{random_walker::RandomWalker, smokey_cursor::main::SmokeyCursor};

/// The trait that all tattoys must follow
#[async_trait::async_trait]
pub(crate) trait Tattoyer {
    /// The unique ID for this tattoy.
    fn id() -> String
    where
        Self: Sized;

    /// Instantiate
    async fn new(state: Arc<SharedState>) -> Result<Self>
    where
        Self: Sized;

    /// Tell the tattoy that the user's terminal has changed size.
    fn set_tty_size(&mut self, width: u16, height: u16);

    /// Run one frame of the tattoy
    async fn tick(&mut self) -> Result<Option<crate::surface::Surface>>;
}

/// How to map from a CLI arg to a tattoy implementation
pub(crate) async fn create_instance(
    tattoy: &str,
    state: &Arc<SharedState>,
) -> Result<Box<dyn Tattoyer + Send>> {
    let state_clone = Arc::clone(state);
    match tattoy {
        "random_walker" => Ok(Box::new(RandomWalker::new(state_clone).await?)),
        "smokey_cursor" => Ok(Box::new(SmokeyCursor::new(state_clone).await?)),
        "scrollbar" => Ok(Box::new(
            super::scrollbar::Scrollbar::new(state_clone).await?,
        )),
        _ => Err(color_eyre::eyre::eyre!(
            "The tattoy, `{tattoy}` was not found"
        )),
    }
}
