//! An in-memory TTY renderer. It takes a stream of bytes and maintains the visual
//! appearance of the terminal without actually physically rendering it.

use std::sync::Arc;

use color_eyre::eyre::Result;
use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;
use tokio::sync::mpsc;

use crate::pty::StreamBytesFromPTY;
use crate::run::FrameUpdate;
use crate::run::Protocol;
use crate::shared_state::SharedState;

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

/// Private fields aren't relevant yet
pub(crate) struct ShadowTTY {
    /// The Wezterm terminal that does most of the actual work of maintaining the terminal 🙇
    terminal: wezterm_term::Terminal,
    /// Shared app state
    state: Arc<SharedState>,
}

impl ShadowTTY {
    /// Create a new Shadow TTY
    pub fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = crate::renderer::Renderer::get_users_tty_size()?;

        let terminal = wezterm_term::Terminal::new(
            wezterm_term::TerminalSize {
                cols: tty_size.cols,
                rows: tty_size.rows,
                pixel_width: 0,
                pixel_height: 0,
                dpi: 0,
            },
            std::sync::Arc::new(WeztermConfig { scrollback: 100 }),
            "Tattoy",
            "O_o",
            Box::<Vec<u8>>::default(),
        );

        Ok(Self { terminal, state })
    }

    /// Start listening to a stream of PTY bytes and render them to a shadow TTY surface
    pub async fn run(
        &mut self,
        mut pty_output: mpsc::Receiver<StreamBytesFromPTY>,
        shadow_output: &mpsc::Sender<FrameUpdate>,
        mut protocol_receive: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "`tokio::select! generates this.`"
        )]
        loop {
            tokio::select! {
                Some(pty_bytes) = pty_output.recv() => self.read_from_pty(&pty_bytes, shadow_output).await?,
                Ok(message) = protocol_receive.recv() => {
                    let is_exit = self.handle_protocol_message(&message);
                    if is_exit { break };
                }
                else => { break }
            }
        }

        tracing::debug!("ShadowTTY loop finished");
        Ok(())
    }

    // TODO:
    // The output of the PTY seems to be capped at 4095 bytes. Making the size of
    // `StreamBytesFromPTY` bigger than that doesn't seem to make a difference. This means
    // that large screen updates `self.build_current_surface()` can be called an
    // unnecessary number of times.
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
    /// Handle bytes from the PTY subprocess.
    async fn read_from_pty(
        &mut self,
        pty_output: &StreamBytesFromPTY,
        shadow_output: &mpsc::Sender<FrameUpdate>,
    ) -> Result<()> {
        self.terminal.advance_bytes(pty_output);
        let surface = self.build_current_surface()?;
        self.update_state_surface(surface)?;

        let result = shadow_output.send(FrameUpdate::PTYSurface).await;
        if let Err(err) = result {
            tracing::error!("Couldn't notify frame update channel about new PTY surface: {err:?}");
        }

        Ok(())
    }

    /// Handle any messages from the global Tattoy protocol
    fn handle_protocol_message(&mut self, message: &Protocol) -> bool {
        tracing::debug!("Shadow TTY received protocol message: {message:?}");
        match message {
            Protocol::End => {
                return false;
            }
            Protocol::Resize { width, height } => {
                self.terminal.resize(wezterm_term::TerminalSize {
                    cols: usize::from(*width),
                    rows: usize::from(*height),
                    pixel_width: 0,
                    pixel_height: 0,
                    dpi: 0,
                });
            }
        };

        true
    }

    /// Send the current PTY surface to the shared state.
    /// Needs to be in its own non-async function like this because of the error:
    ///   'future created by async block is not `Send`'
    fn update_state_surface(&self, surface: termwiz::surface::Surface) -> Result<()> {
        let mut shadow_tty = self
            .state
            .shadow_tty
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        *shadow_tty = surface;
        drop(shadow_tty);
        Ok(())
    }

    /// Converts Wezterms's maintained virtual TTY into a compositable Termwiz surface
    fn build_current_surface(&mut self) -> Result<termwiz::surface::Surface> {
        let tty_size = self.state.get_tty_size()?;
        let width = tty_size.width;
        let height = tty_size.height;
        let mut surface = termwiz::surface::Surface::new(width.into(), height.into());

        // TODO: Explore using this to improve performance:
        //   `self.terminal.screen().get_changed_stable_rows()`
        let screen = self.terminal.screen_mut();
        for row in 0..=height {
            for column in 0..=width {
                let row_i64: i64 = row.into();
                if let Some(cell) = screen.get_cell(column.into(), row_i64) {
                    let attrs = cell.attrs();
                    let cursor = TermwizChange::CursorPosition {
                        x: TermwizPosition::Absolute(column.into()),
                        y: TermwizPosition::Absolute(row.into()),
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
