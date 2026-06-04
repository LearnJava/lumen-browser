//! Activity log — единый журнал активности браузера в `activity.log`.
//!
//! Активируется флагом `--activity-log` (или `--click-log`) либо
//! переменной окружения `LUMEN_ACTIVITY_LOG=1`.
//!
//! Формат строк:
//! ```text
//! [12:34:56.789] CLICK      win=(92, 129) → #text in <a> "ссылка"
//! [12:34:56.789]   outcome: navigate href="page2.html" → resolved="file:///..."
//! [12:34:56.790] NAV        → "file:///...page2.html"
//! [12:34:56.791] LOAD_START "file:///...page2.html"
//! [12:34:56.845] LOAD_OK    "file:///...page2.html"  (54 ms)  title="Page 2"
//! [12:34:56.847] PAGE_READY scroll_y=0  layout_boxes=312
//! ```

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static ENABLED: OnceLock<bool> = OnceLock::new();
/// Unix-ms момента последнего LOAD_START — для вычисления длительности загрузки.
static LOAD_START_MS: AtomicU64 = AtomicU64::new(0);

/// Вызвать один раз при старте с результатом разбора флага --activity-log.
pub fn init(enabled: bool) {
    let _ = ENABLED.set(enabled);
    if enabled {
        // Создаём/очищаем файл при старте, чтобы каждый запуск начинался чисто.
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open("activity.log")
        {
            let _ = writeln!(f, "=== Lumen activity log — {} ===", wall_date());
            let _ = writeln!(f);
        }
    }
}

pub fn is_enabled() -> bool {
    *ENABLED.get().unwrap_or(&false)
}

// ── Внутренние утилиты ───────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn timestamp() -> String {
    let ms = now_ms();
    let secs = ms / 1000;
    let millis = ms % 1000;
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

fn wall_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    // Простой счётчик дней от эпохи — достаточно для заголовка лога.
    format!("day {days} of unix epoch  ({secs} s)")
}

fn append(line: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("activity.log")
    {
        let _ = writeln!(f, "{line}");
    }
}

fn entry(tag: &str, body: &str) {
    append(&format!("[{}] {:<10} {}", timestamp(), tag, body));
}

fn detail(body: &str) {
    append(&format!("               {body}"));
}

// ── Публичный API событий ────────────────────────────────────────────────────

/// Клик мышью: window-координаты и что под курсором.
pub struct ClickInfo<'a> {
    pub win_x: f32,
    pub win_y: f32,
    pub page_x: f32,
    pub page_y: f32,
    pub scroll_y: f32,
    pub hit: Option<HitInfo<'a>>,
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

pub fn log_click(info: &ClickInfo<'_>) {
    if !is_enabled() { return; }

    let hit_str = match &info.hit {
        None => "— no element".to_owned(),
        Some(h) => {
            let class = if h.class_attr.is_empty() { String::new() } else { format!(" .{}", h.class_attr) };
            let id    = if h.id_attr.is_empty()    { String::new() } else { format!(" #{}", h.id_attr) };
            format!("node={} <{}>{}{}  scroll_y={:.0}", h.node_id, h.tag, id, class, info.scroll_y)
        }
    };
    entry("CLICK", &format!(
        "win=({:.0}, {:.0})  page=({:.0}, {:.0})  {}",
        info.win_x, info.win_y, info.page_x, info.page_y, hit_str,
    ));

    let outcome_str = match &info.outcome {
        ClickOutcome::NoHit           => "no element under cursor".to_owned(),
        ClickOutcome::FormAction(k)   => format!("form action: {k}"),
        ClickOutcome::LinkFragment(f) => format!("fragment → #{f}"),
        ClickOutcome::LinkNavigate { href, resolved } =>
            format!("navigate  href=\"{href}\"  resolved=\"{resolved}\""),
        ClickOutcome::LinkBlocked(h)  => format!("blocked (javascript:/mailto:)  href=\"{h}\""),
        ClickOutcome::NoLink          => "no <a href> ancestor → nothing".to_owned(),
    };
    detail(&format!("→ {outcome_str}"));
}

/// Навигация на новый URL запущена (navigate_to вызван).
pub fn log_nav(url: &str) {
    if !is_enabled() { return; }
    entry("NAV", &format!("→ \"{url}\""));
}

/// Фоновый поток загрузки страницы стартовал.
pub fn log_load_start(url: &str) {
    if !is_enabled() { return; }
    LOAD_START_MS.store(now_ms(), Ordering::Relaxed);
    entry("LOAD_START", &format!("\"{url}\""));
}

/// Страница загружена и отрисована.
pub fn log_load_ok(url: &str, title: &str) {
    if !is_enabled() { return; }
    let start = LOAD_START_MS.load(Ordering::Relaxed);
    let elapsed = if start > 0 { now_ms().saturating_sub(start) } else { 0 };
    entry("LOAD_OK", &format!("\"{url}\"  ({elapsed} ms)  title=\"{title}\""));
}

/// Ошибка загрузки.
pub fn log_load_err(url: &str, err: &str) {
    if !is_enabled() { return; }
    let start = LOAD_START_MS.load(Ordering::Relaxed);
    let elapsed = if start > 0 { now_ms().saturating_sub(start) } else { 0 };
    entry("LOAD_ERR", &format!("\"{url}\"  ({elapsed} ms)  — {err}"));
}

/// Скроллинг к фрагменту (#id) без перезагрузки страницы.
pub fn log_fragment(fragment: &str, found: bool) {
    if !is_enabled() { return; }
    let status = if found { "found" } else { "NOT FOUND" };
    entry("FRAGMENT", &format!("#{fragment}  ({status})"));
}

/// Навигация из JS (location.href=, history.pushState, window.open …).
pub fn log_js_nav(kind: &str, url: &str) {
    if !is_enabled() { return; }
    entry("JS_NAV", &format!("[{kind}]  \"{url}\""));
}

/// Страница полностью применена (apply_loaded_page завершён).
pub fn log_page_ready(url: &str, scroll_y: f32) {
    if !is_enabled() { return; }
    entry("PAGE_READY", &format!("\"{url}\"  scroll_y={scroll_y:.0}"));
    append(""); // пустая строка-разделитель после каждого полного цикла загрузки
}
