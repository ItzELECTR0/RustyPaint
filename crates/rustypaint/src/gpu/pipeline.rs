#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub viewport_size: [f32; 2],
    pub canvas_pos: [f32; 2],
    pub canvas_size: [f32; 2],
    pub texture_size: [f32; 2],
    pub workspace_top: [f32; 4],
    pub workspace_bottom: [f32; 4],
    pub checker_light: [f32; 4],
    pub checker_dark: [f32; 4],
    pub zoom: f32,
    pub checker_size: f32,
    pub srgb_target: f32,
    pub show_canvas: f32,
    pub preview: [f32; 4],
    pub handles: f32,
    pub hot_handle: f32,
    pub backing: f32,
    pub shadow: f32,
    pub float_centre: [f32; 2],
    pub float_half: [f32; 2],
    pub float_rotation: f32,
    pub float_present: f32,
    pub ants: f32,
    pub float_handles: f32,
    pub float_hot: f32,
    pub float_reach: f32,
    pub curve_count: f32,
    pub float_opacity: f32,
    pub curve_points: [[f32; 4]; 12],
    pub accent: [f32; 4],
    pub float_masked: f32,
    pub _pad3: [f32; 3],
    pub brush_ring: [f32; 4],
    pub crop: [f32; 4],
    pub marquee: [f32; 4],
}

pub struct Viewport {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    srgb_target: bool,
    canvas: Option<Texture>,
    floating: Option<Texture>,
    blank: Option<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,
}

struct Texture {
    handle: wgpu::Texture,
    size: (u32, u32),
    uploaded: u64,
}

impl iced::widget::shader::Pipeline for Viewport {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rustypaint viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/viewport.wgsl").into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rustypaint viewport bind group layout"),
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rustypaint viewport pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustypaint viewport pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rustypaint viewport uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rustypaint viewport sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            pipeline,
            layout,
            uniforms,
            sampler,
            srgb_target: format.is_srgb(),
            canvas: None,
            floating: None,
            blank: None,
            bind_group: None,
        }
    }
}

