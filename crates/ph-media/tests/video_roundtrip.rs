//! Video I/O round-trip through the ffmpeg executables. Skips when ffmpeg is not installed.

use half::f16;
use ph_media::{Frame, VideoCodec, VideoReader, VideoWriter};
use std::process::Command;

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn write_then_read_back() {
    if !have_ffmpeg() {
        eprintln!("ffmpeg not found; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ph-video-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.mp4");
    let (w, h, frames) = (64u32, 48u32, 12u64);
    let mut writer = VideoWriter::create(&path, w, h, 24.0, VideoCodec::H264, None).unwrap();
    for i in 0..frames {
        // Frame i: grey level rising with i (linear premultiplied, opaque).
        let v = f16::from_f32(i as f32 / frames as f32);
        let px: Vec<f16> = (0..w * h).flat_map(|_| [v, v, v, f16::ONE]).collect();
        writer.write(&Frame::new(w, h, px)).unwrap();
    }
    writer.finish().unwrap();

    let mut reader = VideoReader::open(&path).unwrap();
    assert_eq!((reader.info.width, reader.info.height), (w, h));
    assert!((reader.info.fps - 24.0).abs() < 0.01, "fps {}", reader.info.fps);
    let lum = |f: &Frame| f.pixels[((h / 2) * w + w / 2) as usize * 4].to_f32();
    let (a, b) = (reader.frame(2).unwrap(), reader.frame(10).unwrap());
    assert!(lum(&b) > lum(&a) + 0.2, "frame 10 should be brighter: {} vs {}", lum(&b), lum(&a));
    let back = reader.frame(1).unwrap(); // seek backwards restarts the decoder
    assert!(lum(&back) < lum(&a) + 0.05);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_sequence_reads_like_video() {
    if !have_ffmpeg() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("ph-seq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..5u8 {
        let img = image::RgbaImage::from_pixel(8, 6, image::Rgba([i * 50, 0, 0, 255]));
        img.save(dir.join(format!("frame_{:04}.png", i + 1))).unwrap();
    }
    let pattern = dir.join("frame_%04d.png");
    assert!(ph_media::video::is_sequence_pattern(&pattern));
    let mut r = VideoReader::open(&pattern).unwrap();
    assert_eq!((r.info.width, r.info.height, r.info.frame_count()), (8, 6, 5));
    let red = |f: &Frame| f.pixels[0].to_f32();
    let (f3, f1) = (r.frame(3).unwrap(), r.frame(1).unwrap());
    assert!(red(&f3) > red(&f1), "{} vs {}", red(&f3), red(&f1));
    std::fs::remove_dir_all(&dir).ok();
}
