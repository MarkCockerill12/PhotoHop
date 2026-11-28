//! Help > Manual (F1): the built-in manual with a contents list, search and clickable links.

use crate::app::App;
use crate::manual::{Block, PAGES, Span, anchor, page, search, spans};

#[derive(Default)]
pub struct HelpState {
    pub open: bool,
    pub page: String,
    pub query: String,
    /// Section anchor to scroll to on the next frame.
    scroll_to: Option<String>,
    scroll_top: bool,
}

impl HelpState {
    /// Open the manual at `target` (`page-slug` or `page-slug#section`).
    pub fn open_page(&mut self, target: &str) {
        let (slug, sec) = target.split_once('#').unwrap_or((target, ""));
        self.open = true;
        self.page = if page(slug).is_some() { slug.to_owned() } else { "start".to_owned() };
        self.scroll_to = (!sec.is_empty()).then(|| sec.to_owned());
        self.scroll_top = sec.is_empty();
    }
}

/// Write the manual as HTML next to the temp folder and open it in the default browser.
pub fn open_in_browser() -> anyhow::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join("PhotoHop-Manual.html");
    std::fs::write(&path, crate::manual::html(env!("CARGO_PKG_VERSION")))?;
    #[cfg(windows)]
    std::process::Command::new("cmd").args(["/C", "start", ""]).arg(&path).spawn()?;
    #[cfg(not(windows))]
    std::process::Command::new("xdg-open").arg(&path).spawn()?;
    Ok(path)
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.help.open {
        return;
    }
    if app.help.page.is_empty() {
        app.help.page = "start".into();
    }
    let mut open = true;
    egui::Window::new("PhotoHop Manual").open(&mut open).default_size([900.0, 680.0]).min_size([640.0, 420.0]).resizable(true).collapsible(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui_material_icons::icons::ICON_SEARCH.rich_text());
            let r = ui.add(egui::TextEdit::singleline(&mut app.help.query).hint_text("Search the manual (e.g. shadow, remove background, export video)").desired_width(420.0));
            if ui.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.command) {
                r.request_focus();
            }
            if !app.help.query.is_empty() && ui.small_button(egui_material_icons::icons::ICON_CLOSE.rich_text()).on_hover_text("Clear search").clicked() {
                app.help.query.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Open in web browser").on_hover_text("The same manual as one HTML page you can keep or print").clicked() {
                    app.status = match open_in_browser() {
                        Ok(p) => format!("Manual opened in your browser ({}).", p.display()),
                        Err(e) => format!("Could not open the browser: {e:#}"),
                    };
                }
            });
        });
        ui.separator();
        let avail = ui.available_size();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(230.0, avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt("help_nav").show(ui, |ui| {
                    ui.set_width(220.0);
                    if app.help.query.trim().is_empty() {
                        contents(app, ui);
                    } else {
                        results(app, ui);
                    }
                });
            });
            ui.separator();
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                let mut area = egui::ScrollArea::vertical().id_salt(("help_page", app.help.page.clone()));
                if app.help.scroll_top {
                    area = area.vertical_scroll_offset(0.0);
                    app.help.scroll_top = false;
                }
                area.show(ui, |ui| {
                    ui.set_width(ui.available_width().min(640.0));
                    render_page(app, ui);
                });
            });
        });
    });
    if !open {
        app.help.open = false;
    }
}

fn contents(app: &mut App, ui: &mut egui::Ui) {
    let mut group = "";
    for p in PAGES {
        if p.group != group {
            group = p.group;
            ui.add_space(6.0);
            ui.label(egui::RichText::new(group.to_uppercase()).small().weak());
        }
        if ui.selectable_label(app.help.page == p.slug, p.title()).clicked() {
            app.help.open_page(p.slug);
        }
    }
}

fn results(app: &mut App, ui: &mut egui::Ui) {
    let hits = search(&app.help.query);
    if hits.is_empty() {
        ui.label(egui::RichText::new("Nothing found. Try fewer or different words.").weak());
        return;
    }
    ui.label(egui::RichText::new(format!("{} results", hits.len())).small().weak());
    for h in hits.iter().take(60) {
        let title = match &h.section {
            Some(s) => format!("{} › {s}", h.page_title),
            None => h.page_title.to_owned(),
        };
        let r = ui.add(egui::Button::new(egui::RichText::new(title).strong()).frame(false));
        ui.label(egui::RichText::new(&h.snippet).small().weak());
        if r.clicked() {
            let target = match &h.section {
                Some(s) => format!("{}#{}", h.slug, anchor(s)),
                None => h.slug.to_owned(),
            };
            app.help.open_page(&target);
        }
        ui.add_space(4.0);
    }
}

