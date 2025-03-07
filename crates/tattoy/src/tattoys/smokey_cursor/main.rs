//! The cursor gives off a gas that floats up and interacts with the history

use std::collections::VecDeque;

use color_eyre::eyre::Result;

use super::simulation::Simulation;

/// `SmokeyCursor`
pub(crate) struct SmokeyCursor {
    /// The base Tattoy struct
    tattoy: crate::tattoys::tattoyer::Tattoyer,
    /// All the particles of gas
    simulation: Simulation,
    /// Timestamp of last tick
    durations: VecDeque<f64>,
}

impl SmokeyCursor {
    /// Instatiate
    fn new(output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>) -> Self {
        let tattoy = crate::tattoys::tattoyer::Tattoyer::new(
            "smokey_cursor".to_owned(),
            -10,
            output_channel,
        );

        Self {
            tattoy,
            simulation: Simulation::new(0, 0),
            durations: VecDeque::default(),
        }
    }

    /// Initialise the simulation, because we don't have the dimensions when instantiating Self.
    fn initialise(&mut self) {
        self.simulation = Simulation::new(
            self.tattoy.width.into(),
            usize::from(self.tattoy.height) * 2,
        );
        tracing::debug!("Simulation initialised.");
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
    ) -> Result<()> {
        let mut random_walker = Self::new(output);
        let mut protocol = protocol_tx.subscribe();

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                () = random_walker.tattoy.sleep_until_next_frame_tick() => {
                    random_walker.render().await?;
                },
                Ok(message) = protocol.recv() => {
                    if matches!(message, crate::run::Protocol::End) {
                        break;
                    }
                    random_walker.tattoy.handle_common_protocol_messages(message)?;
                }
            }
        }

        Ok(())
    }

    /// One frame of the tattoy
    async fn render(&mut self) -> Result<()> {
        if !self.tattoy.is_ready() {
            return Ok(());
        }

        if !self.simulation.is_ready() {
            self.initialise();
        }

        let start = std::time::Instant::now();

        self.tattoy.initialise_surface();

        let cursor = self.tattoy.screen.surface.cursor_position();
        let cells = self.tattoy.screen.surface.screen_cells();
        self.simulation.tick(cursor, &cells);

        for particle in &mut self.simulation.particles {
            let position = particle.position_unscaled();

            #[expect(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "We're just rendering to a terminal grid"
            )]
            self.tattoy.surface.add_pixel(
                position.x as usize,
                position.y as usize,
                particle.colour,
            )?;
        }

        let text_coloumn = usize::from(self.tattoy.width - 20);
        let count = self.simulation.particles.len();
        self.tattoy
            .surface
            .add_text(text_coloumn, 0, format!("Particles: {count}"), None, None);

        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            clippy::default_numeric_fallback,
            reason = "This is just debugging output"
        )]
        {
            let average_tick = self.durations.iter().sum::<f64>() / self.durations.len() as f64;
            let fps = 1.0 / average_tick;
            self.tattoy
                .surface
                .add_text(text_coloumn, 1, format!("FPS: {fps:.3}"), None, None);
        };

        self.durations.push_front(start.elapsed().as_secs_f64());
        if self.durations.len() > 30 {
            self.durations.pop_back();
        }

        self.tattoy.send_output().await
    }
}
