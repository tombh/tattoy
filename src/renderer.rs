//! Render the output of the PTY and tattoys

use std::sync::Arc;

use color_eyre::eyre::{ContextCompat as _, Result};
use tokio::sync::mpsc;

use termwiz::surface::Surface as TermwizSurface;
use termwiz::surface::{Change as TermwizChange, Position as TermwizPosition};
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::{ScreenSize, Terminal as TermwizTerminal};
use wezterm_term::Cell;

use crate::run::{FrameUpdate, Protocol};
use crate::shared_state::SharedState;

/// `Render`
pub(crate) struct Renderer {
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

        let (cursor_x, cursor_y) = self.pty.cursor_position();

        let pty_cells = self.pty.screen_cells();
        let tattoy_cells = self.background.screen_cells();
        for y in 0..self.height {
            for x in 0..self.width {
                if x == cursor_x && y == cursor_y {
                    continue;
                }

                Self::build_cell(&mut frame, &tattoy_cells, x, y)?;
                Self::build_cell(&mut frame, &pty_cells, x, y)?;
            }
        }

        output.draw_from_screen(&frame, 0, 0);
        output.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(cursor_x),
            y: TermwizPosition::Absolute(cursor_y),
        });
        output.flush()?;

        Ok(())
    }

    /// Add a single cell to the frame
    fn build_cell(
        frame: &mut TermwizSurface,
        cells: &[&mut [Cell]],
        x: usize,
        y: usize,
    ) -> Result<()> {
        let cell = &cells
            .get(y)
            .context("No y coord for cell")?
            .get(x)
            .context("No x coord for cell")?;
        let character = cell.str();
        let is_cell_bg_default = matches!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );
        if character == " " && is_cell_bg_default {
            return Ok(());
        }

        frame.add_changes(vec![
            TermwizChange::CursorPosition {
                x: TermwizPosition::Absolute(x),
                y: TermwizPosition::Absolute(y),
            },
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Foreground(
                cell.attrs().foreground(),
            )),
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Background(
                cell.attrs().background(),
            )),
        ]);
        frame.add_change(character);
        Ok(())
    }
}
