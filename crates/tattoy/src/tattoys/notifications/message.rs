//! A single notification message.

/// The urgency level of the notification.
#[derive(serde::Deserialize, Debug, Clone, Default, Ord, Eq, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub(crate) enum Level {
    /// Errors
    Error,
    /// Warnings
    #[default]
    Warn,
    /// Informative notifications
    Info,
    /// Debuggin notifications
    Debug,
    /// Tracing notifications
    Trace,
}

#[derive(Debug, Clone)]
pub(crate) struct Message {
    /// The text of the notification.
    pub title: String,
    /// An optional body for the notification
    pub body: Option<String>,
    /// The time at which the notification was created.
    timestamp: tokio::time::Instant,
    /// The leve of the notification.
    pub level: Level,
}

impl Message {
    /// Create a new notification
    pub fn make(text: &str, level: Level, body: Option<String>) -> crate::run::Protocol {
        let message = Self {
            title: text.into(),
            body,
            timestamp: tokio::time::Instant::now(),
            level,
        };
        crate::run::Protocol::Notification(message)
    }

    // TODO: Find the colours in the current palette that most closely resemble these.
    /// The colour of each of level.
    pub const fn colour(&self) -> crate::surface::Colour {
        match self.level {
            Level::Error => (0.3, 0.0, 0.0, 1.0),
            Level::Warn => (0.3, 0.3, 0.0, 1.0),
            Level::Info => (0.0, 0.3, 0.0, 1.0),
            Level::Debug => (0.0, 0.0, 0.3, 1.0),
            Level::Trace => (0.3, 0.3, 0.3, 1.0),
        }
    }

    /// The time in seconds since the notification was created.
    pub fn age(&self) -> f32 {
        (tokio::time::Instant::now() - self.timestamp).as_secs_f32()
    }

    /// Calculate the fade in/out opacity.
    pub fn fade_in_out(&self, duration: f32) -> f32 {
        let transition = 0.2;
        match self.age() {
            before if before < 0.0 => 0.0,
            ease_in if ease_in <= transition => crate::utils::smoothstep(0.0, transition, ease_in),
            show if show <= duration - transition => 1.0,
            ease_out if ease_out <= duration => {
                crate::utils::smoothstep(duration, duration - transition, ease_out)
            }
            _ => 0.0,
        }
    }

    /// Calculate the widest part of the message.
    pub fn max_width(&self) -> usize {
        let mut width = self.title.len();
        if let Some(body) = &self.body {
            for line in body.lines() {
                if line.len() > width {
                    width = line.len();
                }
            }
        }
        width
    }
}
