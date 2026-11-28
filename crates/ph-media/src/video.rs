//! Video I/O through the `ffmpeg`/`ffprobe` executables (raw frames over pipes).
//! Decoding keeps one ffmpeg process alive while frames are read in order and restarts it
//! (with a fast `-ss` seek) when the requested frame is behind or far ahead.

use crate::color;
use crate::image_io::Frame;
use anyhow::{Context, Result, anyhow, bail};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v"];

pub fn is_video_path(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.as_str()))
}

fn ffmpeg_bin() -> std::path::PathBuf {
    crate::process::tool_path("ffmpeg", "PHOTOHOP_FFMPEG")
}

fn ffprobe_bin() -> std::path::PathBuf {
    crate::process::tool_path("ffprobe", "PHOTOHOP_FFPROBE")
}

#[derive(Clone, Debug)]
pub struct VideoInfo {
    /// Display size (after rotation metadata is applied, as ffmpeg does when decoding).
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub has_audio: bool,
    /// More than 8 bits per channel (decoded at 16 bits to avoid banding).
    pub deep: bool,
    pub transfer: color::Transfer,
    /// BT.2020 primaries (converted to the working BT.709 primaries on decode).
    pub bt2020: bool,
}

impl VideoInfo {
    pub fn frame_count(&self) -> u64 {
        (self.duration * self.fps).floor().max(1.0) as u64
    }
}

fn parse_rate(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then_some(n / d)
}

/// An image sequence is addressed by a printf pattern in the file name, e.g. `render_%04d.png`.
pub fn is_sequence_pattern(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().contains("%0"))
}

