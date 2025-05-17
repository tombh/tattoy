//! An in-memory TTY renderer. It takes a stream of PTY output bytes and maintains the visual
//! appearance of a terminal without actually physically rendering it.

use snafu::ResultExt as _;
use tracing::Instrument as _;

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
    pub scrollback_size: usize,
    /// The number of lines that each scroll trigger moves.
    pub scrollback_step: usize,
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            width: 100,
            height: 30,
            command: vec!["bash".into()],
            scrollback_size: 1000,
            scrollback_step: 5,
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
    /// Internally generated input
    pub internal_input_tx: Option<tokio::sync::mpsc::Sender<crate::pty::BytesFromSTDIN>>,
    /// Sends complete snapshots of the current screen state.
    shadow_output: tokio::sync::mpsc::Sender<crate::output::Output>,
}

/// Keep track of the metadata for the last sent output.
#[non_exhaustive]
pub struct LastSent {
    /// The unique sequence number of the last change in the Wezterm terminal.
    pub pty_sequence: usize,
    /// The size of the last sent terminal output.
    pub pty_size: (usize, usize),
}

/// The special ANSI code that applications send to get a reply with the current cursor position.
const CURSOR_POSITION_REQUEST: &str = "\x1b[6n";

/// Enable the user's terminal's 'application mode'.
const APPLICATION_MODE_START: &str = "\x1b[?1h";

/// Disable the user's terminal's 'application mode'.
const APPLICATION_MODE_END: &str = "\x1b[?1l";

/// The time to wait for more output from the PTY. In microseconds (1000s of a millisecond).
const TIME_TO_WAIT_FOR_MORE_PTY_OUTPUT: u64 = 1000;

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
    /// Accumulated PTY output to help minimise render events.
    pub accumulated_pty_output: Vec<u8>,
    /// The timestamp for when to broadcast accumulated PTY output.
    pub wait_for_output_until: Option<tokio::time::Instant>,
    /// The current position of the scollback buffer.
    pub scroll_position: usize,
    /// Metadata about the most recent sent output.
    pub last_sent: LastSent,
}

impl ShadowTerminal {
    /// Create a new Shadow Terminal
    #[inline]
    pub fn new(
        config: Config,
        shadow_output: tokio::sync::mpsc::Sender<crate::output::Output>,
    ) -> Self {
        let (control_tx, _) = tokio::sync::broadcast::channel(64);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(1);

        tracing::debug!("Creating the in-memory Wezterm terminal");
        let terminal = wezterm_term::Terminal::new(
            Self::wezterm_size(config.width.into(), config.height.into()),
            std::sync::Arc::new(WeztermConfig {
                scrollback: config.scrollback_size,
            }),
            "Tattoy",
            "O_o",
            Box::<Vec<u8>>::default(),
        );

        let pty_size = (config.width.into(), config.height.into());
        Self {
            terminal,
            config,
            channels: Channels {
                control_tx,
                output_tx,
                output_rx,
                internal_input_tx: None,
                shadow_output,
            },
            accumulated_pty_output: Vec::new(),
            wait_for_output_until: None,
            scroll_position: 0,
            last_sent: LastSent {
                pty_sequence: 0,
                pty_size,
            },
        }
    }

    /// Start the background PTY process.
    #[inline]
    pub fn start(
        &mut self,
        user_input_rx: tokio::sync::mpsc::Receiver<crate::pty::BytesFromSTDIN>,
    ) -> tokio::task::JoinHandle<Result<(), crate::errors::PTYError>> {
        let (internal_input_tx, internal_input_rx) = tokio::sync::mpsc::channel(1);
        self.channels.internal_input_tx = Some(internal_input_tx);

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
        let current_span = tracing::Span::current();
        tokio::spawn(async move {
            pty.run(user_input_rx, internal_input_rx)
                .instrument(current_span)
                .await
        })
    }

