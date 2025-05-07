//! Support for the Shader Toy convention of a `iChannel0` buffer. In our case it typically
//! contains a pixel representation of the TTY.

impl super::gpu::GPU<'_> {
    /// Update the GPU with the current state of the terminal as RGB values.
    pub fn update_ichannel_texture_data(&self, image_data: &image::RgbaImage) {
        let tty_image_width = image_data.dimensions().0;
        let tty_image_height = image_data.dimensions().1;
        let output_image_size = self.get_image_size();
        if tty_image_width != u32::from(output_image_size.0)
            || tty_image_height != u32::from(output_image_size.1)
        {
            return;
        }

        tracing::debug!("Updating GPU with new TTY image data: {}", image_data.len());
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.ichannel_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * tty_image_width),
                rows_per_image: Some(tty_image_height),
            },
            wgpu::Extent3d {
                width: tty_image_width,
                height: tty_image_height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Recreate the iChannel texture. Most likely occurs when the user's terminal resizes.
    pub fn recreate_ichannel_texture(&mut self) {
        tracing::debug!(
            "Recreating iChannel texture with size: {:?}",
            self.variables.iResolution
        );

        let image_size = self.get_image_size();
        self.ichannel_texture = self
            .device
            .create_texture(&Self::ichannel_texture_descriptor(
                image_size.0,
                image_size.1,
            ));
    }

    /// The texture descriptor for the iChannel texture.
    pub fn ichannel_texture_descriptor(
        width: u16,
        height: u16,
    ) -> wgpu::TextureDescriptor<'static> {
        wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: width.into(),
                height: height.into(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("ichannel_texture"),
            view_formats: &[],
        }
    }
}
