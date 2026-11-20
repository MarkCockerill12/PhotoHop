//! Export: current frame to an image, or the whole timeline to video. Video export renders a
//! few frames per UI frame (full-resolution decoders, no proxies) so the window stays responsive.

use crate::app::App;
use crate::render;
use anyhow::Result;
use ph_core::doc::{LayerContent, MediaKind};
use ph_core::MediaId;
use ph_media::{Frame, VideoCodec, VideoReader, VideoWriter};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png8,
    Png16,
    Jpeg,
    Exr,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 4] = [Self::Png8, Self::Png16, Self::Jpeg, Self::Exr];

    pub fn label(self) -> &'static str {
        match self {
            Self::Png8 => "PNG 8-bit",
            Self::Png16 => "PNG 16-bit",
            Self::Jpeg => "JPEG",
            Self::Exr => "OpenEXR (linear float)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png8 | Self::Png16 => "png",
            Self::Jpeg => "jpg",
            Self::Exr => "exr",
        }
    }
}

pub struct VideoJob {
    readers: HashMap<MediaId, VideoReader>,
    stills: HashMap<MediaId, Frame>,
    writer: Option<VideoWriter>,
    pub frame: u64,
    pub total: u64,
    pub path: PathBuf,
}

pub struct ExportState {
    pub open: bool,
    pub image_format: ImageFormat,
    pub codec: VideoCodec,
    pub audio: bool,
    pub job: Option<VideoJob>,
}

impl Default for ExportState {
    fn default() -> Self {
        Self { open: false, image_format: ImageFormat::Png16, codec: VideoCodec::H264, audio: true, job: None }
    }
}

fn export_key(media: MediaId, frame: u64) -> u64 {
    let mut h = DefaultHasher::new();
    ("export", media, frame).hash(&mut h);
    h.finish()
}

impl App {
    pub fn export_image(&mut self, path: PathBuf) -> Result<()> {
        let t = self.time;
        let mut readers: HashMap<MediaId, VideoReader> = HashMap::new();
        let mut stills: HashMap<MediaId, Frame> = HashMap::new();
        render::composite(&mut self.viewer.gpu, &self.doc, t, &self.paint, |m, mt| {
            fetch_full(&mut readers, &mut stills, m, mt)
        });
        let (w, h, px) = self.viewer.gpu.read_back()?;
        let fmt = self.export.image_format;
        let path = path.with_extension(fmt.extension());
        ph_media::save_frame(&Frame::new(w, h, px), &path, fmt == ImageFormat::Png16)?;
        self.status = format!("Exported {}", path.display());
        Ok(())
    }

    pub fn start_video_export(&mut self, path: PathBuf) -> Result<()> {
        let c = &self.doc.canvas;
        let path = path.with_extension(self.export.codec.extension());
        let audio = if self.export.audio {
            self.doc.layers.iter().filter(|l| l.visible).find_map(|l| match &l.content {
                LayerContent::Media { media, trim_in } => self.doc.media(*media).and_then(|m| match m.kind {
                    MediaKind::Video { has_audio: true, .. } => Some((m.path.clone(), (trim_in - l.start).max(0.0))),
                    _ => None,
                }),
                _ => None,
            })
        } else {
            None
        };
        let writer = VideoWriter::create(
            &path,
            c.width,
            c.height,
            c.fps,
            self.export.codec,
            audio.as_ref().map(|(p, off)| (p.as_path(), *off)),
        )?;
        self.playing = false;
        self.export.job = Some(VideoJob {
            readers: HashMap::new(),
            stills: HashMap::new(),
            writer: Some(writer),
            frame: 0,
            total: c.frame_count(),
            path,
        });
        Ok(())
    }

    /// Render queued video-export frames for up to ~40 ms. Returns true while still running.
    pub fn pump_export(&mut self) -> bool {
        let Some(mut job) = self.export.job.take() else { return false };
        let deadline = Instant::now() + Duration::from_millis(40);
        let result: Result<bool> = (|| {
            while job.frame < job.total && Instant::now() < deadline {
                let t = self.doc.canvas.frame_time(job.frame);
                let (readers, stills) = (&mut job.readers, &mut job.stills);
                render::composite(&mut self.viewer.gpu, &self.doc, t, &self.paint, |m, mt| {
                    fetch_full(readers, stills, m, mt)
                });
                let (w, h, px) = self.viewer.gpu.read_back()?;
                job.writer.as_mut().expect("writer").write(&Frame::new(w, h, px))?;
                job.frame += 1;
            }
            if job.frame >= job.total {
                job.writer.take().expect("writer").finish()?;
                return Ok(false);
            }
            Ok(true)
        })();
        match result {
            Ok(true) => {
                self.export.job = Some(job);
                true
            }
            Ok(false) => {
                self.status = format!("Exported {}", job.path.display());
                false
            }
            Err(e) => {
                self.status = format!("Export failed: {e:#}");
                false
            }
        }
    }
}

fn fetch_full(
    readers: &mut HashMap<MediaId, VideoReader>,
    stills: &mut HashMap<MediaId, Frame>,
    m: &ph_core::MediaRef,
    t: f64,
) -> Option<(u64, Frame)> {
    if m.is_video() {
        if let Entry::Vacant(e) = readers.entry(m.id) {
            e.insert(VideoReader::open(&m.path).ok()?);
        }
        let r = readers.get_mut(&m.id)?;
        let idx = (t * r.info.fps).floor().max(0.0) as u64;
        let f = r.frame(idx).ok()?;
        Some((export_key(m.id, idx), f))
    } else {
        if let Entry::Vacant(e) = stills.entry(m.id) {
            e.insert(ph_media::load_image(&m.path).ok()?);
        }
        Some((export_key(m.id, 0), stills.get(&m.id)?.clone()))
    }
}
