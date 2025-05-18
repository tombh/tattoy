//! Run custom external code that gets rendered as tattoys

use std::io::Write as _;

use color_eyre::eyre::{ContextCompat as _, Result};

/// The default compositing layer the plugin is rendered to. Can be manually set inn the config.
const DEFAULT_LAYER: i16 = -10;
/// The default transparency for the plugin output.
const DEFAULT_OPACITY: f32 = 1.0;

/// User-configurable settings for the minimap
#[derive(serde::Deserialize, Debug, Clone)]
pub struct Config {
    /// The name of the plugin. Can be any string.
    name: String,
    /// The path to the plugin executable.
    path: std::path::PathBuf,
    /// The layer upon which the plugin is rendered.
    layer: Option<i16>,
    /// The transparency of the plugin output.
    opacity: Option<f32>,
    /// Whether the plugin is enabled.
    pub enabled: Option<bool>,
}

/// Plugins
pub struct Plugin {
    /// The base Tattoy struct.
    tattoy: super::tattoyer::Tattoyer,
    /// The user's terminal colours.
    palette: crate::palette::converter::Palette,
    /// The plugin's subprocess
    child: std::process::Child,
    /// STDIN to the plugin process, for sending messages to the plugin.
    plugin_stdin: std::io::BufWriter<std::process::ChildStdin>,
    /// Output stream from spawned plugin process.
    parsed_messages_rx: tokio::sync::mpsc::Receiver<tattoy_protocol::PluginOutputMessages>,
}

impl Plugin {
    /// Instatiate
    async fn new(
        config: &Config,
        listener_rx: tokio::sync::oneshot::Receiver<crate::run::Protocol>,
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        palette: crate::palette::converter::Palette,
        state: std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<Self> {
        let tattoy = super::tattoyer::Tattoyer::new(
            config.name.clone(),
            state,
            config.layer.unwrap_or(DEFAULT_LAYER),
            config.opacity.unwrap_or(DEFAULT_OPACITY),
            output_channel,
        )
        .await;
        let (parsed_messages_tx, parsed_messages_rx) = tokio::sync::mpsc::channel(16);

        let result = Self::spawn(&config.path, listener_rx, parsed_messages_tx);
        match result {
            Ok(mut child) => {
                let stdin = child
                    .stdin
                    .take()
                    .context("Couldn't get STDIN for plugin.")?;
                let stdin_writer = std::io::BufWriter::new(stdin);

                Ok(Self {
                    tattoy,
                    palette,
                    child,
                    plugin_stdin: stdin_writer,
                    parsed_messages_rx,
                })
            }
            Err(error) => {
                tracing::error!("Couldn't start plugin {}: {error:?}", config.name);
                Err(error)
            }
        }
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        config: Config,
        palette: crate::palette::converter::Palette,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        tattoy_protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Result<()> {
        tracing::info!("Starting plugin: {}", config.name);

        let (listener_tx, listener_rx) = tokio::sync::oneshot::channel();
        let mut plugin = Self::new(&config, listener_rx, output, palette, state).await?;
        let mut tattoy_protocol_receiver = tattoy_protocol_tx.subscribe();

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                Some(message) = plugin.parsed_messages_rx.recv() => {
                    let result = plugin.render(message).await;
                    if let Err(error) = result {
                        tracing::error!("{error:?}");
                    }
                },
                Ok(message) = tattoy_protocol_receiver.recv() => {
                    if matches!(message, crate::run::Protocol::End) {
                        plugin.child.kill()?;
                        let result = listener_tx.send(message);
                        if let Err(error) = result {
                            tracing::error!("Couldn't send End message to listener: {error:?}");
                        }
                        tracing::info!("Sent kill to plugin process and our plugin listener.");
                        break;
                    }
                    plugin.handle_protocol_messages(&message)?;
                    plugin.tattoy.handle_common_protocol_messages(message)?;
                }
            }
        }

        tracing::debug!("Exiting main plugin loop for: {}", config.name);

        Ok(())
    }

