//! Here we store all the shared data that the app, particularly tattoys, might use.
//! Access is mediated with locks to support asynchronicity

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::renderer::Renderer;

/// The size of the user's terminal
type TTYSize = (usize, usize);

/// All the shared data the the app uses
#[derive(Default)]
#[non_exhaustive]
pub struct SharedState {
    /// Just the size of the user's terminal. All the tattoys and shadow TTY should follow this
    pub tty_size: std::sync::RwLock<TTYSize>,
    /// This is the user's conventional terminal just maintained in a virtual, "shadow" terminal
    pub shadow_tty: std::sync::RwLock<termwiz::surface::Surface>,
}

impl SharedState {
    /// Initialise the shared state
    pub fn init() -> Result<Arc<Self>> {
        let tty_size = Renderer::get_users_tty_size()?;
        let state = Self::default();
        let mut shared_state_tty_size = state
            .tty_size
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        shared_state_tty_size.0 = tty_size.cols;
        shared_state_tty_size.1 = tty_size.rows;
        drop(shared_state_tty_size);
        Ok(Arc::new(state))
    }

    /// Get a read lock and return the current TTY size
    pub fn get_tty_size(&self) -> Result<TTYSize> {
        let tty_size = self
            .tty_size
            .read()
            .map_err(|err| color_eyre::eyre::eyre!("{err:?}"))?;
        Ok((tty_size.0, tty_size.1))
    }
}
