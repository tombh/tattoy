//! Shared state and behaviour useful to all tattoys.#

use color_eyre::eyre::Result;

/// Shared state and behaviour useful to all tattoys.
pub(crate) struct Tattoyer {
    /// A unique identifier.
    pub id: String,
    /// The compositing layer that the tattoy is rendered to. 0 is the PTY screen itself.
    pub layer: i16,
    /// A channel to send final rendered output.
    pub output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    /// The surface on which to construct this tattoy's frame.
    pub surface: crate::surface::Surface,
    /// TTY width
    pub width: u16,
    /// TTY height
    pub height: u16,
    /// Our own copy of the scrollback. Saves taking costly read locks.
    pub scrollback: shadow_terminal::output::CompleteScrollback,
    /// Our own copy of the screen. Saves taking costly read locks.
    pub screen: shadow_terminal::output::CompleteScreen,
    /// The target frame rate.
    pub frame_rate: u32,
    /// The time at which the previous frame was rendererd.
    pub last_frame_tick: std::time::Instant,
    /// The last known position of an active scroll.
    pub last_scroll_position: usize,
}

impl Tattoyer {
    /// Instantiate
    pub(crate) fn new(
        id: String,
        layer: i16,
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Self {
        Self {
            id: id.clone(),
            layer,
            output_channel,
            surface: crate::surface::Surface::new(id, 0, 0, layer),
            width: 0,
            height: 0,
            scrollback: shadow_terminal::output::CompleteScrollback::default(),
            screen: shadow_terminal::output::CompleteScreen::default(),
            frame_rate: 30,
            last_frame_tick: std::time::Instant::now(),
            last_scroll_position: 0,
        }
    }

    /// Is the tattoy ready to be built?
    pub const fn is_ready(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Create an empty surface ready for building a new frame.
    pub fn initialise_surface(&mut self) {
        self.surface = crate::surface::Surface::new(
            self.id.clone(),
            self.width.into(),
            self.height.into(),
            self.layer,
        );
    }

    /// Keep track of the size of the underlying terminal.
    pub const fn set_tty_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Handle commpm protocol messages, like resizing and new output from the underlying terminal.
    pub(crate) fn handle_common_protocol_messages(
        &mut self,
        message: crate::run::Protocol,
    ) -> Result<()> {
        tracing::trace!(
            "'{}' tattoy recevied protocol message: {message:?}",
            self.id
        );

        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We're just handling the common cases here."
        )]
        match message {
            crate::run::Protocol::Resize { width, height } => {
                self.set_tty_size(width, height);
            }
            crate::run::Protocol::Output(output) => self.handle_pty_output(output)?,
            crate::run::Protocol::Config(config) => self.frame_rate = config.frame_rate,
            _ => (),
        }

        Ok(())
    }

    /// Whether the user is scolling.
    pub const fn is_scrolling(&self) -> bool {
        self.scrollback.position != 0
    }

    /// Has scolling just ended?
    pub const fn is_scrolling_end(&self) -> bool {
        self.last_scroll_position != 0 && !self.is_scrolling()
    }

    /// Is the underlying terminal in the alternate screen.
    pub const fn is_alternate_screen(&self) -> bool {
        matches!(
            self.screen.mode,
            shadow_terminal::output::ScreenMode::Alternate
        )
    }

    /// Handle new output from the underlying PTY.
    pub fn handle_pty_output(&mut self, output: shadow_terminal::output::Output) -> Result<()> {
        match output {
            shadow_terminal::output::Output::Diff(diff) => match diff {
                shadow_terminal::output::SurfaceDiff::Scrollback(scrollback_diff) => {
                    self.scrollback
                        .surface
                        .resize(scrollback_diff.size.0, scrollback_diff.height);
                    self.set_tty_size(
                        scrollback_diff.size.0.try_into()?,
                        scrollback_diff.size.1.try_into()?,
                    );
                    self.scrollback.surface.add_changes(scrollback_diff.changes);
                    self.scrollback.position = scrollback_diff.position;
                }
                shadow_terminal::output::SurfaceDiff::Screen(screen_diff) => {
                    self.screen
                        .surface
                        .resize(screen_diff.size.0, screen_diff.size.1);
                    self.set_tty_size(
                        screen_diff.size.0.try_into()?,
                        screen_diff.size.1.try_into()?,
                    );
                    self.screen.surface.add_changes(screen_diff.changes);
                }
                _ => (),
            },
            shadow_terminal::output::Output::Complete(complete) => match complete {
                shadow_terminal::output::CompleteSurface::Scrollback(complete_scrollback) => {
                    self.scrollback = complete_scrollback;
                }
                shadow_terminal::output::CompleteSurface::Screen(complete_screen) => {
                    self.set_tty_size(
                        complete_screen.surface.dimensions().0.try_into()?,
                        complete_screen.surface.dimensions().1.try_into()?,
                    );
                    self.screen = complete_screen;
                }
                _ => (),
            },
            _ => (),
        }

        Ok(())
    }

    /// Send the final surface to the main renderer.
    pub(crate) async fn send_output(&mut self) -> Result<()> {
        self.output_channel
            .send(crate::run::FrameUpdate::TattoySurface(self.surface.clone()))
            .await?;

        self.last_scroll_position = self.scrollback.position;

        Ok(())
    }

    /// Send a blank frame to the render.
    pub(crate) async fn send_blank_output(&mut self) -> Result<()> {
        self.initialise_surface();
        self.send_output().await
    }

    /// Sleep until the next frame render is due.
    pub async fn sleep_until_next_frame_tick(&mut self) {
        let target = crate::renderer::ONE_MICROSECOND.wrapping_div(self.frame_rate.into());
        let target_frame_rate_micro = std::time::Duration::from_micros(target);
        if let Some(wait) = target_frame_rate_micro.checked_sub(self.last_frame_tick.elapsed()) {
            tokio::time::sleep(wait).await;
        }
        self.last_frame_tick = std::time::Instant::now();
    }

    /// Check if the scrollback output has changed.
    pub fn is_scrollback_output_changed(message: &crate::run::Protocol) -> bool {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We only want to react to messages that cause output changes"
        )]
        match message {
            crate::run::Protocol::Resize { .. } => return true,
            crate::run::Protocol::Output(output) => match output {
                shadow_terminal::output::Output::Diff(
                    shadow_terminal::output::SurfaceDiff::Scrollback(diff),
                ) => {
                    // There is always one change to indicate the current position of the cursor.
                    if diff.changes.len() > 1 {
                        return true;
                    }
                }
                shadow_terminal::output::Output::Complete(
                    shadow_terminal::output::CompleteSurface::Scrollback(_),
                ) => {
                    return true;
                }
                _ => (),
            },
            _ => (),
        }

        false
    }

    /// Check if the screen output has changed.
    pub fn is_screen_output_changed(message: &crate::run::Protocol) -> bool {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We only want to react to messages that cause output changes"
        )]
        match message {
            crate::run::Protocol::Resize { .. } => return true,
            crate::run::Protocol::Output(output) => match output {
                shadow_terminal::output::Output::Diff(
                    shadow_terminal::output::SurfaceDiff::Screen(diff),
                ) => {
                    // There is always one change to indicate the current position of the cursor.
                    if diff.changes.len() > 1 {
                        return true;
                    }
                }
                shadow_terminal::output::Output::Complete(
                    shadow_terminal::output::CompleteSurface::Screen(_),
                ) => {
                    return true;
                }
                _ => (),
            },
            _ => (),
        }

        false
    }
}
