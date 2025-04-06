//! The code for interacting with the GPU.

use color_eyre::eyre::{ContextCompat as _, Result};
use wgpu::util::DeviceExt as _;

// TODO: See if the struct can be defined such that padding isn't needed.
/// Common variables used by Shadertoy shaders.
#[expect(
    non_snake_case,
    reason = "
        Shaders use camelCase, so even though we could easily convert between cases,
        it's just saner to keep as they appear.
    "
)]
// We need this for Rust to store our data correctly for the shaders.
#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Variables {
    /// The dimensions of the TTY.
    pub iResolution: [f32; 3],
    /// Pad to 4x32 (16 bytes or 128 bits).
    _padding: u32,
    /// The coordinates of the mouse or cursor.
    pub iMouse: [f32; 2],
    /// The wall time since the shader started.
    iTime: f32,
    /// The number of rendered shader frames.
    iFrame: u32,
}

/// Code for talking to the GPU.
pub(crate) struct GPU<'gpu> {
    /// Path to the current shader file.
    pub shader_path: std::path::PathBuf,
    /// The time at which rendering began.
    started: std::time::Instant,
    /// Useful varibale data for shaders. Eg, mouse coordinates, wall time, etc
    pub variables: Variables,
    /// The buffer containing shader variable data.
    variables_buffer: wgpu::Buffer,
    /// The layout of the variables buffer binding.
    variables_bindgroup_layout: wgpu::BindGroupLayout,
    /// The texture descriptor
    texture_descriptor: wgpu::TextureDescriptor<'gpu>,

    /// The `wgpu` device.
    device: wgpu::Device,
    /// The GPU render queue.
    queue: wgpu::Queue,

    /// The texture on which the final render is placed.
    texture: wgpu::Texture,
    /// The final render's texture view.
    texture_view: wgpu::TextureView,
    /// The raw data for the final render.
    output_buffer: wgpu::Buffer,
    /// The GPU render pipeline.
    pipeline: Option<wgpu::RenderPipeline>,
}

