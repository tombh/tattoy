//! Run and output a command in the background.

use std::sync::Arc;

use color_eyre::eyre::Result;

/// User-configurable settings for the background command.
#[derive(serde::Deserialize, Debug, Clone)]
pub(crate) struct Config {
    /// Enable/disable the script
    pub enabled: bool,
    /// The transparency of the command output layer
    pub opacity: f32,
    /// The layer of the compositor on which the command output is rendered.
    pub layer: i16,
    /// The command to run.
    command: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            opacity: 0.75,
            layer: -8,
            command: vec!["echo".to_owned(), "No command provided".to_owned()],
        }
    }
}

/// `BGCommand`
pub struct BGCommand {
    /// The base Tattoy struct
    tattoy: super::tattoyer::Tattoyer,
    /// An instance of our headless terminal.
    shadow_terminal: shadow_terminal::active_terminal::ActiveTerminal,
    /// The user's terminal's colour palette in true colour values.
    palette: crate::palette::converter::Palette,
}

impl BGCommand {
    /// Instatiate
    async fn new(
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Self {
        let tattoy = super::tattoyer::Tattoyer::new(
            "bg_command".to_owned(),
            Arc::clone(&state),
            state.config.read().await.bg_command.layer,
            state.config.read().await.bg_command.opacity,
            output_channel,
        )
        .await;

        let command = state.config.read().await.bg_command.command.clone();
        let _span = tracing::span!(tracing::Level::TRACE, "BGCommand").entered();
        let shadow_terminal = shadow_terminal::active_terminal::ActiveTerminal::start(
            shadow_terminal::shadow_terminal::Config {
                width: tattoy.width,
                height: tattoy.height,
                command: command.iter().map(std::convert::Into::into).collect(),
                scrollback_size: 100,
                scrollback_step: 1,
            },
        );

        tracing::debug!("Started BG Command for: `{}`", command.join(" "));
        Self {
            tattoy,
            shadow_terminal,
            palette,
        }
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Result<()> {
        let mut commander = Self::new(output, state, palette).await;
        let mut protocol = protocol_tx.subscribe();

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                Some(pty_output) = commander.shadow_terminal.surface_output_rx.recv() => {
                    commander.handle_bg_command_output(pty_output).await?;
                }
                Ok(message) = protocol.recv() => {
                    commander.handle_protocol_message(&message)?;
                    if matches!(message, crate::run::Protocol::End) {
                        break;
                    }
                    commander.tattoy.handle_common_protocol_messages(message)?;
                }
            }
        }

        Ok(())
    }

    /// Handle output from the headless terminal where the background command was spawned.
    async fn handle_bg_command_output(
        &mut self,
        mut output: shadow_terminal::output::Output,
    ) -> Result<()> {
        self.palette.convert_cells_to_true_colour(&mut output);
        self.tattoy.initialise_surface();
        self.tattoy.opacity = self.tattoy.state.config.read().await.bg_command.opacity;
        self.tattoy.layer = self.tattoy.state.config.read().await.bg_command.layer;

        #[expect(
            clippy::collapsible_match,
            clippy::single_match,
            clippy::wildcard_enum_match_arm,
            reason = "There's some deep types going on and I think it's easier to read"
        )]
        match output {
            shadow_terminal::output::Output::Diff(surface_diff) => match surface_diff {
                shadow_terminal::output::SurfaceDiff::Screen(screen_diff) => {
                    self.tattoy.surface.surface.add_changes(screen_diff.changes);
                }
                _ => (),
            },
            shadow_terminal::output::Output::Complete(complete_surface) => match complete_surface {
                shadow_terminal::output::CompleteSurface::Screen(complete_screen) => {
                    self.tattoy.surface.surface = complete_screen.surface;
                }
                _ => (),
            },
            _ => (),
        }

        self.tattoy.send_output().await?;

        Ok(())
    }

    /// Custom behaviour for protocol messages.
    fn handle_protocol_message(&self, message: &crate::run::Protocol) -> Result<()> {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We're ready to add handlers for other messages"
        )]
        match message {
            crate::run::Protocol::Resize { width, height } => {
                self.shadow_terminal.resize(*width, *height)?;
            }
            crate::run::Protocol::End => {
                self.shadow_terminal.kill()?;
            }
            _ => (),
        }

        Ok(())
    }
}
