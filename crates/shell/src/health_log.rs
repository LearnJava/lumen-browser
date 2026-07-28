//! Session health journal — privacy-first, local-only log of *problems* the
//! browser hit while you were using it, written as JSON Lines to `health.log`.
//!
//! PERF-6: extends the `--activity-log` surface (see [`crate::click_log`]) with a
//! machine-parseable record of the three things that matter for prioritising bug
//! fixes by how often they actually bite real browsing:
//!
//! * `panic` — a Rust panic anywhere in the process, with backtrace and the page
//!   that was open when it fired.
//! * `console_error` — a page's own `console.error(...)` call (site is buggy).
//! * `load_error` — a navigation that failed to load at all.
//! * `broken_render` — a page that loaded but painted nothing despite a
//!   content-bearing DOM (white-screen / broken-render heuristic).
//!
//! Everything stays on the machine — nothing is uploaded (privacy.md principle).
//! The companion analyser `scripts/health_report.py` aggregates `health.log` by
//! host and ranks the repeat offenders, so P3 bug-fix effort follows real-world
//! frequency instead of random discovery.
//!
//! Activation: `--activity-log` / `--click-log` (shared with the click log), the
//! dedicated `--health-log` flag, or `LUMEN_HEALTH_LOG=1`.
//!
//! Each line is one self-contained JSON object, e.g.:
//! ```text
//! {"kind":"console_error","time":"12:34:56.789","ts_ms":1752800096789,"url":"https://example.com/","detail":"TypeError: x is undefined"}
//! ```

use lumen_core::json::JsonValue;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// File the journal is appended to, relative to the working directory (parity
/// with `activity.log` from [`crate::click_log`]).
const HEALTH_LOG_PATH: &str = "health.log";

static ENABLED: OnceLock<bool> = OnceLock::new();
/// Panic hook is installed at most once per process.
static HOOK_INSTALLED: OnceLock<()> = OnceLock::new();

/// URL of the page currently open, so panics on any thread can be attributed to
/// the page that triggered them. Updated on every navigation.
fn current_url() -> &'static Mutex<String> {
    static URL: OnceLock<Mutex<String>> = OnceLock::new();
    URL.get_or_init(|| Mutex::new(String::from("(startup)")))
}

/// A page with at least this many DOM nodes is considered to have real content,
/// so a page that paints *nothing* is suspicious rather than genuinely blank.
const BROKEN_RENDER_DOM_MIN: usize = 20;

/// Call once at startup with the parsed enable flag. Truncates `health.log` so
/// each session starts clean and installs the panic hook when enabled.
pub fn init(enabled: bool) {
    let _ = ENABLED.set(enabled);
    if !enabled {
        return;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(HEALTH_LOG_PATH)
    {
        let mut obj = base("session_start", "");
        obj.remove("url");
        let _ = writeln!(f, "{}", JsonValue::Object(obj));
    }
    install_panic_hook();
}

/// Whether the health journal is active.
pub fn is_enabled() -> bool {
    *ENABLED.get().unwrap_or(&false)
}

/// Remember the page currently open, so a later panic can name it. No-op when
/// disabled. Cheap enough to call on every navigation.
pub fn set_current_url(url: &str) {
    if !is_enabled() {
        return;
    }
    if let Ok(mut g) = current_url().lock() {
        g.clear();
        g.push_str(url);
    }
}

// ── Public event API ─────────────────────────────────────────────────────────

/// A page's own `console.error(...)` — a defect on the site (or in our JS shim).
pub fn log_console_error(url: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    let mut obj = base("console_error", url);
    obj.insert("detail".into(), JsonValue::String(truncate(message)));
    append(obj);
}

/// A navigation that failed to load (network, TLS, decode …).
pub fn log_load_error(url: &str, error: &str) {
    if !is_enabled() {
        return;
    }
    let mut obj = base("load_error", url);
    obj.insert("detail".into(), JsonValue::String(truncate(error)));
    append(obj);
}

/// Record a suspected white-screen / broken render. `dom_nodes` is the DOM arena
/// size, `layout_boxes` the number of laid-out boxes, and `rendered_units` the
/// count of things that actually paint (visible text runs + replaced elements
/// like `<img>`/`<canvas>`/`<video>`). The journal only writes a record when the
/// heuristic fires: a content-bearing DOM that paints nothing at all.
///
/// The heuristic deliberately keeps false positives low by requiring
/// `rendered_units == 0` (not merely "few") — a genuine white screen. It can
/// still miss-fire on a page whose only visible content is a CSS `background`
/// image with no text/replaced boxes; that is an accepted, documented limit.
pub fn log_render_health(url: &str, dom_nodes: usize, layout_boxes: usize, rendered_units: usize) {
    if !is_enabled() {
        return;
    }
    if dom_nodes < BROKEN_RENDER_DOM_MIN || rendered_units > 0 {
        return; // healthy enough — nothing to record
    }
    let mut obj = base("broken_render", url);
    obj.insert(
        "detail".into(),
        JsonValue::String(format!(
            "{dom_nodes} DOM nodes but nothing painted \
             ({layout_boxes} layout boxes, 0 rendered units) — suspected white screen"
        )),
    );
    obj.insert("dom_nodes".into(), JsonValue::Number(dom_nodes as f64));
    obj.insert("layout_boxes".into(), JsonValue::Number(layout_boxes as f64));
    obj.insert("rendered_units".into(), JsonValue::Number(rendered_units as f64));
    append(obj);
}

// ── Internals ────────────────────────────────────────────────────────────────

/// Install a panic hook that appends a `panic` record (message + location +
/// backtrace + open page) and then chains to the previous hook so the default
/// stderr output is preserved. Idempotent.
fn install_panic_hook() {
    if HOOK_INSTALLED.set(()).is_err() {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort extraction of the panic message payload.
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<dyn Any>".to_owned()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_owned());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let url = current_url()
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|e| e.into_inner().clone());

        let mut obj = base("panic", &url);
        obj.insert("detail".into(), JsonValue::String(truncate(&msg)));
        obj.insert("location".into(), JsonValue::String(location));
        obj.insert("backtrace".into(), JsonValue::String(truncate_backtrace(&backtrace)));
        append(obj);

        prev(info);
    }));
}