impl GPU<'_> {
    // TODO: This does not scale. We will need to dynamically recreate the texture to factors of
    // 256 on terminal resizes.
    //
    /// The size of the square GPU texture used to create the fullscren triangle upon which the
    /// Shadertoy shaders draw pixels.
    const TEXTURE_SIZE: u32 = 512;

    /// Needed for GPU buffers and such.
    fn u32_size() -> Result<u32> {
        Ok(std::mem::size_of::<u32>().try_into()?)
    }

    /// Instantiate
    pub async fn new(shader_path: std::path::PathBuf) -> Result<Self> {
        let variables = Variables::default();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .context("Couldn't get GPU adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await?;

        let texture_descriptor = wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: Self::TEXTURE_SIZE,
                height: Self::TEXTURE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: None,
            view_formats: &[],
        };

        let texture = device.create_texture(&texture_descriptor);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let output_buffer_size: wgpu::BufferAddress =
            (Self::u32_size()? * Self::TEXTURE_SIZE * Self::TEXTURE_SIZE).into();
        let output_buffer_desc = wgpu::BufferDescriptor {
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            label: None,
            mapped_at_creation: false,
        };
        let output_buffer = device.create_buffer(&output_buffer_desc);

        let variables_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[variables]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let variables_bindgroup_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("variables_bind_group_layout"),
            });

        let mut gpu = Self {
            shader_path,
            started: std::time::Instant::now(),

            variables,
            variables_buffer,
            variables_bindgroup_layout,
            texture_descriptor,

            device,
            queue,

            texture,
            texture_view,
            output_buffer,
            pipeline: None,
        };

        gpu.build_pipeline().await?;

        Ok(gpu)
    }

    /// (Re)build the render pipeline
    pub async fn build_pipeline(&mut self) -> Result<()> {
        let (vertex_shader, fragment_shader) = self.compile_shaders().await?;
        let render_pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &[&self.variables_bindgroup_layout],
                    push_constant_ranges: &[],
                });

        let render_pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vertex_shader,
                    entry_point: Some("main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &fragment_shader,
                    entry_point: Some("main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.texture_descriptor.format,
                        blend: Some(wgpu::BlendState {
                            alpha: wgpu::BlendComponent::REPLACE,
                            color: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
                    polygon_mode: wgpu::PolygonMode::Fill,
                    // Requires Features::DEPTH_CLIP_CONTROL
                    unclipped_depth: false,
                    // Requires Features::CONSERVATIVE_RASTERIZATION
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                // If the pipeline will be used with a multiview render pass, this
                // indicates how many array layers the attachments will have.
                multiview: None,
                cache: None,
            });

        self.pipeline = Some(render_pipeline);

        Ok(())
    }

    /// Upda the shader variables with the current elapse wall time since the render began.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "The side effects are not serious. The value is only used on the GPU"
    )]
    fn update_wall_time(&mut self) {
        self.variables.iTime =
            (self.started.elapsed().as_millis() as f32) / crate::renderer::MILLIS_PER_SECOND;
    }

    /// Update the `iResolution` variable for the shaders to consume.
    pub fn update_resolution(&mut self, width: u16, height: u16) {
        self.variables.iResolution = [width.into(), height.into(), 0.0];
    }

    /// Update the `iMouse` variable for the shaders to consume.
    pub fn update_mouse_position(&mut self, x: u16, y_cell: u16) {
        let height = self.variables.iResolution[1];
        let y: f32 = (y_cell * 2).into();
        self.variables.iMouse = [x.into(), height - y];
    }

    /// Tick the render
    pub async fn render(&mut self) -> Result<image::ImageBuffer<image::Rgb<f32>, Vec<f32>>> {
        self.update_wall_time();

        self.queue.write_buffer(
            &self.variables_buffer,
            0,
            bytemuck::cast_slice(&[self.variables]),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let render_pass_desc = wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            };
            let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

            if let Some(pipeline) = self.pipeline.as_ref() {
                render_pass.set_pipeline(pipeline);
                render_pass.set_bind_group(0, &self.variables_binding(), &[]);
                render_pass.draw(0..3, 0..1);
            }
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::All,
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(Self::u32_size()? * Self::TEXTURE_SIZE),
                    rows_per_image: Some(Self::TEXTURE_SIZE),
                },
            },
            wgpu::Extent3d {
                width: Self::TEXTURE_SIZE,
                height: Self::TEXTURE_SIZE,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));
        let image = self.convert_final_render_to_image().await;
        self.output_buffer.unmap();

        image
    }

    /// Convert the raw data from the GPU into a iterable image of f32-based true colour pixels.
    async fn convert_final_render_to_image(
        &self,
    ) -> Result<image::ImageBuffer<image::Rgb<f32>, Vec<f32>>> {
        let buffer_slice = self.output_buffer.slice(..);

        let (tx, rx) = tokio::sync::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |gpu_state_result| {
            let result = tx.send(gpu_state_result);
            if let Err(error) = result {
                tracing::error!("GPU ready state result: {error:?}");
            }
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.await??;

        let raw_image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            Self::TEXTURE_SIZE,
            Self::TEXTURE_SIZE,
            buffer_slice.get_mapped_range(),
        )
        .context("Couldn't convert raw GPU buffer to image")?;

        Ok(self.extract_rgb32f_image(&raw_image))
    }

    /// Convert the raw GPU image to more friendly RGB floating point pixels.
    #[expect(
        clippy::as_conversions,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "Resolution is safely within reasonable limits of f32"
    )]
    fn extract_rgb32f_image(
        &self,
        imaged: &image::ImageBuffer<image::Rgba<u8>, wgpu::BufferView<'_>>,
    ) -> image::ImageBuffer<image::Rgb<f32>, Vec<f32>> {
        let width = self.variables.iResolution[0] as u32;
        let height = self.variables.iResolution[1] as u32;

        image::Rgb32FImage::from_fn(width, height, |x, y| {
            if let Some(pixel) = imaged.get_pixel_checked(x, y) {
                [
                    f32::from(pixel[0]) / 255.0,
                    f32::from(pixel[1]) / 255.0,
                    f32::from(pixel[2]) / 255.0,
                ]
                .into()
            } else {
                [0.0, 0.0, 0.0].into()
            }
        })
    }

    /// Complile the GLSL shaders ready for consumption by the GPU.
    async fn compile_shaders(&self) -> Result<(wgpu::ShaderModule, wgpu::ShaderModule)> {
        // The vertex shader never changes, it uses a well-known technique called a fullscreen
        // triangle: https://stackoverflow.com/q/2588875/575773 The triangle covers the entire
        // contents of the viewport and so offers a single place for writing pixels to.
        let vertex_shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Vertex Shader"),
                source: wgpu::ShaderSource::Glsl {
                    shader: include_str!("fullscreen_triangle.glsl").into(),
                    stage: wgpu::naga::ShaderStage::Vertex,
                    defines: std::collections::HashMap::default(),
                },
            });

        // In our usage, the fragment shader is the code that actually omits pixels.
        //
        // We are also following the fragment shader standard used by the Shadertoy.com website.
        // Therefore we also need to provide some header and footer boilerplate to allow
        // copy-pasting shaders without alteration. Just little things like `main()` calling
        // `mainImage()` and providing known globals such as `iResolution`.
        let file = tokio::fs::read(self.shader_path.clone()).await?;
        let contents = String::from_utf8_lossy(&file);
        let header = include_str!("header.glsl");
        let footer = include_str!("footer.glsl");
        let shader = format!("{header}\n{contents}\n{footer}");

        let fragment_shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Fragment Shader"),
                source: wgpu::ShaderSource::Glsl {
                    shader: shader.into(),
                    stage: wgpu::naga::ShaderStage::Fragment,
                    defines: std::collections::HashMap::default(),
                },
            });

        Ok((vertex_shader, fragment_shader))
    }

    /// The bind group for the uniform variables buffer.
    fn variables_binding(&self) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.variables_bindgroup_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.variables_buffer.as_entire_binding(),
            }],
            label: Some("varibales_bind_group"),
        })
    }
}
