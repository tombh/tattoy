//! The code for interacting with the GPU.

use color_eyre::eyre::{ContextCompat as _, Result};
use wgpu::util::DeviceExt as _;

/// Common variables used by Shadertoy shaders.
#[expect(
    non_snake_case,
    reason = "
        Shaders use camelCase, so even though we could easily convert between cases,
        it's just saner to keep as they appear.
    "
)]
// NOTE: The padding is because the total number of bytes must be a factor of 4.
#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Variables {
    /// The dimensions of the TTY.
    pub iResolution: [f32; 3],
    /// Padding
    _padding1: u32,
    /// The coordinates of the mouse.
    pub iMouse: [f32; 2],
    /// The coordinates of the cursor.
    pub iCursor: [f32; 2],
    /// The wall time since the shader started.
    iTime: f32,
    /// The number of rendered shader frames.
    iFrame: u32,
    /// Padding.
    _padding2: [u32; 2],
}

/// Code for talking to the GPU.
pub(crate) struct GPU<'gpu> {
    /// Path to the current shader file.
    pub shader_path: std::path::PathBuf,
    /// The time at which rendering began.
    started: std::time::Instant,

    /// The `wgpu` device.
    pub device: wgpu::Device,
    /// The GPU render queue.
    pub queue: wgpu::Queue,

    /// The layout of all the data that is bound to the shader.
    bindgroup_layout: wgpu::BindGroupLayout,

    /// Useful varibale data for shaders. Eg, mouse coordinates, wall time, etc
    pub variables: Variables,
    /// The buffer containing shader variable data.
    variables_buffer: wgpu::Buffer,

    /// The output texture descriptor
    output_texture_descriptor: wgpu::TextureDescriptor<'gpu>,
    /// The texture on which the final render is placed.
    output_texture: wgpu::Texture,
    /// The raw data for the final render.
    output_buffer: wgpu::Buffer,

    /// The texture for the contents of the TTY.
    pub ichannel_texture: wgpu::Texture,

    /// The GPU render pipeline.
    pipeline: Option<wgpu::RenderPipeline>,
}

