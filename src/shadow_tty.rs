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
use crate::run::Protocol;
use crate::run::SurfaceType;
use crate::run::TattoySurface;
use crate::shared_state::SharedState;

/// Wezterm's internal configuration
#[derive(Debug)]
struct WeztermConfig {
    /// The number of lines to store in the scrollback
    scrollback: usize,
}

#[allow(clippy::missing_trait_methods)]
impl wezterm_term::TerminalConfiguration for WeztermConfig {
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }

    fn color_palette(&self) -> wezterm_term::color::ColorPalette {
        wezterm_term::color::ColorPalette::default()
    }
}

/// Private fields aren't relevant yet
pub struct ShadowTTY {
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
        let tty_size = state.get_tty_size()?;

        let terminal = wezterm_term::Terminal::new(
            wezterm_term::TerminalSize {
                cols: tty_size.0,
                rows: tty_size.1,
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
        mut pty_output: mpsc::UnboundedReceiver<StreamBytes>,
        shadow_output: &mpsc::UnboundedSender<TattoySurface>,
        mut protocol_receive: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        loop {
            #[allow(clippy::multiple_unsafe_ops_per_block)]
            if let Some(bytes) = pty_output.recv().await {
                self.terminal.advance_bytes(bytes);
                self.parse_bytes(bytes);
            };

            // TODO: should this be oneshot?
            if let Ok(message) = protocol_receive.try_recv() {
                match message {
                    Protocol::END => {
                        break;
                    }
                };
            }

            let (surface, surface_copy) = self.build_current_surface()?;
            let mut shadow_tty = self
                .state
                .shadow_tty
                .write()
                .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
            *shadow_tty = surface;
            drop(shadow_tty);

            shadow_output.send(TattoySurface {
                kind: SurfaceType::PTYSurface,
                surface: surface_copy,
            })?;
        }

        tracing::debug!("ShadowTTY loop finished");
        Ok(())
    }

    /// Parse PTY bytes
    /// Just logging for now. But we could do some Tattoy-specific things with this. Like a Tattoy
    /// keyboard shortcut that switches the active tattoy.
    fn parse_bytes(&mut self, bytes: StreamBytes) {
        #[allow(clippy::wildcard_enum_match_arm)]
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
    #[allow(clippy::cast_possible_wrap)]
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::as_conversions)]
    fn build_current_surface(
        &mut self,
    ) -> Result<(termwiz::surface::Surface, termwiz::surface::Surface)> {
        let tty_size = self.state.get_tty_size()?;
        let width = tty_size.0;
        let height = tty_size.1;
        let mut surface1 = termwiz::surface::Surface::new(width, height);
        let mut surface2 = termwiz::surface::Surface::new(width, height);

        let screen = self.terminal.screen_mut();
        for row in 0..=height {
            for column in 0..=width {
                if let Some(cell) = screen.get_cell(column, row as i64) {
                    let attrs = cell.attrs();
                    let cursor = TermwizChange::CursorPosition {
                        x: TermwizPosition::Absolute(column),
                        y: TermwizPosition::Absolute(row),
                    };
                    surface1.add_change(cursor.clone());
                    surface2.add_change(cursor);

                    let colours = vec![
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(
                            attrs.foreground(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                            attrs.background(),
                        )),
                    ];
                    surface1.add_changes(colours.clone());
                    surface2.add_changes(colours);

                    let contents = cell.str();
                    surface1.add_change(contents);
                    surface2.add_change(contents);
                }
            }
        }

        let users_cursor = self.terminal.cursor_pos();
        let cursor = TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(users_cursor.x),
            y: TermwizPosition::Absolute(users_cursor.y as usize),
        };
        surface1.add_change(cursor.clone());
        surface2.add_change(cursor);

        Ok((surface1, surface2))
    }
}
