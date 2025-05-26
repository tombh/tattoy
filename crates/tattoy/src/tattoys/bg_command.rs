//! Run and output a command in the background.

use std::sync::Arc;

use color_eyre::eyre::{ContextCompat as _, Result};

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
    /// Whether the command is expected to exit or not.
    expect_exit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            opacity: 0.75,
            layer: -8,
            command: vec!["echo".to_owned(), "No command provided".to_owned()],
            expect_exit: false,
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
    /// The command to run
    command: Vec<String>,
}

impl BGCommand {
    /// Instatiate
    async fn new(
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: &std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Self {
        let tattoy = super::tattoyer::Tattoyer::new(
            "bg_command".to_owned(),
            Arc::clone(state),
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
            command,
        }
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Result<()> {
        let mut protocol = state.protocol_tx.subscribe();
        let mut commander = Self::new(output, &state, palette).await;

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
                        commander.dump_last_known_output();
                        break;
                    }
                    commander.tattoy.handle_common_protocol_messages(message)?;
                }
                () = commander.tattoy.sleep_until_next_frame_tick() => {
                    let is_exited = commander.check_for_exit_and_notify(&state).await?;
                    if is_exited {
                        break;
                    }
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
                    self.tattoy.initialise_surface();
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

    /// Check if the Shadow Terminal has exited and if so, notify the user of the last known output.
    async fn check_for_exit_and_notify(
        &mut self,
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<bool> {
        if !self.shadow_terminal.task_handle.is_finished() {
            return Ok(false);
        }

        let max_output = (self.tattoy.width * self.tattoy.height).div_euclid(4);
        let mut last_known_output = self.dump_last_known_output();
        last_known_output.truncate(max_output.into());

        let is_empty_output = last_known_output.trim().is_empty();
        let is_unexpected_exit = !state.config.read().await.bg_command.expect_exit;
        if !is_unexpected_exit && !is_empty_output {
            return Ok(true);
        }

        if is_empty_output {
            last_known_output = format!(
                "No output, does `{}` command exist?",
                self.command.first().context("No base command")?
            );
        }

        let is_large_output = last_known_output.len() >= max_output.into();
        if is_unexpected_exit && is_large_output {
            last_known_output = format!("Sample of output:\n{last_known_output}...\n");
        }

        state
            .send_notification(
                "Background command exited",
                crate::tattoys::notifications::message::Level::Error,
                Some(last_known_output),
                true,
            )
            .await;

        Ok(true)
    }

    /// Get the last known output of the command, log and return it.
    fn dump_last_known_output(&mut self) -> std::string::String {
        let mut output = String::new();
        for cell_line in self.tattoy.surface.surface.screen_cells() {
            let mut line = String::new();
            for (x, cell) in cell_line.iter().enumerate() {
                line.push_str(cell.str());
                if x == usize::from(self.tattoy.width) - 4 && !line.contains('\n') {
                    line.push('…');
                    break;
                }
            }
            let no_elipsis = line.replace("…", " ");
            let trimmed_right = no_elipsis.trim_end();
            if line.len() - trimmed_right.len() > 3 {
                line = trimmed_right.into();
            }
            if !line.contains('\n') {
                output.push('\n');
            }
            output.push_str(line.as_str());
        }

        tracing::error!("{output}");
        output.trim().into()
    }
}
