//! Render the output of the PTY and tattoys

use std::sync::Arc;

use color_eyre::eyre::{ContextCompat as _, Result};
use termwiz::cell::Cell;

use termwiz::surface::Surface as TermwizSurface;
use termwiz::surface::{Change as TermwizChange, Position as TermwizPosition};
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::{ScreenSize, Terminal as TermwizTerminal};

use crate::run::FrameUpdate;
use crate::shared_state::SharedState;

/// The number of microseconds in a second.
pub const ONE_MICROSECOND: u64 = 1_000_000;

/// The number of milliseconds in a second.
pub const MILLIS_PER_SECOND: f32 = 1_000.0;

/// The maximum number of unrendered frames to keep in the renderer's backlog.
///
/// When the renderer starts struggling such that it can't render a frame before the next one
/// arrives, the frame messaging channel will keep a backlog of frame updates. To avoid congestion,
/// and thus visual stuttering, the backlog is always merely drained wihtout rendering. Each frame
/// from the draining is used to update the latest copy of the various PTY outputs, Tattoys etc.
/// Only when the backlog reaches 0 does actual rendering restart.
///
/// Once the backlog is full, further frames are dropped, never to be rendered. This would also
/// cause stuttering, but is necessary to prevent completely crashing the app.
///
/// The backlog should only really ever get filled on either notably large terminals or notably
/// slow hardware. So its size should normally only need to be informed by what a reasonable
/// buffer of frames is for extreme conditions. 100 frames should give about 3 seconds of grace.
const MAX_FRAME_BACKLOG: usize = 100;

/// `Render`
#[derive(Default)]
pub(crate) struct Renderer {
    /// Shared app state
    pub state: Arc<SharedState>,
    /// The terminal's width
    pub width: u16,
    /// The terminal's height
    pub height: u16,
    /// Merged tattoy surfaces
    pub tattoys: std::collections::HashMap<String, crate::surface::Surface>,
    /// A shadow version of the user's conventional terminal
    pub pty: TermwizSurface,
    /// Is the cursor currently visible?
    pub is_cursor_visible: bool,
}

impl Renderer {
    /// Create a renderer to render to a user's terminal
    pub fn new(state: Arc<SharedState>) -> Result<Self> {
        let size = Self::get_users_tty_size()?;
        let width = size.cols.try_into()?;
        let height = size.rows.try_into()?;
        let renderer = Self {
            state,
            width,
            height,
            tattoys: std::collections::HashMap::default(),
            pty: TermwizSurface::new(width.into(), height.into()),
            is_cursor_visible: true,
        };

        Ok(renderer)
    }

