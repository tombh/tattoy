//! Run custom external code that gets rendered as tattoys

use color_eyre::eyre::{ContextCompat as _, Result};

/// The default compositing layer the plugin is rendered to. Can be manually set inn the config.
const DEFAULT_LAYER: i16 = -10;

/// User-configurable settings for the minimap
#[derive(serde::Deserialize, Debug, Clone)]
pub struct Config {
    /// The name of the plugin. Can be any string.
    name: String,
    /// The path to the plugin executable.
    path: std::path::PathBuf,
    /// The layer upon which the plugin is rendered.
    layer: Option<i16>,
    /// Whether the plugin is enabled.
    pub enabled: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PluginProtocol {
    /// Output from the plugin that renders text in the terminal.
    OutputText {
        /// The text to display.
        text: String,
        /// The coordinates. [0, 0] is in the top-left.
        coordinates: (u32, u32),
        /// An optional colour for the text's background.
        bg: Option<crate::surface::Colour>,
        /// An optional colour for the text's foreground.
        fg: Option<crate::surface::Colour>,
    },

    /// Output from the plugin that renders pixels in the terminal.
    OutputPixel {
        /// The coordinates. [0, 0] is in the top-left. The y-axis is twice as long as the number
        /// of rows in the terminal.
        coordinates: (u32, u32),
        /// An optional colour for the pixel.
        colour: Option<crate::surface::Colour>,
    },
}

/// Plugins
pub struct Plugin {
    /// The base Tattoy struct.
    tattoy: super::tattoyer::Tattoyer,
    /// Output stream from spawned plugin process.
    parsed_messages_rx: tokio::sync::mpsc::Receiver<PluginProtocol>,
}

impl Plugin {
    /// Instatiate
    fn new(
        config: Config,
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Result<Self> {
        let tattoy = super::tattoyer::Tattoyer::new(
            config.name,
            config.layer.unwrap_or(DEFAULT_LAYER),
            output_channel,
        );
        let (parsed_messages_tx, parsed_messages_rx) = tokio::sync::mpsc::channel(16);
        Self::spawn(&config.path, parsed_messages_tx)?;

        Ok(Self {
            tattoy,
            parsed_messages_rx,
        })
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        config: Config,
        tattoy_protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Result<()> {
        tracing::info!("Starting plugin: {}", config.name);

        let mut plugin = Self::new(config, output)?;
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
                        tracing::info!("Sent kill to plugin");
                        break;
                    }
                    plugin.tattoy.handle_common_protocol_messages(message)?;
                }
            }
        }

        tracing::debug!("Exiting main plugin loop");

        Ok(())
    }

    /// Spawn the plugin process.
    fn spawn(
        path: &std::path::Path,
        parsed_messages_tx: tokio::sync::mpsc::Sender<PluginProtocol>,
    ) -> Result<()> {
        let mut cmd = std::process::Command::new(
            path.to_str()
                .context("Couldn't convert plugin path to string")?,
        );
        cmd.stdout(std::process::Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .context("Couldn't take STDOUT from plugin.")?;

        // TODO:
        //   By not taking advantage of async this may turn out to be a bad idea.
        //   See this issue for progress on supporting async stream deserialisation:
        //     https://github.com/serde-rs/json/issues/316
        let mut reader = std::io::BufReader::new(stdout);

        let tokio_runtime = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            tokio_runtime.block_on(async {
                tracing::trace!("Starting to parse JSON stream from plugin...");
                #[expect(
                    clippy::infinite_loop,
                    reason = "
                        I must admit to not totally understanding this. But for whatever reason
                        the thread is always ended when Tattoy exits. So even though there's no
                        way to exit the loop, the loop never blocks on application end.

                        My theory is that whereas the thread in `tokio::spawn_blocking()` is
                        likely quite similar in functionality to this thread, this thread isn't
                        ever joined which has the advantage of getting automatically garbage
                        collected when the main application exits.
                    "
                )]
                loop {
                    tracing::warn!("(Re)starting parser");
                    Self::listener(&mut reader, &parsed_messages_tx).await;
                }
            });
        });

        Ok(())
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
        parsed_messages_tx: &tokio::sync::mpsc::Sender<PluginProtocol>,
    ) {
        let mut messages =
            serde_json::Deserializer::from_reader(reader).into_iter::<PluginProtocol>();

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
    async fn render(&mut self, output: PluginProtocol) -> Result<()> {
        if !self.tattoy.is_ready() {
            return Ok(());
        }

        self.tattoy.initialise_surface();

        tracing::info!("Rendering from plugin message: {:?}", output);
        match output {
            PluginProtocol::OutputText {
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
            PluginProtocol::OutputPixel {
                coordinates,
                colour,
            } => {
                self.tattoy.surface.add_pixel(
                    coordinates.0.try_into()?,
                    coordinates.1.try_into()?,
                    // TODO: use the terminal palette's default foreground colour
                    colour.unwrap_or(crate::surface::WHITE),
                )?;
            }
        }
        self.tattoy.send_output().await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parsing_output() {
        let expected = serde_json::json!(
            {
                "output_text": {
                    "text": "foo",
                    "coordinates": [1u8, 2u8],
                    "bg": null,
                    "fg": null,
                }
            }
        );

        let output = PluginProtocol::OutputText {
            text: "foo".to_owned(),
            coordinates: (1, 2),
            bg: None,
            fg: None,
        };

        assert_eq!(
            expected.to_string(),
            serde_json::to_string(&output).unwrap()
        );
    }
}
