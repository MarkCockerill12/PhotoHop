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
