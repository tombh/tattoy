//! All of the user config for Tattoy.

use color_eyre::eyre::ContextCompat as _;
use color_eyre::eyre::Result;

/// Managing user config.
pub(crate) struct Config;

impl Config {
    /// Get the stable location of Tattoy's config directory on the user's system.
    pub fn directory() -> Result<std::path::PathBuf> {
        let path = dirs::config_dir()
            .context("Couldn't get standard config filder")?
            .join("tattoy");

        std::fs::create_dir_all(path.clone())?;

        Ok(path)
    }

    /// Get a temporary file handle.
    pub fn temporary_file(name: &str) -> Result<std::path::PathBuf> {
        let file = tempfile::Builder::new()
            .suffix(&format!("tattoy-{name}"))
            .keep(true)
            .tempfile()?;

        Ok(file.path().into())
    }
}