/// Text format for one inline span.
fn format_for(ui: &egui::Ui, span: &Span, size: f32) -> egui::TextFormat {
    let base = egui::TextFormat { font_id: egui::FontId::proportional(size), color: ui.visuals().text_color(), ..Default::default() };
    match span {
        Span::Text(_) => base,
        Span::Bold(_) => egui::TextFormat { color: ui.visuals().strong_text_color(), ..base },
        Span::Code(_) => egui::TextFormat { font_id: egui::FontId::monospace(size - 1.0), background: ui.visuals().code_bg_color, ..base },
        Span::Link(..) => egui::TextFormat { color: super::widgets::ACCENT, underline: egui::Stroke::new(1.0, super::widgets::ACCENT), ..base },
    }
}

/// Append `t` to `job`, highlighting search words.
fn append(job: &mut egui::text::LayoutJob, t: &str, fmt: &egui::TextFormat, query: &[String]) {
    let low = t.to_lowercase();
    let mut i = 0;
    while i < t.len() {
        let next = query.iter().filter_map(|w| low.get(i..).and_then(|r| r.find(w.as_str())).map(|p| (i + p, w.len()))).min();
        match next {
            Some((p, n)) if t.is_char_boundary(p) && t.is_char_boundary(p + n) => {
                job.append(&t[i..p], 0.0, fmt.clone());
                let mut hl = fmt.clone();
                hl.background = egui::Color32::from_rgb(120, 90, 0);
                job.append(&t[p..p + n], 0.0, hl);
                i = p + n;
            }
            _ => {
                job.append(&t[i..], 0.0, fmt.clone());
                i = t.len();
            }
        }
    }
}

fn span_text(s: &Span) -> &str {
    match s {
        Span::Text(t) | Span::Bold(t) | Span::Code(t) | Span::Link(t, _) => t,
    }
}

/// Paragraph text with bold/code styling, search words highlighted and clickable links.
fn paragraph(app: &mut App, ui: &mut egui::Ui, text: &str, query: &[String], size: f32) {
    let sp = spans(text);
    if !sp.iter().any(|s| matches!(s, Span::Link(..))) {
        let mut job = egui::text::LayoutJob::default();
        for s in &sp {
            append(&mut job, span_text(s), &format_for(ui, s, size), query);
        }
        job.wrap.max_width = ui.available_width();
        ui.label(job);
        return;
    }
    // With links: lay the words out one by one so a link can be clicked where it is.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
        for s in &sp {
            let fmt = format_for(ui, s, size);
            for word in span_text(s).split_inclusive(' ') {
                let mut job = egui::text::LayoutJob::default();
                append(&mut job, word, &fmt, query);
                if let Span::Link(_, target) = s {
                    let r = ui.add(egui::Label::new(job).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        if target.starts_with("http") {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(target));
                        } else {
                            app.help.open_page(target);
                        }
                    }
                } else {
                    ui.label(job);
                }
            }
        }
    });
}

fn render_page(app: &mut App, ui: &mut egui::Ui) {
    let Some(p) = page(&app.help.page) else { return };
    let query: Vec<String> = app.help.query.split_whitespace().map(str::to_lowercase).collect();
    let target = app.help.scroll_to.take();
    for b in p.blocks() {
        match b {
            Block::Heading(n, h) => {
                let size = match n {
                    1 => 26.0,
                    2 => 20.0,
                    _ => 16.5,
                };
                if n == 2 {
                    ui.add_space(8.0);
                    ui.separator();
                }
                let r = ui.label(egui::RichText::new(&h).size(size).strong());
                if target.as_deref() == Some(anchor(&h).as_str()) {
                    r.scroll_to_me(Some(egui::Align::TOP));
                }
                ui.add_space(2.0);
            }
            Block::Para(t) => {
                paragraph(app, ui, &t, &query, 15.0);
                ui.add_space(6.0);
            }
            Block::Tip(t) => {
                egui::Frame::new().fill(egui::Color32::from_rgb(52, 44, 22)).inner_margin(8.0).corner_radius(6.0).show(ui, |ui| {
                    paragraph(app, ui, &t, &query, 15.0);
                });
                ui.add_space(6.0);
            }
            Block::Bullets(items) => list(app, ui, &items, false, &query),
            Block::Numbered(items) => list(app, ui, &items, true, &query),
            Block::Table(rows) => {
                egui::Grid::new(("help_table", rows.len(), rows.first().map(|r| r.join("")))).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                    for (k, r) in rows.iter().enumerate() {
                        for c in r {
                            if k == 0 {
                                ui.label(egui::RichText::new(crate::manual::plain(c)).strong());
                            } else {
                                ui.vertical(|ui| {
                                    ui.set_max_width(280.0);
                                    paragraph(app, ui, c, &query, 14.0);
                                });
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
            }
        }
    }
}

fn list(app: &mut App, ui: &mut egui::Ui, items: &[String], numbered: bool, query: &[String]) {
    for (k, item) in items.iter().enumerate() {
        ui.horizontal_top(|ui| {
            ui.label(egui::RichText::new(if numbered { format!("{}.", k + 1) } else { "•".to_owned() }).strong());
            ui.vertical(|ui| paragraph(app, ui, item, query, 15.0));
        });
    }
    ui.add_space(6.0);
}
