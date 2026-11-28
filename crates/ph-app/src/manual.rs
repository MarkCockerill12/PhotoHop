//! The PhotoHop manual: Markdown pages in `docs/manual/`, compiled into the app (Help > Manual,
//! F1) and exported as one self-contained HTML file (`photohop --manual out.html`, shipped in
//! every release). Only a small Markdown subset is used: headings, paragraphs, `-` and `1.`
//! lists, `> ` tips, tables, **bold**, `code` and [links](page-slug#section).

pub struct Page {
    pub slug: &'static str,
    pub group: &'static str,
    pub source: &'static str,
}

macro_rules! page {
    ($slug:literal, $group:literal, $file:literal) => {
        Page { slug: $slug, group: $group, source: include_str!(concat!("../../../docs/manual/", $file)) }
    };
}

pub const PAGES: &[Page] = &[
    page!("start", "Getting started", "01-start.md"),
    page!("tour", "Getting started", "02-tour.md"),
    page!("first-composite", "Getting started", "03-first-composite.md"),
    page!("layers", "Basics", "10-layers.md"),
    page!("position", "Basics", "11-position.md"),
    page!("tools", "Basics", "12-tools.md"),
    page!("cutout", "Making it real", "20-cutout.md"),
    page!("fit", "Making it real", "21-fit.md"),
    page!("several-objects", "Making it real", "22-several-objects.md"),
    page!("colour", "Making it real", "23-colour.md"),
    page!("shadow", "Making it real", "24-shadow.md"),
    page!("camera", "Making it real", "25-camera.md"),
    page!("game", "Making it real", "26-game.md"),
    page!("video", "Video", "30-video.md"),
    page!("timing", "Video", "31-timing.md"),
    page!("window", "Video", "32-window.md"),
    page!("ai", "AI tools", "40-ai.md"),
    page!("blender", "AI tools", "41-blender.md"),
    page!("export", "Saving & exporting", "50-export.md"),
    page!("scopes", "Reference", "60-scopes.md"),
    page!("shortcuts", "Reference", "61-shortcuts.md"),
    page!("menus", "Reference", "62-menus.md"),
    page!("troubleshooting", "Reference", "63-troubleshooting.md"),
    page!("glossary", "Reference", "64-glossary.md"),
];

pub fn page(slug: &str) -> Option<&'static Page> {
    PAGES.iter().find(|p| p.slug == slug)
}

impl Page {
    pub fn title(&self) -> &'static str {
        self.source.lines().next().and_then(|l| l.strip_prefix("# ")).unwrap_or(self.slug)
    }

    pub fn blocks(&self) -> Vec<Block> {
        parse(self.source)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    Bullets(Vec<String>),
    Numbered(Vec<String>),
    Tip(String),
    Table(Vec<Vec<String>>),
}

pub fn parse(src: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let mut list: Option<(bool, Vec<String>)> = None;
    let mut tip: Vec<&str> = Vec::new();
    let mut table: Vec<Vec<String>> = Vec::new();
    fn flush(out: &mut Vec<Block>, para: &mut Vec<&str>, list: &mut Option<(bool, Vec<String>)>, tip: &mut Vec<&str>, table: &mut Vec<Vec<String>>) {
        if !para.is_empty() {
            out.push(Block::Para(para.join(" ")));
            para.clear();
        }
        if let Some((numbered, items)) = list.take() {
            out.push(if numbered { Block::Numbered(items) } else { Block::Bullets(items) });
        }
        if !tip.is_empty() {
            out.push(Block::Tip(tip.join(" ")));
            tip.clear();
        }
        if !table.is_empty() {
            out.push(Block::Table(std::mem::take(table)));
        }
    }
    for line in src.lines() {
        let l = line.trim_end();
        let trimmed = l.trim_start();
        if trimmed.is_empty() {
            flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("### ") {
            flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            out.push(Block::Heading(3, h.to_owned()));
        } else if let Some(h) = trimmed.strip_prefix("## ") {
            flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            out.push(Block::Heading(2, h.to_owned()));
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            out.push(Block::Heading(1, h.to_owned()));
        } else if let Some(t) = trimmed.strip_prefix("> ") {
            if tip.is_empty() {
                flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            }
            tip.push(t);
        } else if trimmed.starts_with('|') {
            if table.is_empty() {
                flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            }
            let cells: Vec<String> = trimmed.trim_matches('|').split('|').map(|c| c.trim().to_owned()).collect();
            if !cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':')) {
                table.push(cells);
            }
        } else if let Some(item) = trimmed.strip_prefix("- ") {
            if !matches!(list, Some((false, _))) {
                flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
                list = Some((false, Vec::new()));
            }
            list.as_mut().expect("list").1.push(item.to_owned());
        } else if let Some(item) = numbered_item(trimmed) {
            if !matches!(list, Some((true, _))) {
                flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
                list = Some((true, Vec::new()));
            }
            list.as_mut().expect("list").1.push(item.to_owned());
        } else if let Some((_, items)) = &mut list
            && line.starts_with("  ")
        {
            // Continuation of the last list item.
            if let Some(last) = items.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        } else if !tip.is_empty() {
            tip.push(trimmed);
        } else {
            if list.is_some() {
                flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
            }
            para.push(trimmed);
        }
    }
    flush(&mut out, &mut para, &mut list, &mut tip, &mut table);
    out
}

