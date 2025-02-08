//! An in-memory TTY renderer. It takes a stream of PTY output bytes and maintains the visual
//! appearance of a terminal without actually physically rendering it.

use snafu::ResultExt as _;
use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;
use tokio::sync::mpsc;

/// Wezterm's internal configuration
#[derive(Debug)]
struct WeztermConfig {
    /// The number of lines to store in the scrollback
    scrollback: usize,
}

impl wezterm_term::TerminalConfiguration for WeztermConfig {
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }

    fn color_palette(&self) -> wezterm_term::color::ColorPalette {
        wezterm_term::color::ColorPalette::default()
    }
}

/// Config for creating a shadow terminal.
#[expect(
    clippy::exhaustive_structs,
    reason = "
        I just really like the ability to specify config in a struct. As if it were JSON.
        I know that means projects depending on this struct run the risk of unexpected
        breakage when I add a new field. But maybe we can manage those expectations by
        making sure that all example code is based off `ShadowTerminalConfig::default()`?
    "
)]
pub struct Config {
    /// Width of terminal
    pub width: u16,
    /// Height of terminal
    pub height: u16,
    /// Initial command for PTY, usually the user's `$SHELL`
    pub command: Vec<std::ffi::OsString>,
    /// The size of ther terminal's scrollback history.
    pub scrollback: usize,
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            width: 100,
            height: 30,
            command: vec!["bash".into()],
            scrollback: 1000,
        }
    }
}

/// The various inter-task/thread channels needed to run the shadow terminal and the PTY
/// simultaneously.
#[non_exhaustive]
pub struct Channels {
    /// Internal channel for control messages like shutdown and resize.
    pub control_tx: tokio::sync::broadcast::Sender<crate::Protocol>,
    /// The channel side that sends terminal output updates.
    pub output_tx: tokio::sync::mpsc::Sender<crate::pty::BytesFromPTY>,
    /// The channel side that receives terminal output updates.
    pub output_rx: tokio::sync::mpsc::Receiver<crate::pty::BytesFromPTY>,
}

// TODO: Would it be useful to keep the PTY's task handle on here, and `await` it in the main loop,
// so that the PTY module always has time to do its shutdown?
//
/// This is the main Shadow Terminal struct that helps run everything is this crate.
///
/// Instantiating this struct will allow you to have steppable control over the shadow terminal. If you
/// want the shadow terminal to run unhindered, you can use `.run()`, though [`ActiveTerminal`] offers a
/// more convenient ready-made wrapper to interect with a running shadow terminal.
#[non_exhaustive]
pub struct ShadowTerminal {
    /// The Wezterm terminal that does most of the actual work of maintaining the terminal 🙇
    pub terminal: wezterm_term::Terminal,
    /// The shadow terminal's config
    pub config: Config,
    /// The various channels needed to run the shadow terminal and its PTY
    pub channels: Channels,
}

impl ShadowTerminal {
    /// Create a new Shadow Terminal
    #[inline]
    pub fn new(config: Config) -> Self {
        let (control_tx, _) = tokio::sync::broadcast::channel(64);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(1);

        tracing::debug!("Creating the in-memory Wezterm terminal");
        let terminal = wezterm_term::Terminal::new(
            wezterm_term::TerminalSize {
                cols: config.width.into(),
                rows: config.height.into(),
                pixel_width: 0,
                pixel_height: 0,
                dpi: 0,
            },
            std::sync::Arc::new(WeztermConfig {
                scrollback: config.scrollback,
            }),
            "Tattoy",
            "O_o",
            Box::<Vec<u8>>::default(),
        );

        Self {
            terminal,
            config,
            channels: Channels {
                control_tx,
                output_tx,
                output_rx,
            },
        }
    }

    /// Start the background PTY process.
    #[inline]
    pub fn start(
        &self,
        input_rx: mpsc::Receiver<crate::pty::BytesFromSTDIN>,
    ) -> tokio::task::JoinHandle<Result<(), crate::errors::PTYError>> {
        let pty = crate::pty::PTY {
            command: self.config.command.clone(),
            width: self.config.width,
            height: self.config.height,
            control_tx: self.channels.control_tx.clone(),
            output_tx: self.channels.output_tx.clone(),
        };

        // I don't think the PTY should be run in a standard thread, because it's not actually CPU
        // intensive in terms of the current thread. It runs in an OS sub process, so in theory
        // shouldn't conflict with Tokio's IO-focussed scheduler?
        tokio::spawn(async move { pty.run(input_rx).await })
    }

