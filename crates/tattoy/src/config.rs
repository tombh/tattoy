//! All of the user config for Tattoy.

use color_eyre::eyre::ContextCompat as _;
use color_eyre::eyre::Result;
use notify::Watcher as _;

/// A copy of the default config file. It gets copied to the user's config folder the first time
/// they start Tattoy.
static DEFAULT_CONFIG: &str = include_str!("../default_config.toml");

/// Bundle an example shader with Tattoy.
static EXAMPLE_SHADER: &str = include_str!("tattoys/shaders/point_lights.glsl");

/// The name of the directory where shader files are kept.
const SHADER_DIRECTORY_NAME: &str = "shaders";

/// Managing user config.
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Are the unsafe methods on the `f32`s?"
)]
#[derive(serde::Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    /// The `TERM` value to send to the underlying PTY. This may not actually be needed, but
    /// currently "TERM=xterm-256color" is fixing some bugs for me.
    pub term: String,
    /// The command to run in the underlying PTY, defaults to the users shell as dedfined in the
    /// `SHELL` env variable.
    pub command: String,
    /// The maximum log level
    pub log_level: String,
    /// The location of the log file.
    pub log_path: std::path::PathBuf,
    /// Colour grading
    pub color: Color,
    /// The smokey particles cursor
    pub smokey_cursor: crate::tattoys::smokey_cursor::config::Config,
    /// The minimap
    pub minimap: crate::tattoys::minimap::Config,
    /// The shaders
    pub shader: crate::tattoys::shaders::main::Config,
}

impl Default for Config {
    fn default() -> Self {
        let command = match std::env::var("SHELL") {
            Ok(command) => command,
            Err(_) => "/usr/bin/bash".into(),
        };

        let log_directory = match dirs::state_dir() {
            Some(directory) => directory,
            None => std::path::PathBuf::new().join("./"),
        };
        let log_path = log_directory.join("tattoy").join("tattoy.log");

        Self {
            term: "xterm-256color".to_owned(),
            command,
            log_level: "none".into(),
            log_path,
            color: Color::default(),
            smokey_cursor: crate::tattoys::smokey_cursor::config::Config::default(),
            minimap: crate::tattoys::minimap::Config::default(),
            shader: crate::tattoys::shaders::main::Config::default(),
        }
    }
}

/// Final colour grading for the whole terminal render.
#[derive(serde::Deserialize)]
pub(crate) struct Color {
    /// Saturation
    pub saturation: f32,
    /// Brightness
    pub brightness: f32,
    /// Hue
    pub hue: f32,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            saturation: 0.0,
            brightness: 0.0,
            hue: 0.0,
        }
    }
}

