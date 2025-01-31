//! Here we store all the shared data that the app, particularly tattoys, might use.
//! Access is mediated with locks to support asynchronicity

use std::sync::Arc;

use color_eyre::eyre::Result;

use crate::renderer::Renderer;

/// The size of the user's terminal
#[derive(Default, Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "It's very unlikely that this is going to have any more fields added to it"
)]
pub struct TTYSize {
    /// Width of the TTY
    pub width: u16,
    /// Height of the TTY
    pub height: u16,
}

/// All the shared data the app uses
#[derive(Default)]
#[non_exhaustive]
pub(crate) struct SharedState {
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
        state.set_tty_size(tty_size.cols.try_into()?, tty_size.rows.try_into()?)?;
        Ok(Arc::new(state))
    }

    /// Get a read lock and return the current TTY size
    pub fn get_tty_size(&self) -> Result<TTYSize> {
        let tty_size = self
            .tty_size
            .read()
            .map_err(|err| color_eyre::eyre::eyre!("{err:?}"))?;
        Ok(TTYSize {
            width: tty_size.width,
            height: tty_size.height,
        })
    }

    /// Get a write lock and set the a new TTY size
    pub fn set_tty_size(&self, width: u16, height: u16) -> Result<()> {
        let mut shared_state_tty_size = self
            .tty_size
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err:?}"))?;
        shared_state_tty_size.width = width;
        shared_state_tty_size.height = height;
        drop(shared_state_tty_size);
        Ok(())
    }
}
