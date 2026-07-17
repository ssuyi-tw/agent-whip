//! Transparent overlay presentation with `wgpu`.
//!
//! `tiny-skia` rasterizes the whip into a full-window premultiplied-RGBA pixmap;
//! this module uploads it to a texture and draws it over a transparent surface
//! via a fullscreen quad. The fragment shader adapts to the surface's alpha mode
//! so transparency works whether the compositor wants premultiplied or straight
//! alpha (and degrades to opaque where the platform offers nothing else).

use std::sync::Arc;
use winit::window::Window;

const SHADER: &str = r#"
struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
  var positions = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
    vec2<f32>(-1.0, 1.0),  vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0)
  );
  var uvs = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0),
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0)
  );
  var out: VOut;
  out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
  out.uv = uvs[vi];
  return out;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> mode: vec4<u32>;

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let c = textureSample(tex, samp, in.uv); // premultiplied from tiny-skia
  if (mode.x == 1u) {
    let a = max(c.a, 0.00001);
    return vec4<f32>(c.rgb / a, c.a); // surface wants straight alpha
  }
  if (mode.x == 2u) {
    return vec4<f32>(c.rgb, 1.0); // opaque surface: no transparency available
  }
  return c; // premultiplied passthrough
}
"#;

/// Instance/adapter/device/queue shared by every overlay window, so N monitors
/// don't spawn N wgpu devices. The adapter and device are created lazily with
/// the first window's surface (adapter selection wants a compatible surface).
pub struct GpuContext {
    instance: wgpu::Instance,
    ready: Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)>,
}

impl GpuContext {
    pub fn new() -> Self {
        GpuContext {
            instance: wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            }),
            ready: None,
        }
    }

    fn ensure_device(&mut self, surface: &wgpu::Surface<'static>) {
        if self.ready.is_some() {
            return;
        }
        let adapter =
            pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: Some(surface),
                apply_limit_buckets: false,
            }))
            .expect("no suitable GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("agent-whip device"),
            required_features: wgpu::Features::empty(),
            // The overlay is a full-screen surface, so we need the adapter's real
            // texture limits — the conservative downlevel defaults cap at 2048px.
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("request device");
        self.ready = Some((adapter, device, queue));
    }
}

/// Per-window presentation state (surface, pipeline, texture). The device and
/// queue are shared via [`GpuContext`].
pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    mode_buf: wgpu::Buffer,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

impl Gpu {
    pub fn new(ctx: &mut GpuContext, window: Arc<Window>) -> Self {
        let phys = window.inner_size();
        let size = (phys.width.max(1), phys.height.max(1));

        let surface = ctx
            .instance
            .create_surface(window.clone())
            .expect("create wgpu surface");
        ctx.ensure_device(&surface);
        let (adapter, device, queue) = {
            let (a, d, q) = ctx.ready.as_ref().unwrap();
            (a.clone(), d.clone(), q.clone())
        };

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let alpha_mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            caps.alpha_modes[0]
        };
        let mode_flag: u32 = match alpha_mode {
            wgpu::CompositeAlphaMode::PreMultiplied => 0,
            wgpu::CompositeAlphaMode::PostMultiplied => 1,
            _ => 2,
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.0,
            height: size.1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let mode_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("alpha-mode"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&mode_buf, 0, bytemuck::cast_slice(&[mode_flag, 0u32, 0, 0]));

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whip-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whip-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whip-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whip-pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("whip-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let texture = create_texture(&device, size);
        let bind_group = create_bind_group(&device, &layout, &texture, &sampler, &mode_buf);

        Gpu {
            surface,
            device,
            queue,
            config,
            pipeline,
            sampler,
            layout,
            mode_buf,
            texture,
            bind_group,
            size,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.size = (w, h);
        self.texture = create_texture(&self.device, self.size);
        self.bind_group = create_bind_group(
            &self.device,
            &self.layout,
            &self.texture,
            &self.sampler,
            &self.mode_buf,
        );
    }

    /// Upload the freshly-rasterized pixmap (premultiplied RGBA8, `size`-sized)
    /// and present it.
    pub fn render(&mut self, pixmap_rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixmap_rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.size.0),
                rows_per_image: Some(self.size.1),
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Timeout / Occluded / validation error — skip this frame.
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("whip-encoder"),
            });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("whip-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &self.bind_group, &[]);
            rpass.draw(0..6, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);
    }
}

fn create_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whip-texture"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
    mode_buf: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("whip-bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: mode_buf.as_entire_binding(),
            },
        ],
    })
}
