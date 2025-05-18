//! Render the output of the PTY and tattoys

use std::str::FromStr as _;
use std::sync::Arc;

use color_eyre::eyre::{bail, Result};
use termwiz::cell::{Cell, CellAttributes};

use termwiz::surface::Surface as TermwizSurface;
use termwiz::surface::{Change as TermwizChange, Position as TermwizPosition};
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::{ScreenSize, Terminal as TermwizTerminal};

use crate::compositor::Compositor;
use crate::run::FrameUpdate;
use crate::shared_state::SharedState;

/// The number of microseconds in a second.
pub const ONE_MICROSECOND: u64 = 1_000_000;

/// The number of milliseconds in a second.
pub const MILLIS_PER_SECOND: f32 = 1_000.0;

/// The minimum rate at which we check that the user's terminal has resized.
///
/// Each time a new frame is rendered a terminal size check is also made, which may lead to checks
/// occuring at a higher rate than this.
pub const CHECK_FOR_RESIZE_RATE: u64 = 30;

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
    /// The base composited frame onto which all tattoys are rendered.
    pub frame: termwiz::surface::Surface,
    /// A little indicator to show that Tattoy is running.
    indicator_cell: Cell,
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
            frame: TermwizSurface::new(width.into(), height.into()),
            indicator_cell: Self::indicator_cell()?,
            is_cursor_visible: true,
        };

        Ok(renderer)
    }

    /// Create the little indicator pixel that shows that Tattoy is running.
    fn indicator_cell() -> Result<Cell> {
        let mut attributes = CellAttributes::default();
        let result = termwiz::color::SrgbaTuple::from_str(crate::utils::TATTOY_BLUE);
        match result {
            Ok(mut rgba) => {
                rgba.3 = 0.7;
                let colour = termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(rgba);
                attributes.set_foreground(colour);
                Ok(Cell::new('▀', attributes))
            }
            Err(()) => bail!("Couldn't convert indicator cell colour to SRGBA"),
        }
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
                Some(update) = surfaces.recv() => {
                    self.handle_frame_update(
                        update, surfaces.len(),
                        &mut composited_terminal,
                        &protocol_tx
                    ).await?;
                }

                // When surface updates are not being sent frequently enough, then we depend
                // on this select branch for checking whether the end user's terminal has
                // resized. Recall that this branch's future is cancelled whenever another
                // select branch triggers, so we shouldn't have an over-abundance of resize
                // checks.
                () = tokio::time::sleep(tokio::time::Duration::from_millis(CHECK_FOR_RESIZE_RATE)) => {
                    self.check_for_user_resize(&mut composited_terminal, &protocol_tx).await?;
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
        update: FrameUpdate,
        backlog: usize,
        composited_terminal: &mut BufferedTerminal<impl TermwizTerminal + Send>,
        protocol_tx: &tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> Result<()> {
        self.check_for_user_resize(composited_terminal, protocol_tx)
            .await?;
        self.render(backlog, update, composited_terminal).await?;

        Ok(())
    }

    /// Handle messages from the global Tattoy protocol.
    const fn handle_protocol_message(&mut self, message: &crate::run::Protocol) {
        if let crate::run::Protocol::CursorVisibility(is_visible) = message {
            self.is_cursor_visible = *is_visible;
        }
    }

    /// Reset the frame for every render.
    fn reset_frame(&mut self) {
        self.frame = TermwizSurface::new(self.width.into(), self.height.into());
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
                // TODO: convert IDs to something more constant.
                if surface_id != "random_walker" && surface_id != "shader" {
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

        self.composite().await?;

        // Hide the cursor without flushing.
        composited_terminal.add_change(TermwizChange::CursorVisibility(
            termwiz::surface::CursorVisibility::Hidden,
        ));

        let changes = composited_terminal.diff_screens(&self.frame);
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
    async fn composite(&mut self) -> Result<()> {
        let is_rendering_enabled = *self.state.is_rendering_enabled.read().await;
        self.reset_frame();

        if is_rendering_enabled {
            self.render_tattoys_below().await?;
        }

        if self.is_a_plugin_replacing_the_pty_layer() && is_rendering_enabled {
            self.render_tattoys(std::cmp::Ordering::Equal).await?;
        } else {
            self.render_pty().await?;
        }

        if is_rendering_enabled {
            self.render_tattoys_above().await?;
            self.colour_grade().await?;
        }
        if self.state.config.read().await.show_tattoy_indicator {
            Compositor::add_indicator(
                &mut self.frame.screen_cells(),
                &self.indicator_cell,
                (self.width - 1).into(),
                0,
            )?;
        }

        Ok(())
    }

    /// Are any of the tattoys replacing the PTY layer?
    fn is_a_plugin_replacing_the_pty_layer(&self) -> bool {
        self.tattoys.values().any(|tattoy| tattoy.layer == 0)
    }

    /// Render all the tattoys that appear below the PTY.
    async fn render_tattoys_below(&mut self) -> Result<()> {
        self.render_tattoys(std::cmp::Ordering::Less).await
    }

    /// Render all the tattoys that appear above the PTY.
    async fn render_tattoys_above(&mut self) -> Result<()> {
        self.render_tattoys(std::cmp::Ordering::Greater).await
    }

    /// Render a tattoy onto the compositor frame.
    async fn render_tattoys(&mut self, comparator: std::cmp::Ordering) -> Result<()> {
        let mut tattoys: Vec<&mut crate::surface::Surface> = self
            .tattoys
            .values_mut()
            .filter(|tattoy| tattoy.layer.cmp(&0) == comparator)
            .collect();
        tattoys.sort_by_key(|tattoy| tattoy.layer);

        let frame_size = self.frame.dimensions();
        let mut frame_cells = self.frame.screen_cells();
        for tattoy in &mut tattoys {
            if tattoy.id == *"shader" && !self.state.config.read().await.shader.render {
                continue;
            }
            let tattoy_frame_size = tattoy.surface.dimensions();
            if tattoy_frame_size != frame_size {
                tracing::warn!(
                    "Not rendering '{}' as its size doesn't match the current frame size",
                    tattoy.id
                );
                continue;
            }
            let tattoy_cells = tattoy.surface.screen_cells();

            for (frame_line, tattoy_line) in frame_cells.iter_mut().zip(tattoy_cells) {
                for (frame_cell, tattoy_cell) in frame_line.iter_mut().zip(tattoy_line) {
                    Compositor::composite_cells(frame_cell, tattoy_cell, tattoy.opacity);
                }
            }
        }

        Ok(())
    }

    /// Render the PTY to the compositor frame.
    async fn render_pty(&mut self) -> Result<()> {
        let frame_size = self.frame.dimensions();
        let mut frame_cells = self.frame.screen_cells();

        let pty_size = self.pty.dimensions();
        let pty_cells = self.pty.screen_cells();

        if pty_size != frame_size {
            tracing::warn!("Not rendering PTY as its size doesn't match the current frame size");
            return Ok(());
        }

        let config = self.state.config.read().await;
        let text_contrast = config.text_contrast.clone();
        let apply_to_readable_text_only = config.text_contrast.apply_to_readable_text_only;
        let render_shader_colours_to_text = config.shader.render_shader_colours_to_text;
        drop(config);

        let maybe_shader_cells = if render_shader_colours_to_text {
            Self::get_shader_cells(self.tattoys.get_mut("shader"), frame_size)
        } else {
            None
        };

        for (y, (frame_line, pty_line)) in frame_cells.iter_mut().zip(pty_cells).enumerate() {
            for (x, (frame_cell, pty_cell)) in frame_line.iter_mut().zip(pty_line).enumerate() {
                Compositor::composite_cells(frame_cell, pty_cell, 1.0);

                if let Some(shader_cells) = maybe_shader_cells.as_ref() {
                    // TODO: it'd be nice to include this with the other iterators, I tried but
                    // just couldn't figure it out.
                    let shader_cell = Compositor::get_cell(shader_cells, x, y)?;
                    Compositor::composite_fg_colour_only(frame_cell, shader_cell);
                }

                if text_contrast.enabled {
                    Compositor::auto_text_contrast(
                        frame_cell,
                        text_contrast.target_contrast,
                        apply_to_readable_text_only,
                    );
                }
            }
        }

        Ok(())
    }

    /// If there's a shader frame then get it.
    fn get_shader_cells(
        maybe_shader: Option<&mut crate::surface::Surface>,
        frame_size: (usize, usize),
    ) -> Option<Vec<&mut [Cell]>> {
        if let Some(shader) = maybe_shader {
            if shader.surface.dimensions() == frame_size {
                let shader_cells = shader.surface.screen_cells();
                Some(shader_cells)
            } else {
                tracing::debug!(
                    "Not using shader to render PTY colours as the shader frame size doesn't match"
                );
                None
            }
        } else {
            tracing::debug!(
                "Not using shader to render PTY colours as the shader tattoy is not enabled, or not ready"
            );
            None
        }
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

    /// Apply colour changes, like saturation, hue, contrast, etc.
    //
    // TODO: consider including this in the final compositing layer, just for the performance
    // gain of not having to iterate over every cell again.
    async fn colour_grade(&mut self) -> Result<()> {
        let config = self.state.config.read().await;

        let saturation: f64 = config.color.saturation.into();
        let light: f64 = config.color.brightness.into();
        let hue: f64 = config.color.hue.into();
        drop(config);

        for line in &mut self.frame.screen_cells().iter_mut() {
            for cell in line.iter_mut() {
                let foreground = cell.attrs().foreground();
                if let Some(mut gradable) = crate::blender::Blender::extract_colour(foreground) {
                    gradable = gradable.saturate(saturation);
                    gradable = gradable.lighten(light);
                    gradable = gradable.adjust_hue_fixed(hue);
                    cell.attrs_mut().set_foreground(
                        termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(gradable),
                    );
                }

                let background = cell.attrs().background();
                if let Some(mut gradable) = crate::blender::Blender::extract_colour(background) {
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
        let mut tattoy_below = crate::surface::Surface::new("below".into(), 1, 1, 1, 1.0);
        tattoy_below.add_pixel(first.0, first.1, first.2).unwrap();
        renderer
            .tattoys
            .insert(tattoy_below.id.clone(), tattoy_below);

        let mut tattoy_above = crate::surface::Surface::new("above".into(), 1, 1, 2, 1.0);
        tattoy_above
            .add_pixel(second.0, second.1, second.2)
            .unwrap();
        renderer
            .tattoys
            .insert(tattoy_above.id.clone(), tattoy_above);

        renderer.composite().await.unwrap();
        let cell = &renderer.frame.screen_cells()[0][0];
        assert_eq!(cell.str(), "▀");

        cell.clone()
    }

    #[tokio::test]
    async fn blending_text() {
        let mut renderer = make_renderer().await;
        let mut tattoy_below = crate::surface::Surface::new("below".into(), 1, 1, 1, 1.0);
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

        let mut tattoy_above = crate::surface::Surface::new("above".into(), 1, 1, 2, 1.0);
        tattoy_above.add_text(0, 0, " ".into(), Some((0.0, 0.0, 0.0, 0.5)), None);
        renderer
            .tattoys
            .insert(tattoy_above.id.clone(), tattoy_above);

        renderer.composite().await.unwrap();
        let cell = &renderer.frame.screen_cells()[0][0];

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
        let mut tattoy_below = crate::surface::Surface::new("below".into(), 1, 1, 1, 1.0);
        tattoy_below.add_text(0, 0, "a".into(), None, Some(crate::surface::WHITE));
        renderer
            .tattoys
            .insert(tattoy_below.id.clone(), tattoy_below);

        let mut tattoy_above = crate::surface::Surface::new("above".into(), 1, 1, 2, 1.0);
        tattoy_above.add_text(0, 0, " ".into(), Some((1.0, 1.0, 1.0, 0.5)), None);
        renderer
            .tattoys
            .insert(tattoy_above.id.clone(), tattoy_above);

        renderer.composite().await.unwrap();
        let cell = &renderer.frame.screen_cells()[0][0];

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
