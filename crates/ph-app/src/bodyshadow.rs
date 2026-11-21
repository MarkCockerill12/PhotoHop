//! Body-shaped shadows: the person's 3D body (SAM 3D Body, rebuilt from their photo) is projected
//! onto the ground plane under their lowest point along the light direction, and the projected
//! triangles are filled into a mask in the photo's own pixels. The shadow therefore starts at the
//! feet (and knees, hands — whatever touches the ground) and has the body's real 3D shape, and it
//! moves with the layer because it lives in the layer's pixel space.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub struct BodyMesh {
    pub verts: Vec<[f32; 3]>,
    pub faces: Vec<[u32; 3]>,
    pub fx: f32,
    pub fy: f32,
    pub cx: f32,
    pub cy: f32,
    pub width: u32,
    pub height: u32,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, std::sync::Arc<BodyMesh>>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, std::sync::Arc<BodyMesh>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Load (and cache) a body.json + its body.obj.
pub fn load(json: &Path) -> Result<std::sync::Arc<BodyMesh>> {
    if let Some(m) = cache().lock().expect("body cache").get(json) {
        return Ok(m.clone());
    }
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(json)?)?;
    let k = &v["intrinsics"];
    let obj = json.with_extension("obj");
    let text = std::fs::read_to_string(&obj).with_context(|| format!("reading {}", obj.display()))?;
    let (mut verts, mut faces) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let p: Vec<f32> = it.take(3).filter_map(|s| s.parse().ok()).collect();
                if p.len() == 3 {
                    verts.push([p[0], p[1], p[2]]);
                }
            }
            Some("f") => {
                let f: Vec<u32> = it.take(3).filter_map(|s| s.split('/').next()?.parse::<u32>().ok()).map(|i| i - 1).collect();
                if f.len() == 3 {
                    faces.push([f[0], f[1], f[2]]);
                }
            }
            _ => {}
        }
    }
    let num = |x: &serde_json::Value| x.as_f64().unwrap_or(0.0) as f32;
    let mesh = std::sync::Arc::new(BodyMesh {
        verts,
        faces,
        fx: num(&k["fx"]),
        fy: num(&k["fy"]),
        cx: num(&k["cx"]),
        cy: num(&k["cy"]),
        width: v["width"].as_u64().unwrap_or(1) as u32,
        height: v["height"].as_u64().unwrap_or(1) as u32,
    });
    cache().lock().expect("body cache").insert(json.to_path_buf(), mesh.clone());
    Ok(mesh)
}

/// Shadow coverage (0/1) at `w`×`h` over the photo, for a shadow direction `angle` (degrees, image
/// space: 0 = right, 90 = towards the camera) and `length` (shadow length / height).
pub fn shadow_mask(mesh: &BodyMesh, angle: f32, length: f32, w: u32, h: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; (w * h) as usize];
    if mesh.verts.is_empty() {
        return out;
    }
    // Camera space is OpenCV (y down): the ground is at the lowest point (largest y).
    let ground = mesh.verts.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
    let (ca, sa) = (angle.to_radians().cos(), angle.to_radians().sin());
    let (sx, sy) = (w as f32 / mesh.width as f32, h as f32 / mesh.height as f32);
    let proj: Vec<[f32; 2]> = mesh
        .verts
        .iter()
        .map(|v| {
            let hgt = ground - v[1];
            // Image "down" in the shadow direction means towards the camera (−z).
            let (x, z) = (v[0] + hgt * length * ca, (v[2] - hgt * length * sa).max(0.05));
            [(mesh.fx * x / z + mesh.cx) * sx, (mesh.fy * ground / z + mesh.cy) * sy]
        })
        .collect();
    for f in &mesh.faces {
        let [a, b, c] = f.map(|i| proj[i as usize]);
        fill_triangle(&mut out, w, h, a, b, c);
    }
    out
}

fn fill_triangle(out: &mut [f32], w: u32, h: u32, a: [f32; 2], b: [f32; 2], c: [f32; 2]) {
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-6 {
        return;
    }
    let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as i64;
    let x1 = a[0].max(b[0]).max(c[0]).ceil().min(w as f32 - 1.0) as i64;
    let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as i64;
    let y1 = a[1].max(b[1]).max(c[1]).ceil().min(h as f32 - 1.0) as i64;
    let edge = |p: [f32; 2], q: [f32; 2], x: f32, y: f32| (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0]);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (e0, e1, e2) = (edge(a, b, px, py), edge(b, c, px, py), edge(c, a, px, py));
            if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0) {
                out[(y as u32 * w + x as u32) as usize] = 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thin upright "pole" 1 m tall standing 4 m in front of the camera.
    fn pole() -> BodyMesh {
        // A 10 cm × 10 cm × 1 m box (top at y = 0.5, feet at y = 1.5).
        let mut verts = Vec::new();
        for &y in &[0.5f32, 1.5] {
            for &(x, z) in &[(-0.05f32, 3.95f32), (0.05, 3.95), (0.05, 4.05), (-0.05, 4.05)] {
                verts.push([x, y, z]);
            }
        }
        let quads = [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
        let faces = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        BodyMesh { verts, faces, fx: 500.0, fy: 500.0, cx: 200.0, cy: 200.0, width: 400, height: 400 }
    }

    #[test]
    fn shadow_starts_at_the_feet_and_points_away_from_the_light() {
        let m = pole();
        // Shadow to the right (angle 0), as long as the pole is tall.
        let s = shadow_mask(&m, 0.0, 1.0, 400, 400);
        let foot_y = (500.0 * 1.5 / 4.0 + 200.0) as u32; // ground row under the pole
        let row = |x: u32| s[(foot_y * 400 + x) as usize];
        assert_eq!(row(200), 1.0, "covers the foot");
        assert_eq!(row(300), 1.0, "extends right, 1 m * 500/4 = 125 px");
        assert_eq!(row(180), 0.0, "nothing to the left");
        assert_eq!(row(340), 0.0, "ends after one body height");
    }
}

#[cfg(test)]
mod real_body {
    /// Opt-in: `PHOTOHOP_TEST_BODY=in.phop:out.phop` builds the top layer's 3D body, then Match light.
    #[test]
    fn body_shadow_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_BODY").ok() else {
            return;
        };
        let (inp, out) = spec.rsplit_once(':').unwrap();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(inp)).unwrap();
        app.selected = app.doc.layers.last().map(|l| l.id);
        app.start_ai(crate::ai::AiTask::BodyShape).unwrap();
        while app.ai_running() {
            std::thread::sleep(std::time::Duration::from_millis(300));
            app.poll_ai();
        }
        eprintln!("{}", app.status);
        eprintln!("{}", app.match_light().unwrap());
        app.save_to(std::path::Path::new(out)).unwrap();
    }
}
