//! GPU compositor: layer sources live as rgba16float textures (linear, premultiplied);
//! each visible layer is blended onto a ping-pong accumulation target by a compute pass,
//! then a display pass converts to sRGB for the viewer.

use crate::pipelines::{CompositeBuffers, CompositeViews, Pipelines};
use glam::{Affine2, Vec2};
use half::f16;
use ph_core::BlendMode;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A small sRGB copy of the composite being read back asynchronously for scopes.
struct ScopeJob {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    ready: Arc<AtomicBool>,
}

/// Downsampled composite for scopes: straight sRGB RGBA8, row-major.
pub struct ScopeImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub const WORK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// One layer to draw this frame.
pub struct LayerDraw {
    /// Key of an uploaded source (see [`Compositor::upload`]).
    pub source: u64,
    /// Logical source size in pixels (the texture may be a smaller proxy).
    pub size: Vec2,
    /// Logical source pixel → canvas pixel.
    pub matrix: Affine2,
    pub opacity: f32,
    pub blend: BlendMode,
    /// Key of an uploaded mask source (red channel = coverage), if any.
    pub mask: Option<u64>,
    pub mask_invert: bool,
    /// The mask came from a decoded video (sRGB-encoded greys) rather than a linear raster.
    pub mask_srgb: bool,
    pub grade: Option<ph_core::GradeSample>,
    /// Baked curve LUT (`[r, g, b, master]` × 256), when the layer has curves.
    pub curves: Option<Vec<[f32; 4]>>,
    /// Integration effects (light wrap, blur/sharpen, grain, relight).
    pub fx: Option<FxSample>,
    /// Key of an uploaded normal map (RGB = normal·0.5+0.5) for relighting.
    pub normals: Option<u64>,
    pub shadow: Option<ShadowDraw>,
    /// Key of an uploaded canvas-sized light map (R: 1 = sunlit, 0 = in the scene's shadow),
    /// used by the fx "Shadows on me" and by a cast shadow that follows the scene's light.
    pub light: Option<u64>,
    /// Key of an uploaded depth map (R: 1 = near, 0 = far) for depth of field.
    pub depth: Option<u64>,
}

/// Effect parameters for one layer at one time, laid out for the GPU (see `FxParams` in WGSL).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FxSample {
    /// light-wrap amount, wrap width (canvas px), blur (canvas px), sharpen
    pub wrap: [f32; 4],
    /// grain amount, grain size (canvas px), mono (0/1), frame seed
    pub grain: [f32; 4],
    /// light direction xyz, relight enabled (0/1)
    pub relight0: [f32; 4],
    /// light colour rgb, intensity
    pub relight1: [f32; 4],
    /// ambient, rim, wrap, amount
    pub relight2: [f32; 4],
    /// rim colour rgb, has normals (0/1)
    pub relight3: [f32; 4],
    /// canvas px → source px (set by the compositor), enabled flag (set by the compositor)
    pub scale: [f32; 4],
    /// haze colour rgb, haze amount
    pub haze: [f32; 4],
    /// pixel block size (canvas px), posterize levels
    pub game: [f32; 4],
    /// "Shadows on me": canvas → canvas cast-shadow projection rows (subject point → ground point)
    pub dapple0: [f32; 4],
    pub dapple1: [f32; 4],
    pub dapple2: [f32; 4],
    /// shade colour (shadow / lit ratio) rgb, strength
    pub dapple3: [f32; 4],
    /// softness (canvas px), enabled (0/1)
    pub dapple4: [f32; 4],
    /// depth of field: max blur (canvas px), focus, sharp range, enabled
    pub lens0: [f32; 4],
    /// bloom amount, threshold, size (canvas px), chromatic aberration (canvas px)
    pub lens1: [f32; 4],
    /// motion blur length (canvas px), direction (radians)
    pub motion: [f32; 4],
}