    /// Handle Tattoy protocol messages.
    fn handle_protocol_messages(&mut self, message: &crate::run::Protocol) -> Result<()> {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We're just handling the common cases here."
        )]
        match message {
            crate::run::Protocol::Resize { .. } => self.send_tty_size()?,
            crate::run::Protocol::Output(_) => self.send_pty_output()?,

            _ => (),
        }

        Ok(())
    }

    /// Send the new terminal size to the plugin.
    fn send_tty_size(&mut self) -> Result<()> {
        let json = serde_json::to_string(&tattoy_protocol::PluginInputMessages::TTYResize {
            width: self.tattoy.width,
            height: self.tattoy.height,
        })?;

        tracing::trace!("Sending JSON to plugin: {json}");
        self.plugin_stdin.write_all(json.as_bytes())?;
        self.plugin_stdin.write_all(b"\n")?;
        self.plugin_stdin.flush()?;

        Ok(())
    }

    /// Send Tattoy's PTY output to the plugin.
    fn send_pty_output(&mut self) -> Result<()> {
        let mut cells = Vec::<tattoy_protocol::Cell>::new();
        for (y, line) in self.tattoy.screen.surface.screen_cells().iter().enumerate() {
            for (x, cell) in line.iter().enumerate() {
                let character = cell.str();
                if character.is_empty() || character == " " {
                    continue;
                }

                // TODO: how to avoid the clone?
                self.palette
                    .cell_attributes_to_true_colour(cell.clone().attrs_mut());

                let bg_attribute =
                    crate::blender::Blender::extract_colour(cell.attrs().background());
                let bg = match bg_attribute {
                    Some(attribute) => attribute.to_tuple_rgba(),
                    None => self.palette.default_background_colour().into(),
                };

                let fg_attribute =
                    crate::blender::Blender::extract_colour(cell.attrs().foreground());
                let fg = match fg_attribute {
                    Some(attribute) => attribute.to_tuple_rgba(),
                    None => self.palette.default_foreground_colour().into(),
                };

                cells.push(
                    tattoy_protocol::Cell::builder()
                        .character(character.to_owned().chars().nth(0).context(
                            "Couldn't get first character from cell, should be impossible.",
                        )?)
                        .coordinates((u32::try_from(x)?, u32::try_from(y)?))
                        .maybe_bg(Some(bg))
                        .maybe_fg(Some(fg))
                        .build(),
                );
            }
        }

        let cursor_position = self.tattoy.screen.surface.cursor_position();
        let json = serde_json::to_string(&tattoy_protocol::PluginInputMessages::PTYUpdate {
            size: (self.tattoy.width, self.tattoy.height),
            cells,
            cursor: (cursor_position.0.try_into()?, cursor_position.1.try_into()?),
        })?;
        tracing::trace!("Sending JSON to plugin: {json}");
        self.plugin_stdin.write_all(json.as_bytes())?;
        self.plugin_stdin.write_all(b"\n")?;
        self.plugin_stdin.flush()?;

        Ok(())
    }

    /// Spawn the plugin process.
    fn spawn(
        path: &std::path::Path,
        mut listener_rx: tokio::sync::oneshot::Receiver<crate::run::Protocol>,
        parsed_messages_tx: tokio::sync::mpsc::Sender<tattoy_protocol::PluginOutputMessages>,
    ) -> Result<std::process::Child> {
        let mut cmd = std::process::Command::new(
            path.to_str()
                .context("Couldn't convert plugin path to string")?,
        );
        cmd.stdout(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::piped());

        let mut child = cmd.spawn()?;

        let stdout = child
            .stdout
            .take()
            .context("Couldn't take STDOUT from plugin.")?;
        // TODO:
        //   By not taking advantage of async this may turn out to be a bad idea.
        //   See this issue for progress on supporting async stream deserialisation:
        //     https://github.com/serde-rs/json/issues/316
        let mut stdout_reader = std::io::BufReader::new(stdout);

        let tokio_runtime = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            tokio_runtime.block_on(async {
                tracing::trace!("Starting to parse JSON stream from plugin...");
                loop {
                    tracing::debug!("(Re)starting parser");
                    Self::listener(&mut stdout_reader, &parsed_messages_tx).await;
                    match listener_rx.try_recv() {
                        Ok(message) => {
                            if matches!(message, crate::run::Protocol::End) {
                                break;
                            }
                        }
                        Err(error) => match error {
                            tokio::sync::oneshot::error::TryRecvError::Empty => (),
                            tokio::sync::oneshot::error::TryRecvError::Closed => break,
                        },
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
                tracing::debug!("Leaving plugin listener loop.");
            });
        });

        Ok(child)
    }

    /// Parse output from the plugin, byte by byte, sending a message whenever it finds a valid
    /// JSON plugin protocol message.
    ///
    /// Apart from JSON not being the most efficient IPC medium, it also may not be the most
    /// efficient to use this streaming parser, as it requires checking for a valid message on
    /// every new byte. The benefit however is that plugin authors do not need to worry about the
    /// format of their messages. Therefore, there's no need to use delimeters of any kind.
    async fn listener(
        reader: &mut std::io::BufReader<std::process::ChildStdout>,
        parsed_messages_tx: &tokio::sync::mpsc::Sender<tattoy_protocol::PluginOutputMessages>,
    ) {
        let mut messages = serde_json::Deserializer::from_reader(reader)
            .into_iter::<tattoy_protocol::PluginOutputMessages>();

        for parse_result in messages.by_ref() {
            match parse_result {
                Ok(message) => {
                    tracing::trace!("Parsed JSON message: {message:?}");
                    let send_result = parsed_messages_tx.send(message).await;
                    tracing::trace!("Sent JSON message");
                    if let Err(error) = send_result {
                        tracing::error!("Couldn't send parsed plugin message: {error:?}");
                    }
                }
                Err(error) => tracing::error!("Error parsing plugin message: {error:?}"),
            }
        }
    }

    /// Tick the render
    async fn render(&mut self, output: tattoy_protocol::PluginOutputMessages) -> Result<()> {
        self.tattoy.initialise_surface();

        tracing::debug!("Rendering from plugin message");
        match output {
            tattoy_protocol::PluginOutputMessages::OutputText {
                text,
                coordinates,
                bg,
                fg,
            } => {
                self.tattoy.surface.add_text(
                    coordinates.0.try_into()?,
                    coordinates.1.try_into()?,
                    text,
                    bg,
                    fg,
                );
            }
            tattoy_protocol::PluginOutputMessages::OutputPixels(pixels) => {
                for pixel in pixels {
                    self.tattoy.surface.add_pixel(
                        pixel.coordinates.0.try_into()?,
                        pixel.coordinates.1.try_into()?,
                        // TODO: use the terminal palette's default foreground colour
                        pixel.color.unwrap_or(crate::surface::WHITE),
                    )?;
                }
            }
            tattoy_protocol::PluginOutputMessages::OutputCells(cells) => {
                for cell in cells {
                    self.tattoy.surface.add_text(
                        cell.coordinates.0.try_into()?,
                        cell.coordinates.1.try_into()?,
                        cell.character.to_string(),
                        cell.bg,
                        cell.fg,
                    );
                }
            }

            #[expect(
                clippy::unreachable,
                reason = "
                    The plugin protocol specifies `non-exhaustive`, but we are also the protocol definers,
                    so we won't get hit by unexpeted protocol changes.
                "
            )]
            _ => unreachable!(),
        }

        self.tattoy.send_output().await?;

        Ok(())
    }
}