fn numbered_item(s: &str) -> Option<&str> {
    let (num, rest) = s.split_once(". ")?;
    (!num.is_empty() && num.len() <= 2 && num.chars().all(|c| c.is_ascii_digit())).then_some(rest)
}

/// Inline pieces of a paragraph.
#[derive(Clone, Debug, PartialEq)]
pub enum Span {
    Text(String),
    Bold(String),
    Code(String),
    /// (text, target): target is `page-slug` or `page-slug#section-anchor`.
    Link(String, String),
}

pub fn spans(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("**")
            && let Some(end) = r.find("**")
        {
            if !buf.is_empty() {
                out.push(Span::Text(std::mem::take(&mut buf)));
            }
            out.push(Span::Bold(r[..end].to_owned()));
            rest = &r[end + 2..];
            continue;
        }
        if let Some(r) = rest.strip_prefix('`')
            && let Some(end) = r.find('`')
        {
            if !buf.is_empty() {
                out.push(Span::Text(std::mem::take(&mut buf)));
            }
            out.push(Span::Code(r[..end].to_owned()));
            rest = &r[end + 1..];
            continue;
        }
        if let Some(r) = rest.strip_prefix('[')
            && let Some(mid) = r.find("](")
            && let Some(end) = r[mid + 2..].find(')')
        {
            if !buf.is_empty() {
                out.push(Span::Text(std::mem::take(&mut buf)));
            }
            out.push(Span::Link(r[..mid].to_owned(), r[mid + 2..mid + 2 + end].to_owned()));
            rest = &r[mid + 2 + end + 1..];
            continue;
        }
        buf.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if !buf.is_empty() {
        out.push(Span::Text(buf));
    }
    out
}

/// Text of a block without Markdown marks (for search).
pub fn plain(text: &str) -> String {
    spans(text)
        .into_iter()
        .map(|s| match s {
            Span::Text(t) | Span::Bold(t) | Span::Code(t) | Span::Link(t, _) => t,
        })
        .collect()
}

pub fn anchor(heading: &str) -> String {
    let mut a = String::new();
    for c in heading.chars() {
        if c.is_alphanumeric() {
            a.extend(c.to_lowercase());
        } else if !a.ends_with('-') && !a.is_empty() {
            a.push('-');
        }
    }
    a.trim_end_matches('-').to_owned()
}

/// A search hit: page, section heading (if any) and a snippet around the first match.
pub struct Hit {
    pub slug: &'static str,
    pub page_title: &'static str,
    pub section: Option<String>,
    pub snippet: String,
    pub score: usize,
}

/// All sections whose text contains every word of `query` (case-insensitive), best first.
pub fn search(query: &str) -> Vec<Hit> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for p in PAGES {
        let mut section: Option<String> = None;
        let mut text = String::new();
        let push = |section: &Option<String>, text: &str, hits: &mut Vec<Hit>| {
            let low = text.to_lowercase();
            if text.is_empty() || !words.iter().all(|w| low.contains(w.as_str())) {
                return;
            }
            let title_low = format!("{} {}", p.title(), section.as_deref().unwrap_or("")).to_lowercase();
            let score = words.iter().map(|w| low.matches(w.as_str()).count() + 10 * usize::from(title_low.contains(w.as_str()))).sum();
            let at = low.find(words[0].as_str()).unwrap_or(0);
            let start = text[..at].char_indices().rev().nth(60).map_or(0, |(i, _)| i);
            let snippet: String = text[start..].chars().take(160).collect();
            hits.push(Hit { slug: p.slug, page_title: p.title(), section: section.clone(), snippet: format!("{}{}", if start > 0 { "…" } else { "" }, snippet.trim()), score });
        };
        for b in p.blocks() {
            match b {
                Block::Heading(1, _) => {}
                Block::Heading(_, h) => {
                    push(&section, &text, &mut hits);
                    text = plain(&h);
                    text.push_str(". ");
                    section = Some(h);
                }
                Block::Para(t) | Block::Tip(t) => {
                    text.push_str(&plain(&t));
                    text.push(' ');
                }
                Block::Bullets(items) | Block::Numbered(items) => {
                    for i in items {
                        text.push_str(&plain(&i));
                        text.push(' ');
                    }
                }
                Block::Table(rows) => {
                    for r in rows {
                        text.push_str(&r.iter().map(|c| plain(c)).collect::<Vec<_>>().join(" "));
                        text.push(' ');
                    }
                }
            }
        }
        push(&section, &text, &mut hits);
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.score));
    hits
}