/// Files matching a sequence pattern, sorted, with their frame numbers.
pub fn sequence_files(pattern: &Path) -> Vec<(u64, PathBuf)> {
    let name = pattern.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(pct) = name.find("%0") else { return Vec::new() };
    let digits_end = name[pct..].find('d').map(|i| pct + i + 1).unwrap_or(pct);
    let (prefix, suffix) = (&name[..pct], &name[digits_end..]);
    let dir = pattern.parent().unwrap_or(Path::new("."));
    let mut out: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let mid = n.strip_prefix(prefix)?.strip_suffix(suffix)?;
            mid.parse::<u64>().ok().map(|k| (k, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// Probe an image sequence (`fps` comes from the caller, sequences carry no timing).
pub fn probe_sequence(pattern: &Path, fps: f64) -> Result<(VideoInfo, u64)> {
    let files = sequence_files(pattern);
    let (first_no, first) = files.first().ok_or_else(|| anyhow!("no files match {}", pattern.display()))?;
    let (w, h) = image::image_dimensions(first).with_context(|| format!("reading {}", first.display()))?;
    let n = files.len() as f64;
    Ok((
        VideoInfo { width: w, height: h, fps, duration: n / fps, has_audio: false, deep: false, transfer: color::Transfer::Sdr, bt2020: false },
        *first_no,
    ))
}

/// Default frame rate assumed for image sequences when probing them like videos.
pub const SEQUENCE_FPS: f64 = 24.0;

pub fn probe(path: &Path) -> Result<VideoInfo> {
    if is_sequence_pattern(path) {
        return probe_sequence(path, SEQUENCE_FPS).map(|(i, _)| i);
    }
    let out = crate::process::command(ffprobe_bin())
        .args(["-v", "error", "-show_streams", "-show_format", "-of", "json"])
        .arg(path)
        .output()
        .context("running ffprobe (is FFmpeg installed and on PATH?)")?;
    if !out.status.success() {
        bail!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = v["streams"].as_array().ok_or_else(|| anyhow!("no streams"))?;
    let vs = streams.iter().find(|s| s["codec_type"] == "video").ok_or_else(|| anyhow!("no video stream"))?;
    let (mut w, mut h) = (vs["width"].as_u64().unwrap_or(0) as u32, vs["height"].as_u64().unwrap_or(0) as u32);
    let rotation = vs["side_data_list"]
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()))
        .or_else(|| vs["tags"]["rotate"].as_str().and_then(|r| r.parse().ok()))
        .unwrap_or(0.0);
    if (rotation.abs() - 90.0).abs() < 1.0 || (rotation.abs() - 270.0).abs() < 1.0 {
        std::mem::swap(&mut w, &mut h);
    }
    // Phones record slightly variable rates; prefer the nominal rate when it agrees with the average.
    let avg = vs["avg_frame_rate"].as_str().and_then(parse_rate);
    let nominal = vs["r_frame_rate"].as_str().and_then(parse_rate);
    let fps = match (nominal, avg) {
        (Some(n), Some(a)) if ((n - a) / a).abs() < 0.01 => n,
        (_, Some(a)) => a,
        (Some(n), None) => n,
        (None, None) => 30.0,
    };
    let duration = vs["duration"]
        .as_str()
        .or_else(|| v["format"]["duration"].as_str())
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");
    if w == 0 || h == 0 {
        bail!("could not read video size");
    }
    let pix = vs["pix_fmt"].as_str().unwrap_or("");
    let deep = ["10", "12", "16", "p010", "p016"].iter().any(|t| pix.contains(t));
    let transfer = match vs["color_transfer"].as_str().unwrap_or("") {
        "smpte2084" => color::Transfer::Pq,
        "arib-std-b67" => color::Transfer::Hlg,
        _ => color::Transfer::Sdr,
    };
    let bt2020 = vs["color_primaries"].as_str().is_some_and(|p| p.starts_with("bt2020"));
    Ok(VideoInfo { width: w, height: h, fps, duration, has_audio, deep, transfer, bt2020 })
}

/// Sequential frame reader for one video file.
pub struct VideoReader {
    path: PathBuf,
    pub info: VideoInfo,
    child: Option<(Child, ChildStdout)>,
    /// Decoded (possibly proxy-scaled) frame size.
    pub out_width: u32,
    pub out_height: u32,
    /// Index of the next frame the running process will deliver.
    next_frame: u64,
    buf: Vec<u8>,
    /// First frame number of an image sequence (None for real video files).
    start_number: Option<u64>,
}

/// Restart the decoder instead of reading forward when the target is further ahead than this.
const MAX_READ_AHEAD: u64 = 48;

impl VideoReader {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_scaled(path, 1)
    }

    /// Open with frames decoded at 1/`div` resolution (preview proxies). `div = 1` is full size.
    pub fn open_scaled(path: &Path, div: u32) -> Result<Self> {
        Self::open_with(path, div, SEQUENCE_FPS)
    }

    /// Like [`VideoReader::open_scaled`], with the frame rate to use if `path` is an image sequence.
    pub fn open_with(path: &Path, div: u32, sequence_fps: f64) -> Result<Self> {
        let (info, start_number) = if is_sequence_pattern(path) {
            let (i, s) = probe_sequence(path, sequence_fps)?;
            (i, Some(s))
        } else {
            (probe(path)?, None)
        };
        let div = div.max(1);
        let even = |v: u32| (v.div_ceil(div) & !1).max(2);
        let (out_width, out_height) = if div == 1 { (info.width, info.height) } else { (even(info.width), even(info.height)) };
        Ok(Self { path: path.to_path_buf(), info, child: None, out_width, out_height, next_frame: 0, buf: Vec::new(), start_number })
    }

    fn spawn_at(&mut self, frame: u64) -> Result<()> {
        self.stop();
        let t = frame as f64 / self.info.fps;
        let mut cmd = crate::process::command(ffmpeg_bin());
        cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if let Some(start) = self.start_number {
            // Image sequences: jump straight to the wanted file instead of seeking.
            cmd.args(["-framerate", &format!("{}", self.info.fps), "-start_number", &format!("{}", start + frame)]);
        } else {
            if t > 0.0 {
                cmd.args(["-ss", &format!("{t:.6}")]);
            }
            cmd.args(["-hwaccel", "auto"]);
        }
        cmd.arg("-i").arg(&self.path);
        cmd.args(["-an", "-sn"]);
        if (self.out_width, self.out_height) != (self.info.width, self.info.height) {
            cmd.args(["-vf", &format!("scale={}:{}:flags=bilinear", self.out_width, self.out_height)]);
        }
        let pix_fmt = if self.wide() { "rgba64le" } else { "rgba" };
        cmd.args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-fps_mode", "passthrough", "-"]);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd.spawn().context("starting ffmpeg (is FFmpeg installed and on PATH?)")?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("ffmpeg stdout"))?;
        self.child = Some((child, stdout));
        self.next_frame = frame;
        Ok(())
    }

    /// Decode at 16 bits per channel (deep or HDR sources).
    fn wide(&self) -> bool {
        self.info.deep || self.info.transfer != color::Transfer::Sdr || self.info.bt2020
    }

    fn stop(&mut self) {
        if let Some((mut c, _)) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Index of the frame the running decoder delivers next (reading it is cheap; earlier ones
    /// need a restart).
    pub fn next_frame(&self) -> u64 {
        if self.child.is_some() { self.next_frame } else { 0 }
    }

    /// Decode frame `index` (clamped to the clip) into working format.
    pub fn frame(&mut self, index: u64) -> Result<Frame> {
        let index = index.min(self.info.frame_count().saturating_sub(1));
        let need_restart =
            self.child.is_none() || index < self.next_frame || index > self.next_frame + MAX_READ_AHEAD;
        if need_restart {
            self.spawn_at(index)?;
        }
        let (w, h) = (self.out_width, self.out_height);
        let size = (w * h * 4) as usize * if self.wide() { 2 } else { 1 };
        self.buf.resize(size, 0);
        loop {
            let (_, out) = self.child.as_mut().ok_or_else(|| anyhow!("decoder not running"))?;
            if let Err(e) = out.read_exact(&mut self.buf) {
                self.stop();
                return Err(anyhow!("end of video or decode error at frame {index}: {e}"));
            }
            let got = self.next_frame;
            self.next_frame += 1;
            if got >= index {
                let px = if self.wide() {
                    let words: Vec<u16> = self.buf.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
                    color::rgba16_video_to_linear_premul(&words, self.info.transfer, self.info.bt2020)
                } else {
                    color::rgba8_srgb_to_linear_premul(&self.buf)
                };
                return Ok(Frame::new(w, h, px));
            }
        }
    }
}

impl Drop for VideoReader {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    ProRes4444,
}

impl VideoCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264 MP4 (phone-ready)",
            Self::H265 => "H.265/HEVC MP4 (smaller)",
            Self::ProRes4444 => "ProRes 4444 MOV (with alpha)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::ProRes4444 => "mov",
            _ => "mp4",
        }
    }
}

