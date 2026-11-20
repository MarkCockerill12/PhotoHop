//! Headless GPU check of the compositor maths. Skips (passes) when no GPU adapter is available.

use glam::{Affine2, Vec2};
use half::f16;
use ph_core::BlendMode;
use ph_gpu::{Compositor, LayerDraw};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

fn solid(w: u32, h: u32, rgba: [f32; 4]) -> Vec<f16> {
    let px = [rgba[0] * rgba[3], rgba[1] * rgba[3], rgba[2] * rgba[3], rgba[3]].map(f16::from_f32);
    px.iter().copied().cycle().take((w * h * 4) as usize).collect()
}

#[test]
fn normal_over_and_multiply() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut c = Compositor::new(&device, &queue);
    c.upload(1, 0, 4, 4, &solid(4, 4, [1.0, 0.0, 0.0, 0.5]));
    let full = Affine2::from_translation(Vec2::ZERO);
    let draw = |blend| LayerDraw { source: 1, size: Vec2::new(4.0, 4.0), matrix: full, opacity: 1.0, blend };

    c.render(4, 4, [0.0, 0.0, 1.0, 1.0], &[draw(BlendMode::Normal)]);
    let (_, _, px) = c.read_back().unwrap();
    let p: Vec<f32> = px[..4].iter().map(|v| v.to_f32()).collect();
    // Half-transparent red over opaque blue = (0.5, 0, 0.5, 1) premultiplied.
    assert!((p[0] - 0.5).abs() < 0.01 && p[1].abs() < 0.01 && (p[2] - 0.5).abs() < 0.01 && (p[3] - 1.0).abs() < 0.01, "{p:?}");

    c.render(4, 4, [0.5, 0.5, 0.5, 1.0], &[draw(BlendMode::Multiply)]);
    let (_, _, px) = c.read_back().unwrap();
    let p: Vec<f32> = px[..4].iter().map(|v| v.to_f32()).collect();
    // Multiply red(1,0,0) with grey 0.5 at 50% coverage: r = 0.5 + 0.5*(0.5-0.5)=0.5, g = 0.5*0.5 = 0.25.
    assert!((p[0] - 0.5).abs() < 0.01 && (p[1] - 0.25).abs() < 0.01, "{p:?}");
}
