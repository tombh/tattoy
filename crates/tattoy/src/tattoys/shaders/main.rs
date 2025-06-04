//! Shadertoy-like shaders. You should be able to copy and paste most shaders found on
//! <https://shadertoy.com>.

use color_eyre::eyre::{ContextCompat as _, Result};
use futures_util::FutureExt as _;

use crate::tattoys::tattoyer::Tattoyer;

/// All the user config for the shader tattoy.
#[expect(
    clippy::struct_excessive_bools,
    reason = "We need the bools for the config"
)]
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct Config {
    /// Enable/disable the shaders on and off
    pub enabled: bool,
    /// The path to a given GLSL shader file.
    pub path: std::path::PathBuf,
    /// The opacity of the rendered shader layer.
    pub opacity: f32,
    /// The layer (or z-index) into which the shaders are rendered.
    pub layer: i16,
    /// The shader is still sent and run on the GPU but it's not rendered to a layer on the
    /// terminal. This is most likely useful in conjunction with `render_shader_colours_to_text`,
    /// as "contents" of the shader are rendered via the terminal's text.
    pub render: bool,
    /// Whether to upload a pixel representation of the user's terminal. Useful for shader's that
    /// replace the text of the terminal, as Ghostty shaders do.
    pub upload_tty_as_pixels: bool,
    /// Define the terminal's text colours based on the colour of the shader pixel at the same
    /// position. This would most likely be used in conjunction with auto contrast enabled,
    /// otherwise the text won't actually be readable.
    pub render_shader_colours_to_text: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "shaders/point_lights.glsl".into(),
            opacity: 0.75,
            layer: -10,
            render: true,
            upload_tty_as_pixels: true,
            render_shader_colours_to_text: false,
        }
    }
}

/// `Shaders`
pub(crate) struct Shaders<'shaders> {
    /// The base Tattoy struct
    tattoy: Tattoyer,
    /// All the special GPU handling code.
    gpu: super::gpu::GPU<'shaders>,
}

