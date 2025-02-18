//! Randomly move a pixel over the screen. It randomly but smoothly changes colour

use std::sync::Arc;

use color_eyre::eyre::Result;
use rand::Rng as _;

use crate::shared_state::SharedState;

use super::index::Tattoyer;

/// `RandomWalker`
#[derive(Default)]
pub struct RandomWalker {
    /// TTY width
    width: u16,
    /// TTY height
    height: u16,
    /// Current x,y position
    position: Position,
    /// Current colour
    colour: crate::surface::Colour,
}

/// Position of the random pixel
type Position = (i32, i32);

/// The rate at which the colour changes
const COLOUR_CHANGE_RATE: f32 = 0.3;

#[async_trait::async_trait]
impl Tattoyer for RandomWalker {
    fn id() -> String {
        "random_walker".into()
    }

    /// Instatiate
    async fn new(state: Arc<SharedState>) -> Result<Self> {
        let tty_size = state.get_tty_size().await;
        let width = tty_size.width;
        let height = tty_size.height;
        let width_i32: i32 = width.into();
        let height_i32: i32 = height.into();
        let position: Position = (
            rand::thread_rng().gen_range(1i32..width_i32),
            rand::thread_rng().gen_range(1i32..height_i32 * 2i32),
        );
        let colour: crate::surface::Colour = (
            rand::thread_rng().gen_range(0.1..1.0),
            rand::thread_rng().gen_range(0.1..1.0),
            rand::thread_rng().gen_range(0.1..1.0),
            1.0,
        );
        Ok(Self {
            width,
            height,
            position,
            colour,
        })
    }

    fn set_tty_size(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Tick the render
    async fn tick(&mut self) -> Result<Option<crate::surface::Surface>> {
        let width_i32: i32 = self.width.into();
        let height_i32: i32 = self.height.into();

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

        let mut surface =
            crate::surface::Surface::new(Self::id(), self.width.into(), self.height.into(), -5);
        let x_usize = usize::try_from(self.position.0)?;
        let y_usize = usize::try_from(self.position.1)?;
        surface.add_pixel(x_usize, y_usize, self.colour)?;
        Ok(Some(surface))
    }
}
