//! Manage the simulation and send and receive JSON from Tattoy.

use crate::simulation::Simulation;
use color_eyre::eyre::Result;
use std::{collections::VecDeque, io::Write as _};

/// The number of microseconds in a second.
pub const ONE_MICROSECOND: u64 = 1_000_000;

/// The target frame rate for renders sent to Tattoy.
pub const TARGET_FRAME_RATE: u64 = 30;

/// The current state of the Tattoy user's terminal.
struct TTY {
    /// The size of the user's terminal.
    size: (u16, u16),
    /// The current position of the cursor in the user's terminal.
    cursor_position: (u16, u16),
    /// The contens of the terminal's cells. Characters and colour values.
    cells: Vec<tattoy_protocol::Cell>,
}

/// `SmokeyCursor`
pub struct SmokeyCursor {
    /// Details about the user's terminal.
    tty: TTY,
    /// All the particles of the gas.
    simulation: Simulation,
    /// Timestamps of recent render ticks.
    durations: VecDeque<f64>,
    /// The time at which the previous frame was rendererd.
    last_frame_tick: tokio::time::Instant,
}

impl SmokeyCursor {
    /// Instatiate
    fn new() -> Self {
        Self {
            tty: TTY {
                size: (0, 0),
                cursor_position: (0, 0),
                cells: Vec::new(),
            },
            last_frame_tick: tokio::time::Instant::now(),
            simulation: Simulation::new(0, 0),
            durations: VecDeque::default(),
        }
    }

    /// Initialise the simulation.
    fn initialise(&mut self) {
        self.simulation = Simulation::new(
            usize::from(self.tty.size.0),
            usize::from(self.tty.size.1 * 2),
        );

        tracing::debug!("Simulation initialised.");
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        mut messages: tokio::sync::mpsc::Receiver<tattoy_protocol::PluginInputMessages>,
    ) -> Result<()> {
        let mut smokey_cursor = Self::new();

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                () = smokey_cursor.sleep_until_next_frame_tick() => {
                    smokey_cursor.render()?;
                },
                Some(message) = messages.recv() => {
                    smokey_cursor.handle_message(message);
                }
            }
        }

        #[expect(unreachable_code, reason = "We rely on Tattoy to shut us down")]
        Ok(())
    }

    /// Sleep until the next frame render is due.
    pub async fn sleep_until_next_frame_tick(&mut self) {
        let target = crate::renderer::ONE_MICROSECOND.wrapping_div(TARGET_FRAME_RATE);
        let target_frame_rate_micro = std::time::Duration::from_micros(target);
        if let Some(wait) = target_frame_rate_micro.checked_sub(self.last_frame_tick.elapsed()) {
            tokio::time::sleep(wait).await;
        }
        self.last_frame_tick = tokio::time::Instant::now();
    }

    /// Handle a protocol message from Tattoy.
    #[expect(clippy::todo, reason = "TODO: support terminal resizing")]
    fn handle_message(&mut self, message: tattoy_protocol::PluginInputMessages) {
        match message {
            tattoy_protocol::PluginInputMessages::PTYUpdate {
                size,
                cells,
                cursor,
            } => {
                self.tty.size = size;
                self.tty.cells = cells;
                self.tty.cursor_position = cursor;
            }
            tattoy_protocol::PluginInputMessages::TTYResize { .. } => todo!(),

            #[expect(
                clippy::unreachable,
                reason = "
                    Tattoy uses `#[non-exhaustive]` so have always be able to handle new
                    message kinds without crashing
                "
            )]
            _ => unreachable!(),
        }
    }

    /// Send a frame to Tattoy.
    fn render(&mut self) -> Result<()> {
        if self.tty.size.0 == 0 || self.tty.size.1 == 0 {
            return Ok(());
        }

        if !self.simulation.is_ready() {
            self.initialise();
        }

        let start = std::time::Instant::now();

        self.simulation
            .tick(self.tty.cursor_position, &self.tty.cells);

        let mut pixels = Vec::<tattoy_protocol::Pixel>::new();
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "We're just rendering to a terminal grid"
        )]
        for particle in &mut self.simulation.particles {
            let position = particle.position_unscaled();
            let pixel = tattoy_protocol::Pixel::builder()
                .coordinates((position.x as u32, position.y as u32))
                .color(particle.colour)
                .build();
            pixels.push(pixel);
        }

        self.durations.push_front(start.elapsed().as_secs_f64());
        if self.durations.len() > 30 {
            self.durations.pop_back();
        }

        Self::send_output(pixels)?;

        Ok(())
    }

    /// Send pixel data to Tattoy for rendering.
    fn send_output(pixels: Vec<tattoy_protocol::Pixel>) -> Result<()> {
        let json =
            serde_json::to_string(&tattoy_protocol::PluginOutputMessages::OutputPixels(pixels))?;
        let mut stdout = std::io::stdout().lock();
        let result = stdout.write_all(json.as_bytes());
        if let Err(error) = result {
            tracing::error!("Error sending json to Tattoy: {error:?}");
        }
        Ok(())
    }
}
