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
use crate::run::SurfaceType;
use crate::run::TattoySurface;

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
    /// The terminal's height
    height: usize,
    /// The terminal's width
    width: usize,
}

impl ShadowTTY {
    /// Create a new Shadow TTY
    #[must_use]
    pub fn new(height: usize, width: usize) -> Self {
        let terminal = wezterm_term::Terminal::new(
            wezterm_term::TerminalSize {
                rows: height,
                cols: width,
                pixel_width: 0,
                pixel_height: 0,
                dpi: 0,
            },
            Arc::new(WeztermConfig { scrollback: 100 }),
            "Tattoy",
            "O_o",
            Box::<Vec<u8>>::default(),
        );

        Self {
            terminal,
            parser: TermwizParser::new(),
            height,
            width,
        }
    }

    /// Start listening to a stream of PTY bytes and render them to a shadow TTY surface
    pub async fn run(
        &mut self,
        mut pty_output: mpsc::UnboundedReceiver<StreamBytes>,
        shadow_output: &mpsc::UnboundedSender<TattoySurface>,
    ) -> Result<()> {
        loop {
            #[allow(clippy::multiple_unsafe_ops_per_block)]
            if let Some(bytes) = pty_output.recv().await {
                self.terminal.advance_bytes(bytes);
                self.parse_bytes(bytes);
            };

            shadow_output.send(TattoySurface {
                kind: SurfaceType::PTYSurface,
                surface: self.build_current_surface(),
            })?;
        }
    }

    /// Parse PTY bytes
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
    fn build_current_surface(&mut self) -> termwiz::surface::Surface {
        let mut surface = termwiz::surface::Surface::new(self.width, self.height);

        let screen = self.terminal.screen_mut();
        for row in 0..=self.height {
            for column in 0..=self.width {
                if let Some(cell) = screen.get_cell(column, row as i64) {
                    let attrs = cell.attrs();
                    surface.add_change(TermwizChange::CursorPosition {
                        x: TermwizPosition::Absolute(column),
                        y: TermwizPosition::Absolute(row),
                    });
                    surface.add_changes(vec![
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(
                            attrs.foreground(),
                        )),
                        TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                            attrs.background(),
                        )),
                    ]);
                    surface.add_change(cell.str());
                }
            }
        }

        let cursor = self.terminal.cursor_pos();
        surface.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(cursor.x),
            y: TermwizPosition::Absolute(cursor.y as usize),
        });

        surface
    }
}
