//! Display notification messages in the UI

use color_eyre::eyre::Result;
use palette::Darken as _;

/// User-configurable settings for the background command.
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub(crate) struct Config {
    /// Enable/disable the display of notifications
    pub enabled: bool,
    /// The transparency of the notifications
    pub opacity: f32,
    /// The minimum level of notifications to display
    pub level: super::message::Level,
    /// The amount of time to display a notification
    pub duration: f32,
}

/// `Notifications`
pub(crate) struct Notifications {
    /// The base Tattoy struct
    tattoy: crate::tattoys::tattoyer::Tattoyer,
    /// All the current notification messages
    messages: Vec<super::message::Message>,
    /// Text colour taken from the palette
    text_colour: termwiz::color::SrgbaTuple,
}

impl Notifications {
    /// Instantiate
    async fn new(
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Result<Self> {
        crate::config::main::Config::load_palette(std::sync::Arc::clone(&state)).await?;
        let text_colour = palette.default_foreground_colour();
        let opacity = state.config.read().await.notifications.opacity;
        let tattoy = crate::tattoys::tattoyer::Tattoyer::new(
            "notifications".to_owned(),
            state,
            200,
            opacity,
            output_channel,
        )
        .await;

        Ok(Self {
            tattoy,
            messages: Vec::new(),
            text_colour,
        })
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
        palette: crate::palette::converter::Palette,
    ) -> Result<()> {
        let mut protocol = state.protocol_tx.subscribe();
        let mut notifications = Self::new(output, std::sync::Arc::clone(&state), palette).await?;

        state
            .initialised_systems
            .write()
            .await
            .push("notifications".to_owned());

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                () = notifications
                     .tattoy
                     .sleep_until_next_frame_tick(), if !notifications.messages.is_empty() => {
                    notifications.render().await?;
                },
                result = protocol.recv() => {
                    if matches!(result, Ok(crate::run::Protocol::End)) {
                        break;
                    }
                    notifications.handle_protocol_message(result)?;
                }
            }
        }

        Ok(())
    }

    /// Handle messages from the main Tattoy app.
    fn handle_protocol_message(
        &mut self,
        result: std::result::Result<crate::run::Protocol, tokio::sync::broadcast::error::RecvError>,
    ) -> Result<()> {
        match result {
            Ok(message) => {
                if let crate::run::Protocol::Notification(notification) = &message {
                    tracing::debug!("Notification received: {notification:?}");
                    self.messages.push(notification.clone());
                }
                self.tattoy.handle_common_protocol_messages(message)?;
            }
            Err(error) => tracing::error!("Receiving protocol message: {error:?}"),
        }

        Ok(())
    }

    /// Remove messages that have been around for longer than the duration set in config.
    fn remove_old_messages(&mut self, duration: f32) {
        self.messages.retain(|message| message.age() < duration);
    }

    /// Tick the render
    async fn render(&mut self) -> Result<()> {
        self.tattoy.initialise_surface();

        let config = self.tattoy.state.config.read().await.notifications.clone();
        self.tattoy.opacity = config.opacity;
        let level = config.level.clone();

        self.remove_old_messages(config.duration);

        let all = self.messages.clone();
        let mut messages = all
            .iter()
            .filter(|message| message.level <= level)
            .collect::<Vec<&super::message::Message>>();
        messages.sort_by(|left, right| left.level.cmp(&right.level));

        let mut y = 0;
        for message in &messages {
            self.add_text(y, message, message.title.as_str(), config.duration, false);

            if let Some(body) = &message.body {
                for line in body.lines() {
                    y += 1;
                    self.add_text(y, message, line, config.duration, true);
                }
            }
            y += 1;
        }

        self.tattoy.send_output().await
    }

    /// Add a line of the notification to the Tattoy surface.
    fn add_text(
        &mut self,
        y: usize,
        message: &super::message::Message,
        text: &str,
        duration: f32,
        is_body: bool,
    ) {
        let fade = message.fade_in_out(duration);
        let text_colour = (
            self.text_colour.0,
            self.text_colour.1,
            self.text_colour.2,
            fade,
        );
        let mut background_colour = message.colour();
        background_colour.3 = fade;
        if is_body {
            let darkenable: palette::Srgba<f32> = palette::rgb::Rgba::from(background_colour);
            background_colour = darkenable.darken(0.3).into();
        }

        let padding = 2;
        let tty_width = usize::from(self.tattoy.width);
        let max_width = message.max_width().clamp(0, tty_width - padding);
        let x = tty_width - max_width - padding;
        let right_padding = max_width - text.len().clamp(0, max_width) + 1;

        self.tattoy.surface.add_text(
            x,
            y,
            format!(" {text}{}", " ".repeat(right_padding)),
            Some(background_colour),
            Some(text_colour),
        );
    }

    /// Format a helpful messsage fragment suggesting to look at logs.
    pub fn logs_help_text(is_logging: bool, log_path: &std::path::Path) -> String {
        if is_logging {
            format!("Check logs for more details: {}", log_path.display())
        } else {
            "Enable logging for more details".into()
        }
    }
}