    /// Instantiate and run
    pub fn start(
        state: Arc<SharedState>,
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> (
        tokio::task::JoinHandle<Result<()>>,
        tokio::sync::mpsc::Sender<FrameUpdate>,
    ) {
        let (surfaces_tx, surfaces_rx) = tokio::sync::mpsc::channel(MAX_FRAME_BACKLOG);
        let handle = tokio::spawn(async move {
            // This would be much simpler if async closures where stable, because then we could use
            // the `?` syntax.
            match Self::new(Arc::clone(&state)) {
                Ok(mut renderer) => {
                    let result = renderer.run(surfaces_rx, protocol_tx.clone()).await;

                    if let Err(error) = result {
                        crate::run::broadcast_protocol_end(&protocol_tx);
                        return Err(error);
                    }
                }
                Err(error) => {
                    crate::run::broadcast_protocol_end(&protocol_tx);
                    return Err(error);
                }
            }

            Ok(())
        });

        (handle, surfaces_tx)
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
    pub async fn check_for_user_resize<T: TermwizTerminal + Send>(
        &mut self,
        composited_terminal: &mut BufferedTerminal<T>,
        protocol_tx: &tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<()> {
        let is_resized = composited_terminal.check_for_resize()?;
        if !is_resized {
            return Ok(());
        }

        composited_terminal.repaint()?;

        let (width, height) = composited_terminal.dimensions();
        self.width = width.try_into()?;
        self.height = height.try_into()?;
        self.state.set_tty_size(self.width, self.height).await;
        protocol_tx.send(crate::run::Protocol::Resize {
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
        mut surfaces: tokio::sync::mpsc::Receiver<FrameUpdate>,
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<()> {
        tracing::debug!("Putting user's terminal into raw mode");
        let mut protocol_rx = protocol_tx.subscribe();
        let mut copy_of_users_terminal = Self::get_termwiz_terminal()?;
        copy_of_users_terminal.set_raw_mode()?;
        let mut composited_terminal = BufferedTerminal::new(copy_of_users_terminal)?;

        tracing::debug!("Starting render loop");
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "`tokio::select!` generates this."
        )]
        loop {
            tokio::select! {
                // For some reason `surfaces.recv()` doesn't let the Tokio scheduler do its thing.
                // Even though there might be a new frame, the scheduler doesn't seem to trigger.
                // So instead we just force the scheduler with this timer.
                () = tokio::time::sleep(tokio::time::Duration::from_micros(1)) => {
                    if surfaces.is_empty() {
                        continue;
                    }
                    self.handle_frame_update(&mut surfaces, &mut composited_terminal, &protocol_tx).await?;
                },
                Ok(message) = protocol_rx.recv() => {
                    self.handle_protocol_message(&message);
                    if matches!(message, crate::run::Protocol::End) {
                        break;
                    }
                }
            }
        }
        tracing::debug!("Exited render loop");

        tracing::debug!("Setting user's terminal to cooked mode");
        composited_terminal.terminal().set_cooked_mode()?;

        Ok(())
    }

    /// Handle PTY output and all Tattoy frames.
    async fn handle_frame_update(
        &mut self,
        surfaces: &mut tokio::sync::mpsc::Receiver<FrameUpdate>,
        composited_terminal: &mut BufferedTerminal<impl TermwizTerminal + Send>,
        protocol_tx: &tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<()> {
        let Some(update) = surfaces.recv().await else {
            return Ok(());
        };

        self.check_for_user_resize(composited_terminal, protocol_tx)
            .await?;
        self.render(surfaces.len(), update, composited_terminal)
            .await?;

        Ok(())
    }

    /// Handle messages from the global Tattoy protocol.
    const fn handle_protocol_message(&mut self, message: &crate::run::Protocol) {
        if let crate::run::Protocol::CursorVisibility(is_visible) = message {
            self.is_cursor_visible = *is_visible;
        }
    }

    /// Do a single render to the user's actual terminal. It uses a diffing algorithm to make
    /// the minimum number of changes.
    async fn render(
        &mut self,
        backlog: usize,
        update: FrameUpdate,
        composited_terminal: &mut BufferedTerminal<impl TermwizTerminal + Send>,
    ) -> Result<()> {
        match update {
            FrameUpdate::TattoySurface(surface) => {
                let surface_id = surface.id.clone();
                self.tattoys.insert(surface_id.clone(), surface);
                if surface_id != "random_walker" && surface_id != "shaders" {
                    tracing::trace!("Rendering {} frame update", surface_id);
                }
            }
            FrameUpdate::PTYSurface => {
                tracing::trace!("Rendering PTY frame update");
                self.get_updated_pty_frame().await;
            }
        }

        if backlog > 0 {
            if backlog > 5 {
                tracing::warn!("Backlog: {backlog}");
            }
            return Ok(());
        }

        let new_frame = self.composite().await?;

        // Hide the cursor without flushing.
        composited_terminal.add_change(TermwizChange::CursorVisibility(
            termwiz::surface::CursorVisibility::Hidden,
        ));

        let changes = composited_terminal.diff_screens(&new_frame);
        composited_terminal.add_changes(changes);

        let (cursor_x, cursor_y) = self.pty.cursor_position();
        composited_terminal.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(cursor_x),
            y: TermwizPosition::Absolute(cursor_y),
        });

        if let Some(cursor_shape) = self.pty.cursor_shape() {
            composited_terminal.add_change(TermwizChange::CursorShape(cursor_shape));
        }

        // This avoids flickering at the cost of slower rendering for complex frame updates.
        composited_terminal.ignore_high_repaint_cost(true);

        // Set the user's cursor visibility to the current PTY's cursor visibility.
        composited_terminal.add_change(TermwizChange::CursorVisibility(
            self.pty.cursor_visibility(),
        ));

        // Tattoy can override the PTY's cursor visibility for example when Tattoy is scrolling.
        if !self.is_cursor_visible {
            composited_terminal.add_change(TermwizChange::CursorVisibility(
                termwiz::surface::CursorVisibility::Hidden,
            ));
        }

        // This is where we actually render to the user's real terminal.
        composited_terminal.flush()?;

        Ok(())
    }

    // TODO: A failed render shouldn't crash the whole tick.
    /// Composite all the tattoys and the PTY together into a single surface (frame).
    async fn composite(&mut self) -> Result<TermwizSurface> {
        let mut surface = TermwizSurface::new(self.width.into(), self.height.into());
        let mut frame = surface.screen_cells();

        if *self.state.is_rendering_enabled.read().await {
            self.render_tattoys_below(&mut frame)?;
        }

        if self.is_a_plugin_replacing_the_pty_layer() {
            self.render_tattoys(&mut frame, std::cmp::Ordering::Equal)?;
        } else {
            self.render_pty(&mut frame)?;
        }

        if *self.state.is_rendering_enabled.read().await {
            self.render_tattoys_above(&mut frame)?;
            self.colour_grade(&mut frame).await?;
        }

        Ok(surface)
    }

    /// Are any of the tattoys replacing the PTY layer?
    fn is_a_plugin_replacing_the_pty_layer(&self) -> bool {
        self.tattoys.values().any(|tattoy| tattoy.layer == 0)
    }

    /// Render all the tattoys that appear below the PTY.
    fn render_tattoys_below(&mut self, frame: &mut Vec<&mut [Cell]>) -> Result<()> {
        self.render_tattoys(frame, std::cmp::Ordering::Less)
    }

    /// Render all the tattoys that appear above the PTY.
    fn render_tattoys_above(&mut self, frame: &mut Vec<&mut [Cell]>) -> Result<()> {
        self.render_tattoys(frame, std::cmp::Ordering::Greater)
    }

    /// Render a tattoy onto the compositor frame.
    fn render_tattoys(
        &mut self,
        frame: &mut Vec<&mut [Cell]>,
        comparator: std::cmp::Ordering,
    ) -> Result<()> {
        let mut tattoys: Vec<&mut crate::surface::Surface> = self
            .tattoys
            .values_mut()
            .filter(|tattoy| tattoy.layer.cmp(&0) == comparator)
            .collect();
        tattoys.sort_by_key(|tattoy| tattoy.layer);

        for tattoy in &mut tattoys {
            let tattoy_frame_size = tattoy.surface.dimensions();
            let tattoy_cells = tattoy.surface.screen_cells();

            for y in 0..self.height {
                for x in 0..self.width {
                    if usize::from(x) < tattoy_frame_size.0 && usize::from(y) < tattoy_frame_size.1
                    {
                        Self::composite_cell(frame, &tattoy_cells, x.into(), y.into())?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Render the PTY to the compositor frame.
    fn render_pty(&mut self, frame: &mut Vec<&mut [Cell]>) -> Result<()> {
        let pty_frame_size = self.pty.dimensions();
        let pty_cells = self.pty.screen_cells();

        for y in 0..self.height {
            for x in 0..self.width {
                if usize::from(x) < pty_frame_size.0 && usize::from(y) < pty_frame_size.1 {
                    Self::composite_cell(frame, &pty_cells, x.into(), y.into())?;
                }
            }
        }

        Ok(())
    }

    /// Fetch the freshly made PTY frame from the shared state.
    async fn get_updated_pty_frame(&mut self) {
        self.pty.resize(self.width.into(), self.height.into());
        let surface = self.state.shadow_tty_screen.read().await;
        let (cursor_x, cursor_y) = surface.cursor_position();
        self.pty = surface.clone();
        drop(surface);

        self.pty.add_change(TermwizChange::CursorPosition {
            x: TermwizPosition::Absolute(cursor_x),
            y: TermwizPosition::Absolute(cursor_y),
        });
    }

    /// Add a single cell to the compositor frame.
    fn composite_cell(
        base: &mut Vec<&mut [Cell]>,
        frame: &[&mut [Cell]],
        x: usize,
        y: usize,
    ) -> Result<()> {
        let composited_cell = base
            .get_mut(y)
            .context(format!("No y coord ({y}) for cell"))?
            .get_mut(x)
            .context(format!("No x coord ({x}) for cell"))?;
        let cell_above = frame
            .get(y)
            .context(format!("No y coord ({y}) for cell"))?
            .get(x)
            .context(format!("No x coord ({x}) for cell"))?;

        let character_above = cell_above.str().to_owned();
        let is_character_above_text = !character_above.is_empty() && character_above != " ";
        if is_character_above_text {
            let old_background = composited_cell.attrs().background();
            let old_foreground = composited_cell.attrs().foreground();
            *composited_cell = cell_above.clone();
            composited_cell.attrs_mut().set_background(old_background);
            composited_cell.attrs_mut().set_foreground(old_foreground);
        }

        let mut opaque = crate::opaque_cell::OpaqueCell::new(composited_cell, None);
        opaque.blend_all(cell_above);

        Ok(())
    }

    /// Apply colour changes, like saturation, hue, contrast, etc.
    //
    // TODO: consider including this in the final compositing layer, just for the performance
    // gain of not having to iterate over every cell again.
    async fn colour_grade(&self, frame: &mut Vec<&mut [Cell]>) -> Result<()> {
        let config = self.state.config.read().await;

        let saturation: f64 = config.color.saturation.into();
        let light: f64 = config.color.brightness.into();
        let hue: f64 = config.color.hue.into();
        drop(config);

        for line in &mut frame.iter_mut() {
            for cell in line.iter_mut() {
                let foreground = cell.attrs().foreground();
                if let Some(mut gradable) =
                    crate::opaque_cell::OpaqueCell::extract_colour(foreground)
                {
                    gradable = gradable.saturate(saturation);
                    gradable = gradable.lighten(light);
                    gradable = gradable.adjust_hue_fixed(hue);
                    cell.attrs_mut().set_foreground(
                        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(gradable),
                    );
                }

                let background = cell.attrs().background();
                if let Some(mut gradable) =
                    crate::opaque_cell::OpaqueCell::extract_colour(background)
                {
                    gradable = gradable.saturate(saturation);
                    gradable = gradable.lighten(light);
                    gradable = gradable.adjust_hue_fixed(hue);
                    cell.attrs_mut().set_background(
                        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(gradable),
                    );
                }
            }
        }

        Ok(())
    }
}

#[expect(
    clippy::indexing_slicing,
    clippy::unreadable_literal,
    reason = "Tests aren't so strict"
)]
#[cfg(test)]
mod test {
    use super::*;

    async fn make_renderer() -> Renderer {
        let renderer = Renderer {
            width: 1,
            height: 1,
            ..Renderer::default()
        };
        *renderer.state.is_rendering_enabled.write().await = true;
        renderer
    }

    async fn blend_pixels(
        first: (usize, usize, crate::surface::Colour),
        second: (usize, usize, crate::surface::Colour),
    ) -> Cell {
        let mut renderer = make_renderer().await;
        let mut tattoy_below = crate::surface::Surface::new("below".into(), 1, 1, 1);
        tattoy_below.add_pixel(first.0, first.1, first.2).unwrap();
        renderer
            .tattoys
            .insert(tattoy_below.id.clone(), tattoy_below);

        let mut tattoy_above = crate::surface::Surface::new("above".into(), 1, 1, 2);
        tattoy_above
            .add_pixel(second.0, second.1, second.2)
            .unwrap();
        renderer
            .tattoys
            .insert(tattoy_above.id.clone(), tattoy_above);

        let mut new_frame = renderer.composite().await.unwrap();
        let cell = &new_frame.screen_cells()[0][0];
        assert_eq!(cell.str(), "▀");

        cell.clone()
    }

    #[tokio::test]
    async fn blending_text() {
        let mut renderer = make_renderer().await;
        let mut tattoy_below = crate::surface::Surface::new("below".into(), 1, 1, 1);
        tattoy_below.add_text(
            0,
            0,
            "a".into(),
            Some(crate::surface::RED),
            Some(crate::surface::WHITE),
        );
        renderer
            .tattoys
            .insert(tattoy_below.id.clone(), tattoy_below);

        let mut tattoy_above = crate::surface::Surface::new("above".into(), 1, 1, 2);
        tattoy_above.add_text(0, 0, " ".into(), Some((0.0, 0.0, 0.0, 0.5)), None);
        renderer
            .tattoys
            .insert(tattoy_above.id.clone(), tattoy_above);

        let mut new_frame = renderer.composite().await.unwrap();
        let cell = &new_frame.screen_cells()[0][0];

        assert_eq!(cell.str(), "a");
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(0.6666667, 0.6666667, 0.6666667, 1.0)
            )
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(0.6666667, 0.0, 0.0, 1.0)
            )
        );
    }

    #[tokio::test]
    async fn blending_text_with_default_bg_below() {
        let mut renderer = make_renderer().await;
        let mut tattoy_below = crate::surface::Surface::new("below".into(), 1, 1, 1);
        tattoy_below.add_text(0, 0, "a".into(), None, Some(crate::surface::WHITE));
        renderer
            .tattoys
            .insert(tattoy_below.id.clone(), tattoy_below);

        let mut tattoy_above = crate::surface::Surface::new("above".into(), 1, 1, 2);
        tattoy_above.add_text(0, 0, " ".into(), Some((1.0, 1.0, 1.0, 0.5)), None);
        renderer
            .tattoys
            .insert(tattoy_above.id.clone(), tattoy_above);

        let mut new_frame = renderer.composite().await.unwrap();
        let cell = &new_frame.screen_cells()[0][0];

        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(0.33333334, 0.33333334, 0.33333334, 1.0)
            )
        );
    }

    #[tokio::test]
    async fn fg_bg_pixels_in_same_cell_dont_blend() {
        let cell = blend_pixels((0, 0, crate::surface::WHITE), (0, 1, crate::surface::RED)).await;
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 1.0, 1.0, 1.0)
            )
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 0.0, 0.0, 1.0)
            )
        );
    }

    #[tokio::test]
    async fn foreground_pixels_without_alpha_dont_blend() {
        let cell = blend_pixels((0, 0, crate::surface::RED), (0, 0, crate::surface::WHITE)).await;
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 1.0, 1.0, 1.0)
            )
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );
    }

    #[tokio::test]
    async fn background_pixels_without_alpha_dont_blend() {
        let cell = blend_pixels((0, 1, crate::surface::RED), (0, 1, crate::surface::WHITE)).await;
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::Default
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 1.0, 1.0, 1.0)
            )
        );
    }

    #[tokio::test]
    async fn foreground_pixels_with_alpha_blend() {
        let cell = blend_pixels((0, 0, crate::surface::RED), (0, 0, (1.0, 1.0, 1.0, 0.5))).await;
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 0.33333334, 0.33333334, 1.0)
            )
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::Default
        );
    }

    #[tokio::test]
    async fn background_pixels_with_alpha_blend() {
        let cell = blend_pixels((0, 1, crate::surface::RED), (0, 1, (1.0, 1.0, 1.0, 0.5))).await;
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::Default
        );
        assert_eq!(
            cell.attrs().background(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 0.33333334, 0.33333334, 1.0)
            )
        );
    }
}
