//! Compute pipelines and bind-group helpers for the compositor.

use crate::compositor::{DISPLAY_FORMAT, WORK_FORMAT};
use wgpu::util::DeviceExt;

/// Textures read/written by one layer composite pass.
pub struct CompositeViews<'a> {
    pub backdrop: &'a wgpu::TextureView,
    pub dst: &'a wgpu::TextureView,
    pub src: &'a wgpu::TextureView,
    pub mask: &'a wgpu::TextureView,
    pub normals: &'a wgpu::TextureView,
    pub light: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
}

/// Uniform buffers bound to one layer composite pass.
pub struct CompositeBuffers<'a> {
    pub params: &'a wgpu::Buffer,
    pub curves: &'a wgpu::Buffer,
    pub fx: &'a wgpu::Buffer,
}

pub struct Pipelines {
    pub composite: wgpu::ComputePipeline,
    pub clear: wgpu::ComputePipeline,
    pub display: wgpu::ComputePipeline,
    pub scope: wgpu::ComputePipeline,
    pub shadow: wgpu::ComputePipeline,
    scope_bgl: wgpu::BindGroupLayout,
    shadow_bgl: wgpu::BindGroupLayout,
    composite_bgl: wgpu::BindGroupLayout,
    clear_bgl: wgpu::BindGroupLayout,
    display_bgl: wgpu::BindGroupLayout,
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    bgl: &wgpu::BindGroupLayout,
    entry: &str,
) -> wgpu::ComputePipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&layout),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

impl Pipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let comp = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });
        let disp = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("display.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/display.wgsl").into()),
        });
        let composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                tex_entry(0),
                storage_entry(1, WORK_FORMAT),
                tex_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform_entry(4),
                tex_entry(5),
                uniform_entry(6),
                uniform_entry(7),
                tex_entry(8),
                tex_entry(9),
                tex_entry(10),
            ],
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[
                tex_entry(0),
                storage_entry(1, WORK_FORMAT),
                tex_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                uniform_entry(4),
                tex_entry(5),
                tex_entry(6),
            ],
        });
        let clear_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clear"),
            entries: &[storage_entry(0, WORK_FORMAT), uniform_entry(1)],
        });
        let display_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("display"),
            entries: &[tex_entry(0), storage_entry(1, DISPLAY_FORMAT), uniform_entry(2)],
        });
        let scope_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scope"),
            entries: &[
                tex_entry(0),
                storage_entry(1, DISPLAY_FORMAT),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        Self {
            shadow: pipeline(device, &comp, &shadow_bgl, "shadow"),
            shadow_bgl,
            scope: pipeline(device, &disp, &scope_bgl, "scope_downsample"),
            scope_bgl,
            composite: pipeline(device, &comp, &composite_bgl, "composite"),
            clear: pipeline(device, &comp, &clear_bgl, "clear"),
            display: pipeline(device, &disp, &display_bgl, "display"),
            composite_bgl,
            clear_bgl,
            display_bgl,
        }
    }

    pub fn scope_bind_group(
        &self,
        device: &wgpu::Device,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scope"),
            layout: &self.scope_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dst) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    pub fn uniform(&self, device: &wgpu::Device, bytes: &[u8]) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("params"),
            contents: bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        })
    }

    pub fn composite_bind_group(
        &self,
        device: &wgpu::Device,
        v: &CompositeViews<'_>,
        sampler: &wgpu::Sampler,
        b: &CompositeBuffers<'_>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v.backdrop) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v.dst) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(v.src) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: b.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(v.mask) },
                wgpu::BindGroupEntry { binding: 6, resource: b.curves.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: b.fx.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(v.normals) },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(v.light) },
                wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(v.depth) },
            ],
        })
    }

    pub fn shadow_bind_group(
        &self,
        device: &wgpu::Device,
        v: &CompositeViews<'_>,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow"),
            layout: &self.shadow_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v.backdrop) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(v.dst) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(v.src) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(v.mask) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(v.light) },
            ],
        })
    }

    pub fn clear_bind_group(&self, device: &wgpu::Device, dst: &wgpu::TextureView, params: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clear"),
            layout: &self.clear_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(dst) },
                wgpu::BindGroupEntry { binding: 1, resource: params.as_entire_binding() },
            ],
        })
    }

    pub fn display_bind_group(
        &self,
        device: &wgpu::Device,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("display"),
            layout: &self.display_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dst) },
                wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
            ],
        })
    }
}