impl Shaders<'_> {
    /// Instantiate
    async fn new(
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<Self> {
        let shader_directory = state.config_path.read().await.clone();
        let shader_path = state.config.read().await.shader.path.clone();
        let tty_size = *state.tty_size.read().await;
        let gpu = super::gpu::GPU::new(
            shader_directory.join(shader_path),
            tty_size.width,
            tty_size.height * 2,
        )
        .await?;
        let layer = state.config.read().await.shader.layer;
        let opacity = state.config.read().await.shader.opacity;
        let tattoy =
            Tattoyer::new("shader".to_owned(), state, layer, opacity, output_channel).await;
        Ok(Self { tattoy, gpu })
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<()> {
        let may_panic = std::panic::AssertUnwindSafe(async {
            let result = Self::main(output, &state).await;

            if let Err(error) = result {
                tracing::error!("GPU pipeline error: {error:?}");
                state
                    .send_notification(
                        "GPU pipeline error",
                        crate::tattoys::notifications::message::Level::Error,
                        Some(error.root_cause().to_string()),
                        true,
                    )
                    .await;
                Err(error)
            } else {
                Ok(())
            }
        });

        if let Err(error) = may_panic.catch_unwind().await {
            let message = if let Some(message) = error.downcast_ref::<String>() {
                message
            } else if let Some(message) = error.downcast_ref::<&str>() {
                message
            } else {
                "Caught a panic with an unknown type."
            };
            tracing::error!("Shader panic: {message:?}");
            state
                .send_notification(
                    "GPU pipeline panic",
                    crate::tattoys::notifications::message::Level::Error,
                    Some(message.into()),
                    true,
                )
                .await;
        }

        Ok(())
    }

    /// Enter the main render loop. We put it in its own function so that we can easily handle any
    /// errors.
    async fn main(
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: &std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<()> {
        let mut protocol = state.protocol_tx.subscribe();
        let mut shaders = Self::new(output, std::sync::Arc::clone(state)).await?;

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is caused by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                () = shaders.tattoy.sleep_until_next_frame_tick() => {
                    shaders.render().await?;
                },
                result = protocol.recv() => {
                    if matches!(result, Ok(crate::run::Protocol::End)) {
                        break;
                    }
                    shaders.handle_protocol_message(result).await?;
                }
            }
        }

        Ok(())
    }

    /// Handle messages from the main Tattoy app.
    async fn handle_protocol_message(
        &mut self,
        protocol_result: std::result::Result<
            crate::run::Protocol,
            tokio::sync::broadcast::error::RecvError,
        >,
    ) -> Result<()> {
        match protocol_result {
            Ok(message) => {
                if let crate::run::Protocol::KeybindEvent(event) = &message {
                    if matches!(event, crate::config::input::KeybindingAction::ShaderPrev) {
                        self.cycle_shader(false).await?;
                    }
                    if matches!(event, crate::config::input::KeybindingAction::ShaderNext) {
                        self.cycle_shader(true).await?;
                    }
                }

                if let crate::run::Protocol::Resize { width, height } = &message {
                    self.gpu.update_resolution(*width, height * 2)?;
                }

                if let crate::run::Protocol::Input(input) = &message {
                    if let termwiz::input::InputEvent::Mouse(mouse) = &input.event {
                        self.gpu.update_mouse_position(mouse.x, mouse.y);
                    }
                }

                let is_screen_changed = Tattoyer::is_screen_output_changed(&message);
                self.tattoy.handle_common_protocol_messages(message)?;

                let is_upload_tty_as_pixels = self
                    .tattoy
                    .state
                    .config
                    .read()
                    .await
                    .shader
                    .upload_tty_as_pixels;
                if is_upload_tty_as_pixels && is_screen_changed {
                    let pty_image = self.tattoy.convert_pty_to_pixel_image(
                        &shadow_terminal::output::SurfaceKind::Screen,
                    )?;
                    let rotated = pty_image.flipv();
                    self.gpu.update_ichannel_texture_data(&rotated.into());
                }
            }
            Err(error) => tracing::error!("Receiving protocol message: {error:?}"),
        }

        Ok(())
    }

    /// Cycle through the shaders in the user's shader directory.
    async fn cycle_shader(&mut self, direction: bool) -> Result<()> {
        let Some(shader_directory) = self.gpu.shader_path.parent() else {
            color_eyre::eyre::bail!("Unreachable: current shader doesn't have a parent path.");
        };
        let Some(current_filename) = self.gpu.shader_path.file_name() else {
            color_eyre::eyre::bail!("Unreachable: couldn't get current shader's filename.");
        };

        let mut all_shaders = std::fs::read_dir(shader_directory)?
            .map(|result| result.map_err(Into::into))
            .collect::<Result<Vec<std::fs::DirEntry>>>()?
            .into_iter()
            .filter_map(|entry| entry.path().is_file().then(|| entry.file_name()))
            .collect::<Vec<std::ffi::OsString>>();
        all_shaders.sort();

        if !direction {
            all_shaders.reverse();
        }

        let Some(new_shader_raw) = all_shaders.first() else {
            color_eyre::eyre::bail!(
                "Unreachable: current shader's directory doesn't have a shader in it."
            );
        };
        let mut new_shader = new_shader_raw.clone();
        let mut is_current_shader_found = false;
        for shader_filename in all_shaders {
            if is_current_shader_found {
                new_shader = shader_filename;
                break;
            }
            tracing::debug!("{:?}=={:?}", shader_filename, current_filename);
            if shader_filename == current_filename {
                is_current_shader_found = true;
            }
        }

        let shader_path = shader_directory.join(new_shader.clone());
        tracing::info!("Changing shader to: {new_shader:?}");

        self.gpu.shader_path = shader_path;
        self.gpu.build_pipeline().await?;

        Ok(())
    }

    /// Tick the render
    async fn render(&mut self) -> Result<()> {
        let cursor = self.tattoy.screen.surface.cursor_position();
        self.gpu
            .update_cursor_position(cursor.0.try_into()?, cursor.1.try_into()?);

        self.tattoy.initialise_surface();
        self.tattoy.opacity = self.tattoy.state.config.read().await.shader.opacity;
        self.tattoy.layer = self.tattoy.state.config.read().await.shader.layer;
        let image = self.gpu.render().await?;

        let tty_height_in_pixels = u32::from(self.tattoy.height) * 2;
        for y in 0..tty_height_in_pixels {
            for x in 0..self.tattoy.width {
                let offset_for_reversal = 1;
                let y_reversed = tty_height_in_pixels - y - offset_for_reversal;
                let pixel = image
                    .get_pixel_checked(x.into(), y_reversed)
                    .context(format!("Couldn't get pixel: {x}x{y_reversed}"))?
                    .0;

                self.tattoy
                    .surface
                    .add_pixel(x.into(), y.try_into()?, pixel.into())?;
            }
        }

        self.tattoy.send_output().await?;

        Ok(())
    }
}