    /// Start listening to a stream of PTY bytes and render them to a shadow Termwiz surface
    #[inline]
    pub async fn run(
        &mut self,
        user_input_rx: tokio::sync::mpsc::Receiver<crate::pty::BytesFromSTDIN>,
    ) {
        tracing::debug!("Starting Shadow Terminal loop...");

        let mut control_rx = self.channels.control_tx.subscribe();
        self.start(user_input_rx);

        tracing::debug!("Starting Shadow Terminal main loop");
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "`tokio::select!` generates this."
        )]
        loop {
            let is_wait = self.wait_for_output_until.is_some();
            let wait_until = self.wait_for_output_until;
            tokio::select! {
                Some(bytes) = self.channels.output_rx.recv() => {
                    self.accumulate_pty_output(&bytes);
                },
                () = Self::wait_for_more_pty_output(wait_until), if is_wait => {
                    let result = self.handle_pty_output().await;
                    if let Err(error) = result {
                        tracing::error!("Handling PTY output: {error:?}");
                    }
                }
                Ok(message) = control_rx.recv() => {
                    self.handle_protocol_message(&message).await;
                    if matches!(message, crate::Protocol::End) {
                        break;
                    }
                }
            }
        }

        tracing::debug!("Shadow Terminal loop finished");
    }

    /// The PTY crate that we use only sends output at 4kb a time. Often, on bigger terminals, a
    /// single change to the PTY can trigger a handful of these payloads. It would be inefficient to
    /// trigger output broadcasts for each mini PTY output. It's better to let the Wezterm terminal
    /// parse all the bytes and only then convert Wezterm's view into a broadcastable surface.
    async fn wait_for_more_pty_output(maybe_wait_until: Option<tokio::time::Instant>) {
        if let Some(wait_until) = maybe_wait_until {
            tokio::time::sleep_until(wait_until).await;
        }
    }

    /// Accumulate PTY outputs.
    fn accumulate_pty_output(&mut self, bytes: &crate::pty::BytesFromPTY) {
        // TODO: I feel like this loop is either inefficient, naive, or both.
        for byte in bytes {
            if byte == &0 {
                break;
            }
            self.accumulated_pty_output.push(*byte);
        }

        let next_output_broadcast = tokio::time::Instant::now()
            + tokio::time::Duration::from_micros(TIME_TO_WAIT_FOR_MORE_PTY_OUTPUT);
        self.wait_for_output_until = Some(next_output_broadcast);
    }

    /// Find bytes in bytes.
    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    /// Handle bytes from the PTY
    pub(crate) async fn handle_pty_output(
        &mut self,
    ) -> Result<(), crate::errors::ShadowTerminalError> {
        let bytes_copy = self.accumulated_pty_output.clone();
        let bytes = bytes_copy.as_slice();

        if Self::find_subsequence(bytes, APPLICATION_MODE_START.as_bytes()).is_some() {
            tracing::trace!("Starting terminal 'application mode'");
            crate::output::raw_string_direct_to_terminal(APPLICATION_MODE_START)
                .with_whatever_context(|err| {
                    format!("Sending 'application mode start' ANSI code: {err:?}")
                })?;
        }

        if Self::find_subsequence(bytes, APPLICATION_MODE_END.as_bytes()).is_some() {
            tracing::trace!("APPLICATION_MODE_END");
            crate::output::raw_string_direct_to_terminal(APPLICATION_MODE_END)
                .with_whatever_context(|err| {
                    format!("Sending 'application mode end' ANSI code: {err:?}")
                })?;
        }

        self.handle_cursor_position_request(bytes).await?;
        self.terminal.advance_bytes(bytes);
        tracing::trace!("Wezterm shadow terminal advanced {} bytes", bytes.len());
        let result = self.send_outputs().await;
        if let Err(error) = result {
            tracing::error!("{error:?}");
        }
        self.accumulated_pty_output.clear();
        self.wait_for_output_until = None;
        Ok(())
    }

    /// Some CLI applications need to know where the current cursor is, so that they can decide how
    /// to draw themselves. They request the cursor position from the host terminal emulator by
    /// sending the special code: `^[6n`. It is the responsibility of the terminal emulator to
    /// respond to this request with another ANSI code containing the coordinates of the cursor.
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "
            When I set this to `&self` then we get an actual compiler error that the `send()` method
            on the channel is not safe because it's not `Send`. I don't understand this.
        "
    )]
    async fn handle_cursor_position_request(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), crate::errors::ShadowTerminalError> {
        if Self::find_subsequence(bytes, CURSOR_POSITION_REQUEST.as_bytes()).is_none() {
            return Ok(());
        }

        let mut payload: crate::pty::BytesFromSTDIN = [0; 128];
        let cursor_position = self.terminal.cursor_pos();
        let response_string = format!("\x1b[{};{}R", cursor_position.y, cursor_position.x);
        let response_bytes = response_string.as_bytes();

        for chunk in response_bytes.chunks(128) {
            crate::pty::PTY::add_bytes_to_buffer(&mut payload, chunk).with_whatever_context(
                |error| format!("Couldn't add response to payload buffer: {error:?}"),
            )?;

            if let Some(sender) = self.channels.internal_input_tx.as_ref() {
                tracing::debug!(
                    "Responding to cursor position request with: {}",
                    response_string.replace('\x1b', "^")
                );
                let result = sender.send(payload).await;
                if let Err(error) = result {
                    snafu::whatever!("Couldn't send internal input: {error:?}");
                }
            }
        }

        Ok(())
    }

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
    /// Send the current state of the shadow terminal as a Termwiz surface or changeset to whoever
    /// is externally listening.
    async fn send_outputs(&mut self) -> Result<(), crate::errors::ShadowTerminalError> {
        let screen_output = self.build_current_output(&crate::output::SurfaceKind::Screen)?;
        self.send_output(screen_output).await?;

        if !self.terminal.is_alt_screen_active() {
            let scrollback_output =
                self.build_current_output(&crate::output::SurfaceKind::Scrollback)?;
            self.send_output(scrollback_output).await?;
        }

        self.last_sent = LastSent {
            pty_sequence: self.terminal.current_seqno(),
            pty_size: (self.terminal.get_size().cols, self.terminal.get_size().rows),
        };

        Ok(())
    }

    /// Send an individual output: scrollback or screen.
    #[expect(
        clippy::needless_pass_by_ref_mut,
        reason = "
            Weirdly, we get the following error when `mut` is not used:
              rustc: future cannot be sent between threads safely
              within `shadow_terminal::ShadowTerminal`, the trait `std::marker::Sync` is not implemented for `std::cell::RefCell<termwiz::escape::parser::ParseState>`
              if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` instead
        "
    )]
    async fn send_output(
        &mut self,
        output: crate::output::Output,
    ) -> Result<(), crate::errors::ShadowTerminalError> {
        let result = self.channels.shadow_output.send(output).await;
        if let Err(error) = result {
            tracing::error!("Sending shadow output: {error:?}");
            return Ok(());
        }

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

    /// Handle any messages from the internal control protocol
    async fn handle_protocol_message(&mut self, message: &crate::Protocol) {
        tracing::debug!("Shadow Terminal received protocol message: {message:?}");

        #[expect(clippy::wildcard_enum_match_arm, reason = "It's our internal protocol")]
        match message {
            crate::Protocol::Resize { width, height } => {
                self.terminal.resize(Self::wezterm_size(
                    usize::from(*width),
                    usize::from(*height),
                ));
                tracing::trace!("Wezterm terminal resized to: {width}x{height}");
            }
            crate::Protocol::Scroll(scroll) => {
                match scroll {
                    crate::Scroll::Up => {
                        let size = self.terminal.get_size();
                        let total_lines = self.terminal.screen().scrollback_rows() - size.rows;

                        self.scroll_position += self.config.scrollback_step;
                        self.scroll_position = self.scroll_position.min(total_lines);
                    }
                    crate::Scroll::Down => {
                        if self.scroll_position < self.config.scrollback_step {
                            self.scroll_position = 0;
                        } else {
                            self.scroll_position -= self.config.scrollback_step;
                        }
                    }
                    crate::Scroll::Cancel => {
                        self.scroll_position = 0;
                    }
                }

                let result = self.send_outputs().await;
                if let Err(error) = result {
                    tracing::error!("Couldn't send PTY output from shadow terminal: {error:?}");
                }
            }

            _ => (),
        }
    }

    /// Just a convenience wrapper around the native Wezterm type
    const fn wezterm_size(width: usize, height: usize) -> wezterm_term::TerminalSize {
        wezterm_term::TerminalSize {
            cols: width,
            rows: height,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        }
    }

    /// Resize the underlying PTY. That's the only way to send the resquired OS `SIGWINCH`.
    ///
    /// # Errors
    /// If the `Protocol::Resize` message cannot be sent.
    #[inline]
    pub fn resize(
        &mut self,
        width: u16,
        height: u16,
    ) -> Result<(), tokio::sync::broadcast::error::SendError<crate::Protocol>> {
        self.channels
            .control_tx
            .send(crate::Protocol::Resize { width, height })?;
        self.terminal
            .resize(Self::wezterm_size(width.into(), height.into()));
        Ok(())
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