impl Viewport {
    pub fn is_srgb_target(&self) -> bool {
        self.srgb_target
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, uniforms: &Uniforms) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    pub fn sync_canvas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        version: u64,
        dirty: Option<(u64, crate::doc::Rect)>,
        pixels: &[u8],
    ) {
        let limit = device.limits().max_texture_dimension_2d;
        if size.0 > limit || size.1 > limit {
            return;
        }

        let stale = match &self.canvas {
            Some(t) => t.size != size,
            None => true,
        };
        if stale {
            self.canvas = Some(Self::allocate(device, size, "rustypaint canvas"));
            self.bind_group = None;
        }

        let Some(texture) = &mut self.canvas else {
            return;
        };
        if texture.uploaded == version {
            return;
        }

        let partial = dirty.filter(|(from, _)| *from == texture.uploaded);
        texture.uploaded = version;

        match partial {
            Some((_, rect)) => {
                let rect = rect.clamped(size.0, size.1);
                if rect.is_empty() {
                    return;
                }
                let span = rect.width() as usize * 4;
                let stride = size.0 as usize * 4;
                let mut staged = Vec::with_capacity(span * rect.height() as usize);
                for y in rect.rows() {
                    let start = y as usize * stride + rect.x0 as usize * 4;
                    staged.extend_from_slice(&pixels[start..start + span]);
                }
                Self::upload(
                    queue,
                    &texture.handle,
                    wgpu::Origin3d {
                        x: rect.x0,
                        y: rect.y0,
                        z: 0,
                    },
                    (rect.width(), rect.height()),
                    &staged,
                );
            }
            None => Self::upload(queue, &texture.handle, wgpu::Origin3d::ZERO, size, pixels),
        }
    }

    fn upload(
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        origin: wgpu::Origin3d,
        size: (u32, u32),
        pixels: &[u8],
    ) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn sync_floating(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        floating: Option<(u32, u32, u64, &[u8])>,
    ) {
        let Some((width, height, version, pixels)) = floating else {
            return;
        };

        let limit = device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return;
        }

        let stale = match &self.floating {
            Some(t) => t.size != (width, height),
            None => true,
        };
        if stale {
            self.floating = Some(Self::allocate(
                device,
                (width, height),
                "rustypaint floating",
            ));
            self.bind_group = None;
        }

        let Some(texture) = &mut self.floating else {
            return;
        };
        if texture.uploaded == version {
            return;
        }
        texture.uploaded = version;
        Self::upload(
            queue,
            &texture.handle,
            wgpu::Origin3d::ZERO,
            (width, height),
            pixels,
        );
    }

    pub fn rebind(&mut self, device: &wgpu::Device) {
        if self.bind_group.is_some() {
            return;
        }
        let Some(canvas) = &self.canvas else { return };

        if self.blank.is_none() {
            self.blank = Some(Self::allocate(device, (1, 1), "rustypaint blank").handle);
        }
        let floating = match (&self.floating, &self.blank) {
            (Some(t), _) => &t.handle,
            (None, Some(b)) => b,
            _ => return,
        };

        let canvas_view = canvas.handle.create_view(&Default::default());
        let float_view = floating.create_view(&Default::default());
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustypaint viewport bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&canvas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&float_view),
                },
            ],
        }));
    }

    fn allocate(device: &wgpu::Device, size: (u32, u32), label: &str) -> Texture {
        let handle = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        Texture {
            handle,
            size,
            uploaded: u64::MAX,
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(bind_group) = &self.bind_group else {
            return false;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uniform_block_is_the_size_the_shader_expects() {
        assert_eq!(std::mem::size_of::<Uniforms>(), 464);
        assert_eq!(std::mem::align_of::<Uniforms>(), 4);
    }

    #[test]
    fn every_field_is_where_the_shader_thinks_it_is() {
        let src = include_str!("shaders/viewport.wgsl");
        let module = naga::front::wgsl::parse_str(src).expect("the shader parses");
        let block = module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { members, .. }
                    if ty.name.as_deref() == Some("Uniforms") =>
                {
                    Some(members.clone())
                }
                _ => None,
            })
            .expect("the shader declares Uniforms");

        let ours: Vec<(&str, usize)> = vec![
            (
                "viewport_size",
                std::mem::offset_of!(Uniforms, viewport_size),
            ),
            ("canvas_pos", std::mem::offset_of!(Uniforms, canvas_pos)),
            ("canvas_size", std::mem::offset_of!(Uniforms, canvas_size)),
            ("texture_size", std::mem::offset_of!(Uniforms, texture_size)),
            (
                "workspace_top",
                std::mem::offset_of!(Uniforms, workspace_top),
            ),
            (
                "workspace_bottom",
                std::mem::offset_of!(Uniforms, workspace_bottom),
            ),
            (
                "checker_light",
                std::mem::offset_of!(Uniforms, checker_light),
            ),
            ("checker_dark", std::mem::offset_of!(Uniforms, checker_dark)),
            ("zoom", std::mem::offset_of!(Uniforms, zoom)),
            ("checker_size", std::mem::offset_of!(Uniforms, checker_size)),
            ("srgb_target", std::mem::offset_of!(Uniforms, srgb_target)),
            ("show_canvas", std::mem::offset_of!(Uniforms, show_canvas)),
            ("preview", std::mem::offset_of!(Uniforms, preview)),
            ("handles", std::mem::offset_of!(Uniforms, handles)),
            ("hot_handle", std::mem::offset_of!(Uniforms, hot_handle)),
            ("backing", std::mem::offset_of!(Uniforms, backing)),
            ("shadow", std::mem::offset_of!(Uniforms, shadow)),
            ("float_centre", std::mem::offset_of!(Uniforms, float_centre)),
            ("float_half", std::mem::offset_of!(Uniforms, float_half)),
            (
                "float_rotation",
                std::mem::offset_of!(Uniforms, float_rotation),
            ),
            (
                "float_present",
                std::mem::offset_of!(Uniforms, float_present),
            ),
            ("ants", std::mem::offset_of!(Uniforms, ants)),
            (
                "float_handles",
                std::mem::offset_of!(Uniforms, float_handles),
            ),
            ("float_hot", std::mem::offset_of!(Uniforms, float_hot)),
            ("float_reach", std::mem::offset_of!(Uniforms, float_reach)),
            ("curve_count", std::mem::offset_of!(Uniforms, curve_count)),
            (
                "float_opacity",
                std::mem::offset_of!(Uniforms, float_opacity),
            ),
            ("curve_points", std::mem::offset_of!(Uniforms, curve_points)),
            ("accent", std::mem::offset_of!(Uniforms, accent)),
            ("float_masked", std::mem::offset_of!(Uniforms, float_masked)),
            ("brush_ring", std::mem::offset_of!(Uniforms, brush_ring)),
            ("crop", std::mem::offset_of!(Uniforms, crop)),
            ("marquee", std::mem::offset_of!(Uniforms, marquee)),
        ];

        assert_eq!(
            block.len(),
            ours.len(),
            "one side has a field the other has not"
        );
        for (member, (name, offset)) in block.iter().zip(&ours) {
            assert_eq!(
                member.name.as_deref(),
                Some(*name),
                "the fields are in a different order"
            );
            assert_eq!(member.offset as usize, *offset, "{name} is in two places");
        }
    }
}
