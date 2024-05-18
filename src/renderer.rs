//! Render the output of the PTY and tattoys

use std::sync::Arc;

use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use termwiz::surface::Surface as TermwizSurface;
use termwiz::surface::{Change as TermwizChange, Position as TermwizPosition};
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::{ScreenSize, Terminal as TermwizTerminal};

use crate::run::{FrameUpdate, Protocol};
use crate::shared_state::SharedState;

///
#[allow(clippy::exhaustive_structs)]
pub struct Renderer {
    /// Shared app state
    pub state: Arc<SharedState>,
    /// The terminal's width
    pub width: usize,
    /// The terminal's height
    pub height: usize,
    /// Merged tattoy surfaces
    pub background: TermwizSurface,
    /// A shadow version of the user's conventional terminal
    pub pty: TermwizSurface,
}

impl Renderer {
    /// Create a renderer to render to a user's terminal
    pub fn new(state: Arc<SharedState>) -> Result<Self> {
        let mut renderer = Self {
            state,
            width: Default::default(),
            height: Default::default(),
            background: TermwizSurface::default(),
            pty: TermwizSurface::default(),
        };

        renderer.update_terminal_size()?;
        Ok(renderer)
    }

    /// We need this just because I can't figure out how to pass `Box<dyn Terminal>` to
    /// `BufferedTerminal::new()`
    fn get_termwiz_terminal() -> Result<impl TermwizTerminal> {
        let capabilities = termwiz::caps::Capabilities::new_from_env()?;
        Ok(termwiz::terminal::new_terminal(capabilities)?)
    }

    /// Just for initialisation
    pub fn get_users_tty_size() -> Result<ScreenSize> {
        let mut terminal = Self::get_termwiz_terminal()?;
        Ok(terminal.get_screen_size()?)
    }

    /// Get the user's current terminal size and propogate it
    pub fn update_terminal_size(&mut self) -> Result<termwiz::terminal::ScreenSize> {
        let mut terminal = Self::get_termwiz_terminal()?;
        let size = terminal.get_screen_size()?;
        self.width = size.cols;
        self.height = size.rows;
        let mut tty_size = self
            .state
            .tty_size
            .write()
            .map_err(|err| color_eyre::eyre::eyre!("{err}"))?;
        tty_size.0 = self.width;
        tty_size.1 = self.height;
        drop(tty_size);
        self.background.resize(self.width, self.height);
        self.pty.resize(self.width, self.height);
        Ok(size)
    }

    /// Handle updates from the PTY and tattoys
    pub async fn run(
        &mut self,
        mut surfaces: mpsc::Receiver<FrameUpdate>,
        mut protocol: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        let mut terminal = Self::get_termwiz_terminal()?;
        terminal.set_raw_mode()?;
        let mut output = BufferedTerminal::new(terminal)?;

        #[allow(clippy::multiple_unsafe_ops_per_block)]
        while let Some(update) = surfaces.recv().await {
            self.render(update, &mut output)?;

            // TODO: should this be oneshot?
            if let Ok(message) = protocol.try_recv() {
                match message {
                    Protocol::END => {
                        break;
                    }
                };
            }
        }

        tracing::debug!("Renderer loop finished");
        Ok(())
    }

    /// Do a single render to the user's actual terminal. It uses a diffing algorithm to make
    /// the minimum number of changes.
    fn render(
        &mut self,
        update: FrameUpdate,
        output: &mut BufferedTerminal<impl TermwizTerminal>,
    ) -> Result<()> {
        let mut frame = TermwizSurface::new(self.width, self.height);

        match update {
            FrameUpdate::TattoySurface(surface) => self.background = surface,
            FrameUpdate::TattoyPixels(_) => (),
            FrameUpdate::PTYSurface(surface) => self.pty = surface,
        }

        frame.draw_from_screen(&self.background, 0, 0);

        let cells = self.pty.screen_cells();
        for (y, line) in cells.iter().enumerate() {
            for (x, cell) in line.iter().enumerate() {
                let attrs = cell.attrs();
                frame.add_changes(vec![
                    TermwizChange::CursorPosition {
                        x: TermwizPosition::Absolute(x),
                        y: TermwizPosition::Absolute(y),
                    },
                    TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(
                        attrs.foreground(),
                    )),
                    TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                        attrs.background(),
                    )),
                ]);

                let character = cell.str();
                if character != " " {
                    frame.add_change(character);
                }
            }
        }

        output.draw_from_screen(&frame, 0, 0);

        let (x, y) = self.pty.cursor_position();
        output.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(x),
            y: TermwizPosition::Absolute(y),
        });

        output.flush()?;
        Ok(())
    }
}
