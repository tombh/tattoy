//! Here we store all the shared data that the app, particularly tattoys, might use.
//! Access is mediated with locks to support asynchronicity

use std::sync::Arc;

use color_eyre::eyre::Result;
use tokio::sync::RwLock;

use crate::renderer::Renderer;

/// The size of the user's terminal
#[derive(Default, Debug, Copy, Clone)]
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
#[non_exhaustive]
pub(crate) struct SharedState {
    /// The channel on which all Tattoy protocol messages are sent.
    pub protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    /// List of asynchronous systems that have initialsed.
    pub initialised_systems: tokio::sync::RwLock<Vec<String>>,
    /// Location of the config directory.
    pub config_path: tokio::sync::RwLock<std::path::PathBuf>,
    /// Name of the main config file.
    pub main_config_file: tokio::sync::RwLock<std::path::PathBuf>,
    /// User config
    pub config: tokio::sync::RwLock<crate::config::main::Config>,
    /// All the user-configured keybindings.
    pub keybindings: tokio::sync::RwLock<crate::config::input::KeybindingsAsEvents>,
    /// Just the size of the user's terminal. All the tattoys and shadow TTY should follow this
    pub tty_size: tokio::sync::RwLock<TTYSize>,
    /// This is a view onto the active screen of the shadow terminal. It's what you would see if
    /// you had some kind of VNC viewer, let's say.
    pub shadow_tty_screen: tokio::sync::RwLock<termwiz::surface::Surface>,
    // TODO: rename to `shadow_primary_screen`
    /// This is the entire scrollback history of the shadow terminal.
    pub shadow_tty_scrollback: tokio::sync::RwLock<shadow_terminal::output::CompleteScrollback>,
    /// Is the user scrolling the scrollback?
    pub is_scrolling: tokio::sync::RwLock<bool>,
    /// Is the underlying shadow terminal in the so-called alternate screen state?
    ///
    /// * A terminal's behaviour alters slightly when it is in this state. Most notably scrolling
    ///   should be sent directly to the PTY and not used to scroll the terminal's history.
    /// * Note that in order to run Tattoy, the _end user's_ terminal is perpetually in the alternate
    ///   screen state. So we have to emulate and proxy actual alternate screen behaviour down to the
    ///   shadow terminal.
    pub is_alternate_screen: tokio::sync::RwLock<bool>,
    /// A counter for every change to the underlying PTY output. Useful for triggering behaviour on
    /// screen state changes.
    pub pty_sequence: tokio::sync::RwLock<usize>,
    /// Is the application logging?
    pub is_logging: tokio::sync::RwLock<bool>,
    /// Is Tattoy rendering anything to the terminal?
    pub is_rendering_enabled: tokio::sync::RwLock<bool>,
}

impl SharedState {
    /// Initialise the shared state
    pub async fn init(
        width: u16,
        height: u16,
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<Arc<Self>> {
        let state = Self {
            protocol_tx,
            initialised_systems: RwLock::default(),
            config_path: RwLock::default(),
            main_config_file: RwLock::default(),
            config: RwLock::default(),
            keybindings: RwLock::default(),
            tty_size: RwLock::new(TTYSize { width, height }),
            shadow_tty_screen: RwLock::default(),
            shadow_tty_scrollback: RwLock::default(),
            is_scrolling: RwLock::default(),
            is_alternate_screen: RwLock::default(),
            pty_sequence: RwLock::default(),
            is_logging: RwLock::default(),
            is_rendering_enabled: RwLock::default(),
        };
        *state.is_rendering_enabled.write().await = true;

        state.set_tty_size(width, height).await;
        Ok(Arc::new(state))
    }

    /// Convenience method to initialise the renderer with the user's terminal's size.
    pub async fn init_with_users_tty_size(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<Arc<Self>> {
        let tty_size = Renderer::get_users_tty_size()?;
        Self::init(
            tty_size.cols.try_into()?,
            tty_size.rows.try_into()?,
            protocol_tx,
        )
        .await
    }

    /// A convience function for sending a notification.
    pub async fn send_notification(
        &self,
        title: &str,
        level: crate::tattoys::notifications::message::Level,
        mut maybe_body: Option<String>,
        include_logs_message: bool,
    ) {
        if let Some(mut body) = maybe_body.clone() {
            if include_logs_message {
                use crate::tattoys::notifications::main::Notifications;
                let logpath = self.config.read().await.log_path.clone();
                let is_logging = *self.is_logging.read().await;
                let logs_help_text = Notifications::logs_help_text(is_logging, &logpath);
                body = format!("{body}\n\n{logs_help_text}");
                maybe_body = Some(body);
            }
        }

        self.protocol_tx
            .send(crate::tattoys::notifications::message::Message::make(
                title, level, maybe_body,
            ))
            .unwrap_or_else(|send_error| {
                tracing::error!("Error sending notification: {send_error:?}");
                0
            });
    }

    /// Get a read lock and return the current TTY size
    pub async fn get_tty_size(&self) -> TTYSize {
        let tty_size = self.tty_size.read().await;
        *tty_size
    }

    /// Get a write lock and set the a new TTY size
    pub async fn set_tty_size(&self, width: u16, height: u16) {
        let mut tty_size = self.tty_size.write().await;
        *tty_size = TTYSize { width, height };
    }

    /// Get a read lock and return whether the user is currently scrolling.
    pub async fn get_is_scrolling(&self) -> bool {
        let is_scrolling = self.is_scrolling.read().await;
        *is_scrolling
    }

    /// Get a write lock and set the scrolling state.
    pub async fn set_is_scrolling(&self, value: bool) {
        let mut is_scrolling = self.is_scrolling.write().await;
        *is_scrolling = value;
    }

    /// Get a read lock and return whether the alternate screen is currently active.
    pub async fn get_is_alternate_screen(&self) -> bool {
        let is_alternate_screen = self.is_alternate_screen.read().await;
        *is_alternate_screen
    }

    /// Get a write lock and set whether the alternate screen is active or not.
    pub async fn set_is_alternate_screen(&self, value: bool) {
        let mut is_alternate_screen = self.is_alternate_screen.write().await;
        *is_alternate_screen = value;
    }
}