/// Build the common fields shared by every record.
fn base(kind: &str, url: &str) -> BTreeMap<String, JsonValue> {
    let mut m = BTreeMap::new();
    m.insert("kind".into(), JsonValue::String(kind.to_owned()));
    m.insert("time".into(), JsonValue::String(timestamp()));
    m.insert("ts_ms".into(), JsonValue::Number(now_ms() as f64));
    m.insert("url".into(), JsonValue::String(url.to_owned()));
    m
}

/// Serialise one record as a JSON line and append it to the journal.
fn append(obj: BTreeMap<String, JsonValue>) {
    let line = JsonValue::Object(obj).to_string();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(HEALTH_LOG_PATH)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Clamp a free-form string so one pathological message cannot bloat the log.
fn truncate(s: &str) -> String {
    const MAX: usize = 2000;
    if s.len() <= MAX {
        return s.to_owned();
    }
    let mut cut = MAX;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}… [{} bytes truncated]", &s[..cut], s.len() - cut)
}

/// Backtraces get a larger budget than messages but still a hard cap.
fn truncate_backtrace(s: &str) -> String {
    const MAX: usize = 8000;
    if s.len() <= MAX {
        return s.to_owned();
    }
    let mut cut = MAX;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}… [{} bytes truncated]", &s[..cut], s.len() - cut)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("hello"), "hello");
    }

    #[test]
    fn truncate_clamps_long_strings_on_char_boundary() {
        let long = "я".repeat(2000); // 4000 bytes
        let out = truncate(&long);
        assert!(out.contains("bytes truncated"));
        // Must not split a multi-byte char — the prefix is valid UTF-8 by construction.
        assert!(out.starts_with('я'));
    }

    #[test]
    fn base_record_has_expected_fields() {
        let obj = base("console_error", "https://example.com/");
        assert_eq!(
            obj.get("kind"),
            Some(&JsonValue::String("console_error".into()))
        );
        assert_eq!(
            obj.get("url"),
            Some(&JsonValue::String("https://example.com/".into()))
        );
        assert!(obj.contains_key("time"));
        assert!(obj.contains_key("ts_ms"));
    }

    #[test]
    fn render_health_threshold_is_conservative() {
        // The white-screen heuristic must ignore trivial pages: a DOM with only a
        // handful of nodes stays below the content-bearing floor, so it can never
        // be flagged as a broken render even when it legitimately paints nothing.
        let trivial_dom = 3usize;
        assert!(
            trivial_dom < BROKEN_RENDER_DOM_MIN,
            "trivial pages must sit below the broken-render DOM floor"
        );
    }
}