/// A cast + contact shadow drawn onto the backdrop before the layer itself.
#[derive(Clone, Copy, Debug)]
pub struct ShadowDraw {
    /// Canvas → canvas projection of the silhouette onto the ground.
    pub cast: Affine2,
    pub contact: Affine2,
    /// Blur radii in canvas pixels.
    pub softness: f32,
    pub contact_softness: f32,
    pub opacity: f32,
    pub contact_opacity: f32,
    pub color: [f32; 3],
    /// Only darken where the light map says the ground is lit.
    pub follow_light: bool,
    /// Key of a body-shaped shadow mask in the layer's own pixel space (alpha = shadow); when set
    /// it is drawn instead of the projected silhouette.
    pub shape: Option<u64>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowParams {
    cast: [[f32; 4]; 3],
    contact: [[f32; 4]; 3],
    size: [f32; 4],
    color: [f32; 4],
    extra: [f32; 4],
}

fn rows(m: Affine2) -> [[f32; 4]; 3] {
    [
        [m.matrix2.x_axis.x, m.matrix2.x_axis.y, 0.0, 0.0],
        [m.matrix2.y_axis.x, m.matrix2.y_axis.y, 0.0, 0.0],
        [m.translation.x, m.translation.y, 0.0, 0.0],
    ]
}

struct GpuSource {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    revision: u64,
}

struct Targets {
    width: u32,
    height: u32,
    accum: [wgpu::Texture; 2],
    accum_views: [wgpu::TextureView; 2],
    display_view: wgpu::TextureView,
    /// Which accum texture holds the latest result.
    latest: usize,
}

pub struct Compositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipes: Pipelines,
    sampler: wgpu::Sampler,
    /// 1×1 white texture bound when a layer has no mask.
    white_view: wgpu::TextureView,
    identity_lut: wgpu::Buffer,
    zero_fx: wgpu::Buffer,
    sources: HashMap<u64, GpuSource>,
    targets: Option<Targets>,
    /// Bumped whenever the display texture is recreated (the UI must re-register it).
    pub display_generation: u64,
    scope_job: Option<ScopeJob>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerParams {
    inv0: [f32; 4],
    inv1: [f32; 4],
    inv2: [f32; 4],
    src_size: [f32; 2],
    opacity: f32,
    blend: u32,
    perceptual: u32,
    has_mask: u32,
    mask_invert: u32,
    has_grade: u32,
    grade: [[f32; 4]; 11],
    has_curves: u32,
    _extra: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DisplayParams {
    checker_size: f32,
    exposure: f32,
    _pad: [f32; 2],
}

fn storage_texture(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn groups(n: u32) -> u32 {
    n.div_ceil(8)
}

impl Compositor {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("layer sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("white"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORK_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &white, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&[half::f16::ONE; 4]),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let white_view = white.create_view(&Default::default());
        let pipes = Pipelines::new(device);
        let identity: Vec<[f32; 4]> = (0..256).map(|i| [i as f32 / 255.0; 4]).collect();
        let identity_lut = pipes.uniform(device, bytemuck::cast_slice(&identity));
        let zero_fx = pipes.uniform(device, bytemuck::bytes_of(&FxSample::default()));
        Self {
            device: device.clone(),
            queue: queue.clone(),
            pipes,
            sampler,
            white_view,
            identity_lut,
            zero_fx,
            sources: HashMap::new(),
            targets: None,
            display_generation: 0,
            scope_job: None,
        }
    }

    pub fn has_source(&self, key: u64, revision: u64) -> bool {
        self.sources.get(&key).is_some_and(|s| s.revision == revision)
    }

    /// Upload (or replace) a source image in working format.
    pub fn upload(&mut self, key: u64, revision: u64, width: u32, height: u32, pixels: &[f16]) {
        let reuse = self.sources.get(&key).is_some_and(|s| s.width == width && s.height == height);
        if !reuse {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("layer source"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: WORK_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.sources.insert(key, GpuSource { texture, view, width, height, revision });
        }
        let src = self.sources.get_mut(&key).expect("just inserted");
        src.revision = revision;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &src.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(pixels),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 8), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }

    /// Drop sources not in `keep` (frees VRAM for frames no longer shown).
    pub fn retain_sources(&mut self, keep: &HashSet<u64>) {
        self.sources.retain(|k, _| keep.contains(k));
    }

    fn ensure_targets(&mut self, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.width == w && t.height == h) {
            return;
        }
        let d = &self.device;
        let accum = [storage_texture(d, w, h, WORK_FORMAT, "accum 0"), storage_texture(d, w, h, WORK_FORMAT, "accum 1")];
        let accum_views = [accum[0].create_view(&Default::default()), accum[1].create_view(&Default::default())];
        let display = storage_texture(d, w, h, DISPLAY_FORMAT, "display");
        let display_view = display.create_view(&Default::default());
        self.targets = Some(Targets { width: w, height: h, accum, accum_views, display_view, latest: 0 });
        self.display_generation += 1;
    }

    /// The sRGB viewer texture (valid after [`Compositor::render`]).
    pub fn display_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.display_view)
    }

