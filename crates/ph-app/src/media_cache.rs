//! Decoded media for the viewer. Stills are decoded once; each video gets a background
//! decoder thread that serves the playhead frame first and then prefetches ahead.

use crossbeam_channel::{Receiver, Sender, unbounded};
use ph_core::{MediaId, MediaRef};
use ph_media::{Frame, VideoReader};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::thread;

/// Frames to decode past the requested one while idle.
const PREFETCH: u64 = 24;
/// Frames kept per video (around the playhead).
const CACHE_FRAMES: usize = 64;

pub fn source_key(media: MediaId, frame: u64) -> u64 {
    let mut h = DefaultHasher::new();
    media.hash(&mut h);
    frame.hash(&mut h);
    h.finish()
}

/// A decoded frame index and its result.
type Decoded = (u64, Result<Frame, String>);

struct VideoSlot {
    request: Sender<u64>,
    results: Receiver<Decoded>,
    frames: HashMap<u64, Frame>,
    fps: f64,
    frame_count: u64,
    last_request: Option<u64>,
    error: Option<String>,
}

pub struct MediaCache {
    images: HashMap<MediaId, Result<Frame, String>>,
    videos: HashMap<MediaId, VideoSlot>,
    /// Preview decode divisor for videos (1 = full, 2 = half, 4 = quarter).
    pub proxy_div: u32,
    ctx: egui::Context,
}

fn spawn_decoder(path: PathBuf, div: u32, fps: f64, ctx: egui::Context) -> (Sender<u64>, Receiver<Decoded>) {
    let (req_tx, req_rx) = unbounded::<u64>();
    let (res_tx, res_rx) = unbounded();
    thread::Builder::new()
        .name("video-decode".into())
        .spawn(move || {
            let mut reader = match VideoReader::open_with(&path, div, fps) {
                Ok(r) => r,
                Err(e) => {
                    let _ = res_tx.send((0, Err(format!("{e:#}"))));
                    return;
                }
            };
            let last = reader.info.frame_count().saturating_sub(1);
            let mut target: Option<u64> = None;
            let mut next: u64 = 0;
            loop {
                // Block when idle, otherwise just drain to the newest request.
                let incoming = if target.is_some_and(|t| next <= (t + PREFETCH).min(last)) {
                    req_rx.try_iter().last()
                } else {
                    match req_rx.recv() {
                        Ok(first) => Some(req_rx.try_iter().last().unwrap_or(first)),
                        Err(_) => return,
                    }
                };
                if let Some(t) = incoming {
                    if target.is_none_or(|old| t < old || t > next) {
                        next = t;
                    }
                    target = Some(t);
                }
                let Some(t) = target else { continue };
                if next > (t + PREFETCH).min(last) {
                    continue;
                }
                let idx = next;
                next += 1;
                let res = reader.frame(idx).map_err(|e| format!("{e:#}"));
                if res_tx.send((idx, res)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        })
        .expect("spawn decoder thread");
    (req_tx, res_rx)
}

impl MediaCache {
    pub fn new(ctx: egui::Context) -> Self {
        Self { images: HashMap::new(), videos: HashMap::new(), proxy_div: 2, ctx }
    }

    /// Forget everything (new/open project, proxy change).
    pub fn clear(&mut self) {
        self.images.clear();
        self.videos.clear();
    }

    fn ensure(&mut self, m: &MediaRef) {
        if m.is_video() {
            if !self.videos.contains_key(&m.id) {
                let media_fps = match m.kind {
                    ph_core::MediaKind::Video { fps, .. } => fps,
                    ph_core::MediaKind::Image => 24.0,
                };
                let (request, results) = spawn_decoder(m.path.clone(), self.proxy_div, media_fps, self.ctx.clone());
                let (fps, frame_count) = match m.kind {
                    ph_core::MediaKind::Video { fps, duration, .. } => (fps, (duration * fps).floor().max(1.0) as u64),
                    ph_core::MediaKind::Image => (1.0, 1),
                };
                self.videos.insert(
                    m.id,
                    VideoSlot { request, results, frames: HashMap::new(), fps, frame_count, last_request: None, error: None },
                );
            }
        } else {
            self.images.entry(m.id).or_insert_with(|| ph_media::load_image(&m.path).map_err(|e| format!("{e:#}")));
        }
    }

    /// Pull finished frames from decoder threads. Call once per UI frame.
    pub fn poll(&mut self) {
        for slot in self.videos.values_mut() {
            for (idx, res) in slot.results.try_iter() {
                match res {
                    Ok(f) => {
                        slot.frames.insert(idx, f);
                    }
                    Err(e) => slot.error = Some(e),
                }
            }
            if slot.frames.len() > CACHE_FRAMES {
                let centre = slot.last_request.unwrap_or(0) as i64;
                let mut keys: Vec<u64> = slot.frames.keys().copied().collect();
                keys.sort_by_key(|k| (*k as i64 - centre).abs());
                for k in keys.into_iter().skip(CACHE_FRAMES) {
                    slot.frames.remove(&k);
                }
            }
        }
    }

    /// Best available frame for `media` at source time `t`: `(gpu key, frame)`.
    /// For videos this requests decoding and may return a nearby frame until the exact one arrives.
    pub fn frame(&mut self, m: &MediaRef, t: f64) -> Option<(u64, Frame)> {
        self.ensure(m);
        if !m.is_video() {
            return match self.images.get(&m.id)? {
                Ok(f) => Some((source_key(m.id, 0), f.clone())),
                Err(_) => None,
            };
        }
        let slot = self.videos.get_mut(&m.id)?;
        let idx = ((t * slot.fps).floor().max(0.0) as u64).min(slot.frame_count.saturating_sub(1));
        if slot.last_request != Some(idx) {
            let _ = slot.request.send(idx);
            slot.last_request = Some(idx);
        }
        if let Some(f) = slot.frames.get(&idx) {
            return Some((source_key(m.id, idx), f.clone()));
        }
        // Fall back to the closest decoded frame so scrubbing never flashes empty.
        let near = slot.frames.keys().min_by_key(|k| (**k as i64 - idx as i64).abs()).copied()?;
        Some((source_key(m.id, near), slot.frames[&near].clone()))
    }

    pub fn error(&self, id: MediaId) -> Option<&str> {
        match self.images.get(&id) {
            Some(Err(e)) => Some(e),
            _ => self.videos.get(&id).and_then(|s| s.error.as_deref()),
        }
    }
}
