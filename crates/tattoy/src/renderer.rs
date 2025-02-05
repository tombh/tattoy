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

    /// Instantiate and run
    pub fn start(
        state: Arc<SharedState>,
        surfaces_rx: mpsc::Receiver<FrameUpdate>,
        protocol_tx: tokio::sync::broadcast::Sender<Protocol>,
    ) -> tokio::task::JoinHandle<Result<()>> {
        let protocol_rx = protocol_tx.subscribe();
        tokio::spawn(async move {
            // This would be much simpler if async closures where stable, because then we could use
            // the `?` syntax.
            match Self::new(Arc::clone(&state)) {
                Ok(mut renderer) => {
                    let result = renderer
                        .run(surfaces_rx, protocol_rx, protocol_tx.clone())
                        .await;

                    if let Err(error) = result {
                        crate::run::broadcast_protocol_end(&protocol_tx);
                        return Err(error);
                    };
                }
                Err(error) => {
                    crate::run::broadcast_protocol_end(&protocol_tx);
                    return Err(error);
                }
            };

            Ok(())
        })
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

    /// Listen for surface updates from the PTY and any running tattoys.
    /// It lives in its own method so that we can catch any errors and ensure that the user's
    /// terminal is always returned to cooked mode.
    async fn run(
        &mut self,
        mut surfaces: mpsc::Receiver<FrameUpdate>,
        mut protocol_rx: tokio::sync::broadcast::Receiver<Protocol>,
        protocol_tx: tokio::sync::broadcast::Sender<Protocol>,
    ) -> Result<()> {
        tracing::debug!("Putting user's terminal into raw mode");
        let mut copy_of_users_terminal = Self::get_termwiz_terminal()?;
        copy_of_users_terminal.set_raw_mode()?;
        let mut composited_terminal = BufferedTerminal::new(copy_of_users_terminal)?;

        tracing::debug!("Starting render loop");
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "`tokio::select! generates this.`"
        )]
        loop {
            tokio::select! {
                Some(update) = surfaces.recv() => {
                    self.handle_resize(&mut composited_terminal, &protocol_tx)?;
                    self.render(update, &mut composited_terminal)?;
                }
                Ok(message) = protocol_rx.recv() => {
                    if matches!(message, Protocol::End) {
                        break;
                    }
                },
                else => { break }
            }
        }
        tracing::debug!("Exited render loop");

        tracing::debug!("Setting user's terminal to cooked mode");
        composited_terminal.terminal().set_cooked_mode()?;

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
            FrameUpdate::PTYSurface => self.get_updated_pty_frame()?,
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
                Self::add_cell(&mut new_frame, &tattoy_cells, x.into(), y.into())?;
                Self::add_cell(&mut new_frame, &pty_cells, x.into(), y.into())?;
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
    fn get_updated_pty_frame(&mut self) -> Result<()> {
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
    fn add_cell(
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
