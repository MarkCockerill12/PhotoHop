//! Compute pipelines and bind-group helpers for the compositor.

use crate::compositor::{DISPLAY_FORMAT, WORK_FORMAT};
use wgpu::util::DeviceExt;

pub struct Pipelines {
    pub composite: wgpu::ComputePipeline,
    pub clear: wgpu::ComputePipeline,
    pub display: wgpu::ComputePipeline,
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
        Self {
            composite: pipeline(device, &comp, &composite_bgl, "composite"),
            clear: pipeline(device, &comp, &clear_bgl, "clear"),
            display: pipeline(device, &disp, &display_bgl, "display"),
            composite_bgl,
            clear_bgl,
            display_bgl,
        }
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
        backdrop: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        src: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(backdrop) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dst) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: params.as_entire_binding() },
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
