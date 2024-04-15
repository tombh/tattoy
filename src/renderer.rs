//! Render the output of the PTY and tattoys

use color_eyre::eyre::Result;
use termwiz::surface::Surface as TermwizSurface;
use termwiz::surface::{Change, Position};
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::Terminal as TermwizTerminal;
use tokio::sync::mpsc;

use crate::run::{SurfaceType, TattoySurface};

///
#[allow(clippy::exhaustive_structs)]
pub struct Renderer {
    /// The terminal's width
    pub width: usize,
    /// The terminal's height
    pub height: usize,
}

impl Renderer {
    /// Create a renderer to render to a user's terminal
    pub fn new() -> Result<Self> {
        let mut renderer = Self {
            width: 0,
            height: 0,
        };
        renderer.update_terminal_size()?;
        Ok(renderer)
    }

    /// Get the user's current terminal size
    /// TODO: Do we have to create a new Termwiz terminal every time?
    pub fn update_terminal_size(&mut self) -> Result<termwiz::terminal::ScreenSize> {
        let capabilities = termwiz::caps::Capabilities::new_from_env()?;
        let mut terminal = termwiz::terminal::new_terminal(capabilities)?;
        let size = terminal.get_screen_size()?;
        self.width = size.cols;
        self.height = size.rows;
        Ok(size)
    }

    /// Handle updates from the PTY and tattoys
    pub fn run(&mut self, mut surfaces: mpsc::UnboundedReceiver<TattoySurface>) -> Result<()> {
        let caps = termwiz::caps::Capabilities::new_from_env()?;
        let mut terminal = termwiz::terminal::new_terminal(caps)?;
        terminal.set_raw_mode()?;
        let mut output = BufferedTerminal::new(terminal)?;
        self.update_terminal_size()?;

        let mut background = TermwizSurface::new(self.width, self.height);
        let mut pty = TermwizSurface::new(self.width, self.height);
        let mut frame = TermwizSurface::new(self.width, self.height);

        while let Some(update) = surfaces.blocking_recv() {
            match update.kind {
                SurfaceType::BGSurface => background = update.surface,
                SurfaceType::PTYSurface => pty = update.surface,
            }

            frame.draw_from_screen(&background, 0, 0);
            let cells = pty.screen_cells();
            for (y, line) in cells.iter().enumerate() {
                for (x, cell) in line.iter().enumerate() {
                    let attrs = cell.attrs();
                    frame.add_change(Change::CursorPosition {
                        x: Position::Absolute(x),
                        y: Position::Absolute(y),
                    });
                    frame.add_changes(vec![
                        Change::Attribute(termwiz::cell::AttributeChange::Foreground(
                            attrs.foreground(),
                        )),
                        Change::Attribute(termwiz::cell::AttributeChange::Background(
                            attrs.background(),
                        )),
                    ]);

                    let character = cell.str();
                    if character != " " {
                        frame.add_change(character);
                    }
                }
            }

            let minimum_changes = output.diff_screens(&frame);
            output.add_changes(minimum_changes);

            let (x, y) = pty.cursor_position();
            output.add_change(Change::CursorPosition {
                x: Position::Absolute(x),
                y: Position::Absolute(y),
            });

            output.flush()?;
        }
        Ok(())
    }
}