impl Config {
    /// Canonical path to the config directory.
    pub async fn directory(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> std::path::PathBuf {
        state.config_path.read().await.clone()
    }

    /// Get the stable location of Tattoy's config directory on the user's system.
    pub fn default_directory() -> Result<std::path::PathBuf> {
        Ok(dirs::config_dir()
            .context("Couldn't get standard config directory")?
            .join("tattoy"))
    }

    /// Figure out where our config is being stored, and create the directory if needed.
    pub async fn setup_directory(
        maybe_custom_path: Option<std::path::PathBuf>,
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<()> {
        let path = match maybe_custom_path {
            None => Self::default_directory()?,
            Some(path_string) => std::path::PathBuf::new().join(path_string),
        };

        std::fs::create_dir_all(path.clone())?;

        let shaders_directory = path.join(SHADER_DIRECTORY_NAME);
        std::fs::create_dir_all(shaders_directory)?;

        *state.config_path.write().await = path;

        Ok(())
    }

    /// Canonical path to the main config file.
    pub async fn main_config_path(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> std::path::PathBuf {
        let directory = Self::directory(state).await;
        directory.join("tattoy.toml")
    }

    /// Load the main config
    pub async fn load(state: &std::sync::Arc<crate::shared_state::SharedState>) -> Result<Self> {
        let config_path = Self::main_config_path(state).await;
        if !config_path.exists() {
            tracing::info!("Copying default config to: {config_path:?}");
            std::fs::write(config_path.clone(), DEFAULT_CONFIG)?;

            let shader_path = Self::directory(state)
                .await
                .join(SHADER_DIRECTORY_NAME)
                .join("point_lights.glsl");
            tracing::info!("Copying example shader to: {shader_path:?}");
            std::fs::write(shader_path, EXAMPLE_SHADER)?;
        }

        tracing::info!("(Re)loading the main Tattoy config from: {config_path:?}");
        let data = std::fs::read_to_string(config_path)?;
        let config = toml::from_str::<Self>(&data)?;
        Ok(config)
    }

    /// Load the main config
    pub async fn update_shared_state(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<()> {
        let mut config_state = state.config.write().await;
        *config_state = Self::load(state).await?;
        drop(config_state);

        Ok(())
    }

    /// Watch the config file for any changes and then automatically update the shared state with
    /// the contents of the new config file.
    pub fn watch(
        state: std::sync::Arc<crate::shared_state::SharedState>,
        tattoy_protocol: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> tokio::task::JoinHandle<Result<()>> {
        tokio::spawn(async move {
            let path = Self::directory(&state).await;
            tracing::debug!("Watching config ({path:?}) for changes.");

            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            let mut tattoy_protocol_rx = tattoy_protocol.subscribe();

            let mut watcher = notify::RecommendedWatcher::new(
                move |res| {
                    let result = tx.blocking_send(res);
                    if let Err(error) = result {
                        tracing::error!("Sending config file watcher notification: {error:?}");
                    }
                },
                notify::Config::default(),
            )?;
            watcher.watch(&path, notify::RecursiveMode::NonRecursive)?;

            #[expect(
                clippy::integer_division_remainder_used,
                reason = "This is caused by the `tokio::select!`"
            )]
            loop {
                tokio::select! {
                    Some(result) = rx.recv() => Self::handle_file_change_event(result, &state).await,
                    Ok(message) = tattoy_protocol_rx.recv() => {
                        if matches!(message, crate::run::Protocol::End) {
                            break;
                        }
                    }
                }
            }

            tracing::debug!("Leaving config watcher loop");
            Ok(())
        })
    }

    /// Handle an event from the config file watcher. Should normally be a notification that the
    /// config file has changed.
    async fn handle_file_change_event(
        result: std::result::Result<notify::Event, notify::Error>,
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) {
        let Ok(event) = result else {
            tracing::error!("Receving config file watcher event: {result:?}");
            return;
        };

        if !matches!(
            event,
            notify::Event {
                kind: notify::event::EventKind::Modify(_),
                ..
            }
        ) {
            return;
        }
        tracing::debug!("Config file change detected, updating shared state.");

        let result_for_update = Self::update_shared_state(state).await;

        if let Err(error) = result_for_update {
            tracing::error!("Updating shared state after config file change: {error:?}");
        }
    }

    /// Get a temporary file handle.
    pub fn temporary_file(name: &str) -> Result<std::path::PathBuf> {
        let file = tempfile::Builder::new()
            .suffix(&format!("tattoy-{name}"))
            .keep(true)
            .tempfile()?;

        Ok(file.path().into())
    }

    /// Load the terminal's palette as true colour values.
    pub async fn load_palette(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<Option<crate::palette::converter::Palette>> {
        let path = crate::palette::parser::Parser::palette_config_path(state).await;
        if path.exists() {
            tracing::info!("Loading the terminal palette's true colours from config");
            let data = std::fs::read_to_string(path)?;
            let map = toml::from_str::<crate::palette::converter::PaletteHashMap>(&data)?;
            let palette = crate::palette::converter::Palette { map };
            Ok(Some(palette))
        } else {
            tracing::debug!("Terminal palette colours config file not found in config directory");
            Ok(None)
        }
    }
}
