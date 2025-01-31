//! An in-memory TTY renderer. It takes a stream of bytes and maintains the visual
//! appearance of the terminal without actually physically rendering it.

use std::sync::Arc;

use color_eyre::eyre::Result;
use termwiz::escape::parser::Parser as TermwizParser;
use termwiz::escape::Action as TermwizAction;
use termwiz::surface::Change as TermwizChange;
use termwiz::surface::Position as TermwizPosition;
use tokio::sync::mpsc;

use crate::pty::StreamBytes;
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
    /// Parser that detects all the weird and wonderful TTY conventions
    parser: TermwizParser,
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

        Ok(Self {
            terminal,
            parser: TermwizParser::new(),
            state,
        })
    }

    /// Start listening to a stream of PTY bytes and render them to a shadow TTY surface
    pub async fn run(
        &mut self,
        mut pty_output: mpsc::Receiver<StreamBytes>,
        shadow_output: &mpsc::Sender<FrameUpdate>,
        mut protocol_receive: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        loop {
            if let Some(bytes) = pty_output.recv().await {
                // Note: I'm surprised that the bytes here aren't able to resize the terminal?
                self.terminal.advance_bytes(bytes);
                self.parse_bytes(bytes);
            };

            // TODO: should this be oneshot?
            if let Ok(message) = protocol_receive.try_recv() {
                match message {
                    Protocol::End => {
                        break;
                    }
                    Protocol::Resize { width, height } => {
                        self.terminal.resize(wezterm_term::TerminalSize {
                            cols: width.into(),
                            rows: height.into(),
                            pixel_width: 0,
                            pixel_height: 0,
                            dpi: 0,
                        });
                    }
                };
            }

            let surface = self.build_current_surface()?;
            self.update_state_surface(surface)?;

            shadow_output.send(FrameUpdate::PTYSurface).await?;
        }

        tracing::debug!("ShadowTTY loop finished");
        Ok(())
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

    /// Parse PTY output bytes. Just logging for now.
    /// Because this is the output of the PTY I don't think we can use it for intercepting
    /// Tattoy-specific keybindings.
    fn parse_bytes(&mut self, bytes: StreamBytes) {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "We're not doing anything dangerous with the wildcard match"
        )]
        self.parser.parse(&bytes, |action| match action {
            TermwizAction::Print(character) => tracing::trace!("{character}"),
            TermwizAction::Control(character) => match character {
                termwiz::escape::ControlCode::HorizontalTab
                | termwiz::escape::ControlCode::LineFeed
                | termwiz::escape::ControlCode::CarriageReturn => {
                    tracing::trace!("{character:?}");
                }
                _ => {}
            },
            TermwizAction::CSI(csi) => {
                tracing::trace!("{csi:?}");
            }
            wild => {
                tracing::trace!("{wild:?}");
            }
        });
    }

    /// Converts Wezterms's maintained virtual TTY into a compositable Termwiz surface
    fn build_current_surface(&mut self) -> Result<termwiz::surface::Surface> {
        let tty_size = self.state.get_tty_size()?;
        let width = tty_size.width;
        let height = tty_size.height;
        let mut surface = termwiz::surface::Surface::new(width.into(), height.into());

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
                    surface.add_change(cursor.clone());

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
                    ];
                    surface.add_changes(attributes.clone());

                    let contents = cell.str();
                    surface.add_change(contents);
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
