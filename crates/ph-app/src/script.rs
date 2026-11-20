//! UI automation for demos and regression checks: `PHOTOHOP_SCRIPT=steps.txt` feeds mouse and
//! keyboard input into PhotoHop's own window (never the system cursor) and saves screenshots.
//! Native file dialogs can't be driven, so `answer PATH` pre-answers the next one.
//!
//! Commands (coordinates in UI points, one per line, `#` comments):
//!   click X Y | dclick X Y | rclick X Y | move X Y | drag X1 Y1 X2 Y2 [STEPS]
//!   key NAME [ctrl] [shift] [alt] | text STRING | scroll X Y DY
//!   wait SECONDS | waitai (until AI / Blender / export jobs finish) | shot NAME | answer PATH | quit

use crate::app::App;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

enum Step {
    Events(Vec<egui::Event>),
    Wait(Duration),
    WaitJobs,
    Shot(String),
    Answer(PathBuf),
    Quit,
}

pub struct Script {
    steps: VecDeque<Step>,
    until: Option<Instant>,
    out: PathBuf,
    pos: egui::Pos2,
    /// Frames to let the UI settle before the next step.
    settle: u32,
    /// Modifier keys held by the script (egui reads them from the frame, not the key event).
    mods: egui::Modifiers,
}

fn key_named(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or_else(|| egui::Key::from_name(&name.to_uppercase()))
}

impl Script {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("PHOTOHOP_SCRIPT")?;
        let text = std::fs::read_to_string(&path).ok()?;
        let out = std::env::var_os("PHOTOHOP_SCRIPT_OUT").map_or_else(|| PathBuf::from("script-shots"), PathBuf::from);
        let _ = std::fs::create_dir_all(&out);
        let mut s = Self { steps: VecDeque::new(), until: None, out, pos: egui::pos2(0.0, 0.0), settle: 0, mods: egui::Modifiers::NONE };
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
            let n: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            let arg = |i: usize| n.get(i).copied().unwrap_or(0.0);
            match cmd {
                "click" | "dclick" | "rclick" => {
                    let p = egui::pos2(arg(0), arg(1));
                    let button = if cmd == "rclick" { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
                    s.steps.push_back(Step::Events(vec![egui::Event::PointerMoved(p)]));
                    for _ in 0..if cmd == "dclick" { 2 } else { 1 } {
                        s.steps.push_back(Step::Events(vec![press(p, button, true)]));
                        s.steps.push_back(Step::Events(vec![press(p, button, false)]));
                    }
                }
                "move" => s.steps.push_back(Step::Events(vec![egui::Event::PointerMoved(egui::pos2(arg(0), arg(1)))])),
                "drag" => {
                    let (a, b) = (egui::pos2(arg(0), arg(1)), egui::pos2(arg(2), arg(3)));
                    let steps = (n.get(4).copied().unwrap_or(12.0) as usize).max(2);
                    s.steps.push_back(Step::Events(vec![egui::Event::PointerMoved(a)]));
                    s.steps.push_back(Step::Events(vec![press(a, egui::PointerButton::Primary, true)]));
                    for k in 1..=steps {
                        s.steps.push_back(Step::Events(vec![egui::Event::PointerMoved(a + (b - a) * (k as f32 / steps as f32))]));
                    }
                    s.steps.push_back(Step::Events(vec![press(b, egui::PointerButton::Primary, false)]));
                }
                "scroll" => s.steps.push_back(Step::Events(vec![
                    egui::Event::PointerMoved(egui::pos2(arg(0), arg(1))),
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, arg(2)), modifiers: Default::default(), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel },
                ])),
                "key" => {
                    let mut it = rest.split_whitespace();
                    let Some(key) = it.next().and_then(key_named) else { continue };
                    let mods: Vec<&str> = it.collect();
                    let modifiers = egui::Modifiers {
                        ctrl: mods.contains(&"ctrl"),
                        command: mods.contains(&"ctrl"),
                        shift: mods.contains(&"shift"),
                        alt: mods.contains(&"alt"),
                        ..Default::default()
                    };
                    let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
                    s.steps.push_back(Step::Events(vec![ev(true)]));
                    s.steps.push_back(Step::Events(vec![ev(false)]));
                }
                "text" => s.steps.push_back(Step::Events(vec![egui::Event::Text(rest.to_string())])),
                "wait" => s.steps.push_back(Step::Wait(Duration::from_secs_f32(arg(0)))),
                "waitai" => s.steps.push_back(Step::WaitJobs),
                "shot" => s.steps.push_back(Step::Shot(rest.to_string())),
                "answer" => s.steps.push_back(Step::Answer(PathBuf::from(rest))),
                "quit" => s.steps.push_back(Step::Quit),
                other => log::warn!("script: unknown command {other}"),
            }
        }
        Some(s)
    }

    /// Called from `raw_input_hook`: inject the next step's events when it is due.
    pub fn feed(&mut self, app: &mut App, ctx: &egui::Context, raw: &mut egui::RawInput) {
        ctx.request_repaint();
        // Keep the pointer where the script left it (no real mouse in this window).
        raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::PointerGone));
        if self.settle > 0 {
            self.settle -= 1;
            return;
        }
        if let Some(t) = self.until {
            if Instant::now() < t {
                return;
            }
            self.until = None;
        }
        let Some(step) = self.steps.pop_front() else { return };
        match step {
            Step::Events(evs) => {
                for e in &evs {
                    match e {
                        egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. } => self.pos = *p,
                        egui::Event::Key { pressed, modifiers, .. } => {
                            self.mods = if *pressed { *modifiers } else { egui::Modifiers::NONE };
                        }
                        _ => {}
                    }
                }
                // egui takes held modifiers from ModifiersChanged, not from the key event.
                raw.events.push(egui::Event::ModifiersChanged(self.mods_for(&evs)));
                raw.events.extend(evs);
                self.settle = 1;
            }
            Step::Wait(d) => self.until = Some(Instant::now() + d),
            Step::WaitJobs => {
                if app.ai_running() || app.blender_job.is_some() || app.export_running() {
                    self.steps.push_front(Step::WaitJobs);
                    self.until = Some(Instant::now() + Duration::from_millis(250));
                }
            }
            Step::Shot(name) => {
                let path = self.out.join(format!("{name}.png"));
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                        let _ = buf.save(&path);
                    }
                });
                log::info!("script: shot {name} (pixels per point {})", ctx.pixels_per_point());
                self.settle = 3;
            }
            Step::Answer(p) => app.dialog_answers.push_back(p),
            Step::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

impl Script {
    fn mods_for(&self, evs: &[egui::Event]) -> egui::Modifiers {
        match evs.first() {
            Some(egui::Event::Key { pressed: true, modifiers, .. }) => *modifiers,
            _ => egui::Modifiers::NONE,
        }
    }
}

fn press(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button, pressed, modifiers: Default::default() }
}