/// Streams raw frames into an ffmpeg encoder; optional audio is copied from a source file.
pub struct VideoWriter {
    child: Child,
    stdin: Option<ChildStdin>,
    width: u32,
    height: u32,
}

impl VideoWriter {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: f64,
        codec: VideoCodec,
        audio_from: Option<(&Path, f64)>,
    ) -> Result<Self> {
        // yuv420p needs even dimensions.
        let mut cmd = crate::process::command(ffmpeg_bin());
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{width}x{height}")]);
        cmd.args(["-r", &format!("{fps}"), "-i", "-"]);
        if let Some((audio, offset)) = audio_from {
            if offset > 0.0 {
                cmd.args(["-ss", &format!("{offset:.6}")]);
            }
            cmd.arg("-i").arg(audio);
            cmd.args(["-map", "0:v", "-map", "1:a?", "-c:a", "aac", "-b:a", "192k", "-shortest"]);
        }
        match codec {
            VideoCodec::H264 => cmd.args(["-c:v", "libx264", "-preset", "slow", "-crf", "16", "-pix_fmt", "yuv420p"]),
            VideoCodec::H265 => cmd.args(["-c:v", "libx265", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-tag:v", "hvc1"]),
            VideoCodec::ProRes4444 => cmd.args(["-c:v", "prores_ks", "-profile:v", "4444", "-pix_fmt", "yuva444p10le"]),
        };
        cmd.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-movflags", "+faststart"]);
        cmd.arg(path);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = cmd.spawn().context("starting ffmpeg encoder")?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin, width, height })
    }

    /// Write one working-format frame.
    pub fn write(&mut self, frame: &Frame) -> Result<()> {
        if frame.width != self.width || frame.height != self.height {
            bail!("frame size mismatch");
        }
        let rgba = color::linear_premul_to_rgba8(&frame.pixels);
        self.stdin.as_mut().ok_or_else(|| anyhow!("encoder closed"))?.write_all(&rgba).context("writing to encoder")?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let out = self.child.wait_with_output()?;
        if !out.status.success() {
            bail!("ffmpeg encode failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    }
}