    /// Start listening to a stream of PTY bytes and render them to a shadow Termwiz surface
    #[inline]
    pub async fn run(
        &mut self,
        input_rx: mpsc::Receiver<crate::pty::BytesFromSTDIN>,
        shadow_output: &mpsc::Sender<termwiz::surface::Surface>,
    ) {
        tracing::debug!("Starting Shadow Terminal loop...");

        let mut control_rx = self.channels.control_tx.subscribe();
        self.start(input_rx);

        tracing::debug!("Starting Shadow Terminal main loop");
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "`tokio::select! generates this.`"
        )]
        loop {
            tokio::select! {
                () = self.read_from_pty() => {
                    let result = self.send_output(shadow_output).await;
                    if let Err(error) = result {
                        tracing::error!("{error:?}");
                    }
                },
                Ok(message) = control_rx.recv() => {
                    self.handle_protocol_message(&message);
                    if matches!(message, crate::Protocol::End) {
                        break;
                    }
                }
                // TODO: I don't actually understand the conditions in which this is called.
                else => {
                    let result = self.kill();
                    if let Err(error) = result {
                        tracing::error!("{error:?}");
                    }
                    break;
                }
            }
        }

        tracing::debug!("Shadow Terminal loop finished");
    }

    // TODO:
    // The output of the PTY seems to be capped at 4095 bytes. Making the size of
    // [`crate::pty::BytesFromPTY`] bigger than that doesn't seem to make a difference. This means
    // that for large screen updates `self.build_current_surface()` can be called an unnecessary
    // number of times.
    //
    // Possible solutions:
    //   * Ideally get the PTY to send bigger payloads.
    //   * Only call `self.build_current_surface()` at a given frame rate, probably 60fps.
    //     This could be augmented with a check for the size so the payloads smaller than
    //     4095 get rendered immediately.
    //   * When receiving a payload of exactly 4095 bytes, wait a fixed amount of time for
    //     more payloads, because in most cases 4095 means that there wasn't enough room to
    //     fit everything in a single payload.
    //   * Make `self.build_current_surface()` able to detect new payloads as they happen
    //     so it can cancel itself and immediately start working on the new one.
    //
    /// Send the current state of the shadow terminal as a Termwiz surface to whoever is externally
    /// listening.
    async fn send_output(
        &mut self,
        shadow_output: &mpsc::Sender<termwiz::surface::Surface>,
    ) -> Result<(), crate::errors::ShadowTerminalError> {
        let surface = self.build_current_surface()?;
        shadow_output
            .send(surface)
            .await
            .with_whatever_context(|err| format!("{err:?}"))?;

        Ok(())
    }

    /// Broadcast the shutdown signal. This should exit both the underlying PTY process and the
    /// main `ShadowTerminal` loop.
    ///
    /// # Errors
    /// If the `End` messaage could not be sent.
    #[inline]
    pub fn kill(&self) -> Result<(), crate::errors::ShadowTerminalError> {
        tracing::debug!("`ShadowTerminal.kill()` called");

        self.channels
            .control_tx
            .send(crate::Protocol::End)
            .with_whatever_context(|err| {
                format!("Couldn't write bytes into PTY's STDIN: {err:?}")
            })?;

        Ok(())
    }

    /// Handle bytes from the PTY subprocess.
    #[inline]
    pub async fn read_from_pty(&mut self) {
        if let Some(bytes) = self.channels.output_rx.recv().await {
            self.terminal.advance_bytes(bytes);
            tracing::trace!("Wezterm shadow terminal advanced {} bytes", bytes.len());
        }
    }

    /// Handle any messages from the internal control protocol
    fn handle_protocol_message(&mut self, message: &crate::Protocol) {
        tracing::debug!("Shadow Terminal received protocol message: {message:?}");
        match message {
            crate::Protocol::End => (),
            crate::Protocol::Resize { width, height } => {
                self.terminal.resize(wezterm_term::TerminalSize {
                    cols: usize::from(*width),
                    rows: usize::from(*height),
                    pixel_width: 0,
                    pixel_height: 0,
                    dpi: 0,
                });
            }
        };
    }

    /// Converts Wezterms's maintained virtual TTY into a compositable Termwiz surface
    fn build_current_surface(
        &mut self,
    ) -> Result<termwiz::surface::Surface, crate::errors::ShadowTerminalError> {
        tracing::trace!("Converting Wezterm terminal state to a `termwiz::surface::Surface`");

        let size = self.terminal.get_size();
        let mut surface = termwiz::surface::Surface::new(size.cols, size.rows);

        // TODO:
        //   * Explore using this to improve performance:
        //     `self.terminal.screen().get_changed_stable_rows()`
        //   * Handle scrolling:
        //     self.terminal.is_alt_screen_active()
        //     screen.scroll_up()
        //     screen.scroll_down()
        let screen = self.terminal.screen_mut();
        for row in 0..=size.rows {
            for column in 0..=size.cols {
                let row_i64: i64 = i64::try_from(row)
                    .with_whatever_context(|err| format!("Couldn't convert row index: {err:?}"))?;
                if let Some(cell) = screen.get_cell(column, row_i64) {
                    let attrs = cell.attrs();
                    let cursor = TermwizChange::CursorPosition {
                        x: TermwizPosition::Absolute(column),
                        y: TermwizPosition::Absolute(row),
                    };
                    surface.add_change(cursor);

                    // TODO: is there a more elegant way to copy over all the attributes?
                    let attributes = vec![
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(
                            attrs.foreground(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                            attrs.background(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Intensity(
                            attrs.intensity(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Italic(
                            attrs.italic(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Underline(
                            attrs.underline(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Blink(
                            attrs.blink(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Reverse(
                            attrs.reverse(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::StrikeThrough(
                            attrs.strikethrough(),
                        )),
                        cell.str().into(),
                    ];
                    surface.add_changes(attributes);
                }
            }
        }

        let users_cursor = self.terminal.cursor_pos();
        let cursor = TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(users_cursor.x),
            #[expect(
                clippy::as_conversions,
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "We're well within the limits of usize"
            )]
            y: TermwizPosition::Absolute(users_cursor.y as usize),
        };
        surface.add_change(cursor);

        Ok(surface)
    }
}

impl Drop for ShadowTerminal {
    #[inline]
    fn drop(&mut self) {
        tracing::trace!("Running ShadowTerminal.drop()");
        let result = self.kill();
        if let Err(error) = result {
            tracing::error!("{error:?}");
        }
    }
}
