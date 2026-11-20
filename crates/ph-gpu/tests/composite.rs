//! Headless GPU check of the compositor maths. Skips (passes) when no GPU adapter is available.

use glam::{Affine2, Vec2};
use half::f16;
use ph_core::BlendMode;
use ph_gpu::{Compositor, FxSample, LayerDraw, ShadowDraw};

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
    let draw = |blend| LayerDraw { source: 1, size: Vec2::new(4.0, 4.0), matrix: full, opacity: 1.0, blend, mask: None, mask_invert: false, grade: None, curves: None, fx: None, normals: None, shadow: None };

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

#[test]
fn grade_and_mask() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut c = Compositor::new(&device, &queue);
    c.upload(1, 0, 4, 4, &solid(4, 4, [0.2, 0.4, 0.1, 1.0]));
    c.upload(2, 0, 4, 4, &solid(4, 4, [0.0, 0.0, 0.0, 1.0])); // black mask
    let full = Affine2::IDENTITY;
    let px0 = |c: &mut Compositor, d: LayerDraw| {
        c.render(4, 4, [0.0, 0.0, 0.0, 0.0], &[d]);
        let (_, _, px) = c.read_back().unwrap();
        [px[0].to_f32(), px[1].to_f32(), px[2].to_f32(), px[3].to_f32()]
    };
    let base = || LayerDraw {
        source: 1,
        size: Vec2::new(4.0, 4.0),
        matrix: full,
        opacity: 1.0,
        blend: BlendMode::Normal,
        mask: None,
        mask_invert: false,
        grade: None,
        curves: None,
        fx: None,
        normals: None,
        shadow: None,
    };

    let identity = ph_core::Grade::default().sample(0.0);
    let p = px0(&mut c, LayerDraw { grade: Some(identity), ..base() });
    assert!((p[0] - 0.2).abs() < 0.005 && (p[1] - 0.4).abs() < 0.005 && (p[2] - 0.1).abs() < 0.005, "identity {p:?}");

    let g = ph_core::Grade { exposure: ph_core::Animated::new(1.0), ..Default::default() };
    let p = px0(&mut c, LayerDraw { grade: Some(g.sample(0.0)), ..base() });
    assert!((p[0] - 0.4).abs() < 0.01 && (p[1] - 0.8).abs() < 0.01, "+1 stop {p:?}");

    let p = px0(&mut c, LayerDraw { mask: Some(2), ..base() });
    assert!(p[3] < 0.001, "black mask hides {p:?}");
    let p = px0(&mut c, LayerDraw { mask: Some(2), mask_invert: true, ..base() });
    assert!((p[3] - 1.0).abs() < 0.001, "inverted black mask shows {p:?}");
}

#[test]
fn scope_readback() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut c = Compositor::new(&device, &queue);
    c.render(64, 32, [1.0, 0.0, 0.0, 1.0], &[]);
    c.request_scope(16);
    let mut img = None;
    for _ in 0..200 {
        img = c.take_scope();
        if img.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let img = img.expect("scope image");
    assert_eq!((img.width, img.height), (16, 8));
    assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
}

#[test]
fn curves_and_qualifier() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut c = Compositor::new(&device, &queue);
    // Saturated red pixel.
    c.upload(1, 0, 4, 4, &solid(4, 4, [0.8, 0.05, 0.05, 1.0]));
    let draw = |grade: ph_core::Grade| LayerDraw {
        source: 1,
        size: Vec2::new(4.0, 4.0),
        matrix: Affine2::IDENTITY,
        opacity: 1.0,
        blend: BlendMode::Normal,
        mask: None,
        mask_invert: false,
        curves: (!grade.curves.is_identity()).then(|| grade.curves.bake()),
        grade: Some(grade.sample(0.0)),
        fx: None,
        normals: None,
        shadow: None,
    };
    let px0 = |c: &mut Compositor, d: LayerDraw| {
        c.render(4, 4, [0.0; 4], &[d]);
        let (_, _, px) = c.read_back().unwrap();
        [px[0].to_f32(), px[1].to_f32(), px[2].to_f32()]
    };
    // Curves: master pulled to 0 -> black.
    let g = ph_core::Grade {
        curves: ph_core::Curves { master: vec![[0.0, 0.0], [1.0, 0.0]], ..Default::default() },
        ..Default::default()
    };
    let p = px0(&mut c, draw(g));
    assert!(p.iter().all(|v| v.abs() < 0.01), "flat-zero curve {p:?}");
    // Qualifier on red, shift hue by 1/3 turn -> red becomes green-dominant.
    let mut g = ph_core::Grade {
        qualifier: ph_core::Qualifier { enabled: true, hue_center: 0.0, hue_width: 0.1, hue_shift: 1.0 / 3.0, ..Default::default() },
        ..Default::default()
    };
    let p = px0(&mut c, draw(g.clone()));
    assert!(p[1] > p[0], "red qualified and rotated towards green {p:?}");
    // Show matte: selected pixel shows white.
    g.qualifier.show_matte = true;
    let p = px0(&mut c, draw(g));
    assert!(p.iter().all(|v| *v > 0.9), "matte {p:?}");
}

#[test]
fn shadow_and_light_wrap() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut c = Compositor::new(&device, &queue);
    // 8x8 opaque white square in the middle of a 32x32 canvas.
    c.upload(1, 0, 8, 8, &solid(8, 8, [1.0, 1.0, 1.0, 1.0]));
    let m = Affine2::from_translation(Vec2::new(12.0, 8.0));
    let base = || LayerDraw {
        source: 1,
        size: Vec2::new(8.0, 8.0),
        matrix: m,
        opacity: 1.0,
        blend: BlendMode::Normal,
        mask: None,
        mask_invert: false,
        grade: None,
        curves: None,
        fx: None,
        normals: None,
        shadow: None,
    };
    let at = |px: &[half::f16], x: usize, y: usize| px[(y * 32 + x) * 4].to_f32();
    // Shadow straight down (pure translation by 8px) darkens the grey background below the square.
    let shadow = ShadowDraw {
        cast: Affine2::from_translation(Vec2::new(0.0, 8.0)),
        contact: Affine2::IDENTITY,
        softness: 0.0,
        contact_softness: 0.0,
        opacity: 1.0,
        contact_opacity: 0.0,
        color: [0.0; 3],
    };
    c.render(32, 32, [0.5, 0.5, 0.5, 1.0], &[LayerDraw { shadow: Some(shadow), ..base() }]);
    let (_, _, px) = c.read_back().unwrap();
    assert!(at(&px, 15, 20) < 0.05, "shadow under the square: {}", at(&px, 15, 20));
    assert!((at(&px, 2, 2) - 0.5).abs() < 0.01, "background elsewhere untouched");
    assert!((at(&px, 15, 12) - 1.0).abs() < 0.01, "square itself drawn on top");

    // Light wrap: a black square over a bright red background picks up red at its edge, not its centre.
    c.upload(2, 0, 8, 8, &solid(8, 8, [0.0, 0.0, 0.0, 1.0]));
    let fx = FxSample { wrap: [1.0, 6.0, 0.0, 0.0], ..Default::default() };
    c.render(32, 32, [1.0, 0.0, 0.0, 1.0], &[LayerDraw { source: 2, fx: Some(fx), ..base() }]);
    let (_, _, px) = c.read_back().unwrap();
    let (edge, centre) = (at(&px, 12, 12), at(&px, 15, 12));
    assert!(edge > 0.1 && edge > centre, "edge {edge} centre {centre}");
}
