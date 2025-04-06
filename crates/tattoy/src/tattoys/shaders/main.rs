//! Shadertoy-like shaders. You should be able to copy and paste most shaders found on
//! <https://shadertoy.com>.

use color_eyre::eyre::{ContextCompat as _, Result};

use crate::tattoys::tattoyer::Tattoyer;

/// All the user config for the shader tattoy.
#[derive(serde::Deserialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct Config {
    /// Enable/disable the shaders on and off
    pub enabled: bool,
    /// The path to a given GLSL shader file.
    pub path: std::path::PathBuf,
    /// The opacity of the rendered shader layer.
    pub opacity: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "shaders/point_lights.glsl".into(),
            opacity: 0.75,
        }
    }
}

/// `Shaders`
pub(crate) struct Shaders<'shaders> {
    /// The base Tattoy struct
    tattoy: Tattoyer,
    /// Shared app state
    state: std::sync::Arc<crate::shared_state::SharedState>,
    /// All the special GPU handling code.
    gpu: super::gpu::GPU<'shaders>,
}

impl Shaders<'_> {
    /// Instantiate
    async fn new(
        output_channel: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<Self> {
        let tattoy = Tattoyer::new("shaders".to_owned(), -10, output_channel);
        let shader_directory = state.config_path.read().await.clone();
        let shader_path = state.config.read().await.shader.path.clone();
        let gpu = super::gpu::GPU::new(shader_directory.join(shader_path)).await?;
        Ok(Self { tattoy, state, gpu })
    }

    /// Our main entrypoint.
    pub(crate) async fn start(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
        output: tokio::sync::mpsc::Sender<crate::run::FrameUpdate>,
        state: std::sync::Arc<crate::shared_state::SharedState>,
    ) -> Result<()> {
        let mut shaders = Self::new(output, state).await?;
        let mut protocol = protocol_tx.subscribe();

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

                self.tattoy.handle_common_protocol_messages(message)?;
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
        if !self.tattoy.is_ready() {
            tracing::trace!("Not rendering shader as Tattoy isn't ready yet.");
            return Ok(());
        }

        self.gpu
            .update_resolution(self.tattoy.width, self.tattoy.height * 2);
        let cursor = self.tattoy.screen.surface.cursor_position();
        self.gpu
            .update_mouse_position(cursor.0.try_into()?, cursor.1.try_into()?);

        self.tattoy.initialise_surface();
        let opacity = self.state.config.read().await.shader.opacity;
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

                self.tattoy.surface.add_pixel(
                    x.into(),
                    y.try_into()?,
                    (pixel[0], pixel[1], pixel[2], opacity),
                )?;
            }
        }

        self.tattoy.send_output().await?;

        Ok(())
    }
}
