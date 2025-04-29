//! Randomly move a pixel over the screen. It randomly but smoothly changes colour

use color_eyre::eyre::Result;
use rand::Rng as _;

/// `RandomWalker`
pub struct RandomWalker {
    /// The base Tattoy struct
    tattoy: super::tattoyer::Tattoyer,
    /// Current x,y position
    position: Position,
    /// Current colour
    colour: crate::surface::Colour,
}

/// Position of the random pixel
type Position = (i32, i32);

/// The rate at which the colour changes
const COLOUR_CHANGE_RATE: f32 = 0.3;

impl RandomWalker {
    /// Instatiate
    fn new(output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>) -> Self {
        let tattoy =
            super::tattoyer::Tattoyer::new("random_walker".to_owned(), -10, 1.0, output_channel);
        let position: Position = (0, 0);
        let colour: crate::surface::Colour = (
            rand::thread_rng().gen_range(0.1..1.0),
            rand::thread_rng().gen_range(0.1..1.0),
            rand::thread_rng().gen_range(0.1..1.0),
            1.0,
        );

        Self {
            tattoy,
            position,
            colour,
        }
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
                    random_walker.handle_protocol_message(&message);
                    random_walker.tattoy.handle_common_protocol_messages(message)?;
                }
            }
        }

        Ok(())
    }

    /// Custom behaviour for protocol messages.
    fn handle_protocol_message(&mut self, message: &crate::run::Protocol) {
        #[expect(
            clippy::single_match,
            clippy::wildcard_enum_match_arm,
            reason = "We're ready to add handlers for other messages"
        )]
        match message {
            crate::run::Protocol::Resize { width, height } => {
                self.position = (
                    rand::thread_rng().gen_range(0i32..i32::from(*width)),
                    rand::thread_rng().gen_range(0i32..i32::from(*height) * 2i32),
                );
            }
            _ => (),
        }
    }

    /// Tick the render
    async fn render(&mut self) -> Result<()> {
        if !self.tattoy.is_ready() {
            return Ok(());
        }

        let width_i32: i32 = self.tattoy.width.into();
        let height_i32: i32 = self.tattoy.height.into();

        self.position.0 += rand::thread_rng().gen_range(0i32..=2i32) - 1i32;
        self.position.0 = self.position.0.clamp(1i32, width_i32 - 1i32);

        self.position.1 += rand::thread_rng().gen_range(0i32..=2i32) - 1i32;
        self.position.1 = self.position.1.clamp(1i32, (height_i32 * 2i32) - 1i32);

        self.colour.0 +=
            rand::thread_rng().gen_range(0.0..COLOUR_CHANGE_RATE) - COLOUR_CHANGE_RATE / 2.0;
        self.colour.0 = self.colour.0.clamp(0.0, 1.0);
        self.colour.1 +=
            rand::thread_rng().gen_range(0.0..COLOUR_CHANGE_RATE) - COLOUR_CHANGE_RATE / 2.0;
        self.colour.1 = self.colour.1.clamp(0.0, 1.0);
        self.colour.2 +=
            rand::thread_rng().gen_range(0.0..COLOUR_CHANGE_RATE) - COLOUR_CHANGE_RATE / 2.0;
        self.colour.2 = self.colour.2.clamp(0.0, 1.0);

        self.tattoy.initialise_surface();
        let x_usize = usize::try_from(self.position.0)?;
        let y_usize = usize::try_from(self.position.1)?;
        self.tattoy
            .surface
            .add_pixel(x_usize, y_usize, self.colour)?;

        self.tattoy.send_output().await
    }
}
