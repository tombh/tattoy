//! All of the user config for Tattoy.

use color_eyre::eyre::ContextCompat as _;
use color_eyre::eyre::Result;
use notify::Watcher as _;

/// A copy of the default config file. It gets copied to the user's config folder the first time
/// they start Tattoy.
static DEFAULT_CONFIG: &str = include_str!("../../default_config.toml");

/// Bundle an example shader with Tattoy.
static EXAMPLE_SHADER: &str = include_str!("../tattoys/shaders/point_lights.glsl");

/// The name of the directory where shader files are kept.
const SHADER_DIRECTORY_NAME: &str = "shaders";

/// The valid log levels. Based on our `tracing` crate.
#[derive(serde::Serialize, serde::Deserialize, clap::ValueEnum, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LogLevel {
    /// Error
    Error,
    /// Warnings
    Warn,
    /// Info
    Info,
    /// Debug
    Debug,
    /// Trace
    Trace,
    /// No logging
    Off,
}

/// Managing user config.
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Are the unsafe methods on the `f32`s?"
)]
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct Config {
    /// The `TERM` value to send to the underlying PTY. This may not actually be needed, but
    /// currently "TERM=xterm-256color" is fixing some bugs for me.
    pub term: String,
    /// The command to run in the underlying PTY, defaults to the users shell as dedfined in the
    /// `SHELL` env variable.
    pub command: String,
    /// The maximum log level
    pub log_level: LogLevel,
    /// The location of the log file.
    pub log_path: std::path::PathBuf,
    /// Keybindings
    pub keybindings: super::input::KeybindingsRaw,
    /// Target frame rate
    pub frame_rate: u32,
    /// Colour grading
    pub color: Color,
    /// Plugins config
    pub plugins: Vec<crate::tattoys::plugins::Config>,
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
            Err(_) => {
                if std::env::var("PSModulePath").is_ok() {
                    "powershell".into()
                } else {
                    "/usr/bin/bash".into()
                }
            }
        };

        let log_directory = match dirs::state_dir() {
            Some(directory) => directory,
            None => std::path::PathBuf::new().join("./"),
        };
        let log_path = log_directory.join("tattoy").join("tattoy.log");

        Self {
            term: "xterm-256color".to_owned(),
            command,
            log_level: LogLevel::Off,
            log_path,
            frame_rate: 30,
            keybindings: super::input::KeybindingsRaw::new(),
            color: Color::default(),
            plugins: Vec::default(),
            smokey_cursor: crate::tattoys::smokey_cursor::config::Config::default(),
            minimap: crate::tattoys::minimap::Config::default(),
            shader: crate::tattoys::shaders::main::Config::default(),
        }
    }
}

/// Final colour grading for the whole terminal render.
#[derive(serde::Deserialize, Debug, Clone)]
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
        let main_config_file = state.main_config_file.read().await.clone();
        directory.join(main_config_file)
    }

    /// Load the main config
    pub async fn load(state: &std::sync::Arc<crate::shared_state::SharedState>) -> Result<Self> {
        let config_path = Self::main_config_path(state).await;
        let config_file_name = config_path
            .file_name()
            .context("Couldn't get file name from config path")?;
        let is_default_config = config_file_name == crate::cli_args::DEFAULT_CONFIG_FILE_NAME;
        if is_default_config && !config_path.exists() {
            std::fs::write(config_path.clone(), DEFAULT_CONFIG)?;

            let shader_path = Self::directory(state)
                .await
                .join(SHADER_DIRECTORY_NAME)
                .join("point_lights.glsl");
            std::fs::write(shader_path, EXAMPLE_SHADER)?;
        }

        tracing::info!("(Re)loading the main Tattoy config from: {config_path:?}");
        let result = std::fs::read_to_string(config_path.clone());
        match result {
            Ok(data) => {
                tracing::trace!("Using config file:\n{data}");
                let config = toml::from_str::<Self>(&data)?;
                Self::load_keybindings(state, &config).await?;
                Ok(config)
            }
            Err(err) => {
                tracing::error!("Loading config: {err:?}");
                color_eyre::eyre::bail!(
                    "Couldn't load config at {config_path:?}: {}",
                    err.to_string()
                );
            }
        }
    }

    /// Parse the shipped default config.
    fn parse_default_config() -> Result<Self> {
        Ok(toml::from_str::<Self>(DEFAULT_CONFIG)?)
    }

    /// Load the main config
    pub async fn load_config_into_shared_state(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<Self> {
        let mut config_state = state.config.write().await;
        let new_config = Self::load(state).await?;
        *config_state = new_config.clone();
        drop(config_state);

        Ok(new_config)
    }

    /// Load all user keybindings.
    #[expect(clippy::iter_over_hash_type, reason = "The ordering doesn't matter")]
    async fn load_keybindings(
        state: &std::sync::Arc<crate::shared_state::SharedState>,
        user_config: &Self,
    ) -> Result<()> {
        let mut keybindings = crate::config::input::KeybindingsAsEvents::new();

        let defaults = Self::parse_default_config()?;
        for (action, binding_config) in defaults.keybindings.clone() {
            let key_event: termwiz::input::KeyEvent = binding_config.try_into()?;
            keybindings.insert(action.clone(), key_event.clone());
        }

        tracing::trace!("Loading user-defined keybindings...");
        for (action, binding_config) in user_config.keybindings.clone() {
            tracing::trace!("Keybinding found for '{action:?}': {binding_config:?}");
            let key_event: termwiz::input::KeyEvent = binding_config.try_into()?;
            keybindings
                .entry(action.clone())
                .or_insert_with(|| key_event.clone());
            tracing::debug!("Keybinding parsed for '{action:?}': {key_event:?}");
        }

        *state.keybindings.write().await = keybindings;
        Ok(())
    }

    /// Watch the config file for any changes and then automatically update the shared state with
    /// the contents of the new config file.
    pub fn watch(
        state: std::sync::Arc<crate::shared_state::SharedState>,
        tattoy_protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> tokio::task::JoinHandle<Result<()>> {
        tokio::spawn(async move {
            let path = Self::directory(&state).await;
            tracing::debug!("Watching config ({path:?}) for changes.");

            let (config_file_change_tx, mut config_file_change_rx) = tokio::sync::mpsc::channel(1);
            let mut tattoy_protocol_rx = tattoy_protocol_tx.subscribe();

            let mut watcher = notify::RecommendedWatcher::new(
                move |res| {
                    let result = config_file_change_tx.blocking_send(res);
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
                    Some(result) = config_file_change_rx.recv() => Self::handle_file_change_event(result, &state, &tattoy_protocol_tx).await,
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
        file_event_result: std::result::Result<notify::Event, notify::Error>,
        state: &std::sync::Arc<crate::shared_state::SharedState>,
        tattoy_protocol_tx: &tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) {
        let Ok(event) = file_event_result else {
            tracing::error!("Receving config file watcher event: {file_event_result:?}");
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

        let maybe_new_config = Self::load_config_into_shared_state(state).await;

        match maybe_new_config {
            Ok(config) => {
                let protocol_send_result =
                    tattoy_protocol_tx.send(crate::run::Protocol::Config(config));
                if let Err(error) = protocol_send_result {
                    tracing::error!("Couldn't send config update on protocol channgel: {error:?}");
                }
            }
            Err(error) => {
                tracing::error!("Updating shared state after config file change: {error:?}");
            }
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
