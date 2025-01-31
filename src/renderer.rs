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
    pub width: u16,
    /// The terminal's height
    pub height: u16,
    /// Merged tattoy surfaces
    pub tattoys: TermwizSurface,
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
            tattoys: TermwizSurface::default(),
            pty: TermwizSurface::default(),
        };

        let size = Self::get_users_tty_size()?;
        renderer.width = size.cols.try_into()?;
        renderer.height = size.rows.try_into()?;

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
    pub fn handle_resize<T: TermwizTerminal>(
        &mut self,
        composited_terminal: &mut BufferedTerminal<T>,
        protocol_tx: &tokio::sync::broadcast::Sender<Protocol>,
    ) -> Result<()> {
        let is_resized = composited_terminal.check_for_resize()?;
        if !is_resized {
            return Ok(());
        }

        composited_terminal.repaint()?;

        let (width, height) = composited_terminal.dimensions();
        self.width = width.try_into()?;
        self.height = height.try_into()?;
        self.state.set_tty_size(self.width, self.height)?;
        protocol_tx.send(Protocol::Resize {
            width: self.width,
            height: self.height,
        })?;

        Ok(())

        // Note: there's no reason to resize the existing `self.pty` and `self.tattoys` because
        // they're just old copies. There's no point resizing them if their contents' aren't also
        // going to be resized. So instead we just wait for new updates from each one, which should
        // be of the right size.
    }

    /// Handle updates from the PTY and tattoys
    pub async fn run(
        &mut self,
        mut surfaces: mpsc::Receiver<FrameUpdate>,
        mut protocol_rx: tokio::sync::broadcast::Receiver<Protocol>,
        protocol_tx: tokio::sync::broadcast::Sender<Protocol>,
    ) -> Result<()> {
        let mut copy_of_users_terminal = Self::get_termwiz_terminal()?;
        copy_of_users_terminal.set_raw_mode()?;
        let mut composited_terminal = BufferedTerminal::new(copy_of_users_terminal)?;

        while let Some(update) = surfaces.recv().await {
            self.handle_resize(&mut composited_terminal, &protocol_tx)?;
            self.render(update, &mut composited_terminal)?;

            // TODO: should this be oneshot?
            if let Ok(message) = protocol_rx.try_recv() {
                match message {
                    Protocol::End => {
                        break;
                    }
                    // I AM THE ONE WHO KNOCKS!
                    // (we sent the resize event so we've already handled it)
                    Protocol::Resize { .. } => (),
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
        composited_terminal: &mut BufferedTerminal<impl TermwizTerminal>,
    ) -> Result<()> {
        match update {
            FrameUpdate::TattoySurface(surface) => self.tattoys = surface,
            FrameUpdate::PTYSurface => self.get_updated_pty()?,
        }

        if !self.are_dimensions_good("PTY", &self.pty.screen_lines()) {
            return Ok(());
        }
        if !self.are_dimensions_good("Tattoy", &self.tattoys.screen_lines()) {
            return Ok(());
        }

        let pty_cells = self.pty.screen_cells();
        let tattoy_cells = self.tattoys.screen_cells();

        let mut new_frame = TermwizSurface::new(self.width.into(), self.height.into());
        for y in 0..self.height {
            for x in 0..self.width {
                Self::build_cell(&mut new_frame, &tattoy_cells, x.into(), y.into())?;
                Self::build_cell(&mut new_frame, &pty_cells, x.into(), y.into())?;
            }
        }
        composited_terminal.draw_from_screen(&new_frame, 0, 0);

        let (cursor_x, cursor_y) = self.pty.cursor_position();
        composited_terminal.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(cursor_x),
            y: TermwizPosition::Absolute(cursor_y),
        });

        // This is where we actually render to the user's real terminal.
        composited_terminal.flush()?;

        Ok(())
    }

    /// Fetch the freshly made PTY frame from the shared state.
    fn get_updated_pty(&mut self) -> Result<()> {
        let surface = self
            .state
            .shadow_tty
            .read()
            .map_err(|err| color_eyre::eyre::eyre!("{err:?}"))?;
        let size = surface.dimensions();
        let (cursor_x, cursor_y) = surface.cursor_position();
        self.pty.draw_from_screen(&surface, 0, 0);
        drop(surface);

        self.pty.resize(size.0, size.1);
        self.pty.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(cursor_x),
            y: TermwizPosition::Absolute(cursor_y),
        });

        Ok(())
    }

    /// Check to see if the incoming frame update is the same size as the user's current terminal.
    fn are_dimensions_good(
        &self,
        kind: &str,
        lines: &[std::borrow::Cow<wezterm_term::Line>],
    ) -> bool {
        if lines.is_empty() {
            tracing::debug!("Not rendering frame because {kind} update is empty");
            return false;
        }

        let update_height = lines.len();
        #[expect(
            clippy::indexing_slicing,
            reason = "The `if` clause above proves that at least index 0 exists"
        )]
        let update_width = lines[0].len();
        if update_height < self.height.into() || update_width < self.width.into() {
            tracing::debug!(
                "Not rendering Tattoy update because dimensions don't match: TTY {}x{}, Tattoy {}x{}",
                self.width,
                self.height,
                update_width,
                update_height,

            );
            return false;
        }

        true
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

        // TODO: is there a more elegant way to copy over all the attributes?
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
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Intensity(
                cell.attrs().intensity(),
            )),
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Italic(
                cell.attrs().italic(),
            )),
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Underline(
                cell.attrs().underline(),
            )),
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Blink(cell.attrs().blink())),
            TermwizChange::Attribute(termwiz::cell::AttributeChange::Reverse(
                cell.attrs().reverse(),
            )),
            TermwizChange::Attribute(termwiz::cell::AttributeChange::StrikeThrough(
                cell.attrs().strikethrough(),
            )),
        ]);
        frame.add_change(character);
        Ok(())
    }
}
