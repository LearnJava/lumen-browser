//! Debug click log — writes click events with hit-test details to `click_debug.log`
//! in the working directory. Enabled via `--click-log` flag or `LUMEN_CLICK_LOG=1`.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Call once at startup with the value of `--click-log` / `LUMEN_CLICK_LOG`.
pub fn init(enabled: bool) {
    let _ = ENABLED.set(enabled);
}

pub fn is_enabled() -> bool {
    *ENABLED.get().unwrap_or(&false)
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let ms = now.subsec_millis();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

fn append(line: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("click_debug.log")
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Всё, что нужно знать о клике — передаётся одной структурой.
pub struct ClickInfo<'a> {
    /// Координаты в window-px (до вычета таб-бара).
    pub win_x: f32,
    pub win_y: f32,
    /// Координаты в page-px (layout-пространство).
    pub page_x: f32,
    pub page_y: f32,
    pub scroll_y: f32,
    /// Результат hit-test: тег, id, class первого подходящего элемента.
    pub hit: Option<HitInfo<'a>>,
    /// Что произошло после hit-test.
    pub outcome: ClickOutcome<'a>,
}

pub struct HitInfo<'a> {
    pub node_id: u32,
    pub tag: &'a str,
    pub id_attr: &'a str,
    pub class_attr: &'a str,
}

pub enum ClickOutcome<'a> {
    NoHit,
    FormAction(&'a str),
    LinkFragment(&'a str),
    LinkNavigate { href: &'a str, resolved: &'a str },
    LinkBlocked(&'a str),
    NoLink,
}

pub fn log(info: &ClickInfo<'_>) {
    if !is_enabled() {
        return;
    }

    let ts = timestamp();
    append(&format!(
        "[{ts}] CLICK  win=({:.1}, {:.1})  page=({:.1}, {:.1})  scroll_y={:.1}",
        info.win_x, info.win_y, info.page_x, info.page_y, info.scroll_y,
    ));

    match &info.hit {
        None => {
            append("         hit:  NONE — no element at these page coordinates");
        }
        Some(h) => {
            append(&format!(
                "         hit:  node={}  <{}>  id=\"{}\"  class=\"{}\"",
                h.node_id, h.tag, h.id_attr, h.class_attr,
            ));
        }
    }

    match &info.outcome {
        ClickOutcome::NoHit => {
            append("         outcome: no hit → nothing");
        }
        ClickOutcome::FormAction(kind) => {
            append(&format!("         outcome: form action ({kind})"));
        }
        ClickOutcome::LinkFragment(frag) => {
            append(&format!("         outcome: fragment nav → #{frag}"));
        }
        ClickOutcome::LinkNavigate { href, resolved } => {
            append(&format!("         outcome: navigate  href=\"{href}\"  resolved=\"{resolved}\""));
        }
        ClickOutcome::LinkBlocked(href) => {
            append(&format!("         outcome: link BLOCKED (javascript:/mailto:)  href=\"{href}\""));
        }
        ClickOutcome::NoLink => {
            append("         outcome: hit element has no <a href> ancestor → nothing");
        }
    }

    append(""); // пустая строка-разделитель
}