// ---------- HTML export ----------

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn inline_html(text: &str) -> String {
    spans(text)
        .into_iter()
        .map(|s| match s {
            Span::Text(t) => esc(&t),
            Span::Bold(t) => format!("<strong>{}</strong>", esc(&t)),
            Span::Code(t) => format!("<code>{}</code>", esc(&t)),
            Span::Link(t, target) => {
                if target.starts_with("http") {
                    return format!("<a href=\"{}\">{}</a>", esc(&target), esc(&t));
                }
                let (slug, sec) = target.split_once('#').unwrap_or((&target, ""));
                let href = if sec.is_empty() { format!("#{slug}") } else { format!("#{slug}--{sec}") };
                format!("<a href=\"{href}\">{}</a>", esc(&t))
            }
        })
        .collect()
}

/// The whole manual as one self-contained HTML page with a sidebar and search.
pub fn html(version: &str) -> String {
    let mut nav = String::new();
    let mut body = String::new();
    let mut group = "";
    for p in PAGES {
        if p.group != group {
            if !group.is_empty() {
                nav.push_str("</ul>");
            }
            group = p.group;
            nav.push_str(&format!("<h4>{}</h4><ul>", esc(group)));
        }
        nav.push_str(&format!("<li><a href=\"#{}\">{}</a></li>", p.slug, esc(p.title())));
        body.push_str(&format!("<section class=\"page\" id=\"{}\">", p.slug));
        for b in p.blocks() {
            match b {
                Block::Heading(1, h) => body.push_str(&format!("<h1>{}</h1><div class=\"sec\">", esc(&h))),
                Block::Heading(n, h) => {
                    body.push_str("</div>");
                    let id = format!("{}--{}", p.slug, anchor(&h));
                    body.push_str(&format!("<div class=\"sec\"><h{n} id=\"{id}\">{}</h{n}>", esc(&h)));
                }
                Block::Para(t) => body.push_str(&format!("<p>{}</p>", inline_html(&t))),
                Block::Tip(t) => body.push_str(&format!("<div class=\"tip\">{}</div>", inline_html(&t))),
                Block::Bullets(items) => {
                    body.push_str("<ul>");
                    for i in items {
                        body.push_str(&format!("<li>{}</li>", inline_html(&i)));
                    }
                    body.push_str("</ul>");
                }
                Block::Numbered(items) => {
                    body.push_str("<ol>");
                    for i in items {
                        body.push_str(&format!("<li>{}</li>", inline_html(&i)));
                    }
                    body.push_str("</ol>");
                }
                Block::Table(rows) => {
                    body.push_str("<table>");
                    for (k, r) in rows.iter().enumerate() {
                        let tag = if k == 0 { "th" } else { "td" };
                        body.push_str("<tr>");
                        for c in r {
                            body.push_str(&format!("<{tag}>{}</{tag}>", inline_html(c)));
                        }
                        body.push_str("</tr>");
                    }
                    body.push_str("</table>");
                }
            }
        }
        body.push_str("</div></section>");
    }
    nav.push_str("</ul>");
    HTML_TEMPLATE.replace("{{VERSION}}", &esc(version)).replace("{{NAV}}", &nav).replace("{{BODY}}", &body)
}