    /// Composite `layers` (bottom first) onto a `w`×`h` canvas filled with `background`.
    pub fn render(&mut self, w: u32, h: u32, background: [f32; 4], layers: &[LayerDraw]) {
        self.ensure_targets(w, h);
        let t = self.targets.as_mut().expect("targets");
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("composite") });

        let clear_buf = self.pipes.uniform(&self.device, bytemuck::bytes_of(&background));
        let clear_bg = self.pipes.clear_bind_group(&self.device, &t.accum_views[0], &clear_buf);
        let mut cur = 0usize;
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("clear"), timestamp_writes: None });
            pass.set_pipeline(&self.pipes.clear);
            pass.set_bind_group(0, &clear_bg, &[]);
            pass.dispatch_workgroups(groups(w), groups(h), 1);
        }

        for layer in layers {
            let Some(src) = self.sources.get(&layer.source) else {
                continue;
            };
            if layer.opacity <= 0.0 {
                continue;
            }
            let mask_view = layer.mask.and_then(|k| self.sources.get(&k)).map(|m| &m.view);
            let light_view = layer.light.and_then(|k| self.sources.get(&k)).map(|m| &m.view);
            let depth_view = layer.depth.and_then(|k| self.sources.get(&k)).map(|m| &m.view);
            let inv = layer.matrix.inverse();
            // Canvas pixels → source pixels (for effect radii given in canvas pixels).
            let px_scale = inv.matrix2.x_axis.length().max(1e-6);

            if let Some(sh) = layer.shadow {
                let shape_view = sh.shape.and_then(|k| self.sources.get(&k)).map(|m| &m.view);
                // A body shadow already sits where it falls: read it straight through the layer.
                let src_from_cast = if shape_view.is_some() { inv } else { inv * sh.cast.inverse() };
                let src_from_contact = inv * sh.contact.inverse();
                let sp = ShadowParams {
                    cast: rows(src_from_cast),
                    contact: rows(src_from_contact),
                    size: [layer.size.x, layer.size.y, sh.softness * px_scale, sh.contact_softness * px_scale],
                    color: [sh.color[0], sh.color[1], sh.color[2], sh.opacity.clamp(0.0, 1.0) * layer.opacity.clamp(0.0, 1.0)],
                    extra: [
                        if shape_view.is_some() { 0.0 } else { sh.contact_opacity.clamp(0.0, 1.0) * layer.opacity.clamp(0.0, 1.0) },
                        f32::from(u8::from(mask_view.is_some() && shape_view.is_none())),
                        f32::from(u8::from(layer.mask_invert)),
                        f32::from(u8::from(sh.follow_light && light_view.is_some())),
                    ],
                };
                let sbuf = self.pipes.uniform(&self.device, bytemuck::bytes_of(&sp));
                let next = 1 - cur;
                let views = CompositeViews {
                    backdrop: &t.accum_views[cur],
                    dst: &t.accum_views[next],
                    src: shape_view.unwrap_or(&src.view),
                    mask: mask_view.unwrap_or(&self.white_view),
                    normals: &self.white_view,
                    light: light_view.unwrap_or(&self.white_view),
                    depth: &self.white_view,
                };
                let bg = self.pipes.shadow_bind_group(&self.device, &views, &self.sampler, &sbuf);
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("shadow"), timestamp_writes: None });
                pass.set_pipeline(&self.pipes.shadow);
                pass.set_bind_group(0, &bg, &[]);
                pass.dispatch_workgroups(groups(w), groups(h), 1);
                drop(pass);
                cur = next;
            }
            let params = LayerParams {
                inv0: [inv.matrix2.x_axis.x, inv.matrix2.x_axis.y, 0.0, 0.0],
                inv1: [inv.matrix2.y_axis.x, inv.matrix2.y_axis.y, 0.0, 0.0],
                inv2: [inv.translation.x, inv.translation.y, 0.0, 0.0],
                src_size: layer.size.to_array(),
                opacity: layer.opacity.clamp(0.0, 1.0),
                blend: layer.blend.shader_id(),
                perceptual: u32::from(layer.blend.space() == ph_core::blend::BlendSpace::Perceptual),
                has_mask: u32::from(mask_view.is_some()),
                mask_invert: u32::from(layer.mask_invert) | (u32::from(layer.mask_srgb) << 1),
                has_grade: u32::from(layer.grade.is_some()),
                grade: layer.grade.map_or([[0.0; 4]; 11], |g| {
                    [g.basic, g.lift, g.gamma, g.gain, g.offset, g.tone, g.match_scale, g.match_offset, g.qual0, g.qual1, g.qual2]
                }),
                has_curves: u32::from(layer.curves.is_some()),
                _extra: [0; 3],
            };
            let lut_buf = layer.curves.as_ref().map(|lut| self.pipes.uniform(&self.device, bytemuck::cast_slice(lut)));
            let normals_view = layer.normals.and_then(|k| self.sources.get(&k)).map(|n| &n.view);
            let fx_buf = layer.fx.map(|mut f| {
                f.scale = [px_scale, 1.0, 0.0, 0.0];
                if normals_view.is_none() {
                    f.relight3[3] = 0.0;
                }
                if light_view.is_none() {
                    f.dapple4[1] = 0.0;
                }
                if depth_view.is_none() {
                    f.lens0[3] = 0.0;
                }
                self.pipes.uniform(&self.device, bytemuck::bytes_of(&f))
            });
            let ubuf = self.pipes.uniform(&self.device, bytemuck::bytes_of(&params));
            let next = 1 - cur;
            let views = CompositeViews {
                backdrop: &t.accum_views[cur],
                dst: &t.accum_views[next],
                src: &src.view,
                mask: mask_view.unwrap_or(&self.white_view),
                normals: normals_view.unwrap_or(&self.white_view),
                light: light_view.unwrap_or(&self.white_view),
                depth: depth_view.unwrap_or(&self.white_view),
            };
            let bufs = CompositeBuffers {
                params: &ubuf,
                curves: lut_buf.as_ref().unwrap_or(&self.identity_lut),
                fx: fx_buf.as_ref().unwrap_or(&self.zero_fx),
            };
            let bg = self.pipes.composite_bind_group(&self.device, &views, &self.sampler, &bufs);
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("layer"), timestamp_writes: None });
            pass.set_pipeline(&self.pipes.composite);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(groups(w), groups(h), 1);
            drop(pass);
            cur = next;
        }
        t.latest = cur;

        let dparams = DisplayParams { checker_size: 16.0, exposure: 1.0, _pad: [0.0; 2] };
        let dbuf = self.pipes.uniform(&self.device, bytemuck::bytes_of(&dparams));
        let dbg = self.pipes.display_bind_group(&self.device, &t.accum_views[cur], &t.display_view, &dbuf);
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("display"), timestamp_writes: None });
            pass.set_pipeline(&self.pipes.display);
            pass.set_bind_group(0, &dbg, &[]);
            pass.dispatch_workgroups(groups(w), groups(h), 1);
        }
        self.queue.submit([enc.finish()]);
    }

    /// Queue a downsampled (≤ `max_width` px wide) copy of the last composite for scopes.
    /// Ignored while a previous request is still in flight. Collect it with [`Compositor::take_scope`].
    pub fn request_scope(&mut self, max_width: u32) {
        if self.scope_job.is_some() {
            return;
        }
        let Some(t) = self.targets.as_ref() else {
            return;
        };
        let w = t.width.min(max_width).max(1);
        let h = ((t.height as f32 * w as f32 / t.width as f32).round() as u32).max(1);
        let tex = storage_texture(&self.device, w, h, DISPLAY_FORMAT, "scope");
        let view = tex.create_view(&Default::default());
        let padded_row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scope readback"),
            size: u64::from(padded_row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bg = self.pipes.scope_bind_group(&self.device, &t.accum_views[t.latest], &view, &self.sampler);
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("scope") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("scope"), timestamp_writes: None });
            pass.set_pipeline(&self.pipes.scope);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(groups(w), groups(h), 1);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let ready = Arc::new(AtomicBool::new(false));
        let flag = ready.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| flag.store(r.is_ok(), Ordering::Release));
        self.scope_job = Some(ScopeJob { buffer, width: w, height: h, padded_row, ready });
    }

    /// The scope image, if the GPU has finished it. Never blocks.
    pub fn take_scope(&mut self) -> Option<ScopeImage> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        if !self.scope_job.as_ref()?.ready.load(Ordering::Acquire) {
            return None;
        }
        let job = self.scope_job.take()?;
        let data = job.buffer.get_mapped_range(..).ok()?;
        let mut rgba = Vec::with_capacity((job.width * job.height * 4) as usize);
        for row in 0..job.height as usize {
            let start = row * job.padded_row as usize;
            rgba.extend_from_slice(&data[start..start + (job.width * 4) as usize]);
        }
        drop(data);
        job.buffer.unmap();
        Some(ScopeImage { width: job.width, height: job.height, rgba })
    }

    /// Read the last composite back as working-format pixels (for export). Blocks on the GPU.
    pub fn read_back(&self) -> anyhow::Result<(u32, u32, Vec<f16>)> {
        let t = self.targets.as_ref().ok_or_else(|| anyhow::anyhow!("nothing rendered yet"))?;
        let (w, h) = (t.width, t.height);
        let unpadded = w * 8;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.accum[t.latest], mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buf.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
        rx.recv()??;
        let data = buf.get_mapped_range(..).map_err(|e| anyhow::anyhow!("map readback buffer: {e:?}"))?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h as usize {
            let start = row * padded as usize;
            out.extend_from_slice(bytemuck::cast_slice::<u8, f16>(&data[start..start + unpadded as usize]));
        }
        drop(data);
        buf.unmap();
        Ok((w, h, out))
    }
}