impl GPU<'_> {
    /// Instantiate
    pub async fn new(shader_path: std::path::PathBuf, width: u16, height: u16) -> Result<Self> {
        tracing::info!(
            "Initialising GPU pipeline for {shader_path:?} with dimensions {width}x{height}"
        );

        let variables = Variables {
            iResolution: [width.into(), height.into(), 0.0],
            ..Default::default()
        };

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

        let output_texture_descriptor =
            Self::output_texture_descriptor(width.into(), height.into());
        let output_texture = device.create_texture(&output_texture_descriptor);
        let output_buffer = device.create_buffer(&Self::output_buffer_descriptor(
            width.into(),
            height.into(),
        )?);

        let variables_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[variables]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bindgroup_layout = device.create_bind_group_layout(&Self::bindgroup_layout());

        let ichannel_texture =
            device.create_texture(&Self::ichannel_texture_descriptor(width, height));
        let mut gpu = Self {
            shader_path,
            started: std::time::Instant::now(),

            device,
            queue,

            variables,
            variables_buffer,
            bindgroup_layout,

            output_texture_descriptor,
            output_texture,
            output_buffer,

            ichannel_texture,

            pipeline: None,
        };

        gpu.build_pipeline().await?;

        Ok(gpu)
    }

    /// The output texture descriptor.
    fn output_texture_descriptor(width: u32, height: u32) -> wgpu::TextureDescriptor<'static> {
        let aligned_width = Self::align_dimension(width);
        let aligned_height = Self::align_dimension(height);
        tracing::debug!("Resizing output texture: {aligned_width}x{aligned_height}");
        wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: aligned_width,
                height: Self::align_dimension(height),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT,
            label: None,
            view_formats: &[],
        }
    }

    /// The output buffer descriptor.
    fn output_buffer_descriptor(
        width: u32,
        height: u32,
    ) -> Result<wgpu::BufferDescriptor<'static>> {
        let output_buffer_size: wgpu::BufferAddress =
            (Self::u32_size()? * Self::align_dimension(width) * Self::align_dimension(height))
                .into();
        Ok(wgpu::BufferDescriptor {
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            label: None,
            mapped_at_creation: false,
        })
    }

    /// Align a buffer or texture dimension to a consistent multiple.
    const fn align_dimension(number: u32) -> u32 {
        let multiple = 256;
        number.div_ceil(multiple) - 1 + multiple
    }

    /// Create the bind group layout that defines where the various shader data is located.
    const fn bindgroup_layout() -> wgpu::BindGroupLayoutDescriptor<'static> {
        wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("bind_group_layout"),
        }
    }

    /// (Re)build the render pipeline
    pub async fn build_pipeline(&mut self) -> Result<()> {
        let (vertex_shader, fragment_shader) = self.compile_shaders().await?;
        let render_pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render Pipeline Layout"),
                    bind_group_layouts: &[&self.bindgroup_layout],
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
                        format: self.output_texture_descriptor.format,
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

    /// The bind group for all data sent to the shader.
    fn create_bind_group(&self) -> wgpu::BindGroup {
        let ichannel_sampler = self
            .device
            .create_sampler(&wgpu::SamplerDescriptor::default());

        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.bindgroup_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.variables_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &self
                            .ichannel_texture
                            .create_view(&wgpu::TextureViewDescriptor::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&ichannel_sampler),
                },
            ],
            label: Some("bind_group"),
        })
    }

    /// Get the size of the actual render image. It is the same size as the user's terminal except
    /// that the height is twice the number of rows because of the UTF8 half-block trick.
    #[expect(
        clippy::as_conversions,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "Resolution is safely within reasonable limits of f32"
    )]
    pub(crate) const fn get_image_size(&self) -> (u16, u16) {
        let width = self.variables.iResolution[0] as u16;
        let height = self.variables.iResolution[1] as u16;
        (width, height)
    }

    /// Needed for GPU buffers and such.
    fn u32_size() -> Result<u32> {
        Ok(std::mem::size_of::<u32>().try_into()?)
    }

    /// Rebuild the output texture and buffer.
    fn rebuild_output_buffer(&mut self) -> Result<()> {
        let image_size = self.get_image_size();
        self.output_texture_descriptor =
            Self::output_texture_descriptor(image_size.0.into(), image_size.1.into());
        self.output_texture = self.device.create_texture(&self.output_texture_descriptor);
        self.output_buffer = self.device.create_buffer(&Self::output_buffer_descriptor(
            image_size.0.into(),
            image_size.1.into(),
        )?);
        Ok(())
    }

    /// Update the shader variables with the current elapsed wall time since the render began.
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
    pub fn update_resolution(&mut self, width: u16, height: u16) -> Result<()> {
        self.variables.iResolution = [width.into(), height.into(), 0.0];
        self.recreate_ichannel_texture();
        self.rebuild_output_buffer()
    }

    /// Update the `iMouse` variable for the shaders to consume.
    pub fn update_mouse_position(&mut self, col: u16, row: u16) {
        let image_height = self.variables.iResolution[1];
        let y: f32 = (row * 2).into();
        self.variables.iMouse = [col.into(), image_height - y];
    }

    /// Update the `iCursor` variable for the shaders to consume.
    pub fn update_cursor_position(&mut self, col: u16, row: u16) {
        let image_height = self.variables.iResolution[1];
        let y: f32 = (row * 2).into();
        self.variables.iCursor = [col.into(), image_height - y];
    }

    /// Tick the render
    pub async fn render(&mut self) -> Result<image::ImageBuffer<image::Rgba<f32>, Vec<f32>>> {
        self.update_wall_time();

        self.queue.write_buffer(
            &self.variables_buffer,
            0,
            bytemuck::cast_slice(&[self.variables]),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let view = &self
            .output_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        {
            let render_pass_desc = wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
                render_pass.set_bind_group(0, &self.create_bind_group(), &[]);
                render_pass.draw(0..3, 0..1);
            }
        }

        let image_size = self.get_image_size();
        let aligned_width = Self::align_dimension(image_size.0.into());
        let aligned_height = Self::align_dimension(image_size.1.into());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::All,
                texture: &self.output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(Self::u32_size()? * aligned_width),
                    rows_per_image: Some(aligned_height),
                },
            },
            wgpu::Extent3d {
                width: aligned_width,
                height: aligned_height,
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
    ) -> Result<image::ImageBuffer<image::Rgba<f32>, Vec<f32>>> {
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

        let image_size = self.get_image_size();
        let aligned_width = Self::align_dimension(image_size.0.into());
        let aligned_height = Self::align_dimension(image_size.1.into());
        let raw_image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            aligned_width,
            aligned_height,
            buffer_slice.get_mapped_range(),
        )
        .context("Couldn't convert raw GPU buffer to image")?;

        Ok(self.extract_rgba32f_image(&raw_image))
    }

    /// Convert the raw GPU image to more friendly RGB floating point pixels.
    fn extract_rgba32f_image(
        &self,
        imaged: &image::ImageBuffer<image::Rgba<u8>, wgpu::BufferView<'_>>,
    ) -> image::ImageBuffer<image::Rgba<f32>, Vec<f32>> {
        let image_size = self.get_image_size();
        image::Rgba32FImage::from_fn(image_size.0.into(), image_size.1.into(), |x, y| {
            if let Some(pixel) = imaged.get_pixel_checked(x, y) {
                [
                    f32::from(pixel[0]) / 255.0,
                    f32::from(pixel[1]) / 255.0,
                    f32::from(pixel[2]) / 255.0,
                    f32::from(pixel[3]) / 255.0,
                ]
                .into()
            } else {
                [0.0, 0.0, 0.0, 0.0].into()
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
}