const HTML_TEMPLATE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>PhotoHop Manual</title>
<style>
:root{--bg:#f7f6f3;--panel:#fff;--ink:#1d1d1f;--muted:#666;--line:#e2e0da;--accent:#c27a00;--tip:#fff6e0;--mark:#ffe08a}
@media (prefers-color-scheme:dark){:root{--bg:#18181a;--panel:#212124;--ink:#ececec;--muted:#a0a0a0;--line:#333;--accent:#ffc440;--tip:#2e2716;--mark:#7a5a00}}
*{box-sizing:border-box}body{margin:0;font:16px/1.6 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;background:var(--bg);color:var(--ink)}
aside{position:fixed;top:0;left:0;bottom:0;width:280px;overflow-y:auto;background:var(--panel);border-right:1px solid var(--line);padding:18px}
aside .brand{font-weight:700;font-size:20px}aside .ver{color:var(--muted);font-size:13px;margin-bottom:12px}
#q{width:100%;padding:9px 11px;border-radius:8px;border:1px solid var(--line);background:var(--bg);color:var(--ink);font-size:15px}
aside h4{margin:18px 0 4px;font-size:12px;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
aside ul{list-style:none;margin:0;padding:0}aside li a{display:block;padding:3px 8px;border-radius:6px;color:var(--ink);text-decoration:none}
aside li a:hover{background:var(--bg)}
main{margin-left:280px;padding:28px 48px 120px;max-width:980px}
.page{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:8px 32px 20px;margin-bottom:28px}
h1{font-size:30px;margin:18px 0 6px}h2{font-size:22px;margin:26px 0 6px;padding-top:6px;border-top:1px solid var(--line)}h3{font-size:18px;margin:20px 0 4px}
a{color:var(--accent)}code{background:var(--bg);border:1px solid var(--line);border-radius:4px;padding:0 4px;font-size:.92em}
.tip{background:var(--tip);border-left:4px solid var(--accent);padding:10px 14px;border-radius:6px;margin:12px 0}
table{border-collapse:collapse;margin:10px 0;width:100%}th,td{border:1px solid var(--line);padding:6px 10px;text-align:left;vertical-align:top}th{background:var(--bg)}
mark{background:var(--mark);color:inherit;border-radius:3px}
#none{display:none;color:var(--muted);padding:20px}
.hidden{display:none}
@media (max-width:820px){aside{position:static;width:auto}main{margin:0;padding:16px}}
</style></head><body>
<aside><div class="brand">PhotoHop Manual</div><div class="ver">for PhotoHop {{VERSION}}</div>
<input id="q" type="search" placeholder="Search the manual…" autocomplete="off">
{{NAV}}</aside>
<main>{{BODY}}<div id="none">Nothing found. Try fewer or different words.</div></main>
<script>
const q=document.getElementById('q');const secs=[...document.querySelectorAll('.sec')];const pages=[...document.querySelectorAll('.page')];
secs.forEach(s=>s.dataset.html=s.innerHTML);
function run(){const words=q.value.toLowerCase().split(/\s+/).filter(Boolean);let any=false;
 secs.forEach(s=>{s.innerHTML=s.dataset.html;const t=s.textContent.toLowerCase();const ok=!words.length||words.every(w=>t.includes(w));s.classList.toggle('hidden',!ok);if(ok&&words.length){any=true;mark(s,words)}});
 pages.forEach(p=>{const vis=[...p.querySelectorAll('.sec')].some(s=>!s.classList.contains('hidden'));p.classList.toggle('hidden',words.length>0&&!vis)});
 document.getElementById('none').style.display=(words.length&&!any)?'block':'none';}
function mark(el,words){const re=new RegExp('('+words.map(w=>w.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')).join('|')+')','gi');
 const walk=n=>{if(n.nodeType===3){if(re.test(n.data)){const span=document.createElement('span');span.innerHTML=n.data.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(re,'<mark>$1</mark>');n.replaceWith(span)}re.lastIndex=0}else if(n.nodeType===1&&!/^(script|style|mark)$/i.test(n.tagName)){[...n.childNodes].forEach(walk)}};walk(el)}
q.addEventListener('input',run);
document.addEventListener('keydown',e=>{if(e.key==='/'&&document.activeElement!==q){e.preventDefault();q.focus()}});
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_parses_and_links_resolve() {
        for p in PAGES {
            assert!(p.source.starts_with("# "), "{} has no title", p.slug);
            for b in p.blocks() {
                let texts: Vec<String> = match b {
                    Block::Para(t) | Block::Tip(t) => vec![t],
                    Block::Bullets(v) | Block::Numbered(v) => v,
                    Block::Table(rows) => rows.into_iter().flatten().collect(),
                    Block::Heading(..) => vec![],
                };
                for t in texts {
                    for s in spans(&t) {
                        if let Span::Link(_, target) = s {
                            let slug = target.split('#').next().unwrap_or_default();
                            if target.starts_with("http") {
                                continue;
                            }
                            assert!(page(slug).is_some(), "{}: broken link to {target}", p.slug);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn search_finds_split() {
        let hits = search("split objects");
        assert!(hits.iter().any(|h| h.slug == "several-objects"));
    }

    #[test]
    fn inline_marks() {
        assert_eq!(spans("a **b** `c` [d](fit)"), vec![
            Span::Text("a ".into()),
            Span::Bold("b".into()),
            Span::Text(" ".into()),
            Span::Code("c".into()),
            Span::Text(" ".into()),
            Span::Link("d".into(), "fit".into())
        ]);
    }

    /// `PHOTOHOP_WRITE_MANUAL=path cargo test -p photohop write_manual` regenerates the HTML.
    #[test]
    fn write_manual() {
        if let Some(p) = std::env::var_os("PHOTOHOP_WRITE_MANUAL") {
            std::fs::write(p, html(env!("CARGO_PKG_VERSION"))).unwrap();
        }
    }
}
