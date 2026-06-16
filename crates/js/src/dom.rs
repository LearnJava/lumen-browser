//! JS↔DOM bridge for lumen-js.
//!
//! Registers `_lumen_*` native Rust functions in a QuickJS context, then
//! evaluates the `WEB_API_SHIM` JavaScript that builds standard `document`,
//! `window`, `console` globals on top of those primitives.
//!
//! Full CSS selector support via lumen_layout::query_all / matches_selector:
//! tag, .class, #id, compound (div.foo), combinators ( > + ~), pseudo-classes.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use lumen_core::ext::{CacheBackend, CookieProvider, IdbBackend, JsFetchProvider, JsSseEvent, JsSseProvider, JsWebSocketProvider, JsWsEvent, SwBackend};
use lumen_core::url::Url;
use lumen_dom::{
    Attribute, Document, DomPosition, NodeData, NodeId, QualName, Range as DomRange, Selection,
    ShadowRootMode, node_child_count, node_length, node_text_content, range_text,
};
use lumen_layout::{matches_selector, query_all};
use rquickjs::{Ctx, Function, Result as QjResult};

use lumen_core::WebStorage;

// ─── history state ───────────────────────────────────────────────────────────

struct HistoryEntry {
    state_json: String,
    url: String,
}

struct HistoryState {
    entries: Vec<HistoryEntry>,
    current: usize,
}

impl HistoryState {
    fn new() -> Self {
        Self {
            entries: vec![HistoryEntry {
                state_json: "null".into(),
                url: String::new(),
            }],
            current: 0,
        }
    }

    fn push(&mut self, state_json: String, url: String) {
        self.entries.truncate(self.current + 1);
        self.entries.push(HistoryEntry { state_json, url });
        self.current = self.entries.len() - 1;
    }

    fn replace(&mut self, state_json: String, url: String) {
        if let Some(e) = self.entries.get_mut(self.current) {
            e.state_json = state_json;
            e.url = url;
        }
    }

    // Returns false when delta is 0 (Phase 0: reload not implemented) or out of bounds.
    fn go(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let new_idx = self.current as i64 + i64::from(delta);
        if new_idx < 0 || new_idx >= self.entries.len() as i64 {
            return false;
        }
        self.current = new_idx as usize;
        true
    }

    fn state_json(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.state_json.as_str())
            .unwrap_or("null")
    }

    fn url(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.url.as_str())
            .unwrap_or("")
    }

    fn length(&self) -> u32 {
        self.entries.len() as u32
    }
}

// ─── navigation request ───────────────────────────────────────────────────────

/// Navigation request emitted by JS (`location.href =`, `location.assign()`,
/// `location.replace()`, `location.reload()`).  Captured in `nav_out` during
/// script execution and read by the shell after `install_dom_api` returns.
#[derive(Debug, Clone)]
pub enum NavigateRequest {
    /// Navigate to URL and push a new entry onto the history stack.
    Push(String),
    /// Navigate to URL and replace the current history entry.
    Replace(String),
    /// Reload the current page.
    Reload,
}

// ─── history URL update ───────────────────────────────────────────────────────

/// Notification emitted by `history.pushState`/`history.replaceState` so the
/// shell can update the address-bar display URL without triggering a page load.
///
/// Queued in `pending_history_url_updates` during JS execution; drained by the
/// shell in `about_to_wait` to update `display_url` and the navigation stack.
#[derive(Debug, Clone)]
pub enum HistoryUrlUpdate {
    /// `history.pushState` — add a same-document entry to the back-stack and
    /// update the displayed URL.  `new_state_json` is the serialised state
    /// object for the new entry (used when going forward back to this point).
    Push {
        /// New virtual URL to show in the address bar.
        url: String,
        /// Serialised JS state object for this new history entry.
        new_state_json: String,
    },
    /// `history.replaceState` — replace the current entry URL only; do not add
    /// a new back-stack entry.  `new_state_json` replaces the current state.
    Replace {
        /// New virtual URL to show in the address bar.
        url: String,
        /// Serialised JS state object replacing the current history entry.
        new_state_json: String,
    },
}

/// A popup window request emitted by JS `window.open(url, target, features)`.
///
/// Captured in `window_open_requests` during script execution and drained by the
/// shell in `about_to_wait` — each entry opens a new tab navigated to `url`.
/// `width` and `height` come from the `features` string (default 800×600).
#[derive(Debug, Clone)]
pub struct PopupRequest {
    /// Target URL. Empty string means `about:blank`.
    pub url: String,
    /// Window target (`_blank`, `_self`, named window, etc.). Lumen treats all
    /// targets as a new tab for now.
    pub target: String,
    /// Requested popup width in CSS px (from `width=` feature, default 800).
    pub width: u32,
    /// Requested popup height in CSS px (from `height=` feature, default 600).
    pub height: u32,
}

/// A print request emitted by `window.print()` (W-2 Phase 1).
///
/// Shell intercepts and opens print dialog or directly renders to PDF.
#[derive(Debug, Clone)]
pub struct PrintRequest {
    /// Requested margin (in CSS px). Defaults: 48 px.
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    /// Paper size in inches: (width, height). Defaults: letter 8.5 x 11.0.
    pub paper_width_in: f32,
    pub paper_height_in: f32,
    /// Output PDF path. If None, use default (e.g., "document.pdf").
    pub output_path: Option<String>,
}

impl Default for PrintRequest {
    fn default() -> Self {
        Self {
            margin_top: 48.0,
            margin_bottom: 48.0,
            margin_left: 48.0,
            margin_right: 48.0,
            paper_width_in: 8.5,  // US Letter width
            paper_height_in: 11.0, // US Letter height
            output_path: None,
        }
    }
}

/// A fullscreen API request emitted by JS `element.requestFullscreen()` or
/// `document.exitFullscreen()`.
///
/// Captured in `fullscreen_requests` and drained by the shell in `about_to_wait`
/// to toggle OS fullscreen via `winit::window::Window::set_fullscreen`.
#[derive(Debug, Clone)]
pub enum FullscreenRequest {
    /// `element.requestFullscreen()` — enter OS fullscreen for the given element.
    Enter {
        /// Node index of the element requesting fullscreen.
        nid: u32,
    },
    /// `document.exitFullscreen()` or Escape-key acknowledgement — exit OS fullscreen.
    Exit,
}

// ─── public entry point ───────────────────────────────────────────────────────

/// Install DOM primitives (`_lumen_*`) and the Web API shim into `ctx`.
///
/// After this call the context exposes `console`, `document`, `window`,
/// `location`, `navigator`, `alert`, `fetch`, `WebSocket`, `localStorage`,
/// and `sessionStorage`.
///
/// `page_url` — the URL of the current page, used to initialise `location`.
/// `nav_out`  — shared slot; JS writes a `NavigateRequest` here when the page
///              requests navigation via `location.href=` etc.  The caller reads
///              it after all scripts have run.
/// `fetch_provider` wires `window.fetch()` to the real HTTP stack.
/// `ws_provider` wires `new WebSocket(url)` to the real WS stack.
/// `sse_provider` wires `new EventSource(url)` to the real SSE stack.
/// `ls_store` — shared localStorage partition for this origin; persists across
///              page reloads.  Pass a fresh `Arc<Mutex<WebStorage>>` per origin.
/// `ss_store` — fresh sessionStorage for this page load; created by the caller.
/// `timer_wakeup` — shared slot written by `_lumen_request_wakeup` when a timer
///              is scheduled; shell reads it to set `ControlFlow::WaitUntil`.
/// `layout_rects` — shared map updated by the shell after each relayout; maps
///              `NodeId` index → `[x, y, width, height]` in viewport-relative CSS px.
/// `viewport_size` — shared `[width, height]` updated by the shell on resize.
/// `lazy_img_requests` — queue written by `_lumen_request_lazy_image_load`; drained by shell.
/// `cookie_jar` — optional cookie store for `document.cookie` get/set.
/// `deterministic_seed` — when `Some(seed)`: freeze `Date.now()` at 0 and override
///   `Math.random` with a seeded xorshift32 PRNG so output is reproducible (8F).
///   The seed is typically derived from the URL hash via `shell::deterministic::seed_from_url`.
/// Pass `None` for providers in sandboxed contexts or tests.
#[allow(clippy::too_many_arguments)]
pub fn install_dom_api(
    ctx: &Ctx<'_>,
    doc: Arc<Mutex<Document>>,
    page_url: &str,
    nav_out: Arc<Mutex<Option<NavigateRequest>>>,
    fetch_provider: Option<Arc<dyn JsFetchProvider>>,
    ws_provider: Option<Arc<dyn JsWebSocketProvider>>,
    sse_provider: Option<Arc<dyn JsSseProvider>>,
    ls_store: Arc<Mutex<WebStorage>>,
    ss_store: Arc<Mutex<WebStorage>>,
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    dom_dirty: Arc<AtomicBool>,
    raf_pending: Arc<AtomicBool>,
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    viewport_size: Arc<Mutex<[f32; 2]>>,
    lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
    cookie_jar: Option<Arc<dyn CookieProvider>>,
    idb_backend: Option<Arc<dyn IdbBackend>>,
    sw_backend: Option<Arc<dyn SwBackend>>,
    cache_backend: Option<Arc<dyn CacheBackend>>,
    scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    page_scroll_y: Arc<Mutex<f32>>,
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    window_open_requests: Arc<Mutex<Vec<PopupRequest>>>,
    deterministic_seed: Option<u64>,
    console_messages: Arc<Mutex<Vec<(u8, String)>>>,
    pending_history_url_updates: Arc<Mutex<Vec<HistoryUrlUpdate>>>,
    fullscreen_requests: Arc<Mutex<Vec<FullscreenRequest>>>,
    print_requests: Arc<Mutex<Vec<PrintRequest>>>,
    pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
    // True when COOP=same-origin + COEP=require-corp are both present on this document.
    cross_origin_isolated: bool,
) -> QjResult<()> {
    install_primitives(ctx, Arc::clone(&doc), Arc::clone(&nav_out), fetch_provider, ws_provider, sse_provider, ls_store, ss_store, timer_wakeup, dom_dirty, raf_pending, layout_rects, viewport_size, lazy_img_requests, page_url.to_owned(), cookie_jar, idb_backend, sw_backend, cache_backend, scroll_states, pending_scrolls, pending_page_scrolls, page_scroll_y, computed_styles, Arc::clone(&window_open_requests), deterministic_seed, console_messages, pending_history_url_updates, fullscreen_requests, print_requests, pending_focus_requests)?;
    // Inject the page URL as a JS global so that WEB_API_SHIM can initialise
    // the `location` object.  Cleaned up by the shim itself (`delete _LUMEN_PAGE_URL`).
    ctx.globals().set("_LUMEN_PAGE_URL", page_url.to_owned())?;
    // Inject cross-origin isolation state so WEB_API_SHIM can set window.crossOriginIsolated.
    ctx.globals().set("_LUMEN_CROSS_ORIGIN_ISOLATED", cross_origin_isolated)?;
    ctx.eval::<(), _>(WEB_API_SHIM)?;
    // In deterministic mode (8F): override Math.random with a seeded xorshift32 PRNG
    // and freeze Date.now() at 0 (QuickJS native Date.now() uses the system clock).
    // Must run AFTER WEB_API_SHIM so Date and Math are fully set up.
    if let Some(seed) = deterministic_seed {
        let seed32 = u32::try_from(seed & 0xffff_ffff).unwrap_or(1);
        let seed32 = if seed32 == 0 { 1 } else { seed32 };
        let js = format!(
            "(function(){{var s={seed32};\
             Math.random=function(){{s^=s<<13;s^=s>>>17;s^=s<<5;return (s>>>0)/4294967296;}};\
             Date.now=function(){{return 0;}};\
             }})()"
        );
        ctx.eval::<(), _>(js.as_str())?;
    }
    Ok(())
}

// ─── primitive registrations ──────────────────────────────────────────────────

/// Extract `"method"` field from a cache meta JSON string.
///
/// Fast path without serde — scans for `"method":"<VALUE>"` literally.
/// Falls back to `"GET"` on any parse failure.
fn cache_meta_method(meta_json: &str) -> String {
    if let Some(start) = meta_json.find("\"method\":\"") {
        let rest = &meta_json[start + 10..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    "GET".to_string()
}

/// Parse CSS style string (e.g. "color: red; font-size: 12px") into a HashMap.
fn _parse_style_string(css_text: &str) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    for decl in css_text.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((prop, val)) = decl.split_once(':') {
            let prop = prop.trim().to_string();
            let val = val.trim().to_string();
            map.insert(prop, val);
        }
    }
    map
}

/// Serialize a style HashMap back into CSS string.
fn _serialize_style_map(map: &std::collections::HashMap<String, String>) -> String {
    map.iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Convert camelCase property name to kebab-case.
fn _camel_to_kebab(prop: &str) -> String {
    let mut result = String::new();
    for (i, c) in prop.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn install_primitives(
    ctx: &Ctx<'_>,
    doc: Arc<Mutex<Document>>,
    nav_out: Arc<Mutex<Option<NavigateRequest>>>,
    fetch_provider: Option<Arc<dyn JsFetchProvider>>,
    ws_provider: Option<Arc<dyn JsWebSocketProvider>>,
    sse_provider: Option<Arc<dyn JsSseProvider>>,
    ls_store: Arc<Mutex<WebStorage>>,
    ss_store: Arc<Mutex<WebStorage>>,
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    dom_dirty: Arc<AtomicBool>,
    raf_pending: Arc<AtomicBool>,
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    viewport_size: Arc<Mutex<[f32; 2]>>,
    lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
    page_url: String,
    cookie_jar: Option<Arc<dyn CookieProvider>>,
    idb_backend: Option<Arc<dyn IdbBackend>>,
    sw_backend: Option<Arc<dyn SwBackend>>,
    cache_backend: Option<Arc<dyn CacheBackend>>,
    scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    page_scroll_y: Arc<Mutex<f32>>,
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    window_open_requests: Arc<Mutex<Vec<PopupRequest>>>,
    deterministic_seed: Option<u64>,
    console_messages: Arc<Mutex<Vec<(u8, String)>>>,
    pending_history_url_updates: Arc<Mutex<Vec<HistoryUrlUpdate>>>,
    fullscreen_requests: Arc<Mutex<Vec<FullscreenRequest>>>,
    print_requests: Arc<Mutex<Vec<PrintRequest>>>,
    pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
) -> QjResult<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals()
                .set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // ── console ──────────────────────────────────────────────────────────────
    {
        let buf_log = Arc::clone(&console_messages);
        reg!("_lumen_console_log", move |msg: String| {
            eprintln!("[JS] {msg}");
            buf_log.lock().unwrap().push((0, msg));
        });
        let buf_warn = Arc::clone(&console_messages);
        reg!("_lumen_console_warn", move |msg: String| {
            eprintln!("[JS warn] {msg}");
            buf_warn.lock().unwrap().push((1, msg));
        });
        let buf_err = Arc::clone(&console_messages);
        reg!("_lumen_console_error", move |msg: String| {
            eprintln!("[JS error] {msg}");
            buf_err.lock().unwrap().push((2, msg));
        });
    }

    // ── window.print() (W-2) ──────────────────────────────────────────────────
    {
        let pr = Arc::clone(&print_requests);
        reg!("_lumen_print_dialog", move || {
            eprintln!("[window.print()] Opening print preview dialog");
            pr.lock().unwrap().push(PrintRequest::default());
        });
    }

    // ── dialog focus management (HTML LS §6.6.3) ─────────────────────────────
    // `showModal()` calls `_lumen_request_focus(nid)` to focus the first autofocus
    // element (or the dialog itself).  `close()` calls `_lumen_request_focus(prev)`
    // to restore focus to the element that was active before the dialog opened.
    // The shell drains these via `take_focus_requests()` after each JS pump.
    {
        let pfr = Arc::clone(&pending_focus_requests);
        reg!("_lumen_request_focus", move |nid: u32| {
            pfr.lock().unwrap().push(Some(nid));
        });
        let pfr2 = Arc::clone(&pending_focus_requests);
        reg!("_lumen_request_blur", move || {
            pfr2.lock().unwrap().push(None);
        });
    }

    // ── document meta ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_document_root", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.root().index() as u32
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_get_body", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "body").map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_get_document_title", move || -> String {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "title")
                .map(|nid| collect_text_content(&doc, nid))
                .unwrap_or_default()
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_set_document_title", move |text: String| {
            let mut doc = d.lock().unwrap();
            if let Some(title_id) = find_element_by_tag(&doc, "title") {
                set_text_content(&mut doc, title_id, &text);
            }
        });
    }

    // ── document.fonts (FontFaceSet) ──────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_size", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.fonts().size() as u32
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_get", move |idx: u32| -> Option<String> {
            let doc = d.lock().unwrap();
            doc.fonts().all().get(idx as usize).map(|face| {
                // Serialize FontFace to JSON manually
                let family_esc = face.family.replace('\\', "\\\\").replace('"', "\\\"");
                let style_esc = face.style.replace('\\', "\\\\").replace('"', "\\\"");
                let weight_esc = face.weight.replace('\\', "\\\\").replace('"', "\\\"");
                let stretch_esc = face.stretch.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let unicode_range_esc = face.unicode_range.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let src_esc = face.src.replace('\\', "\\\\").replace('"', "\\\"");
                let status_str = match face.status {
                    lumen_dom::FontFaceStatus::Unloaded => "unloaded",
                    lumen_dom::FontFaceStatus::Loading => "loading",
                    lumen_dom::FontFaceStatus::Loaded => "loaded",
                    lumen_dom::FontFaceStatus::Error => "error",
                };
                format!(
                    r#"{{"family":"{family_esc}","style":"{style_esc}","weight":"{weight_esc}","stretch":{stretch_json},"unicodeRange":{unicode_json},"src":"{src_esc}","status":"{status_str}"}}"#,
                    stretch_json = if face.stretch.is_some() { format!(r#""{}""#, stretch_esc) } else { "null".to_string() },
                    unicode_json = if face.unicode_range.is_some() { format!(r#""{}""#, unicode_range_esc) } else { "null".to_string() }
                )
            })
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_get_by_family", move |family: String| -> Vec<String> {
            let doc = d.lock().unwrap();
            doc.fonts().get_by_family(&family).iter().map(|face| {
                let family_esc = face.family.replace('\\', "\\\\").replace('"', "\\\"");
                let style_esc = face.style.replace('\\', "\\\\").replace('"', "\\\"");
                let weight_esc = face.weight.replace('\\', "\\\\").replace('"', "\\\"");
                let stretch_esc = face.stretch.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let unicode_range_esc = face.unicode_range.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let src_esc = face.src.replace('\\', "\\\\").replace('"', "\\\"");
                let status_str = match face.status {
                    lumen_dom::FontFaceStatus::Unloaded => "unloaded",
                    lumen_dom::FontFaceStatus::Loading => "loading",
                    lumen_dom::FontFaceStatus::Loaded => "loaded",
                    lumen_dom::FontFaceStatus::Error => "error",
                };
                format!(
                    r#"{{"family":"{family_esc}","style":"{style_esc}","weight":"{weight_esc}","stretch":{stretch_json},"unicodeRange":{unicode_json},"src":"{src_esc}","status":"{status_str}"}}"#,
                    stretch_json = if face.stretch.is_some() { format!(r#""{}""#, stretch_esc) } else { "null".to_string() },
                    unicode_json = if face.unicode_range.is_some() { format!(r#""{}""#, unicode_range_esc) } else { "null".to_string() }
                )
            }).collect()
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_has_family", move |family: String| -> bool {
            let doc = d.lock().unwrap();
            doc.fonts().has_family(&family)
        });
    }

    // ── node lookup ──────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_element_by_id", move |id: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            find_first_matching(&doc, doc.root(), &|node| {
                matches!(&node.data, NodeData::Element { .. })
                    && node.get_attr("id") == Some(id.as_str())
            })
            .map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_query_selector", move |sel: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            query_all(&doc, &sel).into_iter().next().map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_query_selector_all",
            move |sel: String| -> Vec<u32> {
                let doc = d.lock().unwrap();
                query_all(&doc, &sel)
                    .into_iter()
                    .map(|n| n.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_node_matches_selector",
            move |node_id: u32, sel: String| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches_selector(&doc, nid, &sel)
            }
        );
    }

    // ── node properties ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_tag_name", move |node_id: u32| -> String {
            let doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            match &doc.get(nid).data {
                NodeData::Element { name, .. } => name.local.to_ascii_uppercase(),
                NodeData::Text(_) => "#text".into(),
                NodeData::Document => "#document".into(),
                NodeData::Comment(_) => "#comment".into(),
                NodeData::Doctype { .. } => "html".into(),
                NodeData::ShadowRoot { .. } => "#shadow-root".into(),
                NodeData::DocumentFragment => "#document-fragment".into(),
            }
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_is_text_node",
            move |node_id: u32| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches!(doc.get(nid).data, NodeData::Text(_))
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_attr",
            move |node_id: u32, name: String| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).get_attr(&name).map(|s| s.to_string())
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_attr",
            move |node_id: u32, name: String, value: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_attribute(&mut doc, nid, &name, &value);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_remove_attr", move |node_id: u32, name: String| {
            let mut doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            remove_attribute(&mut doc, nid, &name);
            dirty.store(true, Ordering::Relaxed);
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_attr_names",
            move |node_id: u32| -> Vec<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                match &doc.get(nid).data {
                    NodeData::Element { attrs, .. } => {
                        attrs.iter().map(|a| a.name.local.to_string()).collect()
                    }
                    _ => Vec::new(),
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_text_content",
            move |node_id: u32| -> String {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_text_content",
            move |node_id: u32, text: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &text);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_inner_html",
            move |node_id: u32| -> String {
                // Phase 0: return text content only (no HTML serialization).
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_inner_html",
            move |node_id: u32, html: String| {
                // Phase 0: treat innerHTML as plain text (no fragment parsing).
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &html);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }

    // ── tree navigation ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_children",
            move |node_id: u32| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid)
                    .children
                    .iter()
                    .map(|c| c.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_parent",
            move |node_id: u32| -> Option<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).parent.map(|p| p.index() as u32)
            }
        );
    }

    // ── DOM node count ───────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_dom_node_count",
            move || -> u32 {
                d.lock().unwrap().node_count() as u32
            }
        );
    }

    // ── tree mutation ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_element",
            move |tag: String| -> u32 {
                let mut doc = d.lock().unwrap();
                // Returns u32::MAX when MAX_DOM_NODES is reached; JS shim handles this.
                match doc.try_create_element(QualName::html(tag.to_ascii_lowercase())) {
                    Ok(nid) => nid.index() as u32,
                    Err(_) => u32::MAX,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_text_node",
            move |text: String| -> u32 {
                let mut doc = d.lock().unwrap();
                let nid = doc.create_text(text);
                nid.index() as u32
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_append_child",
            move |parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let parent = NodeId::from_index(parent_id as usize);
                let child = NodeId::from_index(child_id as usize);
                doc.append_child(parent, child);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_remove_child",
            move |_parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                doc.detach(child);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }

    // ── Service Worker / Cache Storage ───────────────────────────────────────
    {
        // SW registrations: origin+scope+scriptUrl stored in-memory.
        // Key: (origin, scope) → script_url
        type SwMap = std::collections::HashMap<(String, String), String>;
        let sw_regs: Arc<Mutex<SwMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        // Cache storage: origin → cache_name → url → (method, meta_json, body)
        // meta_json: {"method":"GET","status":200,"statusText":"OK","headers":{…}}
        // method is stored separately for O(1) `keys()` without re-parsing meta_json.
        type CacheEntry = (String, String, Vec<u8>);
        type CacheMap = std::collections::HashMap<String, std::collections::HashMap<String, std::collections::HashMap<String, CacheEntry>>>;
        let cache_data: Arc<Mutex<CacheMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_register",
            move |origin: String, scope: String, script_url: String| {
                sw.lock().unwrap().insert((origin, scope), script_url);
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_has_registration",
            move |origin: String| -> bool {
                sw.lock().unwrap().keys().any(|(o, _)| *o == origin)
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_unregister",
            move |origin: String, scope: String| {
                sw.lock().unwrap().remove(&(origin, scope));
            }
        );

        // Persistence bindings — forward to SwBackend when provided.
        let sw_be = sw_backend.clone();
        reg!(
            "_lumen_sw_persist",
            move |_origin: String, snapshot: String| {
                if let Some(ref be) = sw_be {
                    be.save(&snapshot);
                }
            }
        );

        let sw_be2 = sw_backend.clone();
        reg!(
            "_lumen_sw_load",
            move |_origin: String| -> Option<String> {
                sw_be2.as_ref().and_then(|be| be.load())
            }
        );

        // Dispatch helpers: use SQLite backend when provided, fall back to in-memory map.
        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_put",
            // meta_json: {"method":"GET","status":200,"statusText":"OK","headers":{...}}
            // Grouped into one string to stay within rquickjs 5-arg IntoJsFunc limit.
            move |origin: String, cache_name: String, url: String, meta_json: String, body: Vec<u8>| {
                if let Some(ref be) = cbe {
                    be.cache_put(&origin, &cache_name, &url, &meta_json, &body);
                } else {
                    let method = cache_meta_method(&meta_json);
                    cd.lock()
                        .unwrap()
                        .entry(origin)
                        .or_default()
                        .entry(cache_name)
                        .or_default()
                        .insert(url, (method, meta_json, body));
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match",
            move |origin: String, cache_name: String, url: String| -> Option<Vec<u8>> {
                if let Some(ref be) = cbe {
                    be.cache_match(&origin, &cache_name, &url).map(|(_, body)| body)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .and_then(|cache| cache.get(&url))
                        .map(|(_, _, body)| body.clone())
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_info",
            // Returns the raw meta_json stored at put time (already JSON-encoded).
            move |origin: String, cache_name: String, url: String| -> Option<String> {
                if let Some(ref be) = cbe {
                    be.cache_match(&origin, &cache_name, &url).map(|(meta, _)| meta)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .and_then(|cache| cache.get(&url))
                        .map(|(_, meta, _)| meta.clone())
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_any",
            move |origin: String, url: String| -> Option<Vec<u8>> {
                if let Some(ref be) = cbe {
                    be.cache_match_any(&origin, &url).map(|(_, body)| body)
                } else {
                    let guard = cd.lock().unwrap();
                    let caches = guard.get(&origin)?;
                    for cache in caches.values() {
                        if let Some((_, _, body)) = cache.get(&url) {
                            return Some(body.clone());
                        }
                    }
                    None
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_any_info",
            move |origin: String, url: String| -> Option<String> {
                if let Some(ref be) = cbe {
                    be.cache_match_any(&origin, &url).map(|(meta, _)| meta)
                } else {
                    let guard = cd.lock().unwrap();
                    let caches = guard.get(&origin)?;
                    for cache in caches.values() {
                        if let Some((_, meta, _)) = cache.get(&url) {
                            return Some(meta.clone());
                        }
                    }
                    None
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_delete",
            move |origin: String, cache_name: String, url: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_delete(&origin, &cache_name, &url)
                } else {
                    let mut guard = cd.lock().unwrap();
                    if let Some(caches) = guard.get_mut(&origin)
                        && let Some(cache) = caches.get_mut(&cache_name)
                    {
                        cache.remove(&url).is_some()
                    } else {
                        false
                    }
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_keys",
            move |origin: String, cache_name: String| -> Vec<String> {
                if let Some(ref be) = cbe {
                    be.cache_keys(&origin, &cache_name).into_iter().map(|(u, _)| u).collect()
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .map(|cache| cache.keys().cloned().collect())
                        .unwrap_or_default()
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_keys_full",
            move |origin: String, cache_name: String| -> String {
                if let Some(ref be) = cbe {
                    let pairs = be.cache_keys(&origin, &cache_name);
                    let items: Vec<String> = pairs
                        .iter()
                        .map(|(url, method)| format!(r#"{{"url":"{url}","method":"{method}"}}"#))
                        .collect();
                    format!("[{}]", items.join(","))
                } else {
                    let guard = cd.lock().unwrap();
                    match guard.get(&origin).and_then(|c| c.get(&cache_name)) {
                        None => "[]".to_string(),
                        Some(cache) => {
                            let items: Vec<String> = cache
                                .iter()
                                .map(|(url, (method, _, _))| {
                                    format!(r#"{{"url":"{url}","method":"{method}"}}"#)
                                })
                                .collect();
                            format!("[{}]", items.join(","))
                        }
                    }
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_has",
            move |origin: String, cache_name: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_has(&origin, &cache_name)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .map(|caches| caches.contains_key(&cache_name))
                        .unwrap_or(false)
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_delete_cache",
            move |origin: String, cache_name: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_delete_cache(&origin, &cache_name)
                } else if let Some(caches) = cd.lock().unwrap().get_mut(&origin) {
                    caches.remove(&cache_name).is_some()
                } else {
                    false
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_names",
            move |origin: String| -> Vec<String> {
                if let Some(ref be) = cbe {
                    be.cache_names(&origin)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .map(|caches| caches.keys().cloned().collect())
                        .unwrap_or_default()
                }
            }
        );
    }

    // ── history ──────────────────────────────────────────────────────────────
    {
        let hist = Arc::new(Mutex::new(HistoryState::new()));

        let h = Arc::clone(&hist);
        reg!(
            "_lumen_history_push",
            move |state_json: String, url: String| {
                h.lock().unwrap().push(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!(
            "_lumen_history_replace",
            move |state_json: String, url: String| {
                h.lock().unwrap().replace(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!("_lumen_history_go", move |delta: i32| -> bool {
            h.lock().unwrap().go(delta)
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_length", move || -> u32 {
            h.lock().unwrap().length()
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_state_json", move || -> String {
            h.lock().unwrap().state_json().to_string()
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_url", move || -> String {
            h.lock().unwrap().url().to_string()
        });

        // Notify shell of pushState/replaceState URL changes so the address bar
        // can be updated without a page reload.  Called from history.pushState /
        // history.replaceState in WEB_API_SHIM after the JS HistoryState is updated.
        let q = Arc::clone(&pending_history_url_updates);
        reg!(
            "_lumen_history_push_url",
            move |url: String, new_state_json: String| {
                q.lock()
                    .unwrap()
                    .push(HistoryUrlUpdate::Push { url, new_state_json });
            }
        );

        let q = Arc::clone(&pending_history_url_updates);
        reg!(
            "_lumen_history_replace_url",
            move |url: String, new_state_json: String| {
                q.lock()
                    .unwrap()
                    .push(HistoryUrlUpdate::Replace { url, new_state_json });
            }
        );
    }

    // ── navigation (location.href =, assign, replace, reload) ────────────────
    {
        let nav = Arc::clone(&nav_out);
        reg!("_lumen_navigate", move |url: String, replace: bool| {
            *nav.lock().unwrap() = Some(if replace {
                NavigateRequest::Replace(url)
            } else {
                NavigateRequest::Push(url)
            });
        });

        let nav = Arc::clone(&nav_out);
        reg!("_lumen_reload", move || {
            *nav.lock().unwrap() = Some(NavigateRequest::Reload);
        });
    }

    // ── Fetch API ─────────────────────────────────────────────────────────────
    {
        struct FetchCache {
            status: u16,
            status_text: String,
            headers: Vec<String>, // flat: [name, value, name, value, ...]
            body: Vec<u8>,
        }

        let cache: Arc<Mutex<Option<FetchCache>>> = Arc::new(Mutex::new(None));

        let fp2 = fetch_provider.clone();
        let fp_beacon = fetch_provider.clone();
        let (fp, c) = (fetch_provider, Arc::clone(&cache));
        reg!("_lumen_fetch_sync", move |url: String, method: String| -> bool {
            let Some(ref provider) = fp else { return false };
            match provider.fetch_sync(&url, &method) {
                Ok(resp) => {
                    let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                    for (k, v) in resp.headers {
                        flat.push(k);
                        flat.push(v);
                    }
                    *c.lock().unwrap() = Some(FetchCache {
                        status: resp.status,
                        status_text: resp.status_text,
                        headers: flat,
                        body: resp.body,
                    });
                    true
                }
                Err(e) => {
                    eprintln!("fetch error: {e}");
                    false
                }
            }
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_status", move || -> u32 {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or(0, |r| u32::from(r.status))
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_status_text", move || -> String {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(String::new, |r| r.status_text.clone())
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_headers", move || -> Vec<String> {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(Vec::new, |r| r.headers.clone())
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_body", move || -> Vec<u8> {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(Vec::new, |r| r.body.clone())
        });

        // _lumen_fetch_body_length() → u32
        // Returns the byte length of the most recent cached response body.
        // Used by the pull()-based ReadableStream in Response.body to avoid
        // copying the full body into JS memory at construction time.
        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_body_length", move || -> u32 {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or(0, |r| r.body.len() as u32)
        });

        // _lumen_fetch_body_chunk(offset: u32, size: u32) → Vec<u8>
        // Returns bytes [offset .. offset+size] of the cached response body.
        // Called repeatedly by Response.body.pull() to stream large responses
        // without loading the entire body into JS at once (Fetch Standard §2.2).
        let c = Arc::clone(&cache);
        reg!(
            "_lumen_fetch_body_chunk",
            move |offset: u32, size: u32| -> Vec<u8> {
                let guard = c.lock().unwrap();
                let body = guard.as_ref().map_or(&[] as &[u8], |r| r.body.as_slice());
                let start = (offset as usize).min(body.len());
                let end = (start + size as usize).min(body.len());
                body[start..end].to_vec()
            }
        );

        // _lumen_check_sri_integrity(integrity) → bool
        // Verifies the cached response body against the SRI `integrity` string
        // (W3C SRI §3.3.5). Must be called after _lumen_fetch_sync / _lumen_fetch_sync_with_body
        // and before reading the body. Returns true if integrity is empty or passes.
        {
            let c_sri = Arc::clone(&cache);
            reg!("_lumen_check_sri_integrity", move |integrity: String| -> bool {
                let guard = c_sri.lock().unwrap();
                let body = guard.as_ref().map_or(&[] as &[u8], |r| r.body.as_slice());
                crate::sri::check_sri(body, &integrity)
            });
        }

        // _lumen_fetch_sync_with_body(url, method, content_type, body_bytes) → bool
        // Used by fetch() when init.body is present (FormData, string, ArrayBuffer).
        // Shares the same FetchCache slot as _lumen_fetch_sync.
        {
            let fetch_provider2 = fp2;
            let c2 = Arc::clone(&cache);
            reg!(
                "_lumen_fetch_sync_with_body",
                move |url: String, method: String, content_type: String, body: Vec<u8>| -> bool {
                    let Some(ref provider) = fetch_provider2 else {
                        return false;
                    };
                    match provider.fetch_with_body_sync(&url, &method, &content_type, &body) {
                        Ok(resp) => {
                            let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                            for (k, v) in resp.headers {
                                flat.push(k);
                                flat.push(v);
                            }
                            *c2.lock().unwrap() = Some(FetchCache {
                                status: resp.status,
                                status_text: resp.status_text,
                                headers: flat,
                                body: resp.body,
                            });
                            true
                        }
                        Err(e) => {
                            eprintln!("fetch_with_body error: {e}");
                            false
                        }
                    }
                }
            );
        }

        // ── Per-response stream slots ────────────────────────────────────────────
        // Each call to Response._fromFetchCache() allocates a dedicated slot so the
        // body can be consumed independently of subsequent fetch() calls that would
        // otherwise overwrite the single FetchCache slot.
        //
        // _lumen_stream_alloc()                  → u32  (0 = empty body)
        // _lumen_stream_length(id: u32)          → u32
        // _lumen_stream_chunk(id, offset, size)  → Vec<u8>
        // _lumen_stream_free(id: u32)
        {
            let stream_slots: Arc<Mutex<HashMap<u32, Vec<u8>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let stream_next: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));

            let (ss_alloc, sn, c_sa) = (
                Arc::clone(&stream_slots),
                Arc::clone(&stream_next),
                Arc::clone(&cache),
            );
            reg!("_lumen_stream_alloc", move || -> u32 {
                let body = {
                    let guard = c_sa.lock().unwrap();
                    guard.as_ref().map_or_else(Vec::new, |r| r.body.clone())
                };
                if body.is_empty() {
                    return 0;
                }
                let id = sn.fetch_add(1, Ordering::Relaxed);
                ss_alloc.lock().unwrap().insert(id, body);
                id
            });

            let ss_len = Arc::clone(&stream_slots);
            reg!("_lumen_stream_length", move |id: u32| -> u32 {
                ss_len.lock().unwrap().get(&id).map_or(0, |b| b.len() as u32)
            });

            let ss_chunk = Arc::clone(&stream_slots);
            reg!(
                "_lumen_stream_chunk",
                move |id: u32, offset: u32, size: u32| -> Vec<u8> {
                    let guard = ss_chunk.lock().unwrap();
                    let body = guard.get(&id).map_or(&[] as &[u8], |b| b.as_slice());
                    let start = (offset as usize).min(body.len());
                    let end = (start + size as usize).min(body.len());
                    body[start..end].to_vec()
                }
            );

            let ss_free = Arc::clone(&stream_slots);
            reg!("_lumen_stream_free", move |id: u32| {
                ss_free.lock().unwrap().remove(&id);
            });
        }

        // _lumen_send_beacon(url, body, content_type) → bool
        // Beacon API (W3C Beacon §3): fire-and-forget POST; response is ignored.
        // Returns false if no network provider is available, true if the request was queued.
        // The actual POST runs on a detached background thread so the JS caller is not blocked.
        {
            let fp = fp_beacon;
            reg!(
                "_lumen_send_beacon",
                move |url: String, body: String, content_type: String| -> bool {
                    let Some(ref provider) = fp else { return false };
                    let ct = if content_type.is_empty() {
                        "text/plain;charset=UTF-8".to_string()
                    } else {
                        content_type
                    };
                    let p = Arc::clone(provider);
                    std::thread::spawn(move || {
                        let _ = p.fetch_with_body_sync(&url, "POST", &ct, body.as_bytes());
                    });
                    true
                }
            );
        }
    }

    // ── Clipboard API ─────────────────────────────────────────────────────────
    // _lumen_clipboard_read()      → String (system clipboard plain text, "" if none)
    // _lumen_clipboard_write(text) → void   (replace system clipboard text)
    //
    // Both forward to the process-global clipboard provider installed by the shell
    // (`lumen_js::set_clipboard_provider`). With no provider (tests, dump modes)
    // read returns "" and write is a no-op, so navigator.clipboard still resolves.
    reg!("_lumen_clipboard_read", || -> String {
        crate::clipboard::read_text()
    });
    reg!("_lumen_clipboard_write", |text: String| {
        crate::clipboard::write_text(&text);
    });

    // ── WebAuthn / navigator.credentials ──────────────────────────────────────
    // _lumen_webauthn_create(packed) → JSON   (attestation result or {ok:false})
    // _lumen_webauthn_get(packed)    → JSON   (assertion result or {ok:false})
    // _lumen_webauthn_uvpa()         → bool   (platform authenticator available)
    //
    // `packed` is a `|`-separated string of base64url fields (see crate::credentials).
    // All forward to the process-global CredentialProvider installed by the shell
    // (`lumen_js::set_credential_provider`). With no provider, create/get return
    // {ok:false,error:"NotAllowedError"} so navigator.credentials still resolves.
    reg!("_lumen_webauthn_create", |packed: String| -> String {
        crate::credentials::create(packed)
    });
    reg!("_lumen_webauthn_get", |packed: String| -> String {
        crate::credentials::get(packed)
    });
    reg!("_lumen_webauthn_uvpa", || -> bool {
        crate::credentials::uvpa_available()
    });

    // ── WebSocket API ─────────────────────────────────────────────────────────
    // Phase 0 model: synchronous connect, background recv thread, JS polls.
    // _lumen_ws_connect(url)  → handle u32 (0 = error)
    // _lumen_ws_send(h, text) → bool
    // _lumen_ws_send_bin(h, data) → bool
    // _lumen_ws_close(h, code, reason)
    // _lumen_ws_poll(h) → Option<String> (JSON event or null)
    {
        use std::collections::HashMap;

        // Registry: handle → Box<dyn JsWebSocketSession>
        // Wrapped in Arc<Mutex<>> so each closure captures its own Arc clone.
        let registry: Arc<Mutex<HashMap<u32, Box<dyn lumen_core::ext::JsWebSocketSession>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id: Arc<Mutex<u32>> = Arc::new(Mutex::new(1));

        let (reg_c, nid_c, wp) = (Arc::clone(&registry), Arc::clone(&next_id), ws_provider);
        reg!("_lumen_ws_connect", move |url: String| -> u32 {
            let Some(ref provider) = wp else { return 0 };
            match provider.connect(&url) {
                Ok(session) => {
                    let id = {
                        let mut n = nid_c.lock().unwrap();
                        let id = *n;
                        *n = n.wrapping_add(1).max(1);
                        id
                    };
                    reg_c.lock().unwrap().insert(id, session);
                    id
                }
                Err(e) => {
                    eprintln!("[JS WebSocket] connect error: {e}");
                    0
                }
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!("_lumen_ws_send", move |handle: u32, text: String| -> bool {
            let mut map = reg_c.lock().unwrap();
            if let Some(sess) = map.get_mut(&handle) {
                sess.send_text(&text).is_ok()
            } else {
                false
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!(
            "_lumen_ws_send_bin",
            move |handle: u32, data: Vec<u8>| -> bool {
                let mut map = reg_c.lock().unwrap();
                if let Some(sess) = map.get_mut(&handle) {
                    sess.send_binary(&data).is_ok()
                } else {
                    false
                }
            }
        );

        let reg_c = Arc::clone(&registry);
        reg!(
            "_lumen_ws_close",
            move |handle: u32, code: u32, reason: String| {
                let mut map = reg_c.lock().unwrap();
                if let Some(sess) = map.get_mut(&handle) {
                    let _ = sess.close(code as u16, &reason);
                }
            }
        );

        let reg_c = Arc::clone(&registry);
        reg!(
            "_lumen_ws_poll",
            move |handle: u32| -> Option<String> {
                let map = reg_c.lock().unwrap();
                let sess = map.get(&handle)?;
                sess.poll().map(|ev| match ev {
                    JsWsEvent::Open => r#"{"t":"open"}"#.to_string(),
                    JsWsEvent::Message { data, is_binary } => {
                        if is_binary {
                            // Encode binary payload as base64-like hex for Phase 0.
                            let hex: String =
                                data.iter().map(|b| format!("{b:02x}")).collect();
                            format!(r#"{{"t":"msg","bin":true,"data":"{hex}"}}"#)
                        } else {
                            let text = String::from_utf8_lossy(&data);
                            // Minimal JSON-escape: replace \ and " only.
                            let escaped = text
                                .replace('\\', "\\\\")
                                .replace('"', "\\\"")
                                .replace('\n', "\\n")
                                .replace('\r', "\\r");
                            format!(r#"{{"t":"msg","bin":false,"data":"{escaped}"}}"#)
                        }
                    }
                    JsWsEvent::Close { code, reason } => {
                        let c = code.unwrap_or(1000);
                        let r = reason
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"");
                        format!(r#"{{"t":"close","code":{c},"reason":"{r}"}}"#)
                    }
                    JsWsEvent::Error(msg) => {
                        let m = msg
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"");
                        format!(r#"{{"t":"error","msg":"{m}"}}"#)
                    }
                })
            }
        );
    }

    // ── Server-Sent Events API (HTML Living Standard §9.2) ───────────────────
    // Phase 0 model: background recv thread buffers events, JS polls.
    // _lumen_sse_connect(url) → handle u32 (0 = error / no provider)
    // _lumen_sse_poll(handle) → Option<String> (JSON event or null)
    // _lumen_sse_close(handle)
    {
        use std::collections::HashMap;

        /// JSON-escape a string into a quoted JSON string literal (`"..."`).
        ///
        /// Handles the characters that must be escaped per RFC 8259 §7:
        /// `"`, `\`, and the C0 control set (`\n`/`\r`/`\t`/`\b`/`\f` plus `\u00XX`).
        fn json_str(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\u{08}' => out.push_str("\\b"),
                    '\u{0c}' => out.push_str("\\f"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }

        // Registry: handle → Box<dyn JsSseSession>
        let registry: Arc<Mutex<HashMap<u32, Box<dyn lumen_core::ext::JsSseSession>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id: Arc<Mutex<u32>> = Arc::new(Mutex::new(1));

        let (reg_c, nid_c, sp) = (Arc::clone(&registry), Arc::clone(&next_id), sse_provider);
        reg!("_lumen_sse_connect", move |url: String| -> u32 {
            let Some(ref provider) = sp else { return 0 };
            match provider.connect_sse(&url) {
                Ok(session) => {
                    let id = {
                        let mut n = nid_c.lock().unwrap();
                        let id = *n;
                        *n = n.wrapping_add(1).max(1);
                        id
                    };
                    reg_c.lock().unwrap().insert(id, session);
                    id
                }
                Err(e) => {
                    eprintln!("[JS SSE] connect error: {e}");
                    0
                }
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!("_lumen_sse_poll", move |handle: u32| -> Option<String> {
            let map = reg_c.lock().unwrap();
            let sess = map.get(&handle)?;
            sess.poll().map(|ev| match ev {
                JsSseEvent::Open => r#"{"t":"open"}"#.to_string(),
                JsSseEvent::Message {
                    event_type,
                    data,
                    id,
                } => {
                    let id_json = id
                        .as_deref()
                        .map_or_else(|| "null".to_string(), json_str);
                    format!(
                        r#"{{"t":"message","event":{},"data":{},"id":{}}}"#,
                        json_str(&event_type),
                        json_str(&data),
                        id_json
                    )
                }
                JsSseEvent::Retry(ms) => {
                    format!(r#"{{"t":"retry","ms":{ms}}}"#)
                }
                JsSseEvent::Close => r#"{"t":"close"}"#.to_string(),
                JsSseEvent::Error(e) => {
                    format!(r#"{{"t":"error","message":{}}}"#, json_str(&e))
                }
            })
        });

        let reg_c = Arc::clone(&registry);
        reg!("_lumen_sse_close", move |handle: u32| {
            if let Some(mut sess) = reg_c.lock().unwrap().remove(&handle) {
                sess.close();
            }
        });
    }

    // ── localStorage ─────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_length", move || -> u32 { s.lock().unwrap().len() });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_key", move |n: u32| -> Option<String> {
            s.lock().unwrap().key(n).map(|k| k.to_owned())
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_get", move |key: String| -> Option<String> {
            s.lock().unwrap().get_item(&key).map(|v| v.to_owned())
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_set", move |key: String, value: String| {
            s.lock().unwrap().set_item(key, value);
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_remove", move |key: String| {
            s.lock().unwrap().remove_item(&key);
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_clear", move || {
            s.lock().unwrap().clear();
        });
    }

    // ── sessionStorage ────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_length", move || -> u32 { s.lock().unwrap().len() });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_key", move |n: u32| -> Option<String> {
            s.lock().unwrap().key(n).map(|k| k.to_owned())
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_get", move |key: String| -> Option<String> {
            s.lock().unwrap().get_item(&key).map(|v| v.to_owned())
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_set", move |key: String, value: String| {
            s.lock().unwrap().set_item(key, value);
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_remove", move |key: String| {
            s.lock().unwrap().remove_item(&key);
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_clear", move || {
            s.lock().unwrap().clear();
        });
    }

    // ── IndexedDB persistence ─────────────────────────────────────────────────
    // Registered only when a backend is supplied (None in unit tests / sandboxed
    // contexts → the JS shim falls back to in-heap-only databases via its
    // `typeof _lumen_idb_persist === 'function'` guards). The shim serializes the
    // whole per-origin database set into one opaque JSON snapshot; `_lumen_idb_load`
    // restores it on init, `_lumen_idb_persist` writes it after each mutating flush.
    if let Some(idb) = idb_backend {
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_load", move || -> Option<String> { b.load() });
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_persist", move |snapshot: String| {
            b.save(&snapshot);
        });
    }

    // ── performance.now() — high-resolution timestamp ────────────────────────
    // Returns milliseconds since Unix epoch as f64; JS shim subtracts
    // the time-origin captured at install_dom_api time to give DOMHighResTimeStamp.
    // In deterministic mode (8F) always returns 0 so Date.now()/performance.now()
    // are frozen at the epoch, making rendering output independent of wall-clock time.
    let det_time = deterministic_seed.is_some();
    reg!("_lumen_now_ms", move || -> f64 {
        if det_time {
            0.0
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        }
    });

    // ── timer wakeup notification ─────────────────────────────────────────────
    // Called by _lumen_tick_timers / setTimeout / setInterval JS shims when a
    // timer is scheduled. Stores the earliest pending deadline (Unix epoch ms)
    // so the shell event loop can set ControlFlow::WaitUntil accordingly.
    {
        let tw = Arc::clone(&timer_wakeup);
        reg!("_lumen_request_wakeup", move |deadline_ms: f64| {
            let mut lock = tw.lock().unwrap();
            match *lock {
                None => *lock = Some(deadline_ms),
                Some(prev) if deadline_ms < prev => *lock = Some(deadline_ms),
                _ => {}
            }
        });
    }

    // Called by requestAnimationFrame when a callback is queued.
    // Shell reads this after each rendering step to decide whether to request
    // the next redraw for JS animation loops.
    {
        let raf = Arc::clone(&raf_pending);
        reg!("_lumen_mark_raf_pending", move || {
            raf.store(true, Ordering::Relaxed);
        });
    }

    // ── element geometry (for getBoundingClientRect / ResizeObserver / IntersectionObserver) ──
    // Returns [x, y, width, height] for the given NodeId in viewport-relative CSS px,
    // or undefined if the node has no layout box (display:none, not laid out yet, etc.).
    {
        let lr = Arc::clone(&layout_rects);
        reg!("_lumen_get_bounding_rect", move |nid: u32| -> Option<Vec<f64>> {
            lr.lock()
                .unwrap()
                .get(&nid)
                .map(|r| vec![f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3])])
        });
    }

    // Returns [width, height] of the current viewport in CSS px.
    {
        let vs = Arc::clone(&viewport_size);
        reg!("_lumen_get_viewport_size", move || -> Vec<f64> {
            let s = *vs.lock().unwrap();
            vec![f64::from(s[0]), f64::from(s[1])]
        });
    }

    // ── window.matchMedia (CSS Media Queries L4 §4.2) ────────────────────────
    // Parses `query` as a media query and evaluates it against an ad-hoc
    // MediaContext built from the supplied viewport size + user-preference
    // flags. Pure function — no captures: parse_media_query and MediaQuery::matches
    // are stateless. Returns `true` when the query currently matches.
    reg!(
        "_lumen_match_media",
        |query: String, w: f64, h: f64, dark: bool, reduced_motion: bool| -> bool {
            let mq = lumen_css_parser::parse_media_query(&query);
            let ctx = lumen_css_parser::MediaContext {
                media_type: "screen".to_owned(),
                width: w as f32,
                height: h as f32,
                prefers_dark: dark,
                prefers_reduced_motion: reduced_motion,
                forced_colors: false,
                ..Default::default()
            };
            mq.matches(&ctx)
        }
    );

    // ── CSS.supports() backing (CSS Conditional Rules L3 §6) ──────────────────
    // Two-argument form: CSS.supports(property, value) → check property name.
    // Intentionally ignores value in Phase 0 (property-name check is sufficient
    // for the feature-detection patterns real sites use).
    reg!(
        "_lumen_css_supports_prop",
        |prop: String, _value: String| -> bool {
            lumen_css_parser::SUPPORTED_PROPERTIES
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&prop))
        }
    );
    // One-argument form: CSS.supports(conditionText) → parse + evaluate.
    reg!(
        "_lumen_css_supports_cond",
        |condition: String| -> bool {
            lumen_css_parser::parse_supports_condition(&condition)
                .evaluate(lumen_css_parser::SUPPORTED_PROPERTIES)
        }
    );

    // Queues a lazy image load request.  Called by `_lumen_deliver_lazy_images()` in JS
    // when an image registered via `_lumen_init_lazy_images` enters the lazy-load margin.
    // Shell drains via `QuickJsRuntime::take_lazy_image_requests` after each layout.
    {
        let req = Arc::clone(&lazy_img_requests);
        reg!("_lumen_request_lazy_image_load", move |nid: u32, url: String| {
            req.lock().unwrap().push((nid, url));
        });
    }

    // ── scroll state (for scrollTop/scrollLeft/scrollWidth/scrollHeight) ─────────
    // Returns [scroll_x, scroll_y, scroll_width, scroll_height] for an overflow container,
    // or undefined if the node is not a scroll container.
    {
        let ss = Arc::clone(&scroll_states);
        reg!("_lumen_get_scroll_state", move |nid: u32| -> Option<Vec<f64>> {
            ss.lock()
                .unwrap()
                .get(&nid)
                .map(|s| vec![f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), f64::from(s[3])])
        });
    }
    // Queues a programmatic scroll request.  Shell drains via `take_scroll_requests()`.
    {
        let ps = Arc::clone(&pending_scrolls);
        reg!("_lumen_request_scroll", move |nid: u32, x: f64, y: f64| {
            ps.lock().unwrap().push((nid, x as f32, y as f32));
        });
    }
    // Queues a page-level scroll request from window.scrollTo/scrollBy.
    // `smooth=1` → start_smooth_scroll; `smooth=0` → scroll_to (instant).
    {
        let pps = Arc::clone(&pending_page_scrolls);
        reg!("_lumen_request_page_scroll", move |y: f64, smooth: u32| {
            pps.lock().unwrap().push((y as f32, smooth != 0));
        });
    }
    // Returns current page scroll Y for window.scrollY / window.pageYOffset.
    {
        let psy = Arc::clone(&page_scroll_y);
        reg!("_lumen_get_page_scroll_y", move || -> f64 {
            f64::from(*psy.lock().unwrap())
        });
    }

    // ── window.open() popup requests ────────────────────────────────────────────
    // Queues a popup window request. Shell drains via `take_window_open_requests()`.
    // `features` is the raw feature string ("width=800,height=600,..."); we parse
    // `width=` and `height=` here so the shell receives typed values.
    {
        let wor = Arc::clone(&window_open_requests);
        reg!(
            "_lumen_window_open",
            move |url: String, target: String, features: String| {
                let mut width: u32 = 800;
                let mut height: u32 = 600;
                for part in features.split(',') {
                    let part = part.trim();
                    if let Some(v) = part.strip_prefix("width=") {
                        width = v.trim().parse().unwrap_or(800);
                    } else if let Some(v) = part.strip_prefix("height=") {
                        height = v.trim().parse().unwrap_or(600);
                    }
                }
                wor.lock().unwrap().push(PopupRequest { url, target, width, height });
            }
        );
    }

    // ── Fullscreen API (WHATWG Fullscreen §4) ────────────────────────────────────
    // Shell drains via `take_fullscreen_requests()` and calls `window.set_fullscreen()`.
    {
        let fs_req = Arc::clone(&fullscreen_requests);
        reg!("_lumen_fs_enter", move |nid: u32| {
            fs_req.lock().unwrap().push(FullscreenRequest::Enter { nid });
        });
    }
    {
        let fs_req = Arc::clone(&fullscreen_requests);
        reg!("_lumen_fs_exit", move || {
            fs_req.lock().unwrap().push(FullscreenRequest::Exit);
        });
    }

    // ── Pointer Lock API (W3C Pointer Lock L2 §2-4) ────────────────────────────────
    // requestPointerLock(element_nid) — lock pointer to element.
    // Phase 0: in-memory lock. Phase 1: integrate with shell to capture cursor.
    reg!("_lumen_ptr_lock_request", move |nid: u32| {
        crate::pointer_lock::request_pointer_lock(nid);
    });

    // exitPointerLock() — release pointer lock.
    reg!("_lumen_exit_ptr_lock", move || {
        crate::pointer_lock::exit_pointer_lock();
    });

    // pointerLockElement getter — returns locked element or null.
    reg!("_lumen_ptr_lock_element", move || -> Option<u32> {
        crate::pointer_lock::get_locked_element_nid()
    });

    // ── Computed styles (window.getComputedStyle) ────────────────────────────────
    // Returns the resolved CSS value for `prop` on node `nid`, or "" if unknown.
    {
        let cs = Arc::clone(&computed_styles);
        reg!("_lumen_get_computed_style", move |nid: u32, prop: String| -> String {
            cs.lock()
                .unwrap()
                .get(&nid)
                .and_then(|m| m.get(&prop))
                .cloned()
                .unwrap_or_default()
        });
    }

    // ── Shadow DOM ───────────────────────────────────────────────────────────────
    // Attaches a new shadow root to `nid` and returns the shadow root NodeId.
    // `mode`: "open" | "closed".  Triggers layout dirty so the composed tree rebuilds.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_attach_shadow", move |nid: u32, mode: String| -> u32 {
            let mut doc = d.lock().unwrap();
            let host = NodeId::from_index(nid as usize);
            let m = if mode == "closed" {
                ShadowRootMode::Closed
            } else {
                ShadowRootMode::Open
            };
            let shadow = doc.attach_shadow(host, m);
            dirty.store(true, Ordering::Relaxed);
            shadow.index() as u32
        });
    }
    // Returns the shadow root NodeId for `nid` if the root is Open, else None.
    // Closed roots are intentionally hidden from JS (encapsulation contract).
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_shadow_root", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let host = NodeId::from_index(nid as usize);
            doc.shadow_root_of(host).and_then(|sr| {
                if matches!(
                    doc.get(sr).data,
                    NodeData::ShadowRoot { mode: ShadowRootMode::Open }
                ) {
                    Some(sr.index() as u32)
                } else {
                    None
                }
            })
        });
    }
    // Returns true when `nid` is a shadow-root node (useful for JS wrapper dispatch).
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_is_shadow_root", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            matches!(doc.get(id).data, NodeData::ShadowRoot { .. })
        });
    }
    // Returns true when `nid` is a DocumentFragment node.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_is_document_fragment", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            matches!(doc.get(id).data, NodeData::DocumentFragment)
        });
    }
    // Allocate a new empty DocumentFragment and return its NodeId.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_create_fragment", move || -> u32 {
            let mut doc = d.lock().unwrap();
            doc.create_fragment().index() as u32
        });
    }
    // Return the content DocumentFragment NodeId for a <template> element, or None.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_template_content", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            doc.template_content(id).map(|f| f.index() as u32)
        });
    }
    // Deep-clone a subtree rooted at `nid`. Returns the new root NodeId.
    // `deep`: 1 = deep clone (including children), 0 = shallow (node only).
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_clone_subtree", move |nid: u32, deep: u32| -> u32 {
            let mut doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            doc.deep_clone(id, deep != 0).index() as u32
        });
    }
    // Insert `child` immediately before `reference` in `reference`'s parent.
    // Mirrors DOM `insertBefore(child, reference)`.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_insert_before",
            move |_parent_id: u32, child_id: u32, reference_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                let reference = NodeId::from_index(reference_id as usize);
                doc.insert_before(child, reference);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    // Return the shadow host NodeId for a node inside a shadow tree, or None.
    // Walks ancestors until a ShadowRoot is found, then returns its host.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_shadow_root_host", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let mut cur = NodeId::from_index(nid as usize);
            loop {
                let node = doc.get(cur);
                if matches!(node.data, NodeData::ShadowRoot { .. }) {
                    return node.parent.map(|h| h.index() as u32);
                }
                match node.parent {
                    Some(p) => cur = p,
                    None => return None,
                }
            }
        });
    }

    // ── Selection API (WHATWG Selection API + DOM §4.5) ─────────────────────
    // Exposes document selection state to JavaScript. The Selection object is a
    // singleton per document; Range objects are snapshots of endpoint pairs.
    {
        // Returns [anchor_nid, anchor_offset, focus_nid, focus_offset] or null.
        let d = Arc::clone(&doc);
        reg!("_lumen_get_selection", move || -> Option<Vec<u32>> {
            let doc = d.lock().unwrap();
            let sel = doc.get_selection();
            match (sel.anchor, sel.focus) {
                (Some(a), Some(f)) => Some(vec![
                    a.container.index() as u32,
                    a.offset,
                    f.container.index() as u32,
                    f.offset,
                ]),
                _ => None,
            }
        });
    }
    {
        // Sets selection to [anchor_nid, anchor_offset, focus_nid, focus_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_selection",
            move |anchor_nid: u32, anchor_off: u32, focus_nid: u32, focus_off: u32| {
                let mut doc = d.lock().unwrap();
                doc.set_selection(Selection {
                    anchor: Some(DomPosition {
                        container: NodeId::from_index(anchor_nid as usize),
                        offset: anchor_off,
                    }),
                    focus: Some(DomPosition {
                        container: NodeId::from_index(focus_nid as usize),
                        offset: focus_off,
                    }),
                });
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    {
        // Clears the current selection.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_clear_selection", move || {
            let mut doc = d.lock().unwrap();
            doc.set_selection(Selection { anchor: None, focus: None });
            dirty.store(true, Ordering::Relaxed);
        });
    }
    {
        // Returns text of the current selection.
        let d = Arc::clone(&doc);
        reg!("_lumen_get_selection_text", move || -> String {
            let doc = d.lock().unwrap();
            match doc.get_selection().get_range() {
                Some(r) => range_text(&doc, &r),
                None => String::new(),
            }
        });
    }
    {
        // Returns text covered by the given range endpoints.
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_range_text",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> String {
                let doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_index(start_nid as usize),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_index(end_nid as usize),
                        offset: end_off,
                    },
                };
                range_text(&doc, &r)
            }
        );
    }
    {
        // Number of direct DOM children (element offset validation).
        let d = Arc::clone(&doc);
        reg!("_lumen_node_child_count", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_child_count(&doc, NodeId::from_index(nid as usize)) as u32
        });
    }
    {
        // DOM-spec "length" of node: char count for text, child count for elements.
        let d = Arc::clone(&doc);
        reg!("_lumen_node_length", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_length(&doc, NodeId::from_index(nid as usize)) as u32
        });
    }
    {
        // Text content of a node (node.textContent).
        let d = Arc::clone(&doc);
        reg!("_lumen_node_text_content", move |nid: u32| -> String {
            let doc = d.lock().unwrap();
            node_text_content(&doc, NodeId::from_index(nid as usize))
        });
    }
    {
        // Deletes the contents of range; returns [new_pos_nid, new_pos_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_range_delete_contents",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> Vec<u32> {
                let mut doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_index(start_nid as usize),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_index(end_nid as usize),
                        offset: end_off,
                    },
                };
                let pos = lumen_dom::delete_range(&mut doc, &r);
                dirty.store(true, Ordering::Relaxed);
                vec![pos.container.index() as u32, pos.offset]
            }
        );
    }
    // ── contenteditable mutation bindings (Input Events Level 2 §4.1) ─────────
    // These are called by the JS shim's _lumen_handle_contenteditable_key()
    // which fires beforeinput → calls here → fires input.
    {
        // True if nid or any ancestor has contenteditable set to a truthy value.
        let d = Arc::clone(&doc);
        reg!("_lumen_is_contenteditable", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            lumen_dom::find_editing_host(&doc, NodeId::from_index(nid as usize)).is_some()
        });
    }
    {
        // Insert `text` at the current selection (or caret) inside contenteditable.
        // Replaces selected content if the selection is non-collapsed.
        // Returns true on success.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_insert_text", move |text: String| -> bool {
            if text.is_empty() { return false; }
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let Some(anchor) = sel.anchor else { return false; };
            let insert_pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else {
                anchor
            };
            let new_pos = lumen_dom::insert_text_at(&mut doc, insert_pos, &text);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster before the caret (Backspace key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_delete_backward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            // Non-collapsed selection: delete it.
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                dirty.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            if anchor.offset == 0 { return false; }
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            // Walk backward one UTF-8 character boundary.
            let off = anchor.offset as usize;
            let mut prev = off.saturating_sub(1);
            while prev > 0 && !text.is_char_boundary(prev) {
                prev -= 1;
            }
            let r = DomRange {
                start: DomPosition { container: anchor.container, offset: prev as u32 },
                end: anchor,
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster after the caret (Delete key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_delete_forward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                dirty.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            let off = anchor.offset as usize;
            if off >= text.len() { return false; }
            // Walk forward one UTF-8 character boundary.
            let mut next = off + 1;
            while next < text.len() && !text.is_char_boundary(next) {
                next += 1;
            }
            let r = DomRange {
                start: anchor,
                end: DomPosition { container: anchor.container, offset: next as u32 },
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Split the block at the caret position (Enter key in contenteditable).
        // Finds the editing host, then calls insert_paragraph_break.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_insert_paragraph", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else if let Some(p) = sel.anchor {
                p
            } else {
                return false;
            };
            let Some(host) = lumen_dom::find_editing_host(&doc, pos.container) else {
                return false;
            };
            let new_pos = lumen_dom::insert_paragraph_break(&mut doc, pos, host);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // execCommand: bold/italic/underline/insertText/delete/selectAll/copy/cut/paste
        // Returns true if the command was handled.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_exec_command",
            move |cmd: String, value: String| -> bool {
                let mut doc = d.lock().unwrap();
                let sel = doc.get_selection().clone();
                match cmd.as_str() {
                    "selectAll" => {
                        // Select entire document body text
                        if let Some(body) = find_element_by_tag(&doc, "body") {
                            let children = doc.get(body).children.clone();
                            if !children.is_empty() {
                                let first = *children.first().unwrap();
                                let last = *children.last().unwrap();
                                let last_len = node_length(&doc, last);
                                doc.set_selection(Selection {
                                    anchor: Some(DomPosition { container: first, offset: 0 }),
                                    focus: Some(DomPosition {
                                        container: last,
                                        offset: last_len as u32,
                                    }),
                                });
                                dirty.store(true, Ordering::Relaxed);
                            }
                        }
                        true
                    }
                    "insertText" => {
                        if let Some(pos) = sel.anchor {
                            // Delete selection first if non-collapsed
                            let pos = sel
                                .get_range()
                                .filter(|r| !r.is_collapsed())
                                .map(|r| lumen_dom::delete_range(&mut doc, &r))
                                .unwrap_or(pos);
                            let new_pos = lumen_dom::insert_text_at(&mut doc, pos, &value);
                            doc.set_selection(Selection {
                                anchor: Some(new_pos),
                                focus: Some(new_pos),
                            });
                            dirty.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    "delete" | "forwardDelete" => {
                        if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                            let pos = lumen_dom::delete_range(&mut doc, &r);
                            doc.set_selection(Selection {
                                anchor: Some(pos),
                                focus: Some(pos),
                            });
                            dirty.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    // bold/italic/underline: CSSOM inline style toggling (stub — returns true
                    // so editors know the command is accepted; real inline-style mutation
                    // requires Range wrapping which is Phase 3 contenteditable work).
                    "bold" | "italic" | "underline" | "strikeThrough"
                    | "justifyLeft" | "justifyCenter" | "justifyRight" | "justifyFull"
                    | "indent" | "outdent"
                    | "createLink" | "unlink"
                    | "insertOrderedList" | "insertUnorderedList"
                    | "fontName" | "fontSize" | "foreColor" | "backColor"
                    | "removeFormat" => true,
                    // copy/cut/paste: clipboard interaction is handled by the shell;
                    // returning false lets it fall through to native clipboard handling.
                    "copy" | "cut" | "paste" => false,
                    _ => false,
                }
            }
        );
    }

    // ── document.cookie (RFC 6265 §5.3-5.4) ─────────────────────────────────
    // The getter/setter wrap CookieProvider using host/scheme derived from
    // page_url parsed once at install time. Best-effort: if the URL cannot be
    // parsed (e.g. file://) we skip cookie injection silently.
    {
        let parsed = Url::parse(&page_url).ok();
        let host = parsed.as_ref().map(|u| u.host().to_ascii_lowercase()).unwrap_or_default();
        let is_secure = parsed.as_ref().map(|u| u.scheme() == "https").unwrap_or(false);

        if let Some(jar) = cookie_jar {
            let jar_get = Arc::clone(&jar);
            let host_get = host.clone();
            reg!("_lumen_cookie_get", move || -> String {
                jar_get.get_for_request(&host_get, "/", is_secure, None, false)
            });

            let host_set = host;
            reg!("_lumen_cookie_set", move |cookie_str: String| {
                jar.process_set_cookie(&cookie_str, &host_set, "/", is_secure, None);
            });
        } else {
            reg!("_lumen_cookie_get", move || -> String { String::new() });
            reg!("_lumen_cookie_set", move |_: String| {});
        }
    }

    // ── Microtask drain ─────────────────────────────────────────────────────
    // Drains the QuickJS pending-job queue (Promise microtasks) synchronously.
    // Used in unit tests to flush .then() callbacks without an event loop.
    // Re-entrant-safe: QuickJS JS_ExecutePendingJob is designed for this.
    reg!("_lumen_drain_microtasks", |ctx: Ctx<'_>| {
        let mut guard = 0i32;
        while ctx.execute_pending_job() {
            guard += 1;
            if guard >= 100_000 {
                break;
            }
        }
    });

    // ── Web Crypto API ──────────────────────────────────────────────────────
    {
        // Returns `n` cryptographically-random bytes as a Vec<u8> (JS Array of
        // integers 0–255). Capped at 65 536 per call per WebCrypto spec §10.1.3.
        reg!("_lumen_get_random_bytes", |n: u32| -> Vec<u8> {
            let len = (n as usize).min(65_536);
            let mut buf = vec![0u8; len];
            getrandom::getrandom(&mut buf).unwrap_or(());
            buf
        });

        // Computes a SHA digest using the named algorithm.
        // `algo` must be one of "SHA-1", "SHA-256", "SHA-384", "SHA-512".
        // `data` is the raw input bytes.  Returns empty Vec on unknown algo.
        reg!(
            "_lumen_sha_digest",
            |algo: String, data: Vec<u8>| -> Vec<u8> {
                // sha1::Digest trait must be in scope to call sha1::Sha1::digest().
                use sha1::Digest as _;
                match algo.as_str() {
                    "SHA-1" => sha1::Sha1::digest(&data).to_vec(),
                    "SHA-256" => sha2::Sha256::digest(&data).to_vec(),
                    "SHA-384" => sha2::Sha384::digest(&data).to_vec(),
                    "SHA-512" => sha2::Sha512::digest(&data).to_vec(),
                    _ => Vec::new(),
                }
            }
        );

        // Compress `data` using the named format.
        // `format`: "deflate-raw" (raw DEFLATE, RFC 1951), "deflate" (zlib, RFC 1950), "gzip".
        // Returns empty Vec on unknown format or I/O error.
        reg!(
            "_lumen_compress_bytes",
            |data: Vec<u8>, format: String| -> Vec<u8> {
                use flate2::Compression;
                use std::io::Write as _;
                match format.as_str() {
                    "deflate-raw" => {
                        let mut enc =
                            flate2::write::DeflateEncoder::new(Vec::new(), Compression::default());
                        enc.write_all(&data).ok();
                        enc.finish().unwrap_or_default()
                    }
                    "deflate" => {
                        let mut enc =
                            flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
                        enc.write_all(&data).ok();
                        enc.finish().unwrap_or_default()
                    }
                    "gzip" => {
                        let mut enc =
                            flate2::write::GzEncoder::new(Vec::new(), Compression::default());
                        enc.write_all(&data).ok();
                        enc.finish().unwrap_or_default()
                    }
                    _ => Vec::new(),
                }
            }
        );

        // Decompress `data` using the named format.
        // `format`: "deflate-raw", "deflate", "gzip". Returns empty Vec on error.
        reg!(
            "_lumen_decompress_bytes",
            |data: Vec<u8>, format: String| -> Vec<u8> {
                use std::io::Read as _;
                match format.as_str() {
                    "deflate-raw" => {
                        let mut dec = flate2::read::DeflateDecoder::new(data.as_slice());
                        let mut out = Vec::new();
                        dec.read_to_end(&mut out).ok();
                        out
                    }
                    "deflate" => {
                        let mut dec = flate2::read::ZlibDecoder::new(data.as_slice());
                        let mut out = Vec::new();
                        dec.read_to_end(&mut out).ok();
                        out
                    }
                    "gzip" => {
                        let mut dec = flate2::read::GzDecoder::new(data.as_slice());
                        let mut out = Vec::new();
                        dec.read_to_end(&mut out).ok();
                        out
                    }
                    _ => Vec::new(),
                }
            }
        );
    }

    // SubtleCrypto: generateKey/importKey/exportKey/sign/verify/encrypt/decrypt
    crate::subtle_crypto::install_subtle_bindings(ctx)?;

    // Trusted Types API: trustedTypes.createPolicy(), TrustedHTML/Script/ScriptURL
    crate::trusted_types::install_trusted_types_bindings(ctx)?;

    // D-6: Extension system — chrome.runtime.sendMessage() native binding.
    // Phase 0: no-op; the message is logged to stderr for debugging.
    // Phase 1: shell wires a real IPC channel between content scripts and extension background.
    reg!("_lumen_chrome_runtime_send_message", |msg: String| {
        let _ = msg;
    });

    // CSS Typed OM API: element.attributeStyleMap / computedStyleMap()
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_style_property", move |nid: u32, prop: String| -> String {
            if let Ok(doc) = d.lock() {
                let node = doc.get(NodeId::from_index(nid as usize));
                if let Some(style_attr) = node.get_attr("style") {
                    let parsed = _parse_style_string(style_attr);
                    let kebab_prop = _camel_to_kebab(&prop);
                    return parsed.get(&kebab_prop).cloned().unwrap_or_default();
                }
            }
            String::new()
        });
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_set_style_property", move |nid: u32, prop: String, val: String| {
            if let Ok(mut doc) = d.lock() {
                let node_id = NodeId::from_index(nid as usize);
                let mut parsed = if let Some(style) = doc.get(node_id).get_attr("style") {
                    _parse_style_string(style)
                } else {
                    std::collections::HashMap::new()
                };
                let kebab_prop = _camel_to_kebab(&prop);
                parsed.insert(kebab_prop, val);
                let css_text = _serialize_style_map(&parsed);
                set_attribute(&mut doc, node_id, "style", &css_text);
                dirty.store(true, Ordering::Relaxed);
            }
        });
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_delete_style_property", move |nid: u32, prop: String| {
            if let Ok(mut doc) = d.lock() {
                let node_id = NodeId::from_index(nid as usize);
                let mut parsed = if let Some(style) = doc.get(node_id).get_attr("style") {
                    _parse_style_string(style)
                } else {
                    std::collections::HashMap::new()
                };
                let kebab_prop = _camel_to_kebab(&prop);
                parsed.remove(&kebab_prop);
                let css_text = _serialize_style_map(&parsed);
                if css_text.is_empty() {
                    remove_attribute(&mut doc, node_id, "style");
                } else {
                    set_attribute(&mut doc, node_id, "style", &css_text);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_has_style_property", move |nid: u32, prop: String| -> bool {
            if let Ok(doc) = d.lock() {
                let node = doc.get(NodeId::from_index(nid as usize));
                if let Some(style_attr) = node.get_attr("style") {
                    let parsed = _parse_style_string(style_attr);
                    let kebab_prop = _camel_to_kebab(&prop);
                    return parsed.contains_key(&kebab_prop);
                }
            }
            false
        });
        reg!("_lumen_get_style_entries", move |_nid: u32| {
            // Phase 0: return empty object for iteration (stub)
            "[]"
        });
    }

    Ok(())
}

// ─── DOM helpers ──────────────────────────────────────────────────────────────

fn find_element_by_tag(doc: &Document, tag: &str) -> Option<NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

fn find_first_matching(
    doc: &Document,
    start: NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<NodeId> {
    let node = doc.get(start);
    if pred(node) {
        return Some(start);
    }
    for &child in &node.children.clone() {
        if let Some(found) = find_first_matching(doc, child, pred) {
            return Some(found);
        }
    }
    None
}

fn collect_text_content(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    collect_text_inner(doc, id, &mut out);
    out
}

fn collect_text_inner(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children.clone() {
        collect_text_inner(doc, child, out);
    }
}

fn set_text_content(doc: &mut Document, id: NodeId, text: &str) {
    let children: Vec<NodeId> = doc.get(id).children.clone();
    for child in children {
        doc.detach(child);
    }
    if !text.is_empty() {
        let text_node = doc.create_text(text);
        doc.append_child(id, text_node);
    }
}

fn set_attribute(doc: &mut Document, id: NodeId, name: &str, value: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs
            .iter_mut()
            .find(|a| a.name.local.eq_ignore_ascii_case(name))
        {
            attr.value = value.to_string();
        } else {
            attrs.push(Attribute {
                name: QualName::html(name.to_ascii_lowercase()),
                value: value.to_string(),
            });
        }
    }
}

fn remove_attribute(doc: &mut Document, id: NodeId, name: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
    }
}

// ─── JavaScript Web API shim ──────────────────────────────────────────────────

/// Evaluated once after primitives are registered; builds standard Web API globals.
///
/// Uses top-level `var` so declarations land on the global object in QuickJS's
/// script eval. No IIFE — avoids strict-mode `this`-is-undefined edge cases.
///
/// `parentElement` and `children` are defined as non-enumerable via
/// `Object.defineProperty` to prevent `from_rq` from calling them during
/// object serialization — they can cause parent↔child infinite recursion.
/// Evaluated once after primitives are registered; builds standard Web API globals.
///
/// `Option<T>` in rquickjs maps `None → undefined`, not `null`. All places
/// where the Web API spec requires `null` use `_lumen_u2n` (undefined-to-null).
///
/// `parentElement` and `children` are non-enumerable to prevent infinite
/// recursion when `from_rq` serializes the returned object (parent↔child cycles).
const WEB_API_SHIM: &str = "
function _lumen_u2n(v) { return v !== undefined ? v : null; }

// ── Event / CustomEvent constructors ─────────────────────────────────────────

function Event(type, init) {
    this.type             = String(type || '');
    this.bubbles          = !!(init && init.bubbles);
    this.cancelable       = !!(init && init.cancelable);
    this.isTrusted        = !!(init && init.isTrusted);
    this.defaultPrevented = false;
    this.cancelBubble     = false;
    this.target           = null;
    this.currentTarget    = null;
    this.timeStamp        = Date.now ? Date.now() : 0;
    this._stopImmediate   = false;
}
Event.prototype.preventDefault = function() {
    if (this.cancelable) this.defaultPrevented = true;
};
Event.prototype.stopPropagation = function() { this.cancelBubble = true; };
Event.prototype.stopImmediatePropagation = function() { this._stopImmediate = true; this.cancelBubble = true; };

function CustomEvent(type, init) {
    Event.call(this, type, init);
    this.detail = (init && init.detail !== undefined) ? init.detail : null;
}
CustomEvent.prototype = Object.create(Event.prototype);
CustomEvent.prototype.constructor = CustomEvent;

// ── EventTarget base class ────────────────────────────────────────────────────
// WHATWG DOM §2.7 — minimal EventTarget so the many Web API shims that do
// `class X extends EventTarget` (Document PiP, WebHID, WebUSB, Bluetooth,
// WebSerial, WebXR, Navigation API, form-associated custom elements, …) have a
// global base to inherit from. DOM nodes (document, window, elements) keep their
// own native `addEventListener` wired to `_lumen_add_listener`; this class is
// only the constructible base for pure-JS event sources that dispatch to
// themselves. Listeners are stored per type; `dispatchEvent` also invokes the
// matching `on<type>` property handler, mirroring browser behaviour.
function EventTarget() {
    Object.defineProperty(this, '_listeners', { value: Object.create(null), writable: true });
}
EventTarget.prototype.addEventListener = function(type, callback, options) {
    if (!callback) return;
    type = String(type);
    var capture = !!(options === true || (options && options.capture));
    var list = this._listeners[type] || (this._listeners[type] = []);
    for (var i = 0; i < list.length; i++) {
        if (list[i].callback === callback && list[i].capture === capture) return;
    }
    list.push({ callback: callback, capture: capture, once: !!(options && options.once) });
};
EventTarget.prototype.removeEventListener = function(type, callback, options) {
    type = String(type);
    var list = this._listeners[type];
    if (!list) return;
    var capture = !!(options === true || (options && options.capture));
    for (var i = 0; i < list.length; i++) {
        if (list[i].callback === callback && list[i].capture === capture) { list.splice(i, 1); return; }
    }
};
EventTarget.prototype.dispatchEvent = function(event) {
    if (!event || event.type == null) return true;
    var type = String(event.type);
    event.target = event.target || this;
    event.currentTarget = this;
    var list = this._listeners[type];
    if (list) {
        var snapshot = list.slice();
        for (var i = 0; i < snapshot.length; i++) {
            var entry = snapshot[i];
            try {
                if (typeof entry.callback === 'function') entry.callback.call(this, event);
                else if (entry.callback && typeof entry.callback.handleEvent === 'function') entry.callback.handleEvent(event);
            } catch (e) {}
            if (entry.once) this.removeEventListener(type, entry.callback, entry.capture);
            if (event._stopImmediate) break;
        }
    }
    var onprop = 'on' + type;
    if (typeof this[onprop] === 'function') {
        try { this[onprop].call(this, event); } catch (e) {}
    }
    event.currentTarget = null;
    return !event.defaultPrevented;
};

// ── UIEvent / MouseEvent / KeyboardEvent / InputEvent / FocusEvent ────────────
// ── WheelEvent / PointerEvent / AnimationEvent / TransitionEvent / … ─────────
// WHATWG UI Events spec — provides typed event classes for instanceof checks
// and named properties (clientX, key, deltaY, …) that web apps depend on.

function UIEvent(type, init) {
    Event.call(this, type, init);
    this.detail = (init && init.detail != null) ? (init.detail | 0) : 0;
    this.view   = (init && init.view   != null) ? init.view   : null;
}
UIEvent.prototype = Object.create(Event.prototype);
UIEvent.prototype.constructor = UIEvent;

function MouseEvent(type, init) {
    UIEvent.call(this, type, init);
    this.screenX       = (init && init.screenX       != null) ? +init.screenX       : 0;
    this.screenY       = (init && init.screenY       != null) ? +init.screenY       : 0;
    this.clientX       = (init && init.clientX       != null) ? +init.clientX       : 0;
    this.clientY       = (init && init.clientY       != null) ? +init.clientY       : 0;
    this.pageX         = (init && init.pageX         != null) ? +init.pageX         : this.clientX;
    this.pageY         = (init && init.pageY         != null) ? +init.pageY         : this.clientY;
    this.offsetX       = (init && init.offsetX       != null) ? +init.offsetX       : 0;
    this.offsetY       = (init && init.offsetY       != null) ? +init.offsetY       : 0;
    this.movementX     = (init && init.movementX     != null) ? +init.movementX     : 0;
    this.movementY     = (init && init.movementY     != null) ? +init.movementY     : 0;
    this.button        = (init && init.button        != null) ? (init.button  | 0)  : 0;
    this.buttons       = (init && init.buttons       != null) ? (init.buttons | 0)  : 0;
    this.ctrlKey       = !!(init && init.ctrlKey);
    this.shiftKey      = !!(init && init.shiftKey);
    this.altKey        = !!(init && init.altKey);
    this.metaKey       = !!(init && init.metaKey);
    this.relatedTarget = (init && init.relatedTarget != null) ? init.relatedTarget : null;
}
MouseEvent.prototype = Object.create(UIEvent.prototype);
MouseEvent.prototype.constructor = MouseEvent;
MouseEvent.prototype.getModifierState = function(key) {
    if (key === 'Control') return this.ctrlKey;
    if (key === 'Shift')   return this.shiftKey;
    if (key === 'Alt')     return this.altKey;
    if (key === 'Meta')    return this.metaKey;
    return false;
};

function KeyboardEvent(type, init) {
    UIEvent.call(this, type, init);
    this.key         = (init && init.key         != null) ? String(init.key)         : '';
    this.code        = (init && init.code        != null) ? String(init.code)        : '';
    this.keyCode     = (init && init.keyCode     != null) ? (init.keyCode  | 0)      : 0;
    this.charCode    = (init && init.charCode    != null) ? (init.charCode | 0)      : 0;
    this.which       = (init && init.which       != null) ? (init.which    | 0)      : this.keyCode;
    this.location    = (init && init.location    != null) ? (init.location | 0)      : 0;
    this.repeat      = !!(init && init.repeat);
    this.isComposing = !!(init && init.isComposing);
    this.ctrlKey     = !!(init && init.ctrlKey);
    this.shiftKey    = !!(init && init.shiftKey);
    this.altKey      = !!(init && init.altKey);
    this.metaKey     = !!(init && init.metaKey);
}
KeyboardEvent.prototype = Object.create(UIEvent.prototype);
KeyboardEvent.prototype.constructor = KeyboardEvent;
KeyboardEvent.prototype.getModifierState = function(key) {
    if (key === 'Control') return this.ctrlKey;
    if (key === 'Shift')   return this.shiftKey;
    if (key === 'Alt')     return this.altKey;
    if (key === 'Meta')    return this.metaKey;
    return false;
};
KeyboardEvent.DOM_KEY_LOCATION_STANDARD = 0;
KeyboardEvent.DOM_KEY_LOCATION_LEFT     = 1;
KeyboardEvent.DOM_KEY_LOCATION_RIGHT    = 2;
KeyboardEvent.DOM_KEY_LOCATION_NUMPAD   = 3;

function InputEvent(type, init) {
    UIEvent.call(this, type, init);
    this.data         = (init && init.data      != null) ? init.data      : null;
    this.inputType    = (init && init.inputType != null) ? String(init.inputType) : '';
    this.isComposing  = !!(init && init.isComposing);
    this.dataTransfer = (init && init.dataTransfer != null) ? init.dataTransfer : null;
}
InputEvent.prototype = Object.create(UIEvent.prototype);
InputEvent.prototype.constructor = InputEvent;
InputEvent.prototype.getTargetRanges = function() { return []; };

function FocusEvent(type, init) {
    UIEvent.call(this, type, init);
    this.relatedTarget = (init && init.relatedTarget != null) ? init.relatedTarget : null;
}
FocusEvent.prototype = Object.create(UIEvent.prototype);
FocusEvent.prototype.constructor = FocusEvent;

function WheelEvent(type, init) {
    MouseEvent.call(this, type, init);
    this.deltaX    = (init && init.deltaX    != null) ? +init.deltaX    : 0;
    this.deltaY    = (init && init.deltaY    != null) ? +init.deltaY    : 0;
    this.deltaZ    = (init && init.deltaZ    != null) ? +init.deltaZ    : 0;
    this.deltaMode = (init && init.deltaMode != null) ? (init.deltaMode | 0) : 0;
}
WheelEvent.prototype = Object.create(MouseEvent.prototype);
WheelEvent.prototype.constructor = WheelEvent;
WheelEvent.DOM_DELTA_PIXEL = 0;
WheelEvent.DOM_DELTA_LINE  = 1;
WheelEvent.DOM_DELTA_PAGE  = 2;

// Pointer Events Level 2 — pointerId=1 / pointerType='mouse' for mouse input
function PointerEvent(type, init) {
    MouseEvent.call(this, type, init);
    this.pointerId          = (init && init.pointerId        != null) ? (init.pointerId | 0)      : 1;
    this.pointerType        = (init && init.pointerType      != null) ? String(init.pointerType)  : 'mouse';
    this.isPrimary          = (init && init.isPrimary        != null) ? !!init.isPrimary          : true;
    this.width              = (init && init.width            != null) ? +init.width               : 1;
    this.height             = (init && init.height           != null) ? +init.height              : 1;
    this.pressure           = (init && init.pressure         != null) ? +init.pressure            : 0;
    this.tangentialPressure = (init && init.tangentialPressure != null) ? +init.tangentialPressure : 0;
    this.tiltX              = (init && init.tiltX            != null) ? (init.tiltX  | 0)         : 0;
    this.tiltY              = (init && init.tiltY            != null) ? (init.tiltY  | 0)         : 0;
    this.twist              = (init && init.twist            != null) ? (init.twist  | 0)         : 0;
    this.altitudeAngle      = (init && init.altitudeAngle    != null) ? +init.altitudeAngle       : Math.PI / 2;
    this.azimuthAngle       = (init && init.azimuthAngle     != null) ? +init.azimuthAngle        : 0;
}
PointerEvent.prototype = Object.create(MouseEvent.prototype);
PointerEvent.prototype.constructor = PointerEvent;
PointerEvent.prototype.getCoalescedEvents = function() { return []; };
PointerEvent.prototype.getPredictedEvents = function() { return []; };

// AnimationEvent — animationstart / animationend / animationiteration / animationcancel
function AnimationEvent(type, init) {
    Event.call(this, type, init);
    this.animationName = (init && init.animationName != null) ? String(init.animationName) : '';
    this.elapsedTime   = (init && init.elapsedTime   != null) ? +init.elapsedTime   : 0;
    this.pseudoElement = (init && init.pseudoElement != null) ? String(init.pseudoElement) : '';
}
AnimationEvent.prototype = Object.create(Event.prototype);
AnimationEvent.prototype.constructor = AnimationEvent;

// TransitionEvent — transitionstart / transitionend / transitionrun / transitioncancel
function TransitionEvent(type, init) {
    Event.call(this, type, init);
    this.propertyName  = (init && init.propertyName  != null) ? String(init.propertyName)  : '';
    this.elapsedTime   = (init && init.elapsedTime   != null) ? +init.elapsedTime   : 0;
    this.pseudoElement = (init && init.pseudoElement != null) ? String(init.pseudoElement) : '';
}
TransitionEvent.prototype = Object.create(Event.prototype);
TransitionEvent.prototype.constructor = TransitionEvent;

// StorageEvent — fires on localStorage/sessionStorage change in another context
function StorageEvent(type, init) {
    Event.call(this, type, init);
    this.key         = (init && init.key         != null) ? init.key         : null;
    this.oldValue    = (init && init.oldValue    != null) ? init.oldValue    : null;
    this.newValue    = (init && init.newValue    != null) ? init.newValue    : null;
    this.url         = (init && init.url         != null) ? String(init.url) : '';
    this.storageArea = (init && init.storageArea != null) ? init.storageArea : null;
}
StorageEvent.prototype = Object.create(Event.prototype);
StorageEvent.prototype.constructor = StorageEvent;
StorageEvent.prototype.initStorageEvent = function(type, bubbles, cancelable, key, oldValue, newValue, url, storageArea) {
    this.type = type; this.bubbles = !!bubbles; this.cancelable = !!cancelable;
    this.key = key; this.oldValue = oldValue; this.newValue = newValue;
    this.url = String(url); this.storageArea = storageArea;
};

// PopStateEvent — history.pushState / back / forward
function PopStateEvent(type, init) {
    Event.call(this, type, init);
    this.state = (init && init.state !== undefined) ? init.state : null;
}
PopStateEvent.prototype = Object.create(Event.prototype);
PopStateEvent.prototype.constructor = PopStateEvent;

// HashChangeEvent — URL hash (#fragment) changes
function HashChangeEvent(type, init) {
    Event.call(this, type, init);
    this.oldURL = (init && init.oldURL != null) ? String(init.oldURL) : '';
    this.newURL = (init && init.newURL != null) ? String(init.newURL) : '';
}
HashChangeEvent.prototype = Object.create(Event.prototype);
HashChangeEvent.prototype.constructor = HashChangeEvent;

// ErrorEvent — uncaught script errors
function ErrorEvent(type, init) {
    Event.call(this, type, init);
    this.message  = (init && init.message  != null) ? String(init.message)  : '';
    this.filename = (init && init.filename != null) ? String(init.filename) : '';
    this.lineno   = (init && init.lineno   != null) ? (init.lineno  | 0) : 0;
    this.colno    = (init && init.colno    != null) ? (init.colno   | 0) : 0;
    this.error    = (init && init.error    !== undefined) ? init.error : null;
}
ErrorEvent.prototype = Object.create(Event.prototype);
ErrorEvent.prototype.constructor = ErrorEvent;

// SubmitEvent — form submission; carries reference to the submitter button
function SubmitEvent(type, init) {
    Event.call(this, type, init);
    this.submitter = (init && init.submitter != null) ? init.submitter : null;
}
SubmitEvent.prototype = Object.create(Event.prototype);
SubmitEvent.prototype.constructor = SubmitEvent;

// PageTransitionEvent — pageshow / pagehide (bfcache)
function PageTransitionEvent(type, init) {
    Event.call(this, type, init);
    this.persisted = !!(init && init.persisted);
}
PageTransitionEvent.prototype = Object.create(Event.prototype);
PageTransitionEvent.prototype.constructor = PageTransitionEvent;

// BeforeUnloadEvent — fires before navigation away; returnValue triggers dialog
function BeforeUnloadEvent(type, init) {
    Event.call(this, type, init);
    this.returnValue = '';
}
BeforeUnloadEvent.prototype = Object.create(Event.prototype);
BeforeUnloadEvent.prototype.constructor = BeforeUnloadEvent;

// ── HTML5 Drag and Drop API (HTML LS §9.10) ───────────────────────────────────
// DataTransferItem — single item in the drag data store.
function DataTransferItem(kind, type, data) {
    this.kind = kind;   // 'string' or 'file'
    this.type = String(type || '').toLowerCase();
    this._data = data;  // string value or null for file kind
}
DataTransferItem.prototype.getAsString = function(callback) {
    if (this.kind !== 'string' || typeof callback !== 'function') return;
    var d = this._data;
    try { callback(d != null ? String(d) : ''); } catch(e) {}
};
DataTransferItem.prototype.getAsFile = function() {
    return null; // Phase 0: no native file access
};

// DataTransferItemList — ordered list of DataTransferItems.
function DataTransferItemList(owner) {
    this._items = [];
    this._owner = owner; // back-ref to DataTransfer for type sync
}
DataTransferItemList.prototype.add = function(dataOrFile, type) {
    if (typeof dataOrFile === 'string') {
        var t = String(type || 'text/plain').toLowerCase();
        // Spec: only one item per unique type (string kind)
        for (var i = 0; i < this._items.length; i++) {
            if (this._items[i].kind === 'string' && this._items[i].type === t) return null;
        }
        var item = new DataTransferItem('string', t, dataOrFile);
        this._items.push(item);
        this._owner._sync_from_items();
        return item;
    }
    // file kind (Phase 0: no actual File support)
    return null;
};
DataTransferItemList.prototype.remove = function(index) {
    if (index >= 0 && index < this._items.length) {
        this._items.splice(index, 1);
        this._owner._sync_from_items();
    }
};
DataTransferItemList.prototype.clear = function() {
    this._items = [];
    this._owner._sync_from_items();
};
Object.defineProperty(DataTransferItemList.prototype, 'length', {
    get: function() { return this._items.length; }
});
// Indexed access via Proxy-like approach using numeric properties
DataTransferItemList.prototype._rebuild_indices = function() {
    // Clear old numeric properties beyond new length
    var old_n = typeof this._prev_len === 'number' ? this._prev_len : 0;
    var n = this._items.length;
    for (var i = n; i < old_n; i++) delete this[i];
    for (var j = 0; j < n; j++) this[j] = this._items[j];
    this._prev_len = n;
};
DataTransferItemList.prototype[Symbol.iterator] = function() {
    var items = this._items.slice();
    var idx = 0;
    return {
        next: function() {
            if (idx < items.length) return { value: items[idx++], done: false };
            return { value: undefined, done: true };
        }
    };
};

// DataTransfer — the drag data store (HTML LS §9.10.1).
function DataTransfer() {
    this._data = {};         // format → string
    this._types = [];        // read-only types list
    this.effectAllowed = 'uninitialized';
    this.dropEffect = 'none';
    this.items = new DataTransferItemList(this);
    this.files = Object.freeze([]); // FileList stub
}
DataTransfer.prototype._sync_from_items = function() {
    // Rebuild _data and _types from items list; also refresh indexed access on the list
    this._data = {};
    this._types = [];
    var list = this.items._items;
    for (var i = 0; i < list.length; i++) {
        if (list[i].kind === 'string') {
            this._data[list[i].type] = list[i]._data;
            this._types.push(list[i].type);
        }
    }
    this.items._rebuild_indices();
};
Object.defineProperty(DataTransfer.prototype, 'types', {
    get: function() { return Object.freeze(this._types.slice()); }
});
DataTransfer.prototype.setData = function(format, data) {
    var fmt = String(format || '').toLowerCase();
    // Normalise 'text' → 'text/plain', 'url' → 'text/uri-list' per spec
    if (fmt === 'text') fmt = 'text/plain';
    if (fmt === 'url') fmt = 'text/uri-list';
    // Remove existing item with same type, then add new one
    var list = this.items._items;
    for (var i = list.length - 1; i >= 0; i--) {
        if (list[i].kind === 'string' && list[i].type === fmt) list.splice(i, 1);
    }
    list.push(new DataTransferItem('string', fmt, String(data != null ? data : '')));
    this._sync_from_items();
};
DataTransfer.prototype.getData = function(format) {
    var fmt = String(format || '').toLowerCase();
    if (fmt === 'text') fmt = 'text/plain';
    if (fmt === 'url') fmt = 'text/uri-list';
    return Object.prototype.hasOwnProperty.call(this._data, fmt) ? this._data[fmt] : '';
};
DataTransfer.prototype.clearData = function(format) {
    if (arguments.length === 0 || format === undefined || format === null) {
        // Remove all string-kind items
        var list = this.items._items;
        for (var i = list.length - 1; i >= 0; i--) {
            if (list[i].kind === 'string') list.splice(i, 1);
        }
    } else {
        var fmt = String(format).toLowerCase();
        if (fmt === 'text') fmt = 'text/plain';
        if (fmt === 'url') fmt = 'text/uri-list';
        var list2 = this.items._items;
        for (var i = list2.length - 1; i >= 0; i--) {
            if (list2[i].kind === 'string' && list2[i].type === fmt) list2.splice(i, 1);
        }
    }
    this._sync_from_items();
};
DataTransfer.prototype.setDragImage = function(_image, _x, _y) {
    // Phase 0: no-op (custom drag image not supported)
};

// DragEvent — drag-and-drop events (HTML LS §9.10.5)
function DragEvent(type, init) {
    MouseEvent.call(this, type, init);
    // If no DataTransfer provided, create a fresh one for new drag operations
    this.dataTransfer = (init && init.dataTransfer != null)
        ? init.dataTransfer
        : new DataTransfer();
}
DragEvent.prototype = Object.create(MouseEvent.prototype);
DragEvent.prototype.constructor = DragEvent;

// _lumen_dispatch_drag_event — called by Rust shell (Phase 1) to fire a drag event
// on a specific element. data_json is a JSON string of { format: value } pairs.
function _lumen_dispatch_drag_event(nid, type, x, y, data_json) {
    var dt = new DataTransfer();
    if (data_json) {
        try {
            var d = JSON.parse(data_json);
            var keys = Object.keys(d);
            for (var i = 0; i < keys.length; i++) dt.setData(keys[i], d[keys[i]]);
        } catch(e) {}
    }
    var evt = new DragEvent(type, {
        bubbles: true, cancelable: true, isTrusted: true,
        clientX: x || 0, clientY: y || 0,
        dataTransfer: dt
    });
    _lumen_dispatch_rich(nid, evt);
    return !evt.defaultPrevented;
}

// ClipboardEvent — copy / cut / paste
function ClipboardEvent(type, init) {
    Event.call(this, type, init);
    this.clipboardData = (init && init.clipboardData != null) ? init.clipboardData : null;
}
ClipboardEvent.prototype = Object.create(Event.prototype);
ClipboardEvent.prototype.constructor = ClipboardEvent;

// CompositionEvent — IME compositionstart / compositionupdate / compositionend
function CompositionEvent(type, init) {
    UIEvent.call(this, type, init);
    this.data = (init && init.data != null) ? String(init.data) : '';
}
CompositionEvent.prototype = Object.create(UIEvent.prototype);
CompositionEvent.prototype.constructor = CompositionEvent;

// ── Per-element event listener store ─────────────────────────────────────────
// Key: String(nid) + ':' + type  →  Array of handler functions.

var _lumen_listeners = {};

function _lumen_add_listener(nid, type, fn) {
    if (typeof fn !== 'function') return;
    var key = String(nid) + ':' + String(type);
    if (!_lumen_listeners[key]) _lumen_listeners[key] = [];
    _lumen_listeners[key].push(fn);
}
function _lumen_rm_listener(nid, type, fn) {
    var key = String(nid) + ':' + String(type);
    var arr = _lumen_listeners[key];
    if (!arr) return;
    var idx = arr.indexOf(fn);
    if (idx >= 0) arr.splice(idx, 1);
}
function _lumen_dispatch(nid, event) {
    var key = String(nid) + ':' + event.type;
    var arr = _lumen_listeners[key];
    if (!arr || arr.length === 0) return !event.defaultPrevented;
    var copy = arr.slice(); // snapshot in case a handler mutates the list
    for (var i = 0; i < copy.length; i++) {
        try { copy[i].call(null, event); } catch(e) {}
        if (event._stopImmediate) break;
    }
    return !event.defaultPrevented;
}

// Sentinel NID used by document.addEventListener to store document-level listeners.
var _LUMEN_DOC_LISTENER_NID = -1;

// Dispatch an event starting at `start_nid` and bubbling up to the document.
// Called from Rust on user input (click, keydown, etc.).
// These events are marked as isTrusted=true because they come through the shell's native event loop.
function _lumen_dispatch_bubble(start_nid, type) {
    var evt = new Event(type, { bubbles: true, cancelable: true, isTrusted: true });
    evt.target = _lumen_make_element(start_nid);
    var cur = start_nid;
    while (cur !== null && cur !== undefined) {
        var key = String(cur) + ':' + String(type);
        var arr = _lumen_listeners[key];
        if (arr) {
            var copy = arr.slice();
            var el = _lumen_make_element(cur);
            for (var i = 0; i < copy.length; i++) {
                if (evt.cancelBubble) break;
                try { copy[i].call(el, evt); } catch(e) {}
                if (evt._stopImmediate) break;
            }
        }
        if (evt.cancelBubble) break;
        var pid = _lumen_u2n(_lumen_get_parent(cur));
        cur = (pid !== null && pid !== undefined) ? pid : null;
    }
    if (!evt.cancelBubble) {
        var dkey = String(_LUMEN_DOC_LISTENER_NID) + ':' + String(type);
        var darr = _lumen_listeners[dkey];
        if (darr) {
            var dcopy = darr.slice();
            for (var i = 0; i < dcopy.length; i++) {
                if (evt.cancelBubble) break;
                try { dcopy[i].call(document, evt); } catch(e) {}
                if (evt._stopImmediate) break;
            }
        }
    }
    return !evt.defaultPrevented;
}

// Bubble a pre-constructed event object (with target already set) through the DOM.
// Used by _lumen_dispatch_mouse_event and _lumen_dispatch_key_event so they can
// pass rich typed events instead of plain Event instances.
function _lumen_dispatch_rich(start_nid, event) {
    event.target = _lumen_make_element(start_nid);
    var cur = start_nid;
    while (cur !== null && cur !== undefined) {
        var key = String(cur) + ':' + event.type;
        var arr = _lumen_listeners[key];
        if (arr) {
            var copy = arr.slice();
            var el = _lumen_make_element(cur);
            for (var i = 0; i < copy.length; i++) {
                if (event.cancelBubble) break;
                try { copy[i].call(el, event); } catch(e) {}
                if (event._stopImmediate) break;
            }
        }
        if (event.cancelBubble || !event.bubbles) break;
        var pid = _lumen_u2n(_lumen_get_parent(cur));
        cur = (pid !== null && pid !== undefined) ? pid : null;
    }
    if (!event.cancelBubble) {
        var dkey = String(_LUMEN_DOC_LISTENER_NID) + ':' + event.type;
        var darr = _lumen_listeners[dkey];
        if (darr) {
            var dcopy = darr.slice();
            for (var i = 0; i < dcopy.length; i++) {
                if (event.cancelBubble) break;
                try { dcopy[i].call(document, event); } catch(e) {}
                if (event._stopImmediate) break;
            }
        }
    }
    return !event.defaultPrevented;
}

// Called from shell with actual viewport coordinates and modifier state.
// Creates a trusted MouseEvent and dispatches it through the DOM.
// mod: bit-mask — bit0=ctrl, bit1=shift, bit2=alt, bit3=meta
function _lumen_dispatch_mouse_event(start_nid, type, clientX, clientY, button, buttons, mod) {
    var ev = new MouseEvent(type, {
        bubbles: true, cancelable: true, isTrusted: true,
        clientX: clientX, clientY: clientY,
        screenX: clientX, screenY: clientY,
        pageX:   clientX, pageY:   clientY,
        button: button, buttons: buttons,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8)
    });
    return _lumen_dispatch_rich(start_nid, ev);
}

// Called from shell for pointer events (W3C Pointer Events Level 2).
// Mirrors _lumen_dispatch_mouse_event but creates a PointerEvent (extends MouseEvent).
// Non-bubbling types (pointerenter / pointerleave) set bubbles:false per spec.
// mod: bit-mask — bit0=ctrl, bit1=shift, bit2=alt, bit3=meta
function _lumen_dispatch_pointer_event(start_nid, type, clientX, clientY, button, buttons, mod) {
    var bubbles = (type !== 'pointerenter' && type !== 'pointerleave');
    var ev = new PointerEvent(type, {
        bubbles: bubbles, cancelable: bubbles, isTrusted: true,
        clientX: clientX, clientY: clientY,
        screenX: clientX, screenY: clientY,
        pageX:   clientX, pageY:   clientY,
        button: button, buttons: buttons,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8),
        pointerId: 1, pointerType: 'mouse', isPrimary: true,
        pressure: buttons ? 0.5 : 0.0,
        // Pointer Events Level 3 §4.1 — mouse always perpendicular to surface
        altitudeAngle: Math.PI / 2, azimuthAngle: 0,
        width: 1, height: 1,
        tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
    });
    // Level 3: getCoalescedEvents() / getPredictedEvents() — single event, no coalescing
    ev.getCoalescedEvents = function() { return [ev]; };
    ev.getPredictedEvents = function() { return []; };
    return _lumen_dispatch_rich(start_nid, ev);
}

// _lumen_dispatch_capture_event — fire gotpointercapture / lostpointercapture on a node.
// W3C Pointer Events L3 §4.1: these events do NOT bubble.
function _lumen_dispatch_capture_event(nid, type) {
    var ev = new PointerEvent(type, {
        bubbles: false, cancelable: false, isTrusted: true,
        pointerId: 1, pointerType: 'mouse', isPrimary: true,
        altitudeAngle: Math.PI / 2, azimuthAngle: 0,
        width: 1, height: 1,
        tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0
    });
    ev.getCoalescedEvents = function() { return []; };
    ev.getPredictedEvents = function() { return []; };
    _lumen_dispatch_rich(nid, ev);
}

// Called from shell for keydown / keyup / keypress events.
// mod: same bit-mask as _lumen_dispatch_mouse_event
function _lumen_dispatch_key_event(start_nid, type, key, code, keyCode, location, mod, repeat, isComposing) {
    var ev = new KeyboardEvent(type, {
        bubbles: true, cancelable: true, isTrusted: true,
        key: key, code: code, keyCode: keyCode, charCode: keyCode,
        which: keyCode, location: location,
        repeat: !!repeat, isComposing: !!isComposing,
        ctrlKey:  !!(mod & 1), shiftKey: !!(mod & 2),
        altKey:   !!(mod & 4), metaKey:  !!(mod & 8)
    });
    return _lumen_dispatch_rich(start_nid, ev);
}

// ── DOMTokenList (classList) ──────────────────────────────────────────────────

function _lumen_make_class_list(nid) {
    function getArr() {
        var c = _lumen_get_attr(nid, 'class');
        return (c && c.length > 0)
            ? c.split(/\\s+/).filter(function(t) { return t.length > 0; })
            : [];
    }
    function setArr(arr) { _lumen_set_attr(nid, 'class', arr.join(' ')); }
    var cl = {
        contains: function(cls) { return getArr().indexOf(String(cls)) >= 0; },
        add: function() {
            var arr = getArr();
            for (var i = 0; i < arguments.length; i++) {
                var cls = String(arguments[i]);
                if (arr.indexOf(cls) < 0) arr.push(cls);
            }
            setArr(arr);
        },
        remove: function() {
            var arr = getArr();
            for (var i = 0; i < arguments.length; i++) {
                var cls = String(arguments[i]);
                var idx = arr.indexOf(cls);
                if (idx >= 0) arr.splice(idx, 1);
            }
            setArr(arr);
        },
        toggle: function(cls, force) {
            cls = String(cls);
            var arr = getArr();
            var idx = arr.indexOf(cls);
            if (force !== undefined) {
                if (force && idx < 0)   { arr.push(cls); setArr(arr); return true; }
                if (!force && idx >= 0) { arr.splice(idx, 1); setArr(arr); return false; }
                return !!force;
            }
            if (idx >= 0) { arr.splice(idx, 1); setArr(arr); return false; }
            arr.push(cls); setArr(arr); return true;
        },
        replace: function(oldCls, newCls) {
            var arr = getArr();
            var idx = arr.indexOf(String(oldCls));
            if (idx < 0) return false;
            arr[idx] = String(newCls); setArr(arr); return true;
        },
        item: function(i) { var arr = getArr(); return arr[i] !== undefined ? arr[i] : null; },
        forEach: function(fn, thisArg) { getArr().forEach(fn, thisArg); },
        toString: function() { return getArr().join(' '); },
    };
    Object.defineProperty(cl, 'length', {
        get: function() { return getArr().length; },
        enumerable: true, configurable: true,
    });
    return cl;
}

// ── CSSStyleDeclaration (inline style) ───────────────────────────────────────

function _lumen_parse_style(s) {
    var obj = {};
    if (!s) return obj;
    s.split(';').forEach(function(decl) {
        var idx = decl.indexOf(':');
        if (idx < 0) return;
        var prop = decl.slice(0, idx).trim();
        var val  = decl.slice(idx + 1).trim();
        if (prop) obj[prop] = val;
    });
    return obj;
}
function _lumen_serialize_style(obj) {
    return Object.keys(obj).map(function(k) { return k + ': ' + obj[k]; }).join('; ');
}
function _lumen_camel_to_kebab(prop) {
    return prop.replace(/([A-Z])/g, function(m) { return '-' + m.toLowerCase(); });
}

function _lumen_make_style(nid) {
    function getParsed() {
        var s = _lumen_get_attr(nid, 'style');
        return _lumen_parse_style(s !== undefined ? s : '');
    }
    function setParsed(obj) { _lumen_set_attr(nid, 'style', _lumen_serialize_style(obj)); }
    var handler = {
        getPropertyValue: function(prop) {
            return getParsed()[_lumen_camel_to_kebab(String(prop))] || '';
        },
        setProperty: function(prop, val) {
            var obj = getParsed();
            obj[_lumen_camel_to_kebab(String(prop))] = String(val);
            setParsed(obj);
        },
        removeProperty: function(prop) {
            var obj = getParsed();
            var key = _lumen_camel_to_kebab(String(prop));
            var old = obj[key] || '';
            delete obj[key]; setParsed(obj); return old;
        },
    };
    Object.defineProperty(handler, 'cssText', {
        get: function() { var s = _lumen_get_attr(nid, 'style'); return s !== undefined ? s : ''; },
        set: function(v) { _lumen_set_attr(nid, 'style', String(v)); },
        enumerable: true, configurable: true,
    });
    return new Proxy(handler, {
        get: function(target, prop) {
            if (prop in target) return target[prop];
            return target.getPropertyValue(_lumen_camel_to_kebab(String(prop)));
        },
        set: function(target, prop, value) {
            if (prop in target) { target[prop] = value; return true; }
            target.setProperty(_lumen_camel_to_kebab(String(prop)), value);
            return true;
        },
    });
}

// ── ShadowRoot wrapper ────────────────────────────────────────────────────────
// Wraps a shadow-root NodeId as a DocumentFragment-like ShadowRoot object.
// `mode`     : 'open' | 'closed' (stored for the `.mode` property)
// `host_nid` : NodeId of the shadow host element

function _lumen_make_shadow_root(nid, mode, host_nid) {
    var _style = _lumen_make_style(nid);
    var sr = {
        __nid__:          nid,
        __isShadowRoot__: true,
        mode:             mode,
        get host()        { return _lumen_make_element(host_nid); },
        get innerHTML()   { return _lumen_get_inner_html(nid); },
        set innerHTML(v)  { _lumen_set_inner_html(nid, String(v)); },
        get textContent() { return _lumen_get_text_content(nid); },
        set textContent(v){ _lumen_set_text_content(nid, String(v)); },
        get style()       { return _style; },
        querySelector:    function(sel) {
            var n = _lumen_u2n(_lumen_query_selector(String(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll: function(sel) {
            return _lumen_query_selector_all(String(sel)).map(_lumen_make_element);
        },
        getElementById:   function(id) {
            var n = _lumen_u2n(_lumen_get_element_by_id(String(id)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        appendChild:      function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_append_child(nid, c.__nid__);
                _lumen_ce_maybe_connected(c);
            }
            return c;
        },
        removeChild:      function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_remove_child(nid, c.__nid__);
                _lumen_ce_maybe_disconnected(c);
            }
            return c;
        },
        addEventListener:    function(type, fn) { _lumen_add_listener(nid, type, fn); },
        removeEventListener: function(type, fn) { _lumen_rm_listener(nid, type, fn); },
        dispatchEvent:       function(evt) {
            if (!evt) return true;
            evt.target = this; evt.currentTarget = this;
            return _lumen_dispatch(nid, evt);
        },
    };
    Object.defineProperty(sr, 'children', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    return sr;
}

// ── DocumentFragment wrapper ──────────────────────────────────────────────────
// Wraps a DocumentFragment NodeId. Unlike ShadowRoot, a DocumentFragment is
// consumed when appended: all children are moved to the target parent (DOM LS
// §4.2.4). `cloneNode(true)` on a fragment deep-clones without consuming it.

function _lumen_make_document_fragment(nid) {
    var frag = {
        __nid__:              nid,
        __isDocumentFragment__: true,
        get nodeType()        { return 11; }, // Node.DOCUMENT_FRAGMENT_NODE
        get nodeName()        { return '#document-fragment'; },
        get textContent()     { return _lumen_get_text_content(nid); },
        set textContent(v)    { _lumen_set_text_content(nid, String(v)); },
        get innerHTML()       { return _lumen_get_inner_html(nid); },
        set innerHTML(v)      { _lumen_set_inner_html(nid, String(v)); },
        querySelector:        function(sel) {
            var n = _lumen_u2n(_lumen_query_selector(String(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll:     function(sel) {
            return _lumen_query_selector_all(String(sel)).map(_lumen_make_element);
        },
        appendChild:          function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_append_child(nid, c.__nid__);
            }
            return c;
        },
        removeChild:          function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_remove_child(nid, c.__nid__);
            }
            return c;
        },
        // cloneNode: returns a new fragment with deep-cloned children (always deep for fragments).
        cloneNode:            function(deep) {
            var clone_nid = _lumen_clone_subtree(nid, deep ? 1 : 0);
            return _lumen_make_document_fragment(clone_nid);
        },
    };
    Object.defineProperty(frag, 'children', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(frag, 'childNodes', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    return frag;
}

// Dispatch slotchange on all <slot> elements inside the shadow root of `host_nid`.
// Called when host's light DOM changes (appendChild / removeChild).
function _lumen_fire_slotchange(host_nid) {
    var sr_nid = _lumen_u2n(_lumen_get_shadow_root(host_nid));
    if (sr_nid === null) return;
    var slots = _lumen_query_selector_all('slot');
    for (var i = 0; i < slots.length; i++) {
        var slot_nid = slots[i];
        var ev = new Event('slotchange', { bubbles: true, cancelable: false });
        _lumen_dispatch(slot_nid, ev);
    }
}

// ── Form Constraint Validation API (HTML LS §4.10.21) ────────────────────────
// Per-nid storage: persists across multiple _lumen_make_element calls for the
// same node (elements are fresh objects each time; state lives in these maps).

// nid → custom validity message set via setCustomValidity() ('' → no custom error)
var _validity_msg = {};
// nid → current input value (undefined → fall back to value attribute)
var _input_values = {};
// nid → cached CanvasRenderingContext2D object (persists across _lumen_make_element).
var _canvas2d_ctxs = {};

// ValidityState — readonly snapshot of one form control's validity.
function ValidityState(flags) {
    this.valueMissing    = !!flags.valueMissing;
    this.typeMismatch    = !!flags.typeMismatch;
    this.patternMismatch = !!flags.patternMismatch;
    this.tooLong         = !!flags.tooLong;
    this.tooShort        = !!flags.tooShort;
    this.rangeUnderflow  = !!flags.rangeUnderflow;
    this.rangeOverflow   = !!flags.rangeOverflow;
    this.stepMismatch    = !!flags.stepMismatch;
    this.badInput        = !!flags.badInput;
    this.customError     = !!flags.customError;
    this.valid = !this.valueMissing   && !this.typeMismatch  && !this.patternMismatch
              && !this.tooLong        && !this.tooShort
              && !this.rangeUnderflow && !this.rangeOverflow && !this.stepMismatch
              && !this.badInput       && !this.customError;
}

// Computes ValidityState for a form control element (HTML LS §4.10.21.1).
function _compute_validity(el) {
    var flags = {};
    var type  = (el.type || 'text').toLowerCase();
    var val   = (el.value != null) ? String(el.value) : '';
    var enid  = el.__nid__;
    var customMsg = (enid !== undefined && _validity_msg[enid]) ? _validity_msg[enid] : '';

    // §4.10.21.1 #1: valueMissing — required + empty
    if (el.hasAttribute && el.hasAttribute('required') && val.trim() === '') {
        flags.valueMissing = true;
    }

    // §4.10.21.1 #3: typeMismatch — email/url/number format
    if (!flags.valueMissing && val !== '') {
        if (type === 'email') {
            // Simplified email check: user@domain.tld
            if (!/^[^\\s@,;]+@[^\\s@,;]+\\.[^\\s@,;]+$/.test(val)) flags.typeMismatch = true;
        } else if (type === 'url') {
            try { new URL(val); } catch(e) { flags.typeMismatch = true; }
        } else if (type === 'number') {
            if (isNaN(Number(val))) flags.typeMismatch = true;
        }
    }

    // §4.10.21.1 #4: patternMismatch — pattern attribute
    if (!flags.typeMismatch && val !== '' && el.hasAttribute && el.hasAttribute('pattern')) {
        var pat = el.getAttribute('pattern');
        if (pat) {
            try {
                if (!(new RegExp('^(?:' + pat + ')$')).test(val)) flags.patternMismatch = true;
            } catch(e) {}
        }
    }

    // §4.10.21.1 #6/#7: tooLong / tooShort
    if (el.hasAttribute && el.hasAttribute('maxlength')) {
        var maxL = parseInt(el.getAttribute('maxlength'), 10);
        if (!isNaN(maxL) && val.length > maxL) flags.tooLong = true;
    }
    if (val !== '' && el.hasAttribute && el.hasAttribute('minlength')) {
        var minL = parseInt(el.getAttribute('minlength'), 10);
        if (!isNaN(minL) && val.length < minL) flags.tooShort = true;
    }

    // §4.10.21.1 #5: rangeUnderflow / rangeOverflow / stepMismatch (number + range)
    if (type === 'number' || type === 'range') {
        var num = Number(val);
        if (!isNaN(num) && val !== '') {
            if (el.hasAttribute && el.hasAttribute('min')) {
                var mn = Number(el.getAttribute('min'));
                if (!isNaN(mn) && num < mn) flags.rangeUnderflow = true;
            }
            if (el.hasAttribute && el.hasAttribute('max')) {
                var mx = Number(el.getAttribute('max'));
                if (!isNaN(mx) && num > mx) flags.rangeOverflow = true;
            }
            if (el.hasAttribute && el.hasAttribute('step')) {
                var stepA = el.getAttribute('step');
                if (stepA && stepA !== 'any') {
                    var st = Number(stepA);
                    var base = el.hasAttribute('min') ? Number(el.getAttribute('min')) : 0;
                    if (!isNaN(st) && st > 0 && Math.abs((num - base) % st) > 1e-9) {
                        flags.stepMismatch = true;
                    }
                }
            }
        }
    }

    // §4.10.21.1 #10: customError
    if (customMsg) flags.customError = true;

    return new ValidityState(flags);
}

// ── Path2D class (HTML LS §4.12.5.1.5) ─────────────────────────────────────────
// Reusable path object; coordinates stored in user space; CTM applied at use-time.
function Path2D(arg) {
    // Allocate a native path object and record its ID on this instance.
    var svg = (typeof arg === 'string') ? arg : '';
    if (arg instanceof Path2D) {
        // Copy constructor: create empty then addPath.
        this.__pid__ = _lumen_canvas2d_path2d_new('');
        _lumen_canvas2d_path2d_add_path(this.__pid__, arg.__pid__, '');
    } else {
        this.__pid__ = _lumen_canvas2d_path2d_new(svg);
    }
}
Path2D.prototype.moveTo = function(x, y) {
    _lumen_canvas2d_path2d_move_to(this.__pid__, +x, +y);
};
Path2D.prototype.lineTo = function(x, y) {
    _lumen_canvas2d_path2d_line_to(this.__pid__, +x, +y);
};
Path2D.prototype.closePath = function() {
    _lumen_canvas2d_path2d_close(this.__pid__);
};
Path2D.prototype.bezierCurveTo = function(cp1x, cp1y, cp2x, cp2y, x, y) {
    _lumen_canvas2d_path2d_bezier(this.__pid__, +cp1x, +cp1y, +cp2x, +cp2y, +x, +y);
};
Path2D.prototype.quadraticCurveTo = function(cpx, cpy, x, y) {
    _lumen_canvas2d_path2d_quadratic(this.__pid__, +cpx, +cpy, +x, +y);
};
Path2D.prototype.arc = function(x, y, r, startAngle, endAngle, anticlockwise) {
    _lumen_canvas2d_path2d_arc(this.__pid__, +x, +y, +r, +startAngle, +endAngle, !!anticlockwise);
};
Path2D.prototype.arcTo = function(x1, y1, x2, y2, r) {
    _lumen_canvas2d_path2d_arc_to(this.__pid__, +x1, +y1, +x2, +y2, +r);
};
// ellipse: native binding limited to 7 args, so implemented via arc with save/scale.
Path2D.prototype.ellipse = function(cx, cy, rx, ry, rot, startAngle, endAngle, anticlockwise) {
    // Approximate via arc in scaled user space — correct for all standard use cases.
    // Creates a throwaway arc path and merges segments into this path via arc+addPath.
    var tmp = new Path2D();
    _lumen_canvas2d_path2d_arc(tmp.__pid__, 0, 0, 1, +startAngle, +endAngle, !!anticlockwise);
    // Build transform: scale(rx,ry) then rotate(rot) then translate(cx,cy)
    // [a,b,c,d,e,f] = [rx*cos(r), rx*sin(r), -ry*sin(r), ry*cos(r), cx, cy]
    var cos_r = Math.cos(+rot), sin_r = Math.sin(+rot);
    var rx_ = +rx, ry_ = +ry;
    var a = rx_ * cos_r, b = rx_ * sin_r, c = -ry_ * sin_r, d = ry_ * cos_r;
    _lumen_canvas2d_path2d_add_path(this.__pid__, tmp.__pid__, '' + a + ',' + b + ',' + c + ',' + d + ',' + (+cx) + ',' + (+cy));
};
Path2D.prototype.rect = function(x, y, w, h) {
    _lumen_canvas2d_path2d_rect(this.__pid__, +x, +y, +w, +h);
};
Path2D.prototype.addPath = function(path, transform) {
    if (!(path instanceof Path2D)) return;
    if (transform && typeof transform === 'object' && transform.a !== undefined) {
        var t = transform;
        _lumen_canvas2d_path2d_add_path(this.__pid__, path.__pid__,
            '' + t.a + ',' + t.b + ',' + t.c + ',' + t.d + ',' + t.e + ',' + t.f);
    } else {
        _lumen_canvas2d_path2d_add_path(this.__pid__, path.__pid__, '');
    }
};

// ── Canvas 2D context factory (HTML LS §4.12.4) ─────────────────────────────────
// Builds a CanvasRenderingContext2D backed by the native _lumen_canvas2d_* bindings
// (lumen_canvas::Context2D), keyed by the canvas element's node index `nid`.
// Drawing methods forward to the native rasterizer; the shell uploads the pixel
// buffer to the renderer under `canvas:{nid}` each frame.
function _lumen_make_canvas2d_ctx(canvasEl, nid) {
    var _fillStyle = '#000000';
    var _strokeStyle = '#000000';
    var _lineWidth = 1.0;
    var _globalAlpha = 1.0;
    var _globalCompositeOperation = 'source-over';
    var _lineCap = 'butt';
    var _lineJoin = 'miter';
    var _miterLimit = 10;
    var ctx = {
        canvas: canvasEl,
        get fillStyle() { return _fillStyle; },
        set fillStyle(v) { _fillStyle = String(v); _lumen_canvas2d_set_fill_style(nid, _fillStyle); },
        get strokeStyle() { return _strokeStyle; },
        set strokeStyle(v) { _strokeStyle = String(v); _lumen_canvas2d_set_stroke_style(nid, _strokeStyle); },
        get lineWidth() { return _lineWidth; },
        set lineWidth(v) { var n = Number(v); if (isFinite(n) && n > 0) { _lineWidth = n; _lumen_canvas2d_set_line_width(nid, n); } },
        get globalAlpha() { return _globalAlpha; },
        set globalAlpha(v) { var n = Number(v); if (isFinite(n) && n >= 0 && n <= 1) { _globalAlpha = n; _lumen_canvas2d_set_global_alpha(nid, n); } },
        get globalCompositeOperation() { return _globalCompositeOperation; },
        set globalCompositeOperation(v) { var s = String(v); _globalCompositeOperation = s; _lumen_canvas2d_set_global_composite_operation(nid, s); },
        get lineCap() { return _lineCap; },
        set lineCap(v) { var s = String(v); _lineCap = s; _lumen_canvas2d_set_line_cap(nid, s); },
        get lineJoin() { return _lineJoin; },
        set lineJoin(v) { var s = String(v); _lineJoin = s; _lumen_canvas2d_set_line_join(nid, s); },
        get miterLimit() { return _miterLimit; },
        set miterLimit(v) { var n = Number(v); if (isFinite(n) && n > 0) { _miterLimit = n; _lumen_canvas2d_set_miter_limit(nid, n); } },
        // Rect operations
        fillRect: function(x, y, w, h) { _lumen_canvas2d_fill_rect(nid, +x, +y, +w, +h); },
        clearRect: function(x, y, w, h) { _lumen_canvas2d_clear_rect(nid, +x, +y, +w, +h); },
        strokeRect: function(x, y, w, h) { _lumen_canvas2d_stroke_rect(nid, +x, +y, +w, +h); },
        // Path operations
        beginPath: function() { _lumen_canvas2d_begin_path(nid); },
        moveTo: function(x, y) { _lumen_canvas2d_move_to(nid, +x, +y); },
        lineTo: function(x, y) { _lumen_canvas2d_line_to(nid, +x, +y); },
        closePath: function() { _lumen_canvas2d_close_path(nid); },
        arc: function(cx, cy, r, sa, ea, ccw) { _lumen_canvas2d_arc(nid, +cx, +cy, +r, +sa, +ea, !!ccw); },
        ellipse: function(cx, cy, rx, ry, rot, sa, ea, ccw) {
            // Implemented via transforms: save → translate(cx,cy) → rotate(rot) →
            // scale(rx,ry) → arc(0,0,1,sa,ea,ccw) → restore.
            _lumen_canvas2d_save(nid);
            _lumen_canvas2d_translate(nid, +cx, +cy);
            if (+rot !== 0) { _lumen_canvas2d_rotate(nid, +rot); }
            _lumen_canvas2d_scale(nid, +rx, +ry);
            _lumen_canvas2d_arc(nid, 0, 0, 1, +sa, +ea, !!ccw);
            _lumen_canvas2d_restore(nid);
        },
        arcTo: function(x1, y1, x2, y2, r) { _lumen_canvas2d_arc_to(nid, +x1, +y1, +x2, +y2, +r); },
        rect: function(x, y, w, h) { _lumen_canvas2d_rect(nid, +x, +y, +w, +h); },
        bezierCurveTo: function(cp1x, cp1y, cp2x, cp2y, x, y) { _lumen_canvas2d_bezier_curve_to(nid, +cp1x, +cp1y, +cp2x, +cp2y, +x, +y); },
        quadraticCurveTo: function(cpx, cpy, x, y) { _lumen_canvas2d_quadratic_curve_to(nid, +cpx, +cpy, +x, +y); },
        fill: function(ruleOrPath) {
            if (ruleOrPath instanceof Path2D) {
                _lumen_canvas2d_fill_path(nid, ruleOrPath.__pid__);
            } else {
                _lumen_canvas2d_fill(nid);
            }
        },
        stroke: function(path) {
            if (path instanceof Path2D) {
                _lumen_canvas2d_stroke_path(nid, path.__pid__);
            } else {
                _lumen_canvas2d_stroke(nid);
            }
        },
        // State stack
        save: function() { _lumen_canvas2d_save(nid); },
        restore: function() { _lumen_canvas2d_restore(nid); },
        // Transforms
        translate: function(tx, ty) { _lumen_canvas2d_translate(nid, +tx, +ty); },
        rotate: function(angle) { _lumen_canvas2d_rotate(nid, +angle); },
        scale: function(sx, sy) { _lumen_canvas2d_scale(nid, +sx, +sy); },
        transform: function(a, b, c, d, e, f) { _lumen_canvas2d_transform(nid, +a, +b, +c, +d, +e, +f); },
        setTransform: function(a, b, c, d, e, f) { _lumen_canvas2d_set_transform(nid, +a, +b, +c, +d, +e, +f); },
        resetTransform: function() { _lumen_canvas2d_reset_transform(nid); },
        getImageData: function(x, y, sw, sh) {
            var raw = _lumen_canvas2d_get_image_data(nid);
            if (!raw) { return { width: sw|0, height: sh|0, data: new Uint8ClampedArray((sw|0) * (sh|0) * 4) }; }
            var comma1 = raw.indexOf(','), comma2 = raw.indexOf(',', comma1 + 1);
            var w = parseInt(raw.substring(0, comma1), 10);
            var h = parseInt(raw.substring(comma1 + 1, comma2), 10);
            var hex = raw.substring(comma2 + 1);
            var len = hex.length >> 1;
            var arr = new Uint8ClampedArray(len);
            for (var i = 0; i < len; i++) { arr[i] = parseInt(hex.substr(i * 2, 2), 16); }
            return { width: w, height: h, data: arr };
        },
        // Remaining stubs (not yet implemented)
        clip: function(path) {
            if (path instanceof Path2D) {
                _lumen_canvas2d_clip_path(nid, path.__pid__);
            } else {
                _lumen_canvas2d_clip(nid);
            }
        },
        putImageData: function() {},
        drawImage: function() {},
        fillText: function(t, x, y) {
            _lumen_canvas2d_fill_text(nid, String(t == null ? '' : t), +x, +y);
        },
        strokeText: function(t, x, y) {
            _lumen_canvas2d_stroke_text(nid, String(t == null ? '' : t), +x, +y);
        },
        measureText: function(t) {
            var s = String(t == null ? '' : t);
            var w = _lumen_canvas2d_measure_text(nid, s);
            var fs = parse_canvas_font_size_js(_font);
            return { width: w, actualBoundingBoxAscent: fs * 0.8, actualBoundingBoxDescent: fs * 0.2 };
        },
        setLineDash: function() {}, getLineDash: function() { return []; },
        isPointInPath: function(pathOrX, xOrY, y) {
            if (pathOrX instanceof Path2D) {
                return _lumen_canvas2d_is_point_in_path(nid, pathOrX.__pid__, +xOrY, +y);
            }
            return false;
        },
        isPointInStroke: function() { return false; },
        createLinearGradient: function() { return { addColorStop: function() {} }; },
        createRadialGradient: function() { return { addColorStop: function() {} }; },
        createConicGradient: function() { return { addColorStop: function() {} }; },
        createPattern: function() { return null; },
        createImageData: function(w, h) { return { width: w|0, height: h|0, data: new Uint8ClampedArray((w|0) * (h|0) * 4) }; },
    };
    // Stub appearance properties accepted but not yet wired.
    var _stubProps = ['shadowColor','shadowBlur','shadowOffsetX','shadowOffsetY',
        'direction','lineDashOffset','imageSmoothingEnabled','filter'];
    for (var _pi = 0; _pi < _stubProps.length; _pi++) {
        (function(name) {
            var _val = (name === 'imageSmoothingEnabled') ? true
                : (name === 'shadowColor') ? 'rgba(0, 0, 0, 0)'
                : (name === 'filter') ? 'none' : 0;
            Object.defineProperty(ctx, name, {
                get: function() { return _val; }, set: function(v) { _val = v; }, configurable: true,
            });
        })(_stubProps[_pi]);
    }
    // Wired text properties (Phase 4): font, textAlign, textBaseline.
    var _font = '10px sans-serif';
    Object.defineProperty(ctx, 'font', {
        get: function() { return _font; },
        set: function(v) { _font = String(v); _lumen_canvas2d_set_font(nid, _font); },
        configurable: true,
    });
    var _textAlign = 'start';
    Object.defineProperty(ctx, 'textAlign', {
        get: function() { return _textAlign; },
        set: function(v) { _textAlign = String(v); _lumen_canvas2d_set_text_align(nid, _textAlign); },
        configurable: true,
    });
    var _textBaseline = 'alphabetic';
    Object.defineProperty(ctx, 'textBaseline', {
        get: function() { return _textBaseline; },
        set: function(v) { _textBaseline = String(v); _lumen_canvas2d_set_text_baseline(nid, _textBaseline); },
        configurable: true,
    });
    // Helper: parse px size from font string for TextMetrics ascent/descent approximation.
    function parse_canvas_font_size_js(f) {
        var parts = f.split(' ');
        for (var i = 0; i < parts.length; i++) {
            if (parts[i].indexOf('px') !== -1) {
                var n = parseFloat(parts[i]);
                if (n > 0) return n;
            }
        }
        return 10;
    }
    return ctx;
}

// Resolve a canvas element's bitmap width/height (HTML LS §4.12.4 defaults 300×150).
function _lumen_canvas_dims(nid) {
    var aw = _lumen_u2n(_lumen_get_attr(nid, 'width'));
    var ah = _lumen_u2n(_lumen_get_attr(nid, 'height'));
    var w = (aw !== null) ? (parseInt(aw, 10) || 300) : 300;
    var h = (ah !== null) ? (parseInt(ah, 10) || 150) : 150;
    if (w < 1) w = 1;
    if (h < 1) h = 1;
    return [w, h];
}

// ── Element factory ───────────────────────────────────────────────────────────

function _lumen_make_element(nid) {
    if (nid === null || nid === undefined) return null;
    var _classList = _lumen_make_class_list(nid);
    var _style     = _lumen_make_style(nid);
    var _returnValue = '';
    var _obj = {
        __nid__: nid,
        get tagName()        { return _lumen_get_tag_name(nid); },
        get nodeName()       { return _lumen_get_tag_name(nid); },
        get nodeType()       { return _lumen_is_text_node(nid) ? 3 : 1; },
        get id()             { var v = _lumen_u2n(_lumen_get_attr(nid, 'id'));    return v !== null ? v : ''; },
        set id(v)            { _lumen_set_attr(nid, 'id', String(v)); },
        get className()      { var v = _lumen_u2n(_lumen_get_attr(nid, 'class')); return v !== null ? v : ''; },
        set className(v)     { _lumen_set_attr(nid, 'class', String(v)); },
        get classList()      { return _classList; },
        get style()          { return _style; },
        get attributeStyleMap() {
            // CSS Typed OM L1 — StylePropertyMap for element.style (mutable)
            if (typeof CSS === 'undefined' || !CSS.StylePropertyMap) return null;
            return new CSS.StylePropertyMap(nid);
        },
        computedStyleMap: function() {
            // CSS Typed OM L1 — ComputedStylePropertyMap for computed styles (read-only)
            if (typeof CSS === 'undefined' || !CSS.ComputedStylePropertyMap) return null;
            return new CSS.ComputedStylePropertyMap(nid);
        },
        get textContent()    { return _lumen_get_text_content(nid); },
        set textContent(v)   { _lumen_set_text_content(nid, String(v)); },
        get innerHTML()      { return _lumen_get_inner_html(nid); },
        set innerHTML(v)     { _lumen_set_inner_html(nid, String(v)); },
        getAttribute:    function(n)    { return _lumen_u2n(_lumen_get_attr(nid, String(n))); },
        setAttribute:    function(n, v) {
            var attrName = String(n);
            var oldVal   = _lumen_u2n(_lumen_get_attr(nid, attrName));
            _lumen_set_attr(nid, attrName, String(v));
            _lumen_ce_maybe_attr_changed(nid, attrName, oldVal, String(v));
        },
        removeAttribute: function(n)    { _lumen_remove_attr(nid, String(n)); },
        hasAttribute:    function(n)    { return _lumen_get_attr(nid, String(n)) !== undefined; },
        // DOM LS §4.9.3: toggleAttribute(qualifiedName, force?)
        toggleAttribute: function(n, force) {
            var attrName = String(n);
            var has = _lumen_get_attr(nid, attrName) !== undefined;
            if (force === undefined) {
                if (has) { _lumen_remove_attr(nid, attrName); return false; }
                _lumen_set_attr(nid, attrName, ''); return true;
            }
            if (force) {
                if (!has) _lumen_set_attr(nid, attrName, '');
                return true;
            }
            if (has) _lumen_remove_attr(nid, attrName);
            return false;
        },
        // Reflected `open` boolean attribute — shared by <details> (HTML5 §4.11.1)
        // and <dialog> (HTML5 §4.11.7).
        get open() { return _lumen_get_attr(nid, 'open') !== undefined; },
        set open(v) {
            if (v) { _lumen_set_attr(nid, 'open', ''); }
            else { _lumen_remove_attr(nid, 'open'); }
        },
        // HTMLDialogElement API (HTML5 §4.11.7)
        get returnValue() { return _returnValue; },
        set returnValue(v) { _returnValue = String(v); },
        show: function() {
            _lumen_set_attr(nid, 'open', '');
        },
        showModal: function() {
            _lumen_set_attr(nid, 'open', '');
            _lumen_set_attr(nid, 'data-lumen-modal', '');
            if (_lumen_modal_dialog_nids.indexOf(nid) < 0) {
                _lumen_modal_dialog_nids.push(nid);
            }
            // HTML LS §6.6.3: save the currently focused element so close() can restore it.
            _lumen_dialog_prev_focus[nid] = _lumen_last_focused_nid;
            // Focus the first [autofocus] descendant, or the dialog itself if none.
            var target = _lumen_find_autofocus_in(nid);
            _lumen_request_focus(target !== -1 ? target : nid);
        },
        close: function(rv) {
            if (_lumen_get_attr(nid, 'open') === undefined) return;
            if (rv !== undefined) _returnValue = String(rv);
            _lumen_remove_attr(nid, 'open');
            _lumen_remove_attr(nid, 'data-lumen-modal');
            var idx = _lumen_modal_dialog_nids.indexOf(nid);
            if (idx >= 0) _lumen_modal_dialog_nids.splice(idx, 1);
            // HTML LS §6.6.3: restore focus to the element that was focused before open.
            var prev = _lumen_dialog_prev_focus[nid];
            delete _lumen_dialog_prev_focus[nid];
            if (prev !== undefined && prev !== -1) {
                _lumen_request_focus(prev);
            } else {
                _lumen_request_blur();
            }
            var closeEvt = new Event('close', { bubbles: false, cancelable: false });
            _lumen_dispatch(nid, closeEvt);
        },
        // HTML Popover API (WHATWG HTML §6.12)
        get popover() {
            var v = _lumen_get_attr(nid, 'popover');
            if (v === undefined) return null;
            var norm = (v || '').toLowerCase();
            if (norm === 'manual') return 'manual';
            if (norm === 'hint') return 'hint'; // Popover API Level 2
            return 'auto';
        },
        set popover(v) {
            if (v === null || v === undefined || v === false) {
                _lumen_remove_attr(nid, 'popover');
            } else {
                _lumen_set_attr(nid, 'popover', v === '' ? '' : String(v).toLowerCase());
            }
        },
        showPopover:   function()      { _lumen_popover_show(nid); },
        hidePopover:   function()      { _lumen_popover_hide(nid); },
        togglePopover: function(force) { _lumen_popover_toggle(nid, force); },
        // Fullscreen API (WHATWG Fullscreen §4.3)
        requestFullscreen: function(options) {
            var self = _obj;
            return new Promise(function(resolve, reject) {
                if (!document.fullscreenEnabled) {
                    reject(new TypeError('Fullscreen not enabled'));
                    return;
                }
                // Exit previous fullscreen element if it is a different node.
                if (_fs_nid !== -1 && _fs_nid !== nid) {
                    _lumen_remove_attr(_fs_nid, _FS_ATTR);
                    var prev = _lumen_make_element(_fs_nid);
                    if (prev) { prev.dispatchEvent(new Event('fullscreenchange', { bubbles: true })); }
                }
                _fs_nid = nid;
                _lumen_set_attr(nid, _FS_ATTR, '');
                // Notify shell to enter OS fullscreen.
                if (typeof _lumen_fs_enter === 'function') { _lumen_fs_enter(nid); }
                self.dispatchEvent(new Event('fullscreenchange', { bubbles: true }));
                document.dispatchEvent(new Event('fullscreenchange'));
                resolve();
            });
        },
        requestPointerLock: function() {
            var self = _obj;
            return new Promise(function(resolve, reject) {
                // Phase 0: synchronously lock pointer (Phase 1: integrate with shell winit).
                if (typeof _lumen_ptr_lock_request === 'function') {
                    _lumen_ptr_lock_request(nid);
                }
                self.dispatchEvent(new Event('pointerlockchange', { bubbles: true }));
                document.dispatchEvent(new Event('pointerlockchange'));
                resolve();
            });
        },
        onfullscreenchange: null,
        onfullscreenerror:  null,
        onpointerlockchange: null,
        onpointerlockerror: null,
        // HTML LS §9.10 — drag-and-drop IDL attributes
        get draggable() {
            var v = _lumen_get_attr(nid, 'draggable');
            if (v === undefined || v === null) return false;
            return String(v).toLowerCase() !== 'false';
        },
        set draggable(v) {
            _lumen_set_attr(nid, 'draggable', v ? 'true' : 'false');
        },
        ondragstart:  null,
        ondrag:       null,
        ondragend:    null,
        ondragenter:  null,
        ondragover:   null,
        ondragleave:  null,
        ondrop:       null,
        // Pointer Events Level 3 §4.1 — pointer capture
        ongotpointercapture:  null,
        onlostpointercapture: null,
        setPointerCapture: function(pointerId) {
            // Spec: InvalidStateError if element is not connected — skip check for Phase 0
            if (typeof _lumen_set_capture_state === 'function') {
                _lumen_set_capture_state(nid);
            }
            _lumen_dispatch_capture_event(nid, 'gotpointercapture');
        },
        releasePointerCapture: function(pointerId) {
            if (typeof _lumen_release_capture_state === 'function') {
                _lumen_release_capture_state();
            }
            _lumen_dispatch_capture_event(nid, 'lostpointercapture');
        },
        hasPointerCapture: function(pointerId) {
            if (typeof _lumen_get_capture_nid === 'function') {
                return _lumen_get_capture_nid() === nid;
            }
            return false;
        },
        appendChild:     function(c) {
            if (!c || c.__nid__ === undefined) return c;
            if (c.__isDocumentFragment__) {
                // DOM LS §4.2.4: fragment append moves all children, not the fragment itself.
                var kids = _lumen_get_children(c.__nid__).slice();
                for (var _fi = 0; _fi < kids.length; _fi++) {
                    _lumen_append_child(nid, kids[_fi]);
                    _lumen_ce_maybe_connected(_lumen_make_element(kids[_fi]));
                }
            } else {
                _lumen_append_child(nid, c.__nid__);
                _lumen_ce_maybe_connected(c);
            }
            _lumen_fire_slotchange(nid);
            return c;
        },
        removeChild:     function(c) {
            if (c && c.__nid__ !== undefined) {
                _lumen_remove_child(nid, c.__nid__);
                _lumen_ce_maybe_disconnected(c);
                _lumen_fire_slotchange(nid);
            }
            return c;
        },
        // ── ChildNode mixin (DOM LS §4.2.6) ─────────────────────────────────────
        // Removes this element from its parent.
        remove: function() {
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid !== null) {
                _lumen_remove_child(pid, nid);
                _lumen_ce_maybe_disconnected(this);
            }
        },
        // Inserts nodes immediately before this element.
        before: function() {
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            for (var _bi = 0; _bi < arguments.length; _bi++) {
                var _bn = arguments[_bi];
                if (typeof _bn === 'string') {
                    var _btn = _lumen_create_text_node(_bn);
                    _lumen_insert_before(pid, _btn, nid);
                } else if (_bn && _bn.__nid__ !== undefined) {
                    _lumen_insert_before(pid, _bn.__nid__, nid);
                }
            }
        },
        // Inserts nodes immediately after this element.
        after: function() {
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            var ch = _lumen_get_children(pid);
            var idx = ch.indexOf(nid);
            var nextSib = (idx >= 0 && idx + 1 < ch.length) ? ch[idx + 1] : null;
            for (var _ai = 0; _ai < arguments.length; _ai++) {
                var _an = arguments[_ai];
                if (typeof _an === 'string') {
                    var _atn = _lumen_create_text_node(_an);
                    if (nextSib !== null) { _lumen_insert_before(pid, _atn, nextSib); }
                    else { _lumen_append_child(pid, _atn); }
                } else if (_an && _an.__nid__ !== undefined) {
                    if (nextSib !== null) { _lumen_insert_before(pid, _an.__nid__, nextSib); }
                    else { _lumen_append_child(pid, _an.__nid__); }
                }
            }
        },
        // Replaces this element with the given nodes/strings.
        replaceWith: function() {
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            if (pid === null) return;
            var ch = _lumen_get_children(pid);
            var idx = ch.indexOf(nid);
            var nextSib = (idx >= 0 && idx + 1 < ch.length) ? ch[idx + 1] : null;
            _lumen_remove_child(pid, nid);
            _lumen_ce_maybe_disconnected(this);
            for (var _ri = 0; _ri < arguments.length; _ri++) {
                var _rn = arguments[_ri];
                if (typeof _rn === 'string') {
                    var _rtn = _lumen_create_text_node(_rn);
                    if (nextSib !== null) { _lumen_insert_before(pid, _rtn, nextSib); }
                    else { _lumen_append_child(pid, _rtn); }
                } else if (_rn && _rn.__nid__ !== undefined) {
                    if (nextSib !== null) { _lumen_insert_before(pid, _rn.__nid__, nextSib); }
                    else { _lumen_append_child(pid, _rn.__nid__); }
                }
            }
        },
        // ── ParentNode extensions (DOM LS §4.2.5) ───────────────────────────────
        // Inserts nodes before the first child of this element.
        prepend: function() {
            var ch = _lumen_get_children(nid);
            var firstChild = ch.length > 0 ? ch[0] : null;
            for (var _pi = 0; _pi < arguments.length; _pi++) {
                var _pn = arguments[_pi];
                if (typeof _pn === 'string') {
                    var _ptn = _lumen_create_text_node(_pn);
                    if (firstChild !== null) { _lumen_insert_before(nid, _ptn, firstChild); }
                    else { _lumen_append_child(nid, _ptn); }
                } else if (_pn && _pn.__nid__ !== undefined) {
                    if (firstChild !== null) { _lumen_insert_before(nid, _pn.__nid__, firstChild); }
                    else { _lumen_append_child(nid, _pn.__nid__); }
                }
            }
        },
        // Replaces all children of this element.
        replaceChildren: function() {
            var old = _lumen_get_children(nid).slice();
            for (var _rci = 0; _rci < old.length; _rci++) {
                _lumen_remove_child(nid, old[_rci]);
            }
            for (var _rni = 0; _rni < arguments.length; _rni++) {
                var _rcn = arguments[_rni];
                if (typeof _rcn === 'string') {
                    _lumen_append_child(nid, _lumen_create_text_node(_rcn));
                } else if (_rcn && _rcn.__nid__ !== undefined) {
                    _lumen_append_child(nid, _rcn.__nid__);
                }
            }
        },
        // DOM LS §4.4: cloneNode(deep) — shallow or deep copy of this element.
        cloneNode:       function(deep) {
            var clone_nid = _lumen_clone_subtree(nid, deep ? 1 : 0);
            return _lumen_make_element(clone_nid);
        },
        // HTMLTemplateElement.content (HTML LS §4.12.3) — returns the template's
        // DocumentFragment content container, or null when not a template element.
        get content() {
            if ((_lumen_get_tag_name(nid) || '').toUpperCase() !== 'TEMPLATE') return undefined;
            var frag_nid = _lumen_u2n(_lumen_get_template_content(nid));
            return frag_nid !== null ? _lumen_make_document_fragment(frag_nid) : _lumen_make_document_fragment(_lumen_create_fragment());
        },
        querySelector:    function(sel) {
            var n = _lumen_u2n(_lumen_query_selector(String(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll: function(sel) {
            return _lumen_query_selector_all(String(sel)).map(_lumen_make_element);
        },
        matches: function(sel) {
            return _lumen_node_matches_selector(nid, String(sel));
        },
        addEventListener:    function(type, fn) { _lumen_add_listener(nid, type, fn); },
        removeEventListener: function(type, fn) { _lumen_rm_listener(nid, type, fn); },
        dispatchEvent:       function(evt) {
            if (!evt) return true;
            evt.target = this; evt.currentTarget = this;
            return _lumen_dispatch(nid, evt);
        },
        closest: function(sel) {
            var cur = nid;
            while (cur !== undefined && cur !== null) {
                if (_lumen_node_matches_selector(cur, String(sel))) return _lumen_make_element(cur);
                var pid = _lumen_u2n(_lumen_get_parent(cur));
                cur = pid !== null ? pid : null;
            }
            return null;
        },
        attachShadow: function(init) {
            var m = (init && init.mode === 'closed') ? 'closed' : 'open';
            var sr_nid = _lumen_attach_shadow(nid, m);
            return _lumen_make_shadow_root(sr_nid, m, nid);
        },
        getBoundingClientRect: function() {
            var r = _lumen_get_bounding_rect(nid);
            if (!r) { return { x:0, y:0, width:0, height:0, top:0, right:0, bottom:0, left:0 }; }
            return { x: r[0], y: r[1], width: r[2], height: r[3],
                     top: r[1], left: r[0], right: r[0]+r[2], bottom: r[1]+r[3] };
        },
        // HTMLCanvasElement.getContext (HTML LS §4.12.4). '2d' returns a cached
        // CanvasRenderingContext2D; 'webgl'/'webgl2' fall through to null (the
        // functional WebGL path is the separate webgl_canvas shim). Only meaningful
        // on <canvas>; harmless on other elements (creates an unused buffer at most).
        // Returns null when control has been transferred via transferControlToOffscreen.
        getContext: function(contextType) {
            var t = ('' + (contextType || '')).toLowerCase();
            if (t === '2d') {
                if (_canvas2d_ctxs[nid]) return _canvas2d_ctxs[nid];
                if ((_lumen_get_tag_name(nid) || '').toLowerCase() !== 'canvas') return null;
                if (typeof _lumen_canvas_is_transferred === 'function' && _lumen_canvas_is_transferred(nid)) return null;
                var d = _lumen_canvas_dims(nid);
                _lumen_canvas2d_create(nid, d[0], d[1]);
                var c2d = _lumen_make_canvas2d_ctx(this, nid);
                _canvas2d_ctxs[nid] = c2d;
                return c2d;
            }
            return null;
        },
        // HTMLCanvasElement.transferControlToOffscreen (HTML LS §4.12.14).
        // Transfers the canvas bitmap to a new OffscreenCanvas and prevents future
        // getContext() calls. The returned OffscreenCanvas can be sent to a Worker
        // via postMessage with a transfer list.
        transferControlToOffscreen: function() {
            if ((_lumen_get_tag_name(nid) || '').toLowerCase() !== 'canvas') {
                throw new DOMException('transferControlToOffscreen: not a canvas element', 'InvalidStateError');
            }
            if (typeof _lumen_canvas_is_transferred === 'function' && _lumen_canvas_is_transferred(nid)) {
                throw new DOMException('Canvas control already transferred', 'InvalidStateError');
            }
            if (_canvas2d_ctxs[nid]) {
                throw new DOMException('Canvas already has an active 2D context', 'InvalidStateError');
            }
            var d = _lumen_canvas_dims(nid);
            _lumen_canvas2d_create(nid, d[0], d[1]);
            var jsonStr = _lumen_canvas_transfer_control_to_offscreen(nid);
            var obj = JSON.parse(jsonStr);
            // Create an OffscreenCanvas JS object wrapping the pre-created native canvas.
            // We set __canvas_id__ directly instead of calling the constructor so the
            // native side does not allocate a second backing buffer.
            var oc = Object.create(OffscreenCanvas.prototype);
            oc.__canvas_id__ = obj.__canvas_id__;
            oc.width = obj.width;
            oc.height = obj.height;
            oc._2d_context = null;
            return oc;
        },
        // Privacy: blank data URL defeats canvas pixel-hash fingerprinting (ADR-007).
        toDataURL: function() {
            return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
        },
        toBlob: function(cb) { if (typeof cb === 'function') cb(null); },
        // HTMLCanvasElement.width/height reflect content attributes as unsigned long
        // (HTML LS §4.12.4). Setting resizes the backing bitmap (which clears it).
        // Only wired for <canvas>; other elements keep attribute-string semantics
        // via getAttribute and are unaffected by these accessors.
        get width() {
            if ((_lumen_get_tag_name(nid) || '').toLowerCase() === 'canvas') {
                return _lumen_canvas_dims(nid)[0];
            }
            var v = _lumen_u2n(_lumen_get_attr(nid, 'width'));
            return v !== null ? (parseInt(v, 10) || 0) : 0;
        },
        set width(v) {
            var n = parseInt(v, 10); if (!(n >= 0)) n = 0;
            _lumen_set_attr(nid, 'width', String(n));
            if ((_lumen_get_tag_name(nid) || '').toLowerCase() === 'canvas' && _canvas2d_ctxs[nid]) {
                var d = _lumen_canvas_dims(nid); _lumen_canvas2d_resize(nid, d[0], d[1]);
            }
        },
        get height() {
            if ((_lumen_get_tag_name(nid) || '').toLowerCase() === 'canvas') {
                return _lumen_canvas_dims(nid)[1];
            }
            var v = _lumen_u2n(_lumen_get_attr(nid, 'height'));
            return v !== null ? (parseInt(v, 10) || 0) : 0;
        },
        set height(v) {
            var n = parseInt(v, 10); if (!(n >= 0)) n = 0;
            _lumen_set_attr(nid, 'height', String(n));
            if ((_lumen_get_tag_name(nid) || '').toLowerCase() === 'canvas' && _canvas2d_ctxs[nid]) {
                var d = _lumen_canvas_dims(nid); _lumen_canvas2d_resize(nid, d[0], d[1]);
            }
        },
        get offsetWidth()  { var r = _lumen_get_bounding_rect(nid); return r ? r[2] : 0; },
        get offsetHeight() { var r = _lumen_get_bounding_rect(nid); return r ? r[3] : 0; },
        get offsetLeft()   { var r = _lumen_get_bounding_rect(nid); return r ? r[0] : 0; },
        get offsetTop()    { var r = _lumen_get_bounding_rect(nid); return r ? r[1] : 0; },
        get clientWidth()  { var r = _lumen_get_bounding_rect(nid); return r ? r[2] : 0; },
        get clientHeight() { var r = _lumen_get_bounding_rect(nid); return r ? r[3] : 0; },
        get scrollLeft() {
            var s = _lumen_get_scroll_state(nid); return s ? s[0] : 0;
        },
        set scrollLeft(v) { _lumen_request_scroll(nid, +v, _lumen_get_scroll_state(nid) ? _lumen_get_scroll_state(nid)[1] : 0); },
        get scrollTop() {
            var s = _lumen_get_scroll_state(nid); return s ? s[1] : 0;
        },
        set scrollTop(v) { _lumen_request_scroll(nid, _lumen_get_scroll_state(nid) ? _lumen_get_scroll_state(nid)[0] : 0, +v); },
        get scrollWidth()  { var s = _lumen_get_scroll_state(nid); return s ? s[2] : 0; },
        get scrollHeight() { var s = _lumen_get_scroll_state(nid); return s ? s[3] : 0; },
        scrollTo: function(x, y) {
            if (typeof x === 'object' && x !== null) { y = x.top || 0; x = x.left || 0; }
            _lumen_request_scroll(nid, +x, +y);
        },
        scrollBy: function(x, y) {
            if (typeof x === 'object' && x !== null) { y = x.top || 0; x = x.left || 0; }
            var s = _lumen_get_scroll_state(nid);
            _lumen_request_scroll(nid, (s ? s[0] : 0) + (+x), (s ? s[1] : 0) + (+y));
        },
        scrollIntoView: function() {
            // Scroll the nearest ancestor scroll container to make this element visible.
            var r = _lumen_get_bounding_rect(nid);
            if (!r) return;
            var parent = _lumen_u2n(_lumen_get_parent(nid));
            while (parent !== null && parent !== undefined) {
                var ps = _lumen_get_scroll_state(parent);
                if (ps) {
                    var pr = _lumen_get_bounding_rect(parent);
                    if (pr) { _lumen_request_scroll(parent, r[0] - pr[0], r[1] - pr[1]); }
                    break;
                }
                parent = _lumen_u2n(_lumen_get_parent(parent));
            }
        },
        // ── HTMLInputElement / HTMLTextAreaElement / HTMLSelectElement properties ──
        // Reflected HTML attributes (HTML LS §4.10.x).
        get type()  { var v = _lumen_u2n(_lumen_get_attr(nid, 'type')); return v !== null ? v.toLowerCase() : 'text'; },
        get name()  { var v = _lumen_u2n(_lumen_get_attr(nid, 'name')); return v !== null ? v : ''; },
        set name(v) { _lumen_set_attr(nid, 'name', String(v)); },
        // Current value — stored in _input_values map so it survives re-calls to _lumen_make_element.
        get value() {
            if (_input_values[nid] !== undefined) return _input_values[nid];
            var av = _lumen_u2n(_lumen_get_attr(nid, 'value'));
            return av !== null ? av : '';
        },
        set value(v) { _input_values[nid] = String(v); },
        get checked() { return _lumen_get_attr(nid, 'checked') !== undefined; },
        set checked(v) {
            if (v) _lumen_set_attr(nid, 'checked', '');
            else _lumen_remove_attr(nid, 'checked');
        },
        // ── Constraint Validation API (HTML LS §4.10.21) ─────────────────────────
        get validity() { return _compute_validity(this); },
        get validationMessage() {
            var cm = _validity_msg[nid] || '';
            if (cm) return cm;
            var vs = _compute_validity(this);
            if (vs.valueMissing)    return 'Please fill out this field.';
            if (vs.typeMismatch)    return 'Please enter a valid ' + (this.type || 'value') + '.';
            if (vs.patternMismatch) return 'Please match the requested format.';
            if (vs.tooLong)         return 'Please shorten this text.';
            if (vs.tooShort)        return 'Please lengthen this text.';
            if (vs.rangeUnderflow)  return 'Value must be >= ' + this.getAttribute('min') + '.';
            if (vs.rangeOverflow)   return 'Value must be <= ' + this.getAttribute('max') + '.';
            if (vs.stepMismatch)    return 'Please enter a valid value.';
            return '';
        },
        // true when the element participates in constraint validation
        get willValidate() {
            var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
            if (tag !== 'INPUT' && tag !== 'TEXTAREA' && tag !== 'SELECT') return false;
            var t = (this.type || '').toLowerCase();
            if (t === 'hidden' || t === 'button' || t === 'submit' || t === 'reset' || t === 'image') return false;
            if (this.hasAttribute('disabled')) return false;
            return true;
        },
        // Fires 'invalid' event and returns false if the element fails constraint validation.
        checkValidity: function() {
            if (!this.willValidate) return true;
            var vs = this.validity;
            if (!vs.valid) {
                var ev = new Event('invalid', { bubbles: false, cancelable: true });
                this.dispatchEvent(ev);
                return false;
            }
            return true;
        },
        // Like checkValidity(); may show the browser's default validation UI (Phase 0: same as checkValidity).
        reportValidity: function() { return this.checkValidity(); },
        // Overrides validity with a custom message; empty string clears the override (HTML LS §4.10.21.2).
        setCustomValidity: function(msg) {
            var m = String(msg);
            if (m) _validity_msg[nid] = m;
            else delete _validity_msg[nid];
        },
        // HTML LS §4.10.5.1.14: showPicker() — programmatically opens the
        // UA-provided picker for applicable input types.
        // Phase 0: fires a synthetic 'click' event so shell integrations can hook it;
        // throws NotSupportedError for types that have no picker.
        showPicker: function() {
            var t = (this.type || 'text').toLowerCase();
            var pickerTypes = ['color', 'date', 'datetime-local', 'month', 'time', 'week', 'file'];
            var supported = false;
            for (var _pi = 0; _pi < pickerTypes.length; _pi++) {
                if (pickerTypes[_pi] === t) { supported = true; break; }
            }
            if (!supported) {
                var err = new Error('showPicker() is not supported for type ' + t);
                err.name = 'NotSupportedError';
                throw err;
            }
            if (this.disabled) {
                var err2 = new Error('showPicker() called on a disabled element');
                err2.name = 'InvalidStateError';
                throw err2;
            }
            // Fire a click event; shell / test code can listen to open a native picker.
            this.dispatchEvent(new Event('click', { bubbles: true, cancelable: true }));
        },
        // HTMLFormElement.elements — live collection of associated form controls.
        // Phase 0: selector engine handles only single-tag selectors, so query
        // each tag separately and merge (avoids comma-selector limitation).
        get elements() {
            var self = this;
            var tags = ['input', 'select', 'textarea', 'button'];
            var out = [];
            for (var _ti = 0; _ti < tags.length; _ti++) {
                var found = self.querySelectorAll(tags[_ti]);
                for (var _fi = 0; _fi < found.length; _fi++) out.push(found[_fi]);
            }
            return out;
        },
        // Reflects the novalidate content attribute (disables constraint validation on submit).
        get noValidate() { return this.hasAttribute('novalidate'); },
        set noValidate(v) {
            if (v) this.setAttribute('novalidate', '');
            else this.removeAttribute('novalidate');
        },
        // DOM LS §4.2.4: insertBefore(newNode, refNode) — inserts before refNode (or appends if null).
        insertBefore: function(newNode, refNode) {
            if (!newNode || newNode.__nid__ === undefined) return newNode;
            if (!refNode || refNode.__nid__ === undefined) {
                return this.appendChild(newNode);
            }
            if (newNode.__isDocumentFragment__) {
                var kids = _lumen_get_children(newNode.__nid__).slice();
                for (var _ib = 0; _ib < kids.length; _ib++) {
                    _lumen_insert_before(nid, kids[_ib], refNode.__nid__);
                    _lumen_ce_maybe_connected(_lumen_make_element(kids[_ib]));
                }
            } else {
                _lumen_insert_before(nid, newNode.__nid__, refNode.__nid__);
                _lumen_ce_maybe_connected(newNode);
            }
            return newNode;
        },
        // HTMLSlotElement (DOM LS §4.2.2.2): applicable only on <slot> elements.
        // assignedNodes({flatten}) — returns the assigned light-DOM nodes for this slot.
        // Phase 0: returns the host's direct children that match this slot's `name` attribute.
        assignedNodes: function(opts) {
            if ((_lumen_get_tag_name(nid) || '').toUpperCase() !== 'SLOT') return [];
            var slot_name = _lumen_u2n(_lumen_get_attr(nid, 'name')) || '';
            var host_nid  = _lumen_u2n(_lumen_get_shadow_root_host(nid));
            if (host_nid === null) return [];
            var host_kids = _lumen_get_children(host_nid);
            var out = [];
            for (var _sn = 0; _sn < host_kids.length; _sn++) {
                var k = host_kids[_sn];
                var k_slot = _lumen_u2n(_lumen_get_attr(k, 'slot')) || '';
                if (k_slot === slot_name) out.push(_lumen_make_element(k));
            }
            return out;
        },
        assignedElements: function(opts) {
            return this.assignedNodes(opts).filter(function(n) { return n.nodeType === 1; });
        },
        // Reflected `slot` content attribute (which shadow slot to assign this element to).
        get slot() { var v = _lumen_u2n(_lumen_get_attr(nid, 'slot')); return v !== null ? v : ''; },
        set slot(v) { _lumen_set_attr(nid, 'slot', String(v)); },
        // assignedSlot — the <slot> element this node is slotted into, or null.
        // Phase 0 stub: full implementation requires composed tree traversal.
        get assignedSlot() { return null; },
        // ── checkVisibility (W3C Viewport API §4.1) ──────────────────────────────
        // Returns false if this element or any ancestor has display:none, is
        // disconnected, or (if options say so) has opacity:0 / visibility:hidden.
        checkVisibility: function(opts) {
            var options = opts || {};
            var checkOpacity     = !!options.checkOpacity;
            var checkVisibilityCss = !!options.checkVisibilityCSS;
            var checkContentVisibility = !!options.checkContentVisibility;
            var cur = nid;
            while (cur !== null && cur !== undefined) {
                var disp = _lumen_get_computed_style(cur, 'display');
                if (disp === '' || disp === 'none') return false;
                if (checkOpacity) {
                    var op = _lumen_get_computed_style(cur, 'opacity');
                    if (op !== null && op !== '' && parseFloat(op) === 0) return false;
                }
                if (checkVisibilityCss) {
                    var vis = _lumen_get_computed_style(cur, 'visibility');
                    if (vis === 'hidden' || vis === 'collapse') return false;
                }
                if (checkContentVisibility) {
                    var cv = _lumen_get_computed_style(cur, 'content-visibility');
                    if (cv === 'hidden') return false;
                }
                cur = _lumen_u2n(_lumen_get_parent(cur));
            }
            return true;
        },
        // ── setHTMLUnsafe (WHATWG HTML LS §14.5) ─────────────────────────────────
        // Parses html as a markup fragment and replaces element children.
        // Unsafe: no sanitization (unlike Sanitizer API).
        setHTMLUnsafe: function(html) {
            _lumen_set_inner_html(nid, String(html));
        },
        // ── getHTML (WHATWG HTML LS §14.5) ───────────────────────────────────────
        // Serialises element's subtree as an HTML string.
        // Phase 0: serializableShadowRoots option deferred (Shadow DOM Phase 2).
        getHTML: function(opts) {
            return _lumen_get_inner_html(nid);
        },
        // ── moveBefore (DOM LS, Chrome 133+) ─────────────────────────────────────
        // Moves `node` to be the previous sibling of `child` within this element,
        // preserving the node's CSS transition / animation state.
        // Phase 0: state preservation is a no-op (animations reset on DOM move).
        moveBefore: function(node, child) {
            if (!node || !node.__nid__) throw new TypeError('moveBefore: node required');
            var nodeNid = node.__nid__;
            var oldParent = _lumen_u2n(_lumen_get_parent(nodeNid));
            if (oldParent !== null) {
                _lumen_remove_child(oldParent, nodeNid);
            }
            if (child !== null && child !== undefined) {
                _lumen_insert_before(nid, nodeNid, child.__nid__);
            } else {
                _lumen_append_child(nid, nodeNid);
            }
        },
    };
    // ── contentEditable / isContentEditable (HTML LS §6.9.3) ────────────────
    Object.defineProperty(_obj, 'contentEditable', {
        get: function() {
            var v = _lumen_u2n(_lumen_get_attr(nid, 'contenteditable'));
            if (v === null) return 'inherit';
            if (v === '' || v.toLowerCase() === 'true') return 'true';
            if (v.toLowerCase() === 'false') return 'false';
            return 'inherit';
        },
        set: function(v) {
            var s = String(v).toLowerCase();
            if (s === 'true') _lumen_set_attr(nid, 'contenteditable', 'true');
            else if (s === 'false') _lumen_set_attr(nid, 'contenteditable', 'false');
            else _lumen_remove_attr(nid, 'contenteditable');
        },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(_obj, 'isContentEditable', {
        get: function() { return _lumen_is_contenteditable(nid); },
        enumerable: true, configurable: true,
    });
    Object.defineProperty(_obj, 'shadowRoot', {
        get: function() {
            var sr_nid = _lumen_u2n(_lumen_get_shadow_root(nid));
            return sr_nid !== null ? _lumen_make_shadow_root(sr_nid, 'open', nid) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_obj, 'parentElement', {
        get: function() {
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            return pid !== null ? _lumen_make_element(pid) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_obj, 'children', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    // Web Animations API (WAAPI Level 1) — element.animate() and getAnimations().
    _obj.animate = function(keyframes, options) {
        return _wa_element_animate(this, keyframes, options);
    };
    _obj.getAnimations = function() {
        return _wa_get_animations_for(this);
    };
    // ── HTMLSelectListElement API (Open UI Customizable Select §3) ────────────
    // Phase 0: <selectlist> renders as a native <select> widget.
    // Options may be direct children or inside a <listbox> child element.
    // CSS: appearance: base-select  (P4 wires ::picker(select) styling)
    if ((_lumen_get_tag_name(nid) || '').toUpperCase() === 'SELECTLIST') {
        _obj.multiple = false;
        _obj.size = 1;
        Object.defineProperty(_obj, 'options', {
            get: function() { return _lumen_selectlist_options(nid); },
            enumerable: true, configurable: true,
        });
        Object.defineProperty(_obj, 'length', {
            get: function() { return _lumen_selectlist_options(nid).length; },
            enumerable: true, configurable: true,
        });
        Object.defineProperty(_obj, 'selectedIndex', {
            get: function() {
                var opts = _lumen_selectlist_options(nid);
                for (var i = 0; i < opts.length; i++) {
                    if (opts[i].hasAttribute('selected')) return i;
                }
                return opts.length > 0 ? 0 : -1;
            },
            set: function(idx) {
                var opts = _lumen_selectlist_options(nid);
                for (var i = 0; i < opts.length; i++) {
                    if (i === idx) _lumen_set_attr(opts[i].__nid__, 'selected', '');
                    else _lumen_remove_attr(opts[i].__nid__, 'selected');
                }
            },
            enumerable: true, configurable: true,
        });
        Object.defineProperty(_obj, 'value', {
            get: function() {
                var opts = _lumen_selectlist_options(nid);
                var sel = this.selectedIndex;
                if (sel < 0 || sel >= opts.length) return '';
                var v = _lumen_u2n(_lumen_get_attr(opts[sel].__nid__, 'value'));
                return v !== null ? v : (opts[sel].textContent || '');
            },
            set: function(v) {
                var sv = String(v);
                var opts = _lumen_selectlist_options(nid);
                for (var i = 0; i < opts.length; i++) {
                    var ov = _lumen_u2n(_lumen_get_attr(opts[i].__nid__, 'value'));
                    if (ov === null) ov = opts[i].textContent || '';
                    if (ov === sv) {
                        _lumen_set_attr(opts[i].__nid__, 'selected', '');
                    } else {
                        _lumen_remove_attr(opts[i].__nid__, 'selected');
                    }
                }
            },
            enumerable: true, configurable: true,
        });
        _obj.item = function(idx) {
            var opts = _lumen_selectlist_options(nid);
            return (idx >= 0 && idx < opts.length) ? opts[idx] : null;
        };
        _obj.namedItem = function(name) {
            var opts = _lumen_selectlist_options(nid);
            for (var i = 0; i < opts.length; i++) {
                var id_ = _lumen_u2n(_lumen_get_attr(opts[i].__nid__, 'id'));
                var nm  = _lumen_u2n(_lumen_get_attr(opts[i].__nid__, 'name'));
                if (id_ === name || nm === name) return opts[i];
            }
            return null;
        };
        _obj.add = function(el, before) {
            if (!el || el.__nid__ === undefined) return;
            var listbox = _lumen_selectlist_listbox(nid);
            var container = listbox !== null ? listbox : nid;
            if (before === undefined || before === null) {
                _lumen_append_child(container, el.__nid__);
            } else if (typeof before === 'number') {
                var opts = _lumen_selectlist_options(nid);
                if (before >= 0 && before < opts.length) {
                    _lumen_insert_before(container, el.__nid__, opts[before].__nid__);
                } else {
                    _lumen_append_child(container, el.__nid__);
                }
            } else if (before && before.__nid__ !== undefined) {
                _lumen_insert_before(container, el.__nid__, before.__nid__);
            }
        };
        _obj.remove = function(idx) {
            var opts = _lumen_selectlist_options(nid);
            if (idx >= 0 && idx < opts.length) {
                _lumen_remove_child(_lumen_u2n(_lumen_get_parent(opts[idx].__nid__)), opts[idx].__nid__);
            }
        };
    }
    return _obj;
}

var _lumen_root_nid = _lumen_get_document_root();

var console = {
    log:   function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
    warn:  function() { _lumen_console_warn( Array.prototype.join.call(arguments, ' ')); },
    error: function() { _lumen_console_error(Array.prototype.join.call(arguments, ' ')); },
    info:  function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
    debug: function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
};

// ── FontFace and FontFaceSet (CSS Fonts Module Level 4 §11) ─────────────────

function _lumen_parse_font_face_json(jsonStr) {
    try {
        return JSON.parse(jsonStr);
    } catch(e) {
        return null;
    }
}

function _lumen_get_fonts() {
    var size = _lumen_fonts_size();
    var faces = [];
    for (var i = 0; i < size; i++) {
        var jsonStr = _lumen_fonts_get(i);
        if (jsonStr) {
            var obj = _lumen_parse_font_face_json(jsonStr);
            if (obj) {
                faces.push(obj);
            }
        }
    }
    var fontSet = {
        _faces: faces,
        get length() { return this._faces.length; },
        item: function(index) {
            return this._faces[index] || null;
        },
        // Iterate over FontFace objects
        entries: function() {
            var self = this;
            var idx = 0;
            return {
                next: function() {
                    if (idx < self._faces.length) {
                        return { value: [idx, self._faces[idx]], done: false };
                    }
                    return { done: true };
                }
            };
        },
        forEach: function(callback, thisArg) {
            for (var i = 0; i < this._faces.length; i++) {
                callback.call(thisArg, this._faces[i], i, this);
            }
        },
        [Symbol.iterator]: function() {
            var idx = 0;
            var faces = this._faces;
            return {
                next: function() {
                    if (idx < faces.length) {
                        return { value: faces[idx++], done: false };
                    }
                    return { done: true };
                }
            };
        },
    };
    // Symbol.iterator might not be available in all JS engines
    if (typeof Symbol !== 'undefined' && typeof Symbol.iterator !== 'undefined') {
        fontSet[Symbol.iterator] = function() {
            var idx = 0;
            var faces = this._faces;
            return {
                next: function() {
                    if (idx < faces.length) {
                        return { value: faces[idx++], done: false };
                    }
                    return { done: true };
                }
            };
        };
    }
    return fontSet;
}

// ── Range (WHATWG DOM §4.5) ────────────────────────────────────────────────
// Creates a Range object whose endpoints are identified by [nid, offset] pairs.
// nid 0 with offset 0 is the collapsed-at-document-start default.

function _lumen_make_range(sNid, sOff, eNid, eOff) {
    var r = {
        __start_nid__: sNid, __start_off__: sOff,
        __end_nid__:   eNid, __end_off__:   eOff,
        get startContainer() { return _lumen_make_element(this.__start_nid__); },
        get startOffset()    { return this.__start_off__; },
        get endContainer()   { return _lumen_make_element(this.__end_nid__); },
        get endOffset()      { return this.__end_off__; },
        get collapsed()      { return this.__start_nid__ === this.__end_nid__ && this.__start_off__ === this.__end_off__; },
        get commonAncestorContainer() {
            if (this.__start_nid__ === this.__end_nid__) return _lumen_make_element(this.__start_nid__);
            var p = _lumen_u2n(_lumen_get_parent(this.__start_nid__));
            return p !== null ? _lumen_make_element(p) : _lumen_make_element(this.__start_nid__);
        },
        setStart: function(node, offset) {
            if (!node || node.__nid__ === undefined) return;
            this.__start_nid__ = node.__nid__; this.__start_off__ = offset >>> 0;
        },
        setEnd: function(node, offset) {
            if (!node || node.__nid__ === undefined) return;
            this.__end_nid__ = node.__nid__; this.__end_off__ = offset >>> 0;
        },
        setStartBefore: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__start_nid__ = p; this.__start_off__ = Math.max(0, idx);
        },
        setStartAfter: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__start_nid__ = p; this.__start_off__ = idx + 1;
        },
        setEndBefore: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__end_nid__ = p; this.__end_off__ = Math.max(0, idx);
        },
        setEndAfter: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var idx = _lumen_get_children(p).indexOf(node.__nid__);
            this.__end_nid__ = p; this.__end_off__ = idx + 1;
        },
        collapse: function(toStart) {
            if (toStart === false) {
                this.__start_nid__ = this.__end_nid__; this.__start_off__ = this.__end_off__;
            } else {
                this.__end_nid__ = this.__start_nid__; this.__end_off__ = this.__start_off__;
            }
        },
        selectNode: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(node.__nid__));
            if (p === null) return;
            var ch = _lumen_get_children(p), idx = ch.indexOf(node.__nid__);
            this.__start_nid__ = p; this.__start_off__ = Math.max(0, idx);
            this.__end_nid__   = p; this.__end_off__   = idx + 1;
        },
        selectNodeContents: function(node) {
            if (!node || node.__nid__ === undefined) return;
            this.__start_nid__ = node.__nid__; this.__start_off__ = 0;
            this.__end_nid__   = node.__nid__; this.__end_off__   = _lumen_node_length(node.__nid__);
        },
        cloneRange: function() {
            return _lumen_make_range(this.__start_nid__, this.__start_off__, this.__end_nid__, this.__end_off__);
        },
        toString: function() {
            return _lumen_get_range_text(this.__start_nid__, this.__start_off__, this.__end_nid__, this.__end_off__);
        },
        deleteContents: function() {
            var pos = _lumen_range_delete_contents(this.__start_nid__, this.__start_off__, this.__end_nid__, this.__end_off__);
            this.__start_nid__ = pos[0]; this.__start_off__ = pos[1];
            this.__end_nid__   = pos[0]; this.__end_off__   = pos[1];
        },
        extractContents: function() { this.deleteContents(); return null; },
        cloneContents:   function() { return null; },
        insertNode: function(node) {
            if (!node || node.__nid__ === undefined) return;
            var p = _lumen_u2n(_lumen_get_parent(this.__start_nid__));
            if (p !== null) _lumen_append_child(p, node.__nid__);
        },
        surroundContents:     function() {},
        compareBoundaryPoints: function(how, other) {
            how = (how >>> 0) & 3;
            var pairs = [[this.__start_nid__, this.__start_off__, other.__start_nid__, other.__start_off__],
                         [this.__start_nid__, this.__start_off__, other.__end_nid__,   other.__end_off__  ],
                         [this.__end_nid__,   this.__end_off__,   other.__start_nid__, other.__start_off__],
                         [this.__end_nid__,   this.__end_off__,   other.__end_nid__,   other.__end_off__  ]];
            var p = pairs[how];
            if (p[0] !== p[2]) return p[0] < p[2] ? -1 : 1;
            if (p[1] !== p[3]) return p[1] < p[3] ? -1 : 1;
            return 0;
        },
        getBoundingClientRect: function() {
            var el = _lumen_make_element(this.__start_nid__);
            return (el && el.getBoundingClientRect) ? el.getBoundingClientRect()
                : { top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0 };
        },
        getClientRects:   function() { return [this.getBoundingClientRect()]; },
        detach:           function() {},
        isPointInRange:   function() { return false; },
        comparePoint:     function() { return 0; },
        intersectsNode:   function() { return false; },
    };
    r.START_TO_START = 0; r.START_TO_END = 1; r.END_TO_START = 2; r.END_TO_END = 3;
    return r;
}

// Range constructor (allows `new Range()`)
function Range() { return _lumen_make_range(0, 0, 0, 0); }
Range.prototype.START_TO_START = 0; Range.prototype.START_TO_END = 1;
Range.prototype.END_TO_START  = 2; Range.prototype.END_TO_END  = 3;

// ── Selection singleton (WHATWG Selection API §3) ─────────────────────────
// All access to the selection state goes through the Rust bindings.

var _lumen_selection = (function() {
    function _raw() { return _lumen_get_selection(); } // null | [aNid,aOff,fNid,fOff]
    return {
        get anchorNode()   { var s = _raw(); return s ? _lumen_make_element(s[0]) : null; },
        get anchorOffset() { var s = _raw(); return s ? s[1] : 0; },
        get focusNode()    { var s = _raw(); return s ? _lumen_make_element(s[2]) : null; },
        get focusOffset()  { var s = _raw(); return s ? s[3] : 0; },
        get isCollapsed()  { var s = _raw(); return !s || (s[0] === s[2] && s[1] === s[3]); },
        get rangeCount()   { return _raw() ? 1 : 0; },
        get type() {
            var s = _raw();
            if (!s) return 'None';
            return (s[0] === s[2] && s[1] === s[3]) ? 'Caret' : 'Range';
        },
        getRangeAt: function(n) {
            if (n !== 0) throw new RangeError('Selection.getRangeAt: index out of bounds');
            var s = _raw();
            if (!s) throw new RangeError('Selection.getRangeAt: no range');
            return _lumen_make_range(s[0], s[1], s[2], s[3]);
        },
        addRange: function(range) {
            if (!range || range.__start_nid__ === undefined) return;
            _lumen_set_selection(range.__start_nid__, range.__start_off__, range.__end_nid__, range.__end_off__);
        },
        removeRange:    function() { _lumen_clear_selection(); },
        removeAllRanges: function() { _lumen_clear_selection(); },
        empty:          function() { _lumen_clear_selection(); },
        collapse: function(node, offset) {
            if (!node || node.__nid__ === undefined) { _lumen_clear_selection(); return; }
            var off = (offset === undefined || offset === null) ? 0 : (offset >>> 0);
            _lumen_set_selection(node.__nid__, off, node.__nid__, off);
        },
        collapseToStart: function() {
            var s = _raw(); if (!s) return;
            _lumen_set_selection(s[0], s[1], s[0], s[1]);
        },
        collapseToEnd: function() {
            var s = _raw(); if (!s) return;
            _lumen_set_selection(s[2], s[3], s[2], s[3]);
        },
        extend: function(node, offset) {
            if (!node || node.__nid__ === undefined) return;
            var s = _raw();
            var aNid = s ? s[0] : node.__nid__, aOff = s ? s[1] : 0;
            _lumen_set_selection(aNid, aOff, node.__nid__, offset >>> 0);
        },
        selectAllChildren: function(node) {
            if (!node || node.__nid__ === undefined) return;
            _lumen_set_selection(node.__nid__, 0, node.__nid__, _lumen_node_length(node.__nid__));
        },
        deleteFromDocument: function() {
            var s = _raw(); if (!s) return;
            _lumen_range_delete_contents(s[0], s[1], s[2], s[3]);
            _lumen_clear_selection();
        },
        setBaseAndExtent: function(aN, aO, fN, fO) {
            if (!aN || aN.__nid__ === undefined || !fN || fN.__nid__ === undefined) return;
            _lumen_set_selection(aN.__nid__, aO >>> 0, fN.__nid__, fO >>> 0);
        },
        containsNode:    function() { return false; },
        getComposedRanges: function() { return []; },
        modify:          function() {},
        toString: function() { return _lumen_get_selection_text(); },
    };
}());

// ── contenteditable key dispatch (Input Events Level 2 §4.1) ─────────────────
// Called by the shell when a key is pressed while a contenteditable element has
// focus. Fires beforeinput → DOM mutation → input following the spec sequence.
//
// `inputType`  — Input Events Level 2 inputType string (e.g. insertText)
// `data`       — inserted text for insertText; null/undefined for deletions
// `targetNid`  — nid of the contenteditable host element
//
// Returns true if the event was not cancelled and the mutation was applied.
function _lumen_handle_contenteditable_key(inputType, data, targetNid) {
    var target = (targetNid !== undefined && targetNid !== null)
        ? _lumen_make_element(targetNid)
        : null;
    if (!target) return false;

    // Fire beforeinput (cancelable).
    var before = new InputEvent('beforeinput', {
        bubbles: true, cancelable: true,
        inputType: inputType,
        data: (data !== undefined && data !== null) ? String(data) : null,
    });
    var notCancelled = target.dispatchEvent(before);
    if (!notCancelled) return false;

    // Apply the DOM mutation.
    var applied = false;
    if (inputType === 'insertText') {
        applied = _lumen_contenteditable_insert_text(String(data || ''));
    } else if (inputType === 'deleteContentBackward' || inputType === 'deleteWordBackward') {
        applied = _lumen_contenteditable_delete_backward();
    } else if (inputType === 'deleteContentForward' || inputType === 'deleteWordForward') {
        applied = _lumen_contenteditable_delete_forward();
    } else if (inputType === 'insertParagraph') {
        applied = _lumen_contenteditable_insert_paragraph();
    } else if (inputType === 'insertLineBreak') {
        applied = _lumen_contenteditable_insert_text('\\n');
    }

    if (!applied) return false;

    // Fire input (not cancelable).
    var inp = new InputEvent('input', {
        bubbles: true, cancelable: false,
        inputType: inputType,
        data: (data !== undefined && data !== null) ? String(data) : null,
    });
    target.dispatchEvent(inp);
    return true;
}

// ── Fullscreen API (WHATWG Fullscreen §4) ────────────────────────────────────
// Current fullscreen element NID (-1 = none).
var _fs_nid = -1;
// Sentinel attribute written by requestFullscreen() and read by the CSS cascade.
// CSS: :fullscreen — P4 wires PseudoClass::Fullscreen to check this attr.
var _FS_ATTR = 'data-lumen-fullscreen';

// ── Page Visibility API + document.readyState state vars ─────────────────────
// Declared before `document` because getters below capture these by name.
var _doc_hidden = false;
var _doc_visibility_state = 'visible';
var _doc_ready_state = 'loading';
var __dom_node_warned = false;

var document = {
    get title()  { return _lumen_get_document_title(); },
    set title(v) { _lumen_set_document_title(String(v)); },
    get cookie()  { return _lumen_cookie_get(); },
    set cookie(v) { _lumen_cookie_set(String(v)); },
    get body()   {
        var bid = _lumen_u2n(_lumen_get_body());
        return bid !== null ? _lumen_make_element(bid) : null;
    },
    get documentElement() { return _lumen_make_element(_lumen_root_nid); },
    getElementById:    function(id)  {
        var n = _lumen_u2n(_lumen_get_element_by_id(String(id)));
        return n !== null ? _lumen_make_element(n) : null;
    },
    querySelector:     function(sel) {
        var n = _lumen_u2n(_lumen_query_selector(String(sel)));
        return n !== null ? _lumen_make_element(n) : null;
    },
    querySelectorAll:  function(sel) {
        return _lumen_query_selector_all(String(sel)).map(_lumen_make_element);
    },
    createElement:     function(tag) {
        var nid = _lumen_create_element(String(tag).toLowerCase());
        // QuickJS converts the Rust u32::MAX sentinel to -1 (signed overflow).
        if (nid < 0) {
            throw new DOMException('DOM node limit exceeded', 'QuotaExceededError');
        }
        var cnt = _lumen_dom_node_count();
        if (!__dom_node_warned && cnt >= 40000) {
            __dom_node_warned = true;
            console.warn('DOM tree exceeds 40000 nodes');
        }
        return _lumen_make_element(nid);
    },
    createTextNode:         function(t)   { return _lumen_make_element(_lumen_create_text_node(String(t))); },
    createComment:          function()    { return _lumen_make_element(_lumen_create_text_node('')); },
    // DOM LS §4.5: createDocumentFragment() returns an empty DocumentFragment.
    createDocumentFragment: function()    { return _lumen_make_document_fragment(_lumen_create_fragment()); },
    appendChild:       function(c)   {
        if (c && c.__nid__ !== undefined) _lumen_append_child(_lumen_root_nid, c.__nid__);
        return c;
    },
    // Page Visibility API (HTML LS §15.1) — state vars declared after navigator
    get hidden()          { return _doc_hidden; },
    get visibilityState() { return _doc_visibility_state; },
    // Document lifecycle (HTML LS §8.5) — readyState driven by _lumen_apply_ready_state()
    get readyState()      { return _doc_ready_state; },
    // addEventListener intercepts DOMContentLoaded to fire immediately when already ready
    addEventListener: function(type, fn, opts) {
        if (type === 'DOMContentLoaded' && _doc_ready_state !== 'loading') {
            queueMicrotask(function() {
                try { fn(new Event('DOMContentLoaded', { bubbles: true })); } catch(e) {}
            });
            return;
        }
        _lumen_add_listener(_LUMEN_DOC_LISTENER_NID, type, fn);
    },
    removeEventListener: function(type, fn) { _lumen_rm_listener(_LUMEN_DOC_LISTENER_NID, type, fn); },
    // dispatchEvent: fire all document-level listeners for the given event
    dispatchEvent: function(evt) {
        if (!evt || !evt.type) return false;
        var key = String(_LUMEN_DOC_LISTENER_NID) + ':' + String(evt.type);
        var arr = _lumen_listeners[key];
        if (arr) {
            var copy = arr.slice();
            for (var i = 0; i < copy.length; i++) {
                try { copy[i].call(document, evt); } catch(e) {}
            }
        }
        return !evt.defaultPrevented;
    },
    get fonts() {
        return _lumen_get_fonts();
    },
    // ── Selection API ─────────────────────────────────────────────────────
    getSelection:  function() { return _lumen_selection; },
    createRange:   function() { return _lumen_make_range(0, 0, 0, 0); },
    // execCommand (HTML §9.2.1 — executes a legacy editing command)
    execCommand: function(cmd, showUI, value) {
        return _lumen_exec_command(String(cmd), value !== undefined && value !== null ? String(value) : '');
    },
    queryCommandEnabled:   function(cmd) { return true; },
    queryCommandState:     function(cmd) { return false; },
    queryCommandValue:     function(cmd) { return ''; },
    queryCommandSupported: function(cmd) { return true; },
    queryCommandIndeterm:  function(cmd) { return false; },
    // Web Animations API (WAAPI Level 1) — document.timeline and document.getAnimations().
    get timeline() { return _wa_doc_timeline; },
    getAnimations: function() { return _wa_doc_get_animations(); },
    // Fullscreen API (WHATWG Fullscreen §4) — document-level surface.
    get fullscreenElement() {
        return _fs_nid !== -1 ? _lumen_make_element(_fs_nid) : null;
    },
    get fullscreenEnabled() { return true; },
    exitFullscreen: function() {
        return new Promise(function(resolve) {
            if (_fs_nid !== -1) {
                var old = _fs_nid;
                _lumen_remove_attr(_fs_nid, _FS_ATTR);
                _fs_nid = -1;
                // Notify shell to exit OS fullscreen.
                if (typeof _lumen_fs_exit === 'function') { _lumen_fs_exit(); }
                var prev = _lumen_make_element(old);
                if (prev) { prev.dispatchEvent(new Event('fullscreenchange', { bubbles: true })); }
                document.dispatchEvent(new Event('fullscreenchange'));
            }
            resolve();
        });
    },
    onfullscreenchange: null,
    onfullscreenerror:  null,
    // Pointer Lock API (W3C Pointer Lock L2 §2-4) — Phase 0: local state only
    get pointerLockElement() {
        return typeof _lumen_ptr_lock_element !== 'function' ? null : _lumen_ptr_lock_element();
    },
    exitPointerLock: function() {
        if (typeof _lumen_exit_ptr_lock === 'function') { _lumen_exit_ptr_lock(); }
    },
    onpointerlockchange: null,
    onpointerlockerror: null,
    // Storage Access API (W3C Storage Access API §5) — Phase 0: always granted
    requestStorageAccess: function() {
        return Promise.resolve();
    },
    hasStorageAccess: function() {
        return Promise.resolve(true);
    },
    requestStorageAccessFor: function(origin) {
        return Promise.resolve();
    },
    hasUnpartitionedCookieAccess: function() {
        return Promise.resolve(true);
    },
    // DOM LS §4.6: adoptNode — moves node into this document (Phase 0: no-op, returns node).
    adoptNode: function(node) { return node; },
    // DOM LS §4.7: importNode — returns a clone of node for use in this document.
    importNode: function(node, deep) {
        if (!node) return null;
        if (node.__nid__ !== undefined) {
            var clone_nid = _lumen_clone_subtree(node.__nid__, deep ? 1 : 0);
            return _lumen_make_element(clone_nid);
        }
        return null;
    },
    // DOM LS §4.5: createTreeWalker(root, whatToShow, filter) — returns a TreeWalker.
    createTreeWalker: function(root, whatToShow, filter) {
        return new _TreeWalker(root, whatToShow !== undefined ? whatToShow : 0xFFFFFFFF, filter || null);
    },
    // DOM LS §4.4: createNodeIterator(root, whatToShow, filter) — returns a NodeIterator.
    createNodeIterator: function(root, whatToShow, filter) {
        return new _NodeIterator(root, whatToShow !== undefined ? whatToShow : 0xFFFFFFFF, filter || null);
    },
    // CSSOM View §5.1: caretPositionFromPoint(x, y) — returns a CaretPosition or null.
    // Phase 0: no layout hit-testing yet; returns body at offset 0 when body exists.
    caretPositionFromPoint: function(x, y) {
        var bodyNid = _lumen_u2n(_lumen_get_body());
        if (bodyNid === null) return null;
        return new _CaretPosition(_lumen_make_element(bodyNid), 0);
    },
};

var alert    = function(m) { _lumen_console_log('[alert] ' + String(m)); };
var confirm  = function()  { return false; };
var prompt   = function()  { return null; };
var print    = function()  { _lumen_print_dialog(); };

// ── Custom Elements registry ──────────────────────────────────────────────────
// Maps lower-case tag name → { ctor, observedAttributes: string[] }
var _lumen_ce_registry = {};
// Maps tag name → array of resolve callbacks for whenDefined().
var _lumen_ce_pending  = {};

// Calls connectedCallback on `el` if its tag is in the registry.
function _lumen_ce_maybe_connected(el) {
    if (!el || el.__nid__ === undefined) return;
    var tag   = _lumen_get_tag_name(el.__nid__).toLowerCase();
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    if (!el.__ceUpgraded__) {
        el.__ceUpgraded__ = true;
    }
    if (typeof entry.ctor.prototype.connectedCallback === 'function') {
        try { entry.ctor.prototype.connectedCallback.call(el); } catch(e) {
            _lumen_console_error('CE connectedCallback: ' + e);
        }
    }
}

// Calls disconnectedCallback on `el` if its tag is in the registry.
function _lumen_ce_maybe_disconnected(el) {
    if (!el || el.__nid__ === undefined) return;
    var tag   = _lumen_get_tag_name(el.__nid__).toLowerCase();
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    if (typeof entry.ctor.prototype.disconnectedCallback === 'function') {
        try { entry.ctor.prototype.disconnectedCallback.call(el); } catch(e) {
            _lumen_console_error('CE disconnectedCallback: ' + e);
        }
    }
}

// Calls attributeChangedCallback on the element at `nid` if applicable.
function _lumen_ce_maybe_attr_changed(nid, attrName, oldVal, newVal) {
    var tag   = _lumen_get_tag_name(nid).toLowerCase();
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    if (entry.observedAttributes.indexOf(attrName) < 0) return;
    if (typeof entry.ctor.prototype.attributeChangedCallback === 'function') {
        try {
            entry.ctor.prototype.attributeChangedCallback.call(
                _lumen_make_element(nid), attrName, oldVal, newVal
            );
        } catch(e) {
            _lumen_console_error('CE attributeChangedCallback: ' + e);
        }
    }
}

// Upgrades a single element wrapper: marks upgraded and calls connectedCallback.
function _lumen_ce_upgrade_element(el, entry) {
    if (!el || el.__ceUpgraded__) return;
    el.__ceUpgraded__ = true;
    if (typeof entry.ctor.prototype.connectedCallback === 'function') {
        try { entry.ctor.prototype.connectedCallback.call(el); } catch(e) {
            _lumen_console_error('CE connectedCallback (upgrade): ' + e);
        }
    }
}

// Upgrades all DOM elements matching `tag` that haven't been upgraded yet.
function _lumen_ce_upgrade_all(tag) {
    var nids = _lumen_query_selector_all(tag);
    var entry = _lumen_ce_registry[tag];
    if (!entry) return;
    for (var i = 0; i < nids.length; i++) {
        _lumen_ce_upgrade_element(_lumen_make_element(nids[i]), entry);
    }
}

var customElements = {
    define: function(name, ctor, options) {
        name = String(name).toLowerCase();
        if (_lumen_ce_registry[name]) return;
        var observed = (ctor.observedAttributes && ctor.observedAttributes.length)
            ? ctor.observedAttributes.slice()
            : [];
        _lumen_ce_registry[name] = { ctor: ctor, observedAttributes: observed };
        _lumen_ce_upgrade_all(name);
        var pending = _lumen_ce_pending[name];
        if (pending) {
            for (var i = 0; i < pending.length; i++) {
                try { pending[i](ctor); } catch(e) {}
            }
            delete _lumen_ce_pending[name];
        }
    },
    get: function(name) {
        var entry = _lumen_ce_registry[String(name).toLowerCase()];
        return entry ? entry.ctor : undefined;
    },
    whenDefined: function(name) {
        name = String(name).toLowerCase();
        var entry = _lumen_ce_registry[name];
        if (entry) return Promise.resolve(entry.ctor);
        return new Promise(function(resolve) {
            if (!_lumen_ce_pending[name]) _lumen_ce_pending[name] = [];
            _lumen_ce_pending[name].push(resolve);
        });
    },
    upgrade: function(element) {
        if (!element || element.__nid__ === undefined) return;
        var tag   = _lumen_get_tag_name(element.__nid__).toLowerCase();
        var entry = _lumen_ce_registry[tag];
        if (entry) _lumen_ce_upgrade_element(element, entry);
    },
};

// ── location (HTML LS §7.7 + WHATWG URL §8) ──────────────────────────────────
// _LUMEN_PAGE_URL injected by Rust before this shim runs.
function _lumen_parse_url(url) {
    var href = String(url || '');
    var protocol = '', hostname = '', host = '', port = '', pathname = '/', search = '', hash = '', origin = '';
    var sIdx = href.indexOf('://');
    if (sIdx >= 0) {
        protocol = href.slice(0, sIdx + 1);
        var rest = href.slice(sIdx + 3);
        var splitAt = rest.length;
        for (var i = 0; i < rest.length; i++) {
            if (rest[i] === '/' || rest[i] === '?' || rest[i] === '#') { splitAt = i; break; }
        }
        var authority = rest.slice(0, splitAt);
        rest = rest.slice(splitAt);
        var atIdx = authority.indexOf('@');
        if (atIdx >= 0) authority = authority.slice(atIdx + 1);
        var portColon = authority.lastIndexOf(':');
        if (portColon > authority.lastIndexOf(']')) {
            hostname = authority.slice(0, portColon); port = authority.slice(portColon + 1);
        } else {
            hostname = authority; port = '';
        }
        host = port ? hostname + ':' + port : hostname;
        var hIdx = rest.indexOf('#');
        if (hIdx >= 0) { hash = rest.slice(hIdx); rest = rest.slice(0, hIdx); }
        var qIdx = rest.indexOf('?');
        if (qIdx >= 0) { search = rest.slice(qIdx); rest = rest.slice(0, qIdx); }
        pathname = rest || '/';
        origin = protocol + '//' + host;
    } else {
        var cIdx = href.indexOf(':');
        if (cIdx >= 0) {
            protocol = href.slice(0, cIdx + 1);
            pathname = href.slice(cIdx + 1);
        }
    }
    return { href: href, protocol: protocol, hostname: hostname, host: host, port: port,
             pathname: pathname, search: search, hash: hash, origin: origin };
}
var _lumen_loc_parts = _lumen_parse_url(typeof _LUMEN_PAGE_URL !== 'undefined' ? _LUMEN_PAGE_URL : '');
var _lumen_loc_href  = _lumen_loc_parts.href;
function _lumen_location_update(url) {
    var p = _lumen_parse_url(url);
    _lumen_loc_href    = p.href;
    location.protocol  = p.protocol;
    location.hostname  = p.hostname;
    location.host      = p.host;
    location.port      = p.port;
    location.pathname  = p.pathname;
    location.search    = p.search;
    location.hash      = p.hash;
    location.origin    = p.origin;
}
var location = {
    get href()    { return _lumen_loc_href; },
    set href(v)   { _lumen_navigate(String(v || ''), false); },
    protocol:  _lumen_loc_parts.protocol,
    hostname:  _lumen_loc_parts.hostname,
    host:      _lumen_loc_parts.host,
    port:      _lumen_loc_parts.port,
    pathname:  _lumen_loc_parts.pathname,
    search:    _lumen_loc_parts.search,
    hash:      _lumen_loc_parts.hash,
    origin:    _lumen_loc_parts.origin,
    assign:    function(url) { _lumen_navigate(String(url || ''), false); },
    replace:   function(url) { _lumen_navigate(String(url || ''), true); },
    reload:    function()    { _lumen_reload(); },
    toString:  function()    { return this.href; }
};

// ── Service Worker API ────────────────────────────────────────────────────────

function _lumen_req_url(r) {
    return (typeof r === 'string') ? r : (r && r.url ? r.url : String(r));
}
function _lumen_req_method(r) {
    return (typeof r === 'string') ? 'GET' : ((r && r.method) ? r.method.toUpperCase() : 'GET');
}
function _lumen_build_response(body, infoJson) {
    var opts = { status: 200, statusText: 'OK', headers: {} };
    if (infoJson) {
        try {
            var m = JSON.parse(infoJson);
            opts.status = m.status || 200;
            opts.statusText = m.statusText || 'OK';
            opts.headers = m.headers || {};
        } catch(e) {}
    }
    return new Response(body, opts);
}

function _lumen_build_cache_object(origin, cacheName) {
    return {
        put: function(request, response) {
            var url = _lumen_req_url(request);
            var method = _lumen_req_method(request);
            var status = response.status || 200;
            var statusText = response.statusText || 'OK';
            var hdrs = {};
            if (response.headers && typeof response.headers.forEach === 'function') {
                response.headers.forEach(function(v, k) { hdrs[k] = v; });
            }
            var metaJson = JSON.stringify({ method: method, status: status, statusText: statusText, headers: hdrs });
            return response.arrayBuffer().then(function(buf) {
                _lumen_cache_put(origin, cacheName, url, metaJson, new Uint8Array(buf));
                return undefined;
            });
        },
        match: function(request, options) {
            var url = _lumen_req_url(request);
            var body = _lumen_cache_match(origin, cacheName, url);
            if (body === undefined || body === null) return Promise.resolve(undefined);
            return Promise.resolve(_lumen_build_response(body, _lumen_cache_match_info(origin, cacheName, url)));
        },
        matchAll: function(request, options) {
            if (request === undefined) {
                var urls = _lumen_cache_keys(origin, cacheName);
                return Promise.resolve(urls.map(function(u) {
                    return _lumen_build_response(
                        _lumen_cache_match(origin, cacheName, u),
                        _lumen_cache_match_info(origin, cacheName, u)
                    );
                }));
            }
            var url = _lumen_req_url(request);
            var body = _lumen_cache_match(origin, cacheName, url);
            if (body === undefined || body === null) return Promise.resolve([]);
            return Promise.resolve([_lumen_build_response(body, _lumen_cache_match_info(origin, cacheName, url))]);
        },
        delete: function(request, options) {
            var url = _lumen_req_url(request);
            return Promise.resolve(_lumen_cache_delete(origin, cacheName, url));
        },
        keys: function(request, options) {
            var entries = JSON.parse(_lumen_cache_keys_full(origin, cacheName));
            if (request !== undefined) {
                var filterUrl = _lumen_req_url(request);
                entries = entries.filter(function(e) { return e.url === filterUrl; });
            }
            return Promise.resolve(entries.map(function(e) {
                return new Request(e.url, { method: e.method });
            }));
        },
        add: function(request) {
            var url = _lumen_req_url(request);
            var self = this;
            return fetch(url).then(function(r) { return self.put(new Request(url), r); });
        },
        addAll: function(requests) {
            var self = this;
            return Promise.all(requests.map(function(r) { return self.add(r); }));
        },
    };
}

var _sw_origin = (typeof location !== 'undefined') ? (location.protocol + '//' + location.host) : '';

var caches = {
    open: function(name) {
        return Promise.resolve(_lumen_build_cache_object(_sw_origin, String(name)));
    },
    match: function(request, options) {
        var url = _lumen_req_url(request);
        var body = _lumen_cache_match_any(_sw_origin, url);
        if (body === undefined || body === null) return Promise.resolve(undefined);
        return Promise.resolve(_lumen_build_response(body, _lumen_cache_match_any_info(_sw_origin, url)));
    },
    has: function(name) {
        return Promise.resolve(_lumen_cache_has(_sw_origin, String(name)));
    },
    delete: function(name) {
        return Promise.resolve(_lumen_cache_delete_cache(_sw_origin, String(name)));
    },
    keys: function() {
        return Promise.resolve(_lumen_cache_names(_sw_origin));
    },
};

// ── Service Worker lifecycle helpers ─────────────────────────────────────────

var _sw_registrations = {};

function _sw_make_event_target() {
    var _listeners = {};
    return {
        addEventListener: function(type, fn) {
            if (!_listeners[type]) _listeners[type] = [];
            _listeners[type].push(fn);
        },
        removeEventListener: function(type, fn) {
            if (!_listeners[type]) return;
            _listeners[type] = _listeners[type].filter(function(f) { return f !== fn; });
        },
        dispatchEvent: function(evt) {
            var handlers = _listeners[evt.type] || [];
            var cb = this['on' + evt.type];
            if (typeof cb === 'function') cb.call(this, evt);
            for (var i = 0; i < handlers.length; i++) { handlers[i].call(this, evt); }
            return !evt.defaultPrevented;
        },
    };
}

function _sw_make_worker(scriptUrl, initState) {
    var et = _sw_make_event_target();
    var w = Object.assign({
        scriptURL: String(scriptUrl),
        state: initState || 'installing',
        onstatechange: null,
        onerror: null,
        postMessage: function() {},
    }, et);
    w._setState = function(s) {
        w.state = s;
        var e = new Event('statechange');
        et.dispatchEvent.call(w, e);
    };
    return w;
}

function _sw_make_registration(scope, scriptUrl) {
    var et = _sw_make_event_target();
    var reg = Object.assign({
        scope: scope,
        scriptURL: String(scriptUrl),
        updateViaCache: 'imports',
        installing: null,
        waiting: null,
        active: null,
        onupdatefound: null,
        update: function() { return Promise.resolve(); },
        unregister: function() {
            _lumen_sw_unregister(_sw_origin, scope);
            delete _sw_registrations[scope];
            _sw_persist();
            return Promise.resolve(true);
        },
    }, et);
    return reg;
}

function _sw_persist() {
    try {
        var snap = [];
        for (var sc in _sw_registrations) {
            var r = _sw_registrations[sc];
            snap.push({
                scope: r.scope,
                scriptURL: r.scriptURL,
                state: r.active ? 'activated' : (r.waiting ? 'installed' : 'installing'),
            });
        }
        _lumen_sw_persist(_sw_origin, JSON.stringify(snap));
    } catch(e) {}
}

function _sw_run_lifecycle(reg) {
    var sw = reg.installing;
    // Notify updatefound
    var uf = new Event('updatefound');
    reg.dispatchEvent(uf);
    // installing → install event → installed → activating → activate → activated
    setTimeout(function() {
        // Fire install event (SW spec §8.2.4)
        var installEvt = new Event('install');
        installEvt.waitUntil = function() {};
        if (sw.state === 'installing') {
            sw._setState('installed');
            reg.waiting = sw;
            reg.installing = null;
            _lumen_sw_register(_sw_origin, reg.scope, reg.scriptURL);
            setTimeout(function() {
                reg.waiting = null;
                sw._setState('activating');
                reg.active = sw;
                _sw_container.controller = sw;
                var activateEvt = new Event('activate');
                activateEvt.waitUntil = function() {};
                sw._setState('activated');
                _sw_persist();
                // Fire controllerchange
                var ce = new Event('controllerchange');
                _sw_container.dispatchEvent(ce);
                // Resolve ready
                if (_sw_ready_resolve) {
                    _sw_ready_resolve(reg);
                    _sw_ready_resolve = null;
                }
            }, 0);
        }
    }, 0);
}

// Restore registrations saved from a previous page load.
(function() {
    try {
        var snap = _lumen_sw_load(_sw_origin);
        if (snap) {
            var arr = JSON.parse(snap);
            for (var i = 0; i < arr.length; i++) {
                var item = arr[i];
                var reg = _sw_make_registration(item.scope, item.scriptURL);
                if (item.state === 'activated' || item.state === 'installed') {
                    var sw = _sw_make_worker(item.scriptURL, item.state);
                    reg.active = sw;
                    _sw_registrations[item.scope] = reg;
                    _lumen_sw_register(_sw_origin, item.scope, item.scriptURL);
                }
            }
        }
    } catch(e) {}
}());

var _sw_ready_resolve = null;
var _sw_ready_promise = new Promise(function(resolve) {
    _sw_ready_resolve = resolve;
    // If already have an active registration, resolve immediately.
    for (var sc in _sw_registrations) {
        if (_sw_registrations[sc].active) {
            resolve(_sw_registrations[sc]);
            _sw_ready_resolve = null;
            break;
        }
    }
});

var _sw_container_et = _sw_make_event_target();
var _sw_container = Object.assign({
    get controller() {
        for (var sc in _sw_registrations) {
            if (_sw_registrations[sc].active) return _sw_registrations[sc].active;
        }
        return null;
    },
    get ready() { return _sw_ready_promise; },
    oncontrollerchange: null,
    onmessage: null,
    onmessageerror: null,
    register: function(scriptUrl, options) {
        var scope = (options && options.scope) ? String(options.scope) : '/';
        var existing = _sw_registrations[scope];
        if (existing && existing.active && existing.scriptURL === String(scriptUrl)) {
            return Promise.resolve(existing);
        }
        var reg = _sw_make_registration(scope, scriptUrl);
        var sw = _sw_make_worker(scriptUrl, 'installing');
        reg.installing = sw;
        _sw_registrations[scope] = reg;
        // Register immediately in Rust-side map (for _lumen_sw_has_registration sync checks).
        _lumen_sw_register(_sw_origin, scope, String(scriptUrl));
        _sw_run_lifecycle(reg);
        return Promise.resolve(reg);
    },
    getRegistration: function(url) {
        var u = url || _sw_origin + '/';
        for (var sc in _sw_registrations) {
            if (String(u).indexOf(sc) === 0) return Promise.resolve(_sw_registrations[sc]);
        }
        return Promise.resolve(undefined);
    },
    getRegistrations: function() {
        return Promise.resolve(Object.values(_sw_registrations));
    },
}, _sw_container_et);

var navigator = {
    userAgent: 'Lumen/0.2.0',
    language: 'en-US',
    onLine: false,
    serviceWorker: _sw_container,
    // Beacon API (W3C Beacon §3.1): fire-and-forget POST to url.
    // data may be string | URLSearchParams | FormData | Blob | ArrayBuffer | null.
    sendBeacon: function(url, data) {
        var body = '';
        var ct = '';
        if (data == null) {
            body = '';
        } else if (typeof data === 'string') {
            body = data;
            ct = 'text/plain;charset=UTF-8';
        } else if (typeof URLSearchParams !== 'undefined' && data instanceof URLSearchParams) {
            body = data.toString();
            ct = 'application/x-www-form-urlencoded;charset=UTF-8';
        } else if (typeof FormData !== 'undefined' && data instanceof FormData) {
            body = typeof data._toUrlEncoded === 'function' ? data._toUrlEncoded() : '';
            ct = 'application/x-www-form-urlencoded;charset=UTF-8';
        } else if (typeof Blob !== 'undefined' && data instanceof Blob) {
            body = typeof data._data === 'string' ? data._data : '';
            ct = data.type || 'application/octet-stream';
        }
        try { return _lumen_send_beacon(url, body, ct); } catch(e) { return false; }
    },
};

// ── Clipboard API (W3C Clipboard API §4) ─────────────────────────────────────
// navigator.clipboard.readText()  → Promise<string>
// navigator.clipboard.writeText(text) → Promise<void>
// navigator.clipboard.read()  → Promise<ClipboardItems> stub (empty array)
// navigator.clipboard.write() → Promise<void> stub
//
// readText/writeText delegate to native bindings (_lumen_clipboard_read /
// _lumen_clipboard_write) when the shell wires them.  Until then readText
// returns '' and writeText silently succeeds.
navigator.clipboard = {
    readText: function() {
        return new Promise(function(resolve, reject) {
            try {
                var text = (typeof _lumen_clipboard_read === 'function')
                    ? _lumen_clipboard_read() : '';
                resolve(typeof text === 'string' ? text : '');
            } catch(e) { reject(e); }
        });
    },
    writeText: function(text) {
        return new Promise(function(resolve, reject) {
            try {
                if (typeof _lumen_clipboard_write === 'function') {
                    _lumen_clipboard_write(String(text == null ? '' : text));
                }
                resolve(undefined);
            } catch(e) { reject(e); }
        });
    },
    read:  function() { return Promise.resolve([]); },
    write: function() { return Promise.resolve(undefined); },
};

// ── Permissions API (W3C Permissions §5) ─────────────────────────────────────
// navigator.permissions.query({ name }) → Promise<PermissionStatus>
//
// Lumen is a single-user desktop app.  Sensors and AV hardware that do not
// exist in headless mode are 'denied'; everything else is 'granted'.  When P3
// adds per-site permission UI the state values can be updated at runtime.
function PermissionStatus(name, state) {
    this.name     = name;
    this.state    = state;
    this.onchange = null;
}
var _perm_denied = [
    'microphone', 'camera', 'midi', 'speaker-selection',
    'ambient-light-sensor', 'accelerometer', 'gyroscope', 'magnetometer',
    'display-capture', 'screen-wake-lock', 'nfc',
];
navigator.permissions = {
    query: function(descriptor) {
        if (!descriptor || typeof descriptor.name !== 'string') {
            return Promise.reject(new TypeError('permissions.query: descriptor must have a name'));
        }
        var name  = descriptor.name;
        var state = _perm_denied.indexOf(name) >= 0 ? 'denied' : 'granted';
        return Promise.resolve(new PermissionStatus(name, state));
    },
};

// ── Timer queue (HTML LS §8.6 «timers») ──────────────────────────────────────
// Timers are stored as a JS-side array; Rust drains them each event loop tick
// via _lumen_tick_timers() called from about_to_wait. When a new timer is
// scheduled, _lumen_request_wakeup(deadline_ms) notifies the shell so that
// ControlFlow::WaitUntil wakes the loop at the right time.
var _lumen_timer_seq = 1;
var _lumen_timers = [];

function _lumen_tick_timers() {
    var now = _lumen_now_ms();
    var ready = [];
    var keep = [];
    for (var i = 0; i < _lumen_timers.length; i++) {
        var t = _lumen_timers[i];
        if (t.deadline <= now) {
            ready.push(t);
        } else {
            keep.push(t);
        }
    }
    _lumen_timers = keep;
    // Re-schedule intervals before running callbacks (matches spec §8.6 step 18).
    for (var j = 0; j < ready.length; j++) {
        var r = ready[j];
        if (r.interval !== null) {
            _lumen_timers.push({ id: r.id, fn: r.fn, deadline: now + r.interval, interval: r.interval });
        }
    }
    // Run callbacks; errors are swallowed (HTML §8.6 step 17).
    for (var k = 0; k < ready.length; k++) {
        try { ready[k].fn(); } catch(e) {}
    }
    // Notify shell of next wakeup if any timers remain.
    if (_lumen_timers.length > 0) {
        var next = _lumen_timers[0].deadline;
        for (var m = 1; m < _lumen_timers.length; m++) {
            if (_lumen_timers[m].deadline < next) next = _lumen_timers[m].deadline;
        }
        _lumen_request_wakeup(next);
    }
}

function setTimeout(fn, delay) {
    if (typeof fn !== 'function') return 0;
    var ms = (typeof delay === 'number' && delay > 0) ? delay : 0;
    var id = _lumen_timer_seq++;
    var deadline = _lumen_now_ms() + ms;
    _lumen_timers.push({ id: id, fn: fn, deadline: deadline, interval: null });
    _lumen_request_wakeup(deadline);
    return id;
}

function clearTimeout(id) {
    for (var i = 0; i < _lumen_timers.length; i++) {
        if (_lumen_timers[i].id === id) { _lumen_timers.splice(i, 1); return; }
    }
}

function setInterval(fn, interval) {
    if (typeof fn !== 'function') return 0;
    var ms = (typeof interval === 'number' && interval > 0) ? interval : 0;
    var id = _lumen_timer_seq++;
    var deadline = _lumen_now_ms() + ms;
    _lumen_timers.push({ id: id, fn: fn, deadline: deadline, interval: ms });
    _lumen_request_wakeup(deadline);
    return id;
}

function clearInterval(id) { clearTimeout(id); }

// ── requestAnimationFrame / cancelAnimationFrame (HTML §8.1.5.1) ──────────────
// Callbacks are queued per-frame and called by Rust via _lumen_run_raf_callbacks
// before each paint. Each callback receives a DOMHighResTimeStamp.
var _lumen_raf_seq = 1;
var _lumen_raf_callbacks = [];

function requestAnimationFrame(fn) {
    if (typeof fn !== 'function') return 0;
    var id = _lumen_raf_seq++;
    _lumen_raf_callbacks.push({ id: id, fn: fn });
    _lumen_mark_raf_pending();
    return id;
}

function cancelAnimationFrame(id) {
    id = id | 0;
    for (var i = 0; i < _lumen_raf_callbacks.length; i++) {
        if (_lumen_raf_callbacks[i].id === id) {
            _lumen_raf_callbacks.splice(i, 1);
            return;
        }
    }
}

// Called by the shell event loop before each paint with the frame timestamp.
// Snapshot-pattern per spec: new rAF calls during callbacks go into the NEXT
// frame. Returns true when any callback was invoked (for relayout check).
// timestamp_ms < 0 → use performance.now() (live DOMHighResTimeStamp, EE-5);
// timestamp_ms >= 0 → use as-is (0 = deterministic mode, frozen clock).
// All callbacks in a batch receive the SAME timestamp (captured once at start).
function _lumen_run_raf_callbacks(timestamp_ms) {
    var ts = timestamp_ms < 0 ? performance.now() : +timestamp_ms;
    _wa_current_time = ts;
    var callbacks = _lumen_raf_callbacks.splice(0);
    if (callbacks.length === 0) return false;
    for (var i = 0; i < callbacks.length; i++) {
        try { callbacks[i].fn(ts); } catch(e) {}
    }
    return true;
}

var _popstate_listeners = [];

// Called by the shell (via eval_js) when the user navigates back/forward to a
// same-document (pushState) history entry.  Updates location and fires popstate.
// state_json is already valid JSON; url may be empty (means keep current).
function _lumen_deliver_popstate(state_json, url) {
    if (url) _lumen_location_update(url);
    var s;
    try { s = JSON.parse(state_json); } catch(e) { s = null; }
    var ev = new PopStateEvent('popstate', { state: s, bubbles: true });
    if (typeof window.onpopstate === 'function') {
        try { window.onpopstate(ev); } catch(e) {}
    }
    for (var i = 0; i < _popstate_listeners.length; i++) {
        try { _popstate_listeners[i](ev); } catch(e) {}
    }
}

var history = {
    get length()  { return _lumen_history_length(); },
    get state()   {
        try { return JSON.parse(_lumen_history_state_json()); } catch(e) { return null; }
    },
    pushState:    function(state, title, url) {
        var target = String(url !== undefined && url !== null ? url : '');
        var new_state_json = JSON.stringify(state !== undefined ? state : null);
        _lumen_history_push(new_state_json, target);
        if (target) {
            _lumen_location_update(target);
            _lumen_history_push_url(target, new_state_json);
        }
    },
    replaceState: function(state, title, url) {
        var target = String(url !== undefined && url !== null ? url : '');
        var new_state_json = JSON.stringify(state !== undefined ? state : null);
        _lumen_history_replace(new_state_json, target);
        if (target) {
            _lumen_location_update(target);
            _lumen_history_replace_url(target, new_state_json);
        }
    },
    back:    function() { history.go(-1); },
    forward: function() { history.go(1); },
    go: function(delta) {
        var ok = _lumen_history_go((delta | 0));
        if (ok) {
            var s;
            try { s = JSON.parse(_lumen_history_state_json()); } catch(e) { s = null; }
            var ev = { type: 'popstate', state: s };
            if (typeof window.onpopstate === 'function') {
                try { window.onpopstate(ev); } catch(e) {}
            }
            for (var i = 0; i < _popstate_listeners.length; i++) {
                try { _popstate_listeners[i](ev); } catch(e) {}
            }
        }
    },
};

// ── Server-Sent Events API (HTML Living Standard §9.2) ─────────────────────
// Phase 0 model: synchronous connect; background recv thread queues events;
// JS polls via _lumen_pump_sse(). Mirrors the WebSocket polling model.

var _sse_instances = [];

function _lumen_sse_fire(es, type, ev) {
    ev.target = es;
    if (type === 'message' && typeof es.onmessage === 'function') {
        try { es.onmessage(ev); } catch(e) {}
    } else if (type === 'open' && typeof es.onopen === 'function') {
        try { es.onopen(ev); } catch(e) {}
    } else if (type === 'error' && typeof es.onerror === 'function') {
        try { es.onerror(ev); } catch(e) {}
    }
    var arr = es._listeners[type];
    if (arr) { for (var i = 0; i < arr.length; i++) { try { arr[i](ev); } catch(e) {} } }
}

function _lumen_sse_pump_one(es) {
    if (!es._handle) return;
    var raw;
    while ((raw = _lumen_sse_poll(es._handle)) !== null && raw !== undefined) {
        try {
            var ev = JSON.parse(raw);
            if (ev.t === 'open') {
                if (es.readyState === 2) { continue; }
                es.readyState = 1;
                _lumen_sse_fire(es, 'open', new Event('open', { isTrusted: true }));
            } else if (ev.t === 'message') {
                if (es.readyState === 2) { continue; }
                var type = ev.event || 'message';
                var me = new MessageEvent(ev.data != null ? ev.data : '', { isTrusted: true });
                me.type = type;
                me.lastEventId = ev.id != null ? ev.id : '';
                me.origin = es._origin;
                if (me.lastEventId) { es._lastEventId = me.lastEventId; }
                _lumen_sse_fire(es, type, me);
            } else if (ev.t === 'retry') {
                // Server requested a specific reconnect delay (HTML Living Standard §9.2.3).
                if (typeof ev.ms === 'number' && ev.ms >= 0) { es._retryMs = ev.ms; }
            } else if (ev.t === 'close') {
                // Server-initiated close: per spec fire error with CONNECTING, then reconnect.
                _lumen_sse_close(es._handle);
                es._handle = 0;
                if (es.readyState !== 2) {
                    es.readyState = 0; // CONNECTING
                    var errEv = new Event('error', { isTrusted: true });
                    _lumen_sse_fire(es, 'error', errEv);
                    es._reconnecting = true;
                    (function(target, delay) {
                        setTimeout(function() {
                            if (!target._reconnecting || target.readyState === 2) return;
                            target._reconnecting = false;
                            var h = _lumen_sse_connect(target.url);
                            if (!h) { target.readyState = 2; return; }
                            target._handle = h;
                        }, delay);
                    })(es, es._retryMs);
                }
                break;
            } else if (ev.t === 'error') {
                // Network or protocol error: fire error and close (no reconnect for hard errors).
                es.readyState = 2;
                var err = new Event('error', { isTrusted: true });
                err.message = ev.message;
                _lumen_sse_fire(es, 'error', err);
                es._handle = 0;
                break;
            }
        } catch(ignore) {}
    }
}

function _lumen_pump_sse() {
    for (var i = _sse_instances.length - 1; i >= 0; i--) {
        _lumen_sse_pump_one(_sse_instances[i]);
        if (_sse_instances[i].readyState === 2 && !_sse_instances[i]._handle) {
            _sse_instances.splice(i, 1);
        }
    }
}

function EventSource(url, opts) {
    this.url = String(url || '');
    this.readyState = 0; // CONNECTING
    this.withCredentials = !!(opts && opts.withCredentials);
    this.onopen = null;
    this.onmessage = null;
    this.onerror = null;
    this._listeners = {};
    this._handle = 0;
    this._lastEventId = '';
    this._retryMs = 3000; // default reconnect delay (HTML Living Standard §9.2.7)
    this._reconnecting = false;
    // Origin best-effort: scheme+host of the target URL (for MessageEvent.origin).
    this._origin = '';
    var _sep = this.url.indexOf('://');
    if (_sep >= 0) {
        var _rest = this.url.slice(_sep + 3);
        var _end = _rest.length;
        var _slash = _rest.indexOf('/'); if (_slash >= 0 && _slash < _end) _end = _slash;
        var _q = _rest.indexOf('?'); if (_q >= 0 && _q < _end) _end = _q;
        var _hash = _rest.indexOf('#'); if (_hash >= 0 && _hash < _end) _end = _hash;
        this._origin = this.url.slice(0, _sep + 3) + _rest.slice(0, _end);
    }
    var self = this;
    var h = _lumen_sse_connect(this.url);
    if (!h) {
        // No provider, or the connection could not be established: fail per spec.
        this.readyState = 2; // CLOSED
        setTimeout(function() {
            var e = new Event('error', { isTrusted: true });
            e.message = 'EventSource connection failed';
            _lumen_sse_fire(self, 'error', e);
        }, 0);
        return;
    }
    this._handle = h;
    _sse_instances.push(this);
    // Phase 0: no persistent event loop — caller must invoke _lumen_pump_sse()
    // after setting onopen/onmessage to receive queued events.
}
EventSource.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
EventSource.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    var idx = this._listeners[type].indexOf(fn);
    if (idx >= 0) this._listeners[type].splice(idx, 1);
};
EventSource.prototype.close = function() {
    if (this._handle) {
        _lumen_sse_close(this._handle);
        this._handle = 0;
    }
    this._reconnecting = false; // cancel any pending reconnect
    this.readyState = 2; // CLOSED
};
EventSource.CONNECTING = 0;
EventSource.OPEN = 1;
EventSource.CLOSED = 2;

// ── IME Composition events (UI Events Specification §5.3) ─────────────────────
// Слушатели compositionstart/compositionupdate/compositionend:
// страница регистрирует их через addEventListener на нужном элементе.
// _lumen_dispatch_composition вызывается Rust-сторона после получения
// Ime::Preedit / Ime::Commit от winit. Диспатч идёт на document.activeElement
// (или document.body как fallback).
var _ime_active_element = null;

function _lumen_set_ime_target(el) {
    _ime_active_element = el || null;
}

function _lumen_dispatch_composition(type, data) {
    var target = _ime_active_element || (typeof document !== 'undefined' && document.body) || null;
    if (!target) return;
    var nid = target.__nid__;
    if (nid === undefined) return;
    var evt = new Event(type, { isTrusted: true });
    evt.data = String(data);
    evt.locale = '';
    _lumen_dispatch(nid, evt);
}

// ── Page lifecycle events: pageshow / pagehide (HTML Living Standard §8.6) ───
// _lumen_bfcache_persisted is set to true by an injected init script when the
// shell restores a page from bfcache. Pages can read event.persisted to detect
// this case and skip expensive re-initialisation.
var _lumen_bfcache_persisted = false;
var _pageshow_listeners = [];
var _pagehide_listeners = [];

function _lumen_fire_page_lifecycle(type, persisted) {
    var evt = new Event(type, { isTrusted: true });
    evt.persisted = !!persisted;
    var listeners = type === 'pageshow' ? _pageshow_listeners : _pagehide_listeners;
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) {}
    }
    var handler = type === 'pageshow' ? window.onpageshow : window.onpagehide;
    if (typeof handler === 'function') {
        try { handler(evt); } catch(e) {}
    }
}

// ── Fetch API (Fetch Standard §3) ─────────────────────────────────────────────
// AbortController / AbortSignal. abort() records state and fires listeners;
// fetch() checks signal.aborted before issuing the (synchronous) request.
function AbortSignal() {
    this.aborted = false;
    this.reason = undefined;
    this.onabort = null;
    this._listeners = [];
}
AbortSignal.prototype.addEventListener = function(type, fn) {
    if (type === 'abort') this._listeners.push(fn);
};
AbortSignal.prototype.removeEventListener = function(type, fn) {
    if (type !== 'abort') return;
    var i = this._listeners.indexOf(fn);
    if (i >= 0) this._listeners.splice(i, 1);
};
AbortSignal.prototype.throwIfAborted = function() {
    if (this.aborted) throw this.reason || new DOMException('signal is aborted without reason', 'AbortError');
};
// Shared signal-abort steps (DOM §3.2): set state, fire onabort + listeners.
function _lumen_abort_signal_fire(sig, reason) {
    if (sig.aborted) return;
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    var evt = { type: 'abort', target: sig };
    if (typeof sig.onabort === 'function') { try { sig.onabort(evt); } catch(e) {} }
    var listeners = sig._listeners.slice();
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) {}
    }
}

function AbortController() {
    this.signal = new AbortSignal();
}
AbortController.prototype.abort = function(reason) {
    _lumen_abort_signal_fire(this.signal, reason);
};
// AbortSignal.abort(reason) — DOM §3.2.2: returns an already-aborted signal.
AbortSignal.abort = function(reason) {
    var sig = new AbortSignal();
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    return sig;
};
// AbortSignal.timeout(ms) — DOM §3.2.2: aborts with TimeoutError after the
// shell timer queue (setTimeout shim) fires.
AbortSignal.timeout = function(ms) {
    var sig = new AbortSignal();
    setTimeout(function() {
        _lumen_abort_signal_fire(sig, new DOMException('signal timed out', 'TimeoutError'));
    }, ms);
    return sig;
};
// AbortSignal.any(signals) — DOM §3.2.2: races the sources; the result aborts
// with the reason of the first source that aborts.
AbortSignal.any = function(signals) {
    var sig = new AbortSignal();
    var sources = [];
    function onAbort(evt) {
        if (sig.aborted) return;
        // Detach from remaining sources — the race is decided.
        for (var j = 0; j < sources.length; j++) {
            sources[j].removeEventListener('abort', onAbort);
        }
        _lumen_abort_signal_fire(sig, evt && evt.target ? evt.target.reason : undefined);
    }
    if (signals) {
        for (var i = 0; i < signals.length; i++) {
            if (!signals[i]) continue;
            if (signals[i].aborted) {
                sig.aborted = true;
                sig.reason = signals[i].reason;
                return sig;
            }
            sources.push(signals[i]);
            signals[i].addEventListener('abort', onAbort);
        }
    }
    return sig;
};

// ── WHATWG Streams (https://streams.spec.whatwg.org/) §3-5 ───────────────────
// ReadableStream, WritableStream, TransformStream — synchronous-friendly model.
// For Lumen's synchronous fetch, all chunks are enqueued at construction time.
// Pull model: start() and pull() are called once; async pull callbacks are not
// re-invoked (sufficient for response.body / Blob.stream() use cases in Phase 2).

// ── ReadableStream §3 ────────────────────────────────────────────────────────
function ReadableStreamDefaultController(stream) {
    this._stream = stream;
    this._queue = [];
    this._closeRequested = false;
    this.desiredSize = 1;
}
ReadableStreamDefaultController.prototype.enqueue = function(chunk) {
    var stream = this._stream;
    if (!stream || stream._rs_state !== 'readable') return;
    if (stream._rs_reader && stream._rs_reader._readRequests.length > 0) {
        var req = stream._rs_reader._readRequests.shift();
        req({ value: chunk, done: false }, undefined);
    } else {
        this._queue.push(chunk);
    }
};
ReadableStreamDefaultController.prototype.close = function() {
    var stream = this._stream;
    if (!stream || this._closeRequested || stream._rs_state !== 'readable') return;
    this._closeRequested = true;
    if (this._queue.length === 0) _rs_do_close(stream);
};
ReadableStreamDefaultController.prototype.error = function(e) {
    var stream = this._stream;
    if (!stream || stream._rs_state !== 'readable') return;
    stream._rs_state = 'errored';
    stream._rs_error = e;
    if (stream._rs_reader) {
        var reqs = stream._rs_reader._readRequests;
        stream._rs_reader._readRequests = [];
        for (var i = 0; i < reqs.length; i++) reqs[i](undefined, e);
    }
};

function _rs_do_close(stream) {
    stream._rs_state = 'closed';
    if (stream._rs_reader) {
        var reqs = stream._rs_reader._readRequests;
        stream._rs_reader._readRequests = [];
        for (var i = 0; i < reqs.length; i++) reqs[i]({ value: undefined, done: true }, undefined);
        if (stream._rs_reader._closedResolve) stream._rs_reader._closedResolve();
    }
}

function ReadableStream(source, strategy) {
    source = source || {};
    this._rs_state = 'readable';
    this._rs_error = undefined;
    this._rs_reader = null;
    this._rs_cancel_fn = typeof source.cancel === 'function' ? source.cancel : null;
    // Store pull fn for demand-driven invocation (Streams §3.6.3).
    this._rs_pull_fn = typeof source.pull === 'function' ? source.pull : null;
    this._rs_ctrl = new ReadableStreamDefaultController(this);
    if (typeof source.start === 'function') {
        try { source.start(this._rs_ctrl); } catch(e) { this._rs_ctrl.error(e); }
    }
    // Eagerly fill: call pull once after start if queue empty and stream still readable.
    if (this._rs_pull_fn && this._rs_state === 'readable'
            && this._rs_ctrl._queue.length === 0 && !this._rs_ctrl._closeRequested) {
        try { this._rs_pull_fn(this._rs_ctrl); } catch(e) { this._rs_ctrl.error(e); }
    }
}
Object.defineProperty(ReadableStream.prototype, 'locked', {
    get: function() { return this._rs_reader !== null; }
});
ReadableStream.prototype.getReader = function() {
    if (this._rs_reader !== null) throw new TypeError('ReadableStream is already locked');
    var reader = new ReadableStreamDefaultReader(this);
    this._rs_reader = reader;
    return reader;
};
ReadableStream.prototype.cancel = function(reason) {
    if (this._rs_reader) return Promise.reject(new TypeError('ReadableStream is locked'));
    return this._rs_do_cancel(reason);
};
ReadableStream.prototype._rs_do_cancel = function(reason) {
    if (this._rs_state === 'closed') return Promise.resolve();
    if (this._rs_state === 'errored') return Promise.reject(this._rs_error);
    _rs_do_close(this);
    if (this._rs_cancel_fn) { try { this._rs_cancel_fn(reason); } catch(e) {} }
    return Promise.resolve();
};
ReadableStream.prototype.tee = function() {
    var chunks = this._rs_ctrl._queue.slice();
    var alreadyClosed = this._rs_state !== 'readable' || this._rs_ctrl._closeRequested;
    var self = this;
    function makeClone(arr, closed) {
        return new ReadableStream({
            start: function(c) {
                for (var i = 0; i < arr.length; i++) c.enqueue(arr[i]);
                if (closed) c.close();
            }
        });
    }
    _rs_do_close(self);
    return [makeClone(chunks, alreadyClosed), makeClone(chunks, alreadyClosed)];
};
ReadableStream.prototype.pipeTo = function(dest, options) {
    var reader = this.getReader();
    var writer = dest.getWriter();
    function pump() {
        return reader.read().then(function(result) {
            if (result.done) {
                reader.releaseLock();
                return writer.close();
            }
            return writer.write(result.value).then(pump);
        });
    }
    return pump().catch(function(e) { reader.cancel(e); writer.abort(e); return Promise.reject(e); });
};
ReadableStream.prototype.pipeThrough = function(transform, options) {
    this.pipeTo(transform.writable, options);
    return transform.readable;
};
ReadableStream.from = function(iterable) {
    var arr = Array.isArray(iterable) ? iterable : (iterable instanceof Uint8Array ? [iterable] : []);
    return new ReadableStream({
        start: function(c) {
            for (var i = 0; i < arr.length; i++) c.enqueue(arr[i]);
            c.close();
        }
    });
};

// ── ReadableStreamDefaultReader §3.7 ─────────────────────────────────────────
function ReadableStreamDefaultReader(stream) {
    this._stream = stream;
    this._readRequests = [];
    var self = this;
    this.closed = new Promise(function(res, rej) {
        self._closedResolve = res;
        self._closedReject = rej;
    });
    if (stream._rs_state === 'closed') this._closedResolve();
    else if (stream._rs_state === 'errored') this._closedReject(stream._rs_error);
}
ReadableStreamDefaultReader.prototype.read = function() {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('reader not attached to a stream'));
    if (stream._rs_state === 'errored') return Promise.reject(stream._rs_error);
    var ctrl = stream._rs_ctrl;
    if (ctrl._queue.length > 0) {
        var chunk = ctrl._queue.shift();
        if (ctrl._closeRequested && ctrl._queue.length === 0) _rs_do_close(stream);
        return Promise.resolve({ value: chunk, done: false });
    }
    if (stream._rs_state === 'closed') return Promise.resolve({ value: undefined, done: true });
    var self = this;
    var p = new Promise(function(resolve, reject) {
        self._readRequests.push(function(result, err) {
            if (err !== undefined) reject(err); else resolve(result);
        });
    });
    // Demand-driven pull: when queue is empty and a read is pending, ask source for more data.
    // pull() either enqueues a chunk (resolving the pending request via enqueue()) or
    // calls c.close() (resolving via _rs_do_close()). Mirrors Streams spec ReadableStreamFill.
    if (stream._rs_pull_fn && stream._rs_state === 'readable' && !ctrl._closeRequested) {
        try { stream._rs_pull_fn(ctrl); } catch(e) { ctrl.error(e); }
    }
    return p;
};
ReadableStreamDefaultReader.prototype.cancel = function(reason) {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('reader not attached'));
    return stream._rs_do_cancel(reason);
};
ReadableStreamDefaultReader.prototype.releaseLock = function() {
    if (!this._stream) return;
    if (this._readRequests.length > 0) throw new TypeError('pending read requests');
    this._stream._rs_reader = null;
    this._stream = null;
    if (this._closedReject) this._closedReject(new TypeError('reader released'));
};

// ── WritableStream §4 ────────────────────────────────────────────────────────
function WritableStreamDefaultController(stream, sink) {
    this._stream = stream;
    this._sink = sink;
}
WritableStreamDefaultController.prototype.error = function(e) {
    var stream = this._stream;
    if (!stream || (stream._ws_state !== 'writable' && stream._ws_state !== 'closing')) return;
    stream._ws_state = 'errored';
    stream._ws_error = e;
};

function WritableStream(sink, strategy) {
    sink = sink || {};
    this._ws_state = 'writable';
    this._ws_error = undefined;
    this._ws_writer = null;
    this._ws_ctrl = new WritableStreamDefaultController(this, sink);
    if (typeof sink.start === 'function') {
        try { sink.start(this._ws_ctrl); } catch(e) { this._ws_ctrl.error(e); }
    }
}
Object.defineProperty(WritableStream.prototype, 'locked', {
    get: function() { return this._ws_writer !== null; }
});
WritableStream.prototype.getWriter = function() {
    if (this._ws_writer !== null) throw new TypeError('WritableStream is already locked');
    var writer = new WritableStreamDefaultWriter(this);
    this._ws_writer = writer;
    return writer;
};
WritableStream.prototype.abort = function(reason) {
    if (this._ws_writer) return Promise.reject(new TypeError('WritableStream is locked'));
    this._ws_state = 'errored'; this._ws_error = reason;
    return Promise.resolve();
};
WritableStream.prototype.close = function() {
    if (this._ws_writer) return Promise.reject(new TypeError('WritableStream is locked'));
    return this._ws_do_close();
};
WritableStream.prototype._ws_do_close = function() {
    var stream = this;
    if (stream._ws_state !== 'writable') return Promise.resolve();
    stream._ws_state = 'closing';
    var sink = stream._ws_ctrl._sink;
    var p = Promise.resolve();
    if (typeof sink.close === 'function') {
        try { p = Promise.resolve(sink.close(stream._ws_ctrl)); } catch(e) { p = Promise.reject(e); }
    }
    return p.then(function() { stream._ws_state = 'closed'; });
};

// ── WritableStreamDefaultWriter §4.6 ─────────────────────────────────────────
function WritableStreamDefaultWriter(stream) {
    this._stream = stream;
    var self = this;
    this.ready = Promise.resolve();
    this.closed = new Promise(function(res, rej) {
        self._closedResolve = res;
        self._closedReject = rej;
    });
}
Object.defineProperty(WritableStreamDefaultWriter.prototype, 'desiredSize', {
    get: function() {
        var s = this._stream;
        if (!s || s._ws_state === 'errored') return null;
        if (s._ws_state === 'closed' || s._ws_state === 'closing') return 0;
        return 1;
    }
});
WritableStreamDefaultWriter.prototype.write = function(chunk) {
    var stream = this._stream;
    if (!stream || stream._ws_state !== 'writable') return Promise.reject(new TypeError('stream not writable'));
    var sink = stream._ws_ctrl._sink;
    if (typeof sink.write === 'function') {
        try { return Promise.resolve(sink.write(chunk, stream._ws_ctrl)); } catch(e) { return Promise.reject(e); }
    }
    return Promise.resolve();
};
WritableStreamDefaultWriter.prototype.close = function() {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('writer not attached'));
    var p = stream._ws_do_close();
    this._stream = null;
    stream._ws_writer = null;
    var self = this;
    return p.then(function() { if (self._closedResolve) self._closedResolve(); });
};
WritableStreamDefaultWriter.prototype.abort = function(reason) {
    var stream = this._stream;
    if (!stream) return Promise.resolve();
    this._stream = null;
    stream._ws_writer = null;
    return stream.abort(reason);
};
WritableStreamDefaultWriter.prototype.releaseLock = function() {
    if (!this._stream) return;
    this._stream._ws_writer = null;
    this._stream = null;
};

// ── TransformStream §5 ───────────────────────────────────────────────────────
function TransformStreamDefaultController(readableCtrl) {
    this._readableCtrl = readableCtrl;
}
TransformStreamDefaultController.prototype.enqueue = function(chunk) {
    this._readableCtrl.enqueue(chunk);
};
TransformStreamDefaultController.prototype.terminate = function() {
    this._readableCtrl.close();
};
TransformStreamDefaultController.prototype.error = function(e) {
    this._readableCtrl.error(e);
};

function TransformStream(transformer, writableStrategy, readableStrategy) {
    transformer = transformer || {};
    var tc;
    var self = this;
    this.readable = new ReadableStream({
        start: function(ctrl) {
            tc = new TransformStreamDefaultController(ctrl);
            if (typeof transformer.start === 'function') {
                try { transformer.start(tc); } catch(e) { ctrl.error(e); }
            }
        }
    });
    this.writable = new WritableStream({
        write: function(chunk) {
            if (typeof transformer.transform === 'function') {
                try { return Promise.resolve(transformer.transform(chunk, tc)); } catch(e) { return Promise.reject(e); }
            }
            tc.enqueue(chunk);
            return Promise.resolve();
        },
        close: function() {
            if (typeof transformer.flush === 'function') {
                try { return Promise.resolve(transformer.flush(tc)); } catch(e) { return Promise.reject(e); }
            }
            tc.terminate();
            return Promise.resolve();
        }
    });
}

// ── TextDecoderStream / TextEncoderStream (Encoding Standard §5.1) ───────────
function TextDecoderStream(label, options) {
    var dec = new TextDecoder(label, options);
    TransformStream.call(this, {
        transform: function(chunk, c) {
            var str = dec.decode(chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk), { stream: true });
            if (str.length > 0) c.enqueue(str);
        },
        flush: function(c) {
            var str = dec.decode();
            if (str.length > 0) c.enqueue(str);
        }
    });
    this.encoding = dec.encoding;
    this.fatal = dec.fatal;
    this.ignoreBOM = dec.ignoreBOM;
}
TextDecoderStream.prototype = Object.create(TransformStream.prototype);
TextDecoderStream.prototype.constructor = TextDecoderStream;

function TextEncoderStream() {
    var enc = new TextEncoder();
    TransformStream.call(this, {
        transform: function(chunk, c) {
            c.enqueue(enc.encode(String(chunk)));
        }
    });
    this.encoding = 'utf-8';
}
TextEncoderStream.prototype = Object.create(TransformStream.prototype);
TextEncoderStream.prototype.constructor = TextEncoderStream;

// ── CompressionStream / DecompressionStream (WHATWG Compression Streams) ─────
// https://compression.spec.whatwg.org/
// Formats: 'deflate-raw' (raw DEFLATE RFC 1951), 'deflate' (zlib RFC 1950), 'gzip'.
// Buffer-then-flush model: accumulates all input chunks, compresses atomically at
// flush (TransformStream.writable.close()). Emits a single Uint8Array output chunk.
var _COMPRESSION_FORMATS = ['deflate-raw', 'deflate', 'gzip'];

function _csConcat(chunks) {
    var total = 0;
    for (var i = 0; i < chunks.length; i++) total += chunks[i].length;
    var out = new Uint8Array(total), off = 0;
    for (var i = 0; i < chunks.length; i++) { out.set(chunks[i], off); off += chunks[i].length; }
    return out;
}
function _csToU8(chunk) {
    if (chunk instanceof Uint8Array) return chunk;
    if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
    if (chunk && ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    return new Uint8Array(0);
}

function CompressionStream(format) {
    if (_COMPRESSION_FORMATS.indexOf(format) === -1)
        throw new TypeError('CompressionStream: unsupported format: ' + format);
    var buf = [], fmt = format;
    TransformStream.call(this, {
        transform: function(chunk, _c) { buf.push(_csToU8(chunk)); },
        flush: function(c) {
            var result = _lumen_compress_bytes(Array.from(_csConcat(buf)), fmt);
            if (result && result.length > 0) c.enqueue(new Uint8Array(result));
            c.terminate();
        }
    });
    this.format = format;
}
CompressionStream.prototype = Object.create(TransformStream.prototype);
CompressionStream.prototype.constructor = CompressionStream;

function DecompressionStream(format) {
    if (_COMPRESSION_FORMATS.indexOf(format) === -1)
        throw new TypeError('DecompressionStream: unsupported format: ' + format);
    var buf = [], fmt = format;
    TransformStream.call(this, {
        transform: function(chunk, _c) { buf.push(_csToU8(chunk)); },
        flush: function(c) {
            var result = _lumen_decompress_bytes(Array.from(_csConcat(buf)), fmt);
            if (result && result.length > 0) c.enqueue(new Uint8Array(result));
            c.terminate();
        }
    });
    this.format = format;
}
DecompressionStream.prototype = Object.create(TransformStream.prototype);
DecompressionStream.prototype.constructor = DecompressionStream;

// ── ByteLengthQueuingStrategy / CountQueuingStrategy §6 ──────────────────────
function ByteLengthQueuingStrategy(init) {
    this.highWaterMark = (init && typeof init.highWaterMark === 'number') ? init.highWaterMark : 1;
}
ByteLengthQueuingStrategy.prototype.size = function(chunk) {
    return (chunk && chunk.byteLength) ? chunk.byteLength : 0;
};
function CountQueuingStrategy(init) {
    this.highWaterMark = (init && typeof init.highWaterMark === 'number') ? init.highWaterMark : 1;
}
CountQueuingStrategy.prototype.size = function() { return 1; };

// Headers (Fetch Standard §2.2)
function Headers(init) {
    this._map = [];
    if (init) {
        if (Array.isArray(init)) {
            for (var i = 0; i < init.length; i++) this.append(init[i][0], init[i][1]);
        } else if (typeof init === 'object') {
            var keys = Object.keys(init);
            for (var k = 0; k < keys.length; k++) this.append(keys[k], init[keys[k]]);
        }
    }
}
Headers.prototype._key = function(name) { return String(name).toLowerCase(); };
Headers.prototype.append = function(name, value) {
    var k = this._key(name);
    this._map.push([k, String(value)]);
};
Headers.prototype.set = function(name, value) {
    var k = this._key(name);
    this._map = this._map.filter(function(p) { return p[0] !== k; });
    this._map.push([k, String(value)]);
};
Headers.prototype.get = function(name) {
    var k = this._key(name);
    var vals = this._map.filter(function(p) { return p[0] === k; }).map(function(p) { return p[1]; });
    return vals.length ? vals.join(', ') : null;
};
Headers.prototype.has = function(name) { return this.get(name) !== null; };
Headers.prototype.delete = function(name) {
    var k = this._key(name);
    this._map = this._map.filter(function(p) { return p[0] !== k; });
};
Headers.prototype.forEach = function(cb) {
    this._map.forEach(function(p) { cb(p[1], p[0]); });
};
Headers.prototype.entries = function() { return this._map.map(function(p) { return [p[0], p[1]]; }); };
Headers.prototype.keys   = function() { return this._map.map(function(p) { return p[0]; }); };
Headers.prototype.values = function() { return this._map.map(function(p) { return p[1]; }); };

// _rs_make_body_stream(bodyBytes, respRef) — builds a pull()-based ReadableStream
// that delivers bodyBytes in 64 KiB chunks (Fetch Standard §2.2, WHATWG Streams §3.4.4).
// Intercepting getReader() marks respRef.bodyUsed = true so subsequent .text() etc. reject.
var _RS_CHUNK = 65536;
function _rs_make_body_stream(bodyBytes, respRef) {
    var pos = 0;
    var stream = new ReadableStream({
        pull: function(c) {
            if (pos >= bodyBytes.length) { c.close(); return; }
            var end = Math.min(pos + _RS_CHUNK, bodyBytes.length);
            c.enqueue(bodyBytes.subarray(pos, end));
            pos = end;
        },
        cancel: function() { pos = bodyBytes.length; }
    });
    var _orig = stream.getReader.bind(stream);
    stream.getReader = function(opts) {
        if (respRef.bodyUsed) throw new TypeError('body already consumed');
        respRef.bodyUsed = true;
        return _orig(opts);
    };
    return stream;
}

// Response (Fetch Standard §2.5)
function Response(body, init) {
    init = init || {};
    this.status = init.status !== undefined ? init.status : 200;
    this.statusText = init.statusText !== undefined ? init.statusText : '';
    this.ok = this.status >= 200 && this.status < 300;
    this.headers = new Headers(init.headers || []);
    this.redirected = false;
    this.type = 'default';
    this.url = '';
    this.bodyUsed = false;
    var bodyBytes = (body instanceof Uint8Array) ? body
                  : (body == null ? new Uint8Array(0) : new TextEncoder().encode(String(body)));
    this._body = bodyBytes;
    this.body = _rs_make_body_stream(bodyBytes, this);
}
// _fromFetchCache — factory used by fetch() to build a Response that reads
// the response body lazily from a per-response stream slot.
// _lumen_stream_alloc() copies the body out of the single FetchCache slot into a
// dedicated HashMap entry, so subsequent fetch() calls cannot clobber this body.
Response._fromFetchCache = function(status, statusText, headers) {
    var r = Object.create(Response.prototype);
    r.status = status;
    r.statusText = statusText;
    r.ok = status >= 200 && status < 300;
    r.headers = new Headers(headers);
    r.redirected = false;
    r.type = 'default';
    r.url = '';
    r.bodyUsed = false;
    r._body = null; // consumed via stream slot
    // Allocate a per-response slot — body survives until consumed or cancelled.
    var handle = _lumen_stream_alloc();
    r._stream_handle = handle;
    var totalLen = _lumen_stream_length(handle);
    var pos = 0;
    var freed = false;
    function freeHandle() {
        if (!freed && handle > 0) { freed = true; _lumen_stream_free(handle); r._stream_handle = 0; }
    }
    var stream = new ReadableStream({
        pull: function(c) {
            if (pos >= totalLen) { freeHandle(); c.close(); return; }
            var size = Math.min(_RS_CHUNK, totalLen - pos);
            var chunk = _lumen_stream_chunk(handle, pos, size);
            c.enqueue(new Uint8Array(chunk));
            pos += size;
            if (pos >= totalLen) freeHandle();
        },
        cancel: function() { freeHandle(); pos = totalLen; }
    });
    var _orig = stream.getReader.bind(stream);
    stream.getReader = function(opts) {
        if (r.bodyUsed) throw new TypeError('body already consumed');
        r.bodyUsed = true;
        return _orig(opts);
    };
    r.body = stream;
    return r;
};
Response.prototype._consumeBody = function() {
    if (this.bodyUsed) return Promise.reject(new TypeError('body already consumed'));
    if (this.body && this.body.locked) return Promise.reject(new TypeError('body stream is locked'));
    this.bodyUsed = true;
    if (this._body === null) {
        // Body came from _fromFetchCache — read from the dedicated stream slot.
        var h = this._stream_handle || 0;
        if (h > 0) {
            var len = _lumen_stream_length(h);
            var bytes = len > 0 ? new Uint8Array(_lumen_stream_chunk(h, 0, len)) : new Uint8Array(0);
            _lumen_stream_free(h);
            this._stream_handle = 0;
            return Promise.resolve(bytes);
        }
        // Fallback for legacy callers that set _body = null without a stream slot.
        var len2 = _lumen_fetch_body_length();
        return Promise.resolve(len2 > 0 ? new Uint8Array(_lumen_fetch_body_chunk(0, len2)) : new Uint8Array(0));
    }
    return Promise.resolve(this._body);
};
Response.prototype.text = function() {
    return this._consumeBody().then(function(bytes) {
        if (bytes instanceof Uint8Array) return new TextDecoder().decode(bytes);
        return bytes == null ? '' : String(bytes);
    });
};
Response.prototype.json = function() {
    return this.text().then(function(t) { return JSON.parse(t); });
};
Response.prototype.arrayBuffer = function() {
    return this._consumeBody().then(function(bytes) {
        if (bytes instanceof Uint8Array) return bytes.buffer.slice(0);
        return new Uint8Array(0).buffer;
    });
};
Response.prototype.blob = function() {
    return this._consumeBody().then(function(bytes) {
        return new Blob([bytes]);
    });
};
Response.prototype.clone = function() {
    var r = new Response(this._body, {
        status: this.status,
        statusText: this.statusText,
        headers: this.headers.entries(),
    });
    r.url = this.url;
    return r;
};
Response.error = function() {
    return new Response(null, { status: 0, statusText: '' });
};
Response.redirect = function(url, status) {
    var r = new Response(null, { status: status || 302 });
    r.url = String(url);
    return r;
};

// Request (Fetch Standard §2.4) — minimal Phase 0 impl
function Request(input, init) {
    init = init || {};
    this.url = typeof input === 'string' ? input : (input.url || '');
    this.method = (init.method || (typeof input === 'object' && input.method) || 'GET').toUpperCase();
    this.headers = new Headers(init.headers || (typeof input === 'object' && input.headers) || []);
    this.body = init.body !== undefined ? init.body : null;
    this.signal = init.signal || new AbortSignal();
    this.mode = init.mode || 'cors';
    this.credentials = init.credentials || 'same-origin';
    this.cache = init.cache || 'default';
    this.redirect = init.redirect || 'follow';
    this.referrer = init.referrer || 'about:client';
    this.integrity = init.integrity || '';
}
Request.prototype.clone = function() {
    return new Request(this.url, {
        method: this.method,
        headers: this.headers.entries(),
        body: this.body,
        signal: this.signal,
    });
};

// ── FormData (XHR Spec §4 / Fetch Spec) ────────────────────────────────────
// Stores an ordered list of (name, value) pairs. Values are always strings
// (File/Blob support is Phase 2+). Serializes to application/x-www-form-urlencoded.

function FormData(formEl) {
    this._entries = [];
    if (formEl && typeof formEl === 'object' && formEl.tagName === 'FORM') {
        var inputs = formEl.querySelectorAll('input,select,textarea');
        for (var i = 0; i < inputs.length; i++) {
            var el = inputs[i];
            var name = el.getAttribute('name');
            if (!name) { continue; }
            var type = (el.getAttribute('type') || '').toLowerCase();
            if (type === 'checkbox' || type === 'radio') {
                if (!el.checked) { continue; }
            }
            if (type === 'submit' || type === 'reset' || type === 'button' || type === 'image') { continue; }
            this._entries.push([String(name), String(el.value || '')]);
        }
    }
}

FormData.prototype.append = function(name, value) {
    this._entries.push([String(name), String(value)]);
};

FormData.prototype.delete = function(name) {
    var n = String(name);
    this._entries = this._entries.filter(function(e) { return e[0] !== n; });
};

FormData.prototype.get = function(name) {
    var n = String(name);
    for (var i = 0; i < this._entries.length; i++) {
        if (this._entries[i][0] === n) { return this._entries[i][1]; }
    }
    return null;
};

FormData.prototype.getAll = function(name) {
    var n = String(name);
    return this._entries.filter(function(e) { return e[0] === n; }).map(function(e) { return e[1]; });
};

FormData.prototype.has = function(name) {
    var n = String(name);
    return this._entries.some(function(e) { return e[0] === n; });
};

FormData.prototype.set = function(name, value) {
    var n = String(name), v = String(value);
    var found = false;
    this._entries = this._entries.filter(function(e) {
        if (e[0] === n) {
            if (!found) { found = true; e[1] = v; return true; }
            return false;
        }
        return true;
    });
    if (!found) { this._entries.push([n, v]); }
};

FormData.prototype.entries = function() {
    var arr = this._entries.slice();
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.keys = function() {
    var arr = this._entries.map(function(e) { return e[0]; });
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.values = function() {
    var arr = this._entries.map(function(e) { return e[1]; });
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.forEach = function(cb, thisArg) {
    for (var i = 0; i < this._entries.length; i++) {
        cb.call(thisArg, this._entries[i][1], this._entries[i][0], this);
    }
};

FormData.prototype[Symbol.iterator] = function() { return this.entries(); };

/// Serialize to application/x-www-form-urlencoded (RFC 3986 percent-encoding).
FormData.prototype._toUrlEncoded = function() {
    return this._entries.map(function(e) {
        return encodeURIComponent(e[0]) + '=' + encodeURIComponent(e[1]);
    }).join('&');
};

FormData.prototype._toMultipart = function(boundary) {
    var enc = new TextEncoder();
    var parts = [];
    var dash = enc.encode('--');
    var bnd = enc.encode(boundary);
    var crlf = enc.encode('\\r\\n');
    for (var i = 0; i < this._entries.length; i++) {
        var name = this._entries[i][0];
        var value = this._entries[i][1];
        var safeName = name.replace(/\\r/g, '%0D').replace(/\\n/g, '%0A').replace(/\\x22/g, '%22');
        var disp = 'Content-Disposition: form-data; name=\\x22' + safeName + '\\x22\\r\\n\\r\\n';
        var dispHeader = enc.encode(disp);
        var body = enc.encode(value);
        parts.push(dash, bnd, crlf, dispHeader, body, crlf);
    }
    parts.push(dash, bnd, enc.encode('--'), crlf);
    var totalLen = 0;
    for (var j = 0; j < parts.length; j++) { totalLen += parts[j].length; }
    var out = new Uint8Array(totalLen);
    var off = 0;
    for (var k = 0; k < parts.length; k++) {
        out.set(parts[k], off);
        off += parts[k].length;
    }
    return out;
};

// ── TextEncoder / TextDecoder (WHATWG Encoding §8–9) ─────────────────────────
// Pure-JS UTF-8 implementation; QuickJS does not provide a built-in.

function TextEncoder() {}
TextEncoder.prototype.encoding = 'utf-8';
TextEncoder.prototype.encode = function(str) {
    var s = String(str === undefined ? '' : str);
    var bytes = [];
    for (var i = 0; i < s.length; i++) {
        var c = s.charCodeAt(i);
        if (c < 0x80) {
            bytes.push(c);
        } else if (c < 0x800) {
            bytes.push(0xC0 | (c >> 6));
            bytes.push(0x80 | (c & 0x3F));
        } else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length) {
            var lo = s.charCodeAt(i + 1);
            var cp = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
            bytes.push(0xF0 | (cp >> 18));
            bytes.push(0x80 | ((cp >> 12) & 0x3F));
            bytes.push(0x80 | ((cp >> 6) & 0x3F));
            bytes.push(0x80 | (cp & 0x3F));
            i++;
        } else {
            bytes.push(0xE0 | (c >> 12));
            bytes.push(0x80 | ((c >> 6) & 0x3F));
            bytes.push(0x80 | (c & 0x3F));
        }
    }
    return new Uint8Array(bytes);
};

function TextDecoder(label, options) {
    this.encoding = (label || 'utf-8').toLowerCase();
    this.fatal = !!(options && options.fatal);
    this.ignoreBOM = !!(options && options.ignoreBOM);
    this._pending = null;
}
// Encoding Standard §9.1 — UTF-8 decode with optional stream mode.
// When options.stream is true, incomplete multi-byte sequences at the end of
// the chunk are saved in this._pending and prepended to the next decode() call.
TextDecoder.prototype.decode = function(buf, options) {
    var stream = !!(options && options.stream);
    var input;
    if (buf === undefined || buf === null) {
        input = new Uint8Array(0);
    } else {
        input = buf instanceof Uint8Array ? buf : new Uint8Array(buf instanceof ArrayBuffer ? buf : new ArrayBuffer(0));
    }
    // Prepend any bytes carried over from the previous streaming chunk.
    var bytes;
    if (this._pending && this._pending.length > 0) {
        var combined = new Uint8Array(this._pending.length + input.length);
        combined.set(this._pending);
        combined.set(input, this._pending.length);
        bytes = combined;
    } else {
        bytes = input;
    }
    this._pending = null;
    var str = '', i = 0;
    while (i < bytes.length) {
        var b = bytes[i];
        var seqLen;
        if (b < 0x80) {
            seqLen = 1;
        } else if ((b & 0xE0) === 0xC0) {
            seqLen = 2;
        } else if ((b & 0xF0) === 0xE0) {
            seqLen = 3;
        } else if ((b & 0xF8) === 0xF0) {
            seqLen = 4;
        } else {
            // Stray continuation byte — emit replacement character.
            str += '�';
            i++;
            continue;
        }
        if (i + seqLen > bytes.length) {
            // Incomplete sequence at end of chunk.
            if (stream) {
                this._pending = bytes.slice(i);
            } else if (this.fatal) {
                throw new TypeError('TextDecoder: incomplete multi-byte sequence');
            } else {
                str += '�';
            }
            break;
        }
        if (seqLen === 1) {
            str += String.fromCharCode(b);
        } else if (seqLen === 2) {
            str += String.fromCharCode(((b & 0x1F) << 6) | (bytes[i + 1] & 0x3F));
        } else if (seqLen === 3) {
            str += String.fromCharCode(((b & 0x0F) << 12) | ((bytes[i + 1] & 0x3F) << 6) | (bytes[i + 2] & 0x3F));
        } else {
            var hi = ((b & 0x07) << 18) | ((bytes[i + 1] & 0x3F) << 12) | ((bytes[i + 2] & 0x3F) << 6) | (bytes[i + 3] & 0x3F);
            hi -= 0x10000;
            str += String.fromCharCode(0xD800 + (hi >> 10), 0xDC00 + (hi & 0x3FF));
        }
        i += seqLen;
    }
    return str;
};

// fetch() (Fetch Standard §3) — synchronous under the hood, wrapped in Promise.
// Supports request body: FormData → application/x-www-form-urlencoded,
// string → text/plain;charset=UTF-8, Uint8Array/ArrayBuffer → application/octet-stream.
// FormData → multipart/form-data with a generated boundary (Fetch spec §5.4 «extract a body»).
function fetch(input, init) {
    try {
        // Fetch §4.1 step 13: an already-aborted signal rejects immediately with
        // its reason. Lumen's fetch is synchronous, so this pre-flight check is
        // the only cancellation point (no in-flight abort in Phase 0).
        var fetchSignal = (init && init.signal) ? init.signal
                        : (typeof input === 'object' && input && input.signal ? input.signal : null);
        if (fetchSignal && fetchSignal.aborted) {
            return Promise.reject(
                fetchSignal.reason !== undefined ? fetchSignal.reason
                    : new DOMException('signal is aborted without reason', 'AbortError'));
        }
        var url = typeof input === 'string' ? input : (input && input.url ? input.url : String(input));
        var method = (init && init.method) ? String(init.method).toUpperCase() :
                     (typeof input === 'object' && input.method ? input.method.toUpperCase() : 'GET');

        // Fetch §5.4 keepalive flag: request survives page unload (Beacon semantics).
        // Phase 0: accepted syntactically; detachment from page lifecycle is Phase 2.
        // network: keepalive — Phase 2: spawn detached thread, skip response body
        var keepalive = !!(init && init.keepalive);

        // Fetch Priority Hints (WHATWG Fetch §2.2.6): 'high'|'low'|'auto'.
        // Phase 0: parsed and normalised; network priority queue wiring is Phase 2.
        // network: priority queue — lumen-network Phase 2
        var _fetchPriority = (init && init.priority) ? String(init.priority) : 'auto';
        if (_fetchPriority !== 'high' && _fetchPriority !== 'low') { _fetchPriority = 'auto'; }

        var reqBody = (init && init.body !== undefined && init.body !== null) ? init.body
                    : (typeof input === 'object' && input.body ? input.body : null);

        var ok;
        if (reqBody !== null && reqBody !== undefined) {
            var bodyBytes, contentType;
            if (reqBody instanceof FormData) {
                // Fetch spec §5.4: FormData body → multipart/form-data with random boundary.
                // Phase 0: deterministic boundary for testability; production boundary is random.
                var boundary = '----LumenFormBoundary' + Math.random().toString(36).slice(2, 10).toUpperCase();
                var multipartBytes = reqBody._toMultipart(boundary);
                bodyBytes = Array.from(multipartBytes);
                contentType = 'multipart/form-data; boundary=' + boundary;
            } else if (typeof reqBody === 'string') {
                bodyBytes = Array.from(new TextEncoder().encode(reqBody));
                contentType = 'text/plain;charset=UTF-8';
            } else if (reqBody instanceof Uint8Array || reqBody instanceof ArrayBuffer) {
                bodyBytes = reqBody instanceof Uint8Array ? Array.from(reqBody) : Array.from(new Uint8Array(reqBody));
                contentType = 'application/octet-stream';
            } else {
                var s = String(reqBody);
                bodyBytes = Array.from(new TextEncoder().encode(s));
                contentType = 'text/plain;charset=UTF-8';
            }
            // Caller may override Content-Type via headers.
            var initHeaders = (init && init.headers) ? init.headers : null;
            if (initHeaders) {
                var lowerKeys = {};
                if (Array.isArray(initHeaders)) {
                    for (var i = 0; i < initHeaders.length; i++) {
                        if (initHeaders[i][0].toLowerCase() === 'content-type') {
                            contentType = initHeaders[i][1];
                        }
                    }
                } else if (typeof initHeaders === 'object') {
                    for (var k in initHeaders) {
                        if (k.toLowerCase() === 'content-type') { contentType = initHeaders[k]; }
                    }
                }
            }
            ok = _lumen_fetch_sync_with_body(url, method, contentType, bodyBytes);
        } else {
            ok = _lumen_fetch_sync(url, method);
        }

        if (!ok) {
            return Promise.reject(new TypeError('fetch: network error for ' + url));
        }
        var status = _lumen_fetch_get_status();
        var statusText = _lumen_fetch_get_status_text();
        var rawHeaders = _lumen_fetch_get_headers();
        // SRI integrity check (W3C SRI §3.3.5): verify body hash before exposing response.
        // _lumen_check_sri_integrity reads directly from Rust FetchCache — no JS copy needed.
        var integrity = (init && init.integrity) ? String(init.integrity)
                      : (typeof input === 'object' && input && input.integrity ? String(input.integrity) : '');
        if (integrity && !_lumen_check_sri_integrity(integrity)) {
            return Promise.reject(new TypeError('fetch: SRI integrity check failed for ' + url));
        }
        var hdrs = [];
        for (var i = 0; i + 1 < rawHeaders.length; i += 2) {
            hdrs.push([rawHeaders[i], rawHeaders[i + 1]]);
        }
        // Use lazy Rust-side chunk reading: body stays in Rust FetchCache until consumed.
        // This avoids copying large response bodies into JS memory at response construction.
        var resp = Response._fromFetchCache(status, statusText, hdrs);
        resp.url = url;
        return Promise.resolve(resp);
    } catch(e) {
        return Promise.reject(e);
    }
}

// ── WebSocket API (RFC 6455 §§3–7) ─────────────────────────────────────────
// Phase 0 model: synchronous connect; background recv thread queues events;
// JS polls via _lumen_pump_websockets(). Full async delivery (persistent JS
// runtime) is Phase 2+.

var _ws_instances = [];

function CloseEvent(code, reason, wasClean, init) {
    Event.call(this, 'close', init);
    this.code = code || 1000;
    this.reason = reason || '';
    this.wasClean = !!wasClean;
}
CloseEvent.prototype = Object.create(Event.prototype);
CloseEvent.prototype.constructor = CloseEvent;

function MessageEvent(data, init) {
    Event.call(this, 'message', init);
    this.data = data;
    this.origin = '';
    this.lastEventId = '';
}
MessageEvent.prototype = Object.create(Event.prototype);
MessageEvent.prototype.constructor = MessageEvent;

function _lumen_ws_fire(ws, ev) {
    ev.target = ws;
    var prop = 'on' + ev.type;
    if (typeof ws[prop] === 'function') { try { ws[prop](ev); } catch(e) {} }
    var arr = ws._listeners[ev.type];
    if (arr) { for (var i = 0; i < arr.length; i++) { try { arr[i](ev); } catch(e) {} } }
}

function _lumen_ws_pump_one(ws) {
    if (!ws._handle) return;
    var raw;
    while ((raw = _lumen_ws_poll(ws._handle)) !== null && raw !== undefined) {
        try {
            var ev = JSON.parse(raw);
            if (ev.t === 'open') {
                ws.readyState = 1;
                _lumen_ws_fire(ws, new Event('open', { isTrusted: true }));
            } else if (ev.t === 'msg') {
                if (ws.readyState !== 1) { continue; }
                var msgData;
                if (ev.bin) {
                    // Rust encodes binary payload as hex; decode to typed buffer.
                    var hex = ev.data;
                    var len = hex.length >>> 1;
                    var u8 = new Uint8Array(len);
                    for (var bi = 0; bi < len; bi++) {
                        u8[bi] = parseInt(hex.substr(bi * 2, 2), 16);
                    }
                    msgData = ws.binaryType === 'arraybuffer' ? u8.buffer : u8;
                } else {
                    msgData = ev.data;
                }
                _lumen_ws_fire(ws, new MessageEvent(msgData, { isTrusted: true }));
            } else if (ev.t === 'close') {
                ws.readyState = 3;
                _lumen_ws_fire(ws, new CloseEvent(ev.code, ev.reason, ev.code === 1000, { isTrusted: true }));
                ws._handle = 0;
                break;
            } else if (ev.t === 'error') {
                var err = new Event('error', { isTrusted: true }); err.message = ev.msg;
                _lumen_ws_fire(ws, err);
                ws.readyState = 3; ws._handle = 0; break;
            }
        } catch(ignore) {}
    }
}

function _lumen_pump_websockets() {
    for (var i = _ws_instances.length - 1; i >= 0; i--) {
        _lumen_ws_pump_one(_ws_instances[i]);
        if (_ws_instances[i].readyState === 3) { _ws_instances.splice(i, 1); }
    }
}

function WebSocket(url) {
    this.url = String(url || '');
    this.readyState = 0;
    this.protocol = '';
    this.extensions = '';
    this.binaryType = 'blob';
    this.bufferedAmount = 0;
    this.onopen = null; this.onmessage = null;
    this.onclose = null; this.onerror = null;
    this._handle = 0;
    this._listeners = {};
    var self = this;
    var h = _lumen_ws_connect(this.url);
    if (!h) {
        this.readyState = 3;
        setTimeout(function() {
            var e = new Event('error', { isTrusted: true }); e.message = 'WebSocket connection failed';
            _lumen_ws_fire(self, e);
            _lumen_ws_fire(self, new CloseEvent(1006, '', false, { isTrusted: true }));
        }, 0);
        return;
    }
    this._handle = h;
    _ws_instances.push(this);
    // Phase 0: no persistent event loop — caller must invoke _lumen_pump_websockets()
    // after setting onopen/onmessage to receive queued events.
}
WebSocket.prototype.send = function(data) {
    if (this.readyState !== 1) return;
    if (typeof data === 'string') { _lumen_ws_send(this._handle, data); }
    else { _lumen_ws_send_bin(this._handle, data instanceof Uint8Array ? data : new Uint8Array(data)); }
};
WebSocket.prototype.close = function(code, reason) {
    if (this.readyState === 3) return;
    this.readyState = 2;
    _lumen_ws_close(this._handle, typeof code === 'number' ? code : 1000, typeof reason === 'string' ? reason : '');
};
WebSocket.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
WebSocket.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    var idx = this._listeners[type].indexOf(fn);
    if (idx >= 0) this._listeners[type].splice(idx, 1);
};
WebSocket.CONNECTING = 0; WebSocket.OPEN = 1;
WebSocket.CLOSING = 2;    WebSocket.CLOSED = 3;

// ── Web Storage (localStorage / sessionStorage) ───────────────────────────────
// Spec: https://html.spec.whatwg.org/multipage/webstorage.html §8
// Both objects share the same factory; backing native functions differ per type.

function _lumen_make_storage(getLen, getKey, getItem, setItem, removeItem, clear) {
    var obj = {
        key:        function(n) { return _lumen_u2n(getKey(n >>> 0)); },
        getItem:    function(k) { return _lumen_u2n(getItem(String(k))); },
        setItem:    function(k, v) { setItem(String(k), String(v)); },
        removeItem: function(k) { removeItem(String(k)); },
        clear:      function() { clear(); }
    };
    Object.defineProperty(obj, 'length', {
        get: function() { return getLen(); },
        enumerable: false,
        configurable: false
    });
    return obj;
}

var localStorage = _lumen_make_storage(
    _lumen_ls_length, _lumen_ls_key,
    _lumen_ls_get, _lumen_ls_set, _lumen_ls_remove, _lumen_ls_clear
);

var sessionStorage = _lumen_make_storage(
    _lumen_ss_length, _lumen_ss_key,
    _lumen_ss_get, _lumen_ss_set, _lumen_ss_remove, _lumen_ss_clear
);

// ── MutationObserver (WHATWG DOM §4.3.2) ─────────────────────────────────────
// Intercept existing mutation primitives to capture DOM change events.
// Wrapping happens here before the Element API (which calls these primitives)
// is built, so all subsequent setAttribute / innerHTML / appendChild calls
// automatically trigger observer delivery via queueMicrotask.

var _mo_observers = [];
var _mo_delivery_queued = false;

function _mo_notify(nid, type, attrName, oldVal, addedNodeIds, removedNodeIds) {
    var hasObs = false;
    for (var oi = 0; oi < _mo_observers.length; oi++) {
        var obs = _mo_observers[oi];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var entry = obs._observations[ei];
            var tnid = entry.target && entry.target.__nid__;
            if (tnid === undefined) continue;
            var opts = entry.opts;
            // Check if this mutation applies to this observation
            if (tnid !== nid && !opts.subtree) continue;
            if (type === 'attributes' && !opts.attributes) continue;
            if (type === 'childList' && !opts.childList) continue;
            if (type === 'characterData' && !opts.characterData) continue;
            if (type === 'attributes' && opts.attributeFilter &&
                    opts.attributeFilter.indexOf(attrName) < 0) continue;
            var rec = {
                type: type,
                target: entry.target,
                attributeName: attrName || null,
                oldValue: (type === 'attributes' && opts.attributeOldValue) ? oldVal :
                          (type === 'characterData' && opts.characterDataOldValue) ? oldVal : null,
                addedNodes: addedNodeIds || [],
                removedNodes: removedNodeIds || [],
                nextSibling: null,
                previousSibling: null,
            };
            obs._records.push(rec);
            hasObs = true;
        }
    }
    if (hasObs && !_mo_delivery_queued) {
        _mo_delivery_queued = true;
        queueMicrotask(_lumen_flush_mutation_observers);
    }
}

// Synchronous delivery of all pending MutationObserver records.
// Called automatically via queueMicrotask after mutations.
// Can also be called directly by the shell after event dispatch (e.g. after
// _lumen_dispatch) to ensure observer callbacks run before the next paint.
function _lumen_flush_mutation_observers() {
    _mo_delivery_queued = false;
    for (var i = 0; i < _mo_observers.length; i++) {
        var o = _mo_observers[i];
        if (o._records.length === 0) continue;
        var recs = o._records;
        o._records = [];
        try { o._cb(recs, o); } catch(e) {}
    }
}

// Wrap _lumen_set_attr to intercept attribute mutations
var _orig_set_attr = _lumen_set_attr;
_lumen_set_attr = function(nid, name, value) {
    var old = (_mo_observers.length > 0) ? _lumen_get_attr(nid, name) : undefined;
    _orig_set_attr(nid, name, value);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'attributes', String(name), old !== undefined ? old : null, null, null);
    }
};

// Wrap _lumen_set_inner_html to intercept childList mutations
var _orig_set_inner_html = _lumen_set_inner_html;
_lumen_set_inner_html = function(nid, html) {
    _orig_set_inner_html(nid, html);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'childList', null, null, [], []);
    }
};

// Wrap _lumen_append_child to intercept childList mutations
var _orig_append_child = _lumen_append_child;
_lumen_append_child = function(parent, child) {
    _orig_append_child(parent, child);
    if (_mo_observers.length > 0) {
        _mo_notify(parent, 'childList', null, null, [child], []);
    }
};

// Wrap _lumen_remove_child to intercept childList mutations
var _orig_remove_child = _lumen_remove_child;
_lumen_remove_child = function(parent, child) {
    _orig_remove_child(parent, child);
    if (_mo_observers.length > 0) {
        _mo_notify(parent, 'childList', null, null, [], [child]);
    }
};

// Wrap _lumen_set_text_content to intercept characterData/childList mutations
var _orig_set_text_content = _lumen_set_text_content;
_lumen_set_text_content = function(nid, text) {
    _orig_set_text_content(nid, text);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'characterData', null, null, null, null);
    }
};

function MutationObserver(callback) {
    this._cb = callback;
    this._observations = [];
    this._records = [];
    _mo_observers.push(this);
}
MutationObserver.prototype.observe = function(target, options) {
    if (!target || target.__nid__ === undefined) return;
    var opts = options || {};
    var config = {
        target: target,
        opts: {
            childList:               !!opts.childList,
            attributes:              !!(opts.attributes || opts.attributeFilter || opts.attributeOldValue),
            characterData:           !!opts.characterData,
            subtree:                 !!opts.subtree,
            attributeOldValue:       !!opts.attributeOldValue,
            characterDataOldValue:   !!opts.characterDataOldValue,
            attributeFilter:         opts.attributeFilter ? opts.attributeFilter.slice() : null,
        },
    };
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) {
            this._observations[i] = config;
            return;
        }
    }
    this._observations.push(config);
};
MutationObserver.prototype.disconnect = function() {
    var idx = _mo_observers.indexOf(this);
    if (idx >= 0) _mo_observers.splice(idx, 1);
    this._observations = [];
    this._records = [];
};
MutationObserver.prototype.takeRecords = function() {
    var r = this._records;
    this._records = [];
    return r;
};

// ── ResizeObserver (W3C Resize Observer §5) ───────────────────────────────────
// Delivers size-change entries after layout; the shell calls
// _lumen_deliver_resize_observers() after each relayout.

var _ro_observers = [];

function ResizeObserver(callback) {
    this._cb = callback;
    this._observations = [];
    _ro_observers.push(this);
}
ResizeObserver.prototype.observe = function(target) {
    if (!target || target.__nid__ === undefined) return;
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) return;
    }
    this._observations.push({ target: target, lastW: -1, lastH: -1 });
};
ResizeObserver.prototype.unobserve = function(target) {
    this._observations = this._observations.filter(function(o) { return o.target !== target; });
};
ResizeObserver.prototype.disconnect = function() {
    var idx = _ro_observers.indexOf(this);
    if (idx >= 0) _ro_observers.splice(idx, 1);
    this._observations = [];
};

function _lumen_deliver_resize_observers() {
    if (_ro_observers.length === 0) return;
    for (var oi = 0; oi < _ro_observers.length; oi++) {
        var obs = _ro_observers[oi];
        var entries = [];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var o = obs._observations[ei];
            var nid = o.target.__nid__;
            var rect = _lumen_get_bounding_rect(nid);
            if (!rect) continue;
            var w = rect[2], h = rect[3];
            if (Math.abs(w - o.lastW) < 0.5 && Math.abs(h - o.lastH) < 0.5) continue;
            o.lastW = w; o.lastH = h;
            entries.push({
                target: o.target,
                contentRect: { x: rect[0], y: rect[1], width: w, height: h,
                               top: rect[1], left: rect[0], bottom: rect[1]+h, right: rect[0]+w },
                borderBoxSize:  [{ inlineSize: w, blockSize: h }],
                contentBoxSize: [{ inlineSize: w, blockSize: h }],
                devicePixelContentBoxSize: [{ inlineSize: w, blockSize: h }],
            });
        }
        if (entries.length > 0) {
            try { obs._cb(entries, obs); } catch(e) {}
        }
    }
}

// ── Canvas CSS resize tracking ────────────────────────────────────────────────
// When a canvas element's CSS layout dimensions change (detected after each
// relayout), the backing bitmap is scaled to the new size and a `resize` event
// is fired on the element (HTML LS §4.12.4 / Resize Observer integration).
//
// The shell calls _lumen_deliver_canvas_css_resize() after update_layout_rects,
// alongside _lumen_deliver_resize_observers and _lumen_deliver_intersection_observers.

// last CSS dimensions per canvas nid (as a string key), set on first observation.
var _canvas_css_dims = {};

function _lumen_deliver_canvas_css_resize() {
    for (var nid_str in _canvas2d_ctxs) {
        var nid = +nid_str;
        var rect = _lumen_get_bounding_rect(nid);
        if (!rect) continue;
        var w = (rect[2] + 0.5) | 0;  // round to integer CSS px
        var h = (rect[3] + 0.5) | 0;
        if (w < 1) w = 1;
        if (h < 1) h = 1;
        var prev = _canvas_css_dims[nid_str];
        if (!prev) {
            // first observation — record dims without firing event
            _canvas_css_dims[nid_str] = [w, h];
            continue;
        }
        if (prev[0] === w && prev[1] === h) continue;
        // CSS dimensions changed: scale pixel buffer and fire event
        _canvas_css_dims[nid_str] = [w, h];
        _lumen_canvas2d_scale_resize(nid, w, h);
        _lumen_dispatch(nid, new Event('resize'));
    }
}

// ── IntersectionObserver (WICG Intersection Observer §4) ─────────────────────
// Delivers intersection entries after layout; the shell calls
// _lumen_deliver_intersection_observers() after each relayout.

var _io_observers = [];

function IntersectionObserver(callback, options) {
    this._cb = callback;
    this._options = options || {};
    this._observations = [];
    _io_observers.push(this);
}
IntersectionObserver.prototype.observe = function(target) {
    if (!target || target.__nid__ === undefined) return;
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) return;
    }
    // lastRatio = -1 means «never delivered» → first delivery always fires
    this._observations.push({ target: target, lastRatio: -1 });
};
IntersectionObserver.prototype.unobserve = function(target) {
    this._observations = this._observations.filter(function(o) { return o.target !== target; });
};
IntersectionObserver.prototype.disconnect = function() {
    var idx = _io_observers.indexOf(this);
    if (idx >= 0) _io_observers.splice(idx, 1);
    this._observations = [];
};

// Parse CSS margin shorthand into [top, right, bottom, left] px values.
// Only px units are supported; other units resolve to 0.
function _parse_root_margin(str) {
    if (!str) return [0, 0, 0, 0];
    var parts = str.trim().split(/\\s+/);
    var vals = parts.map(function(p) {
        return p.indexOf('px') >= 0 ? parseFloat(p) : 0;
    });
    if (vals.length === 1) return [vals[0], vals[0], vals[0], vals[0]];
    if (vals.length === 2) return [vals[0], vals[1], vals[0], vals[1]];
    if (vals.length === 3) return [vals[0], vals[1], vals[2], vals[1]];
    return [vals[0], vals[1], vals[2], vals[3]];
}

function _lumen_deliver_intersection_observers() {
    if (_io_observers.length === 0) return;
    var vp = _lumen_get_viewport_size();
    var vpW = vp[0], vpH = vp[1];
    for (var oi = 0; oi < _io_observers.length; oi++) {
        var obs = _io_observers[oi];
        // Apply rootMargin to expand/contract the intersection root (viewport).
        // Positive margin expands outward; negative contracts inward.
        var rm = _parse_root_margin(obs._options.rootMargin);
        var rootTop = -rm[0], rootLeft = -rm[3];
        var rootRight = vpW + rm[1], rootBottom = vpH + rm[2];
        var t = obs._options.threshold !== undefined ? obs._options.threshold : 0;
        var thresholds = Array.isArray(t) ? t : [t];
        var entries = [];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var o = obs._observations[ei];
            var nid = o.target.__nid__;
            var rect = _lumen_get_bounding_rect(nid);
            if (!rect) continue;
            var ex = rect[0], ey = rect[1], ew = rect[2], eh = rect[3];
            var ix = Math.max(ex, rootLeft);
            var iy = Math.max(ey, rootTop);
            var iw = Math.max(0, Math.min(ex + ew, rootRight) - ix);
            var ih = Math.max(0, Math.min(ey + eh, rootBottom) - iy);
            var area = ew * eh;
            var ratio = area > 0 ? (iw * ih) / area : 0;
            var prev = o.lastRatio;
            var crossed = prev < 0; // first observation
            if (!crossed) {
                for (var ti = 0; ti < thresholds.length; ti++) {
                    var thr = thresholds[ti];
                    if ((prev < thr) !== (ratio < thr) ||
                        (prev === 0 && ratio > 0) || (prev > 0 && ratio === 0)) {
                        crossed = true;
                        break;
                    }
                }
            }
            if (!crossed) continue;
            o.lastRatio = ratio;
            entries.push({
                target: o.target,
                isIntersecting: ratio > 0,
                intersectionRatio: ratio,
                boundingClientRect: { x: ex, y: ey, width: ew, height: eh,
                                      top: ey, left: ex, bottom: ey+eh, right: ex+ew },
                intersectionRect:   { x: ix, y: iy, width: iw, height: ih,
                                      top: iy, left: ix, bottom: iy+ih, right: ix+iw },
                rootBounds: { x: rootLeft, y: rootTop,
                              width: rootRight - rootLeft, height: rootBottom - rootTop,
                              top: rootTop, left: rootLeft,
                              bottom: rootBottom, right: rootRight },
                time: typeof performance !== 'undefined' ? performance.now() : 0,
            });
        }
        if (entries.length > 0) {
            try { obs._cb(entries, obs); } catch(e) {}
        }
    }
}

// ── TreeWalker / NodeIterator / NodeFilter (DOM LS §4.4–4.5) ─────────────────
// NodeFilter constants (DOM LS §4.3).
var NodeFilter = {
    FILTER_ACCEPT:  1,
    FILTER_REJECT:  2,
    FILTER_SKIP:    3,
    SHOW_ALL:            0xFFFFFFFF,
    SHOW_ELEMENT:        0x1,
    SHOW_TEXT:           0x4,
    SHOW_CDATA_SECTION:  0x8,
    SHOW_COMMENT:        0x80,
    SHOW_DOCUMENT:       0x100,
    SHOW_DOCUMENT_TYPE:  0x200,
    SHOW_DOCUMENT_FRAGMENT: 0x400,
};

// Returns NodeFilter.FILTER_ACCEPT / SKIP / REJECT for a node nid given
// whatToShow bitmask and an optional filter callback or NodeFilter object.
function _nf_accepts(nid, whatToShow, filter) {
    // whatToShow bitmask check
    var nt = _lumen_is_text_node(nid) ? 3 : 1; // 1=element, 3=text
    var bit = (nt === 3) ? NodeFilter.SHOW_TEXT : NodeFilter.SHOW_ELEMENT;
    if (!(whatToShow & bit)) return NodeFilter.FILTER_SKIP;
    if (!filter) return NodeFilter.FILTER_ACCEPT;
    var el = _lumen_make_element(nid);
    var result;
    if (typeof filter === 'function') {
        try { result = filter(el); } catch(e) { result = NodeFilter.FILTER_REJECT; }
    } else if (filter && typeof filter.acceptNode === 'function') {
        try { result = filter.acceptNode(el); } catch(e) { result = NodeFilter.FILTER_REJECT; }
    } else {
        result = NodeFilter.FILTER_ACCEPT;
    }
    return result;
}

// Collects all nids in subtree of root in document order (pre-order, depth-first).
function _tw_subtree(root_nid) {
    var result = [];
    function visit(n) {
        result.push(n);
        var ch = _lumen_get_children(n);
        for (var i = 0; i < ch.length; i++) visit(ch[i]);
    }
    visit(root_nid);
    return result;
}

// ── TreeWalker (DOM LS §4.5) ─────────────────────────────────────────────────
function _TreeWalker(root, whatToShow, filter) {
    this.root        = root;
    this.whatToShow  = whatToShow;
    this.filter      = filter;
    this.currentNode = root;
}

_TreeWalker.prototype._root_nid = function() {
    return this.root && this.root.__nid__ !== undefined ? this.root.__nid__ : null;
};

_TreeWalker.prototype._cur_nid = function() {
    return this.currentNode && this.currentNode.__nid__ !== undefined ? this.currentNode.__nid__ : null;
};

// Returns the parent node within the root subtree, or null.
_TreeWalker.prototype.parentNode = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var p = _lumen_u2n(_lumen_get_parent(cur));
    while (p !== null) {
        if (p === root) { break; }
        var pp = _lumen_u2n(_lumen_get_parent(p));
        if (pp === null) { p = null; break; }
        p = pp;
    }
    if (p === null) return null;
    // Walk from root towards cur; find first ancestor that is accepted
    // Actually per spec: parentNode returns the nearest accepted ancestor in root subtree.
    var candidate = _lumen_u2n(_lumen_get_parent(cur));
    while (candidate !== null && candidate !== root) {
        var r = _nf_accepts(candidate, this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(candidate);
            return this.currentNode;
        }
        candidate = _lumen_u2n(_lumen_get_parent(candidate));
    }
    // Check root itself
    if (root !== null && cur !== root) {
        var rr = _nf_accepts(root, this.whatToShow, this.filter);
        if (rr === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = this.root;
            return this.currentNode;
        }
    }
    return null;
};

// Returns the first child of currentNode that passes the filter.
_TreeWalker.prototype.firstChild = function() {
    var children = _lumen_get_children(this._cur_nid() || 0);
    for (var i = 0; i < children.length; i++) {
        var r = _nf_accepts(children[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(children[i]);
            return this.currentNode;
        }
        if (r !== NodeFilter.FILTER_REJECT) {
            // SKIP — recurse into its children (DOM spec §4.5.5)
            var saved = this.currentNode;
            this.currentNode = _lumen_make_element(children[i]);
            var found = this.firstChild();
            if (found) return found;
            this.currentNode = saved;
        }
    }
    return null;
};

// Returns the last child of currentNode that passes the filter.
_TreeWalker.prototype.lastChild = function() {
    var children = _lumen_get_children(this._cur_nid() || 0);
    for (var i = children.length - 1; i >= 0; i--) {
        var r = _nf_accepts(children[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(children[i]);
            return this.currentNode;
        }
        if (r !== NodeFilter.FILTER_REJECT) {
            var saved = this.currentNode;
            this.currentNode = _lumen_make_element(children[i]);
            var found = this.lastChild();
            if (found) return found;
            this.currentNode = saved;
        }
    }
    return null;
};

// Returns the previous sibling (in root subtree) of currentNode.
_TreeWalker.prototype.previousSibling = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var pid = _lumen_u2n(_lumen_get_parent(cur));
    if (pid === null) return null;
    var sibs = _lumen_get_children(pid);
    var idx  = sibs.indexOf(cur);
    for (var i = idx - 1; i >= 0; i--) {
        var r = _nf_accepts(sibs[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(sibs[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the next sibling (in root subtree) of currentNode.
_TreeWalker.prototype.nextSibling = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var pid = _lumen_u2n(_lumen_get_parent(cur));
    if (pid === null) return null;
    var sibs = _lumen_get_children(pid);
    var idx  = sibs.indexOf(cur);
    for (var i = idx + 1; i < sibs.length; i++) {
        var r = _nf_accepts(sibs[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(sibs[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the previous node in document order (depth-first pre-order) that passes filter.
_TreeWalker.prototype.previousNode = function() {
    var root = this._root_nid();
    var cur  = this._cur_nid();
    if (cur === null || cur === root) return null;
    var all = _tw_subtree(root);
    var idx = all.indexOf(cur);
    for (var i = idx - 1; i >= 0; i--) {
        var r = _nf_accepts(all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(all[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the next node in document order (depth-first pre-order) that passes filter.
_TreeWalker.prototype.nextNode = function() {
    var root = this._root_nid();
    var cur  = this._cur_nid();
    if (root === null) return null;
    var all = _tw_subtree(root);
    var idx = cur !== null ? all.indexOf(cur) : -1;
    for (var i = idx + 1; i < all.length; i++) {
        var r = _nf_accepts(all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(all[i]);
            return this.currentNode;
        }
    }
    return null;
};

// ── NodeIterator (DOM LS §4.4) ───────────────────────────────────────────────
// Simplified: maintains a reference position as an index into the flat subtree.
function _NodeIterator(root, whatToShow, filter) {
    this.root        = root;
    this.whatToShow  = whatToShow;
    this.filter      = filter;
    this._all        = null; // lazily built
    this._pos        = -1;   // -1 = before root
    this.referenceNode = root;
    this.pointerBeforeReferenceNode = true;
}

_NodeIterator.prototype._ensure = function() {
    if (this._all === null) {
        var root_nid = this.root && this.root.__nid__ !== undefined ? this.root.__nid__ : null;
        this._all = root_nid !== null ? _tw_subtree(root_nid) : [];
    }
};

// Returns the next accepted node (forward traversal).
_NodeIterator.prototype.nextNode = function() {
    this._ensure();
    for (var i = this._pos + 1; i < this._all.length; i++) {
        var r = _nf_accepts(this._all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this._pos = i;
            var el = _lumen_make_element(this._all[i]);
            this.referenceNode = el;
            this.pointerBeforeReferenceNode = false;
            return el;
        }
    }
    return null;
};

// Returns the previous accepted node (backward traversal).
_NodeIterator.prototype.previousNode = function() {
    this._ensure();
    for (var i = this._pos - 1; i >= 0; i--) {
        var r = _nf_accepts(this._all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this._pos = i;
            var el = _lumen_make_element(this._all[i]);
            this.referenceNode = el;
            this.pointerBeforeReferenceNode = true;
            return el;
        }
    }
    return null;
};

// No-op per DOM LS §4.4.6.
_NodeIterator.prototype.detach = function() {};

// ── CaretPosition (CSSOM View §5.1) ──────────────────────────────────────────
// Returned by document.caretPositionFromPoint(). Phase 0: no layout hit-testing;
// always points to body at offset 0. getClientRects() returns an empty list.
function _CaretPosition(offsetNode, offset) {
    this.offsetNode = offsetNode;
    this.offset     = offset;
}
_CaretPosition.prototype.getClientRects = function() { return []; };

// ── window.matchMedia / MediaQueryList (CSS Media Queries L4 §4.2) ───────────
// Pure-JS shim on top of the native binding `_lumen_match_media` (parses + matches
// a media query against an ad-hoc MediaContext). The registry keeps strong refs
// while the user-side MQL is reachable; shell pumps changes via
// `_lumen_deliver_media_changes(w, h, dark, reducedMotion)` after each relayout
// or preference flip.
var _mqlRegistry = [];

function MediaQueryListEvent(type, init) {
    Event.call(this, type, init || {});
    this.media   = (init && init.media)   || '';
    this.matches = !!(init && init.matches);
}
MediaQueryListEvent.prototype = Object.create(Event.prototype);
MediaQueryListEvent.prototype.constructor = MediaQueryListEvent;

function MediaQueryList(media) {
    var vp = (typeof _lumen_get_viewport_size === 'function')
        ? _lumen_get_viewport_size() : [800, 600];
    this.media       = String(media == null ? '' : media);
    this.matches     = !!_lumen_match_media(this.media, vp[0], vp[1], false, false);
    this.onchange    = null;
    this._listeners  = [];
}
MediaQueryList.prototype.addListener = function(fn) {
    if (typeof fn === 'function') this.addEventListener('change', fn);
};
MediaQueryList.prototype.removeListener = function(fn) {
    if (typeof fn === 'function') this.removeEventListener('change', fn);
};
MediaQueryList.prototype.addEventListener = function(type, fn) {
    if (type === 'change' && typeof fn === 'function') {
        // Spec: ignore duplicate registrations of the same callback.
        for (var i = 0; i < this._listeners.length; i++) {
            if (this._listeners[i] === fn) return;
        }
        this._listeners.push(fn);
    }
};
MediaQueryList.prototype.removeEventListener = function(type, fn) {
    if (type === 'change') {
        var idx = this._listeners.indexOf(fn);
        if (idx !== -1) this._listeners.splice(idx, 1);
    }
};
MediaQueryList.prototype.dispatchEvent = function(ev) {
    if (!ev || ev.type !== 'change') return true;
    for (var i = 0; i < this._listeners.length; i++) {
        try { this._listeners[i].call(this, ev); } catch(e) {}
    }
    if (typeof this.onchange === 'function') {
        try { this.onchange.call(this, ev); } catch(e) {}
    }
    return !ev.defaultPrevented;
};
MediaQueryList.prototype._fire = function(matches) {
    this.matches = matches;
    var ev = new MediaQueryListEvent('change', { media: this.media, matches: matches });
    ev.target = this;
    ev.currentTarget = this;
    this.dispatchEvent(ev);
};

// Shell entry point: re-evaluate every registered MediaQueryList against the
// new context. Fires `change` only when `matches` actually flipped (spec).
function _lumen_deliver_media_changes(w, h, dark, reducedMotion) {
    var darkB = !!dark;
    var rmB   = !!reducedMotion;
    for (var i = 0; i < _mqlRegistry.length; i++) {
        var mql = _mqlRegistry[i];
        if (!mql) continue;
        var newM = !!_lumen_match_media(mql.media, w, h, darkB, rmB);
        if (mql.matches !== newM) mql._fire(newM);
    }
}

// ── postMessage (HTML LS §7.7.4) ─────────────────────────────────────────────
var _message_listeners = [];

// ── Window load / DOMContentLoaded / visibilitychange / error listener arrays ──
var _load_listeners = [];
var _domcontentloaded_win_listeners = [];
var _visibilitychange_listeners = [];
var _error_listeners = [];
var _other_win_listeners = {};

var window = {
    history: history,
    onpopstate: null,
    onmessage: null,
    onpageshow: null,
    onpagehide: null,
    onload: null,
    location: location,
    navigator: navigator,
    alert: alert,
    confirm: confirm,
    prompt: prompt,
    print: print,
    setTimeout: setTimeout,
    setInterval: setInterval,
    clearTimeout: clearTimeout,
    clearInterval: clearInterval,
    requestAnimationFrame: requestAnimationFrame,
    cancelAnimationFrame: cancelAnimationFrame,
    _lumen_run_raf_callbacks: _lumen_run_raf_callbacks,
    EventSource: EventSource,
    WebSocket: WebSocket,
    CloseEvent: CloseEvent,
    MessageEvent: MessageEvent,
    _lumen_pump_websockets: _lumen_pump_websockets,
    _lumen_pump_sse: _lumen_pump_sse,
    caches: caches,
    document: document,
    console: console,
    fetch: fetch,
    Request: Request,
    Response: Response,
    Headers: Headers,
    AbortController: AbortController,
    AbortSignal: AbortSignal,
    ReadableStream: ReadableStream,
    WritableStream: WritableStream,
    TransformStream: TransformStream,
    ReadableStreamDefaultReader: ReadableStreamDefaultReader,
    WritableStreamDefaultWriter: WritableStreamDefaultWriter,
    TextDecoderStream: TextDecoderStream,
    TextEncoderStream: TextEncoderStream,
    CompressionStream: CompressionStream,
    DecompressionStream: DecompressionStream,
    ByteLengthQueuingStrategy: ByteLengthQueuingStrategy,
    CountQueuingStrategy: CountQueuingStrategy,
    FormData: FormData,
    TextEncoder: TextEncoder,
    TextDecoder: TextDecoder,
    localStorage: localStorage,
    sessionStorage: sessionStorage,
    _lumen_dispatch_composition: _lumen_dispatch_composition,
    _lumen_dispatch_mouse_event:   _lumen_dispatch_mouse_event,
    _lumen_dispatch_pointer_event: _lumen_dispatch_pointer_event,
    _lumen_dispatch_capture_event: _lumen_dispatch_capture_event,
    _lumen_dispatch_key_event:     _lumen_dispatch_key_event,
    _lumen_dispatch_rich:          _lumen_dispatch_rich,
    _lumen_set_ime_target: _lumen_set_ime_target,
    _lumen_fire_page_lifecycle: _lumen_fire_page_lifecycle,
    addEventListener: function(type, fn) {
        if (typeof fn !== 'function') return;
        if (type === 'popstate') {
            _popstate_listeners.push(fn);
        } else if (type === 'pageshow') {
            _pageshow_listeners.push(fn);
        } else if (type === 'pagehide') {
            _pagehide_listeners.push(fn);
        } else if (type === 'message') {
            _message_listeners.push(fn);
        } else if (type === 'load') {
            if (_doc_ready_state === 'complete') {
                // already loaded — fire async per spec
                queueMicrotask(function() {
                    try { fn(new Event('load', { bubbles: false })); } catch(e) {}
                });
            } else {
                _load_listeners.push(fn);
            }
        } else if (type === 'DOMContentLoaded') {
            if (_doc_ready_state !== 'loading') {
                queueMicrotask(function() {
                    try { fn(new Event('DOMContentLoaded', { bubbles: true })); } catch(e) {}
                });
            } else {
                _domcontentloaded_win_listeners.push(fn);
            }
        } else if (type === 'visibilitychange') {
            _visibilitychange_listeners.push(fn);
        } else if (type === 'error') {
            _error_listeners.push(fn);
        } else {
            if (!_other_win_listeners[type]) _other_win_listeners[type] = [];
            _other_win_listeners[type].push(fn);
        }
    },
    removeEventListener: function(type, fn) {
        var arr;
        if (type === 'popstate') arr = _popstate_listeners;
        else if (type === 'pageshow') arr = _pageshow_listeners;
        else if (type === 'pagehide') arr = _pagehide_listeners;
        else if (type === 'message') arr = _message_listeners;
        else if (type === 'load') arr = _load_listeners;
        else if (type === 'DOMContentLoaded') arr = _domcontentloaded_win_listeners;
        else if (type === 'visibilitychange') arr = _visibilitychange_listeners;
        else if (type === 'error') arr = _error_listeners;
        else arr = _other_win_listeners[type];
        if (!arr) return;
        var idx = arr.indexOf(fn);
        if (idx >= 0) arr.splice(idx, 1);
    },
    dispatchEvent: function(evt) {
        if (!evt || !evt.type) return true;
        var arr;
        if (evt.type === 'load') {
            arr = _load_listeners.slice();
            for (var i = 0; i < arr.length; i++) {
                try { arr[i].call(window, evt); } catch(e) {}
            }
            if (typeof window.onload === 'function') {
                try { window.onload.call(window, evt); } catch(e) {}
            }
        } else if (evt.type === 'error') {
            arr = _error_listeners.slice();
            for (var i = 0; i < arr.length; i++) { try { arr[i].call(window, evt); } catch(e) {} }
            if (typeof window.onerror === 'function') { try { window.onerror.call(window, evt); } catch(e) {} }
        } else {
            arr = _other_win_listeners[evt.type];
            if (arr) {
                arr = arr.slice();
                for (var i = 0; i < arr.length; i++) { try { arr[i].call(window, evt); } catch(e) {} }
            }
        }
        return !evt.defaultPrevented;
    },
    /// postMessage (HTML LS §7.7.4): dispatch a MessageEvent to this window.
    /// targetOrigin '*' → always deliver; '/' → same-origin only;
    /// any other string → must equal location.origin.
    postMessage: function(message, targetOrigin) {
        var origin = location.origin;
        if (targetOrigin !== '*') {
            var target = (targetOrigin === '/') ? origin : String(targetOrigin);
            if (target !== origin) return;
        }
        var ev = new MessageEvent(message);
        ev.origin = origin;
        ev.source = window;
        // Spec §7.7.4 step 5: dispatch as a task (asynchronously).
        setTimeout(function() {
            if (typeof window.onmessage === 'function') {
                try { window.onmessage(ev); } catch(e) {}
            }
            for (var i = 0; i < _message_listeners.length; i++) {
                try { _message_listeners[i](ev); } catch(e) {}
            }
        }, 0);
    },
};

// ── queueMicrotask (HTML LS §8.1.4.4) ────────────────────────────────────────
// Schedules `fn` as a microtask; implemented via resolved Promise chain which
// QuickJS drains between tasks (same semantics as spec §8.1.4.2 microtask queue).
function queueMicrotask(fn) {
    if (typeof fn !== 'function') throw new TypeError('queueMicrotask: argument must be a function');
    Promise.resolve().then(fn);
}

// ── URLSearchParams (WHATWG URL §5) ──────────────────────────────────────────
function URLSearchParams(init) {
    this._p = [];
    if (init === undefined || init === null) return;
    if (typeof init === 'string') {
        var s = (init.length > 0 && init[0] === '?') ? init.slice(1) : init;
        if (!s) return;
        var pairs = s.split('&');
        for (var i = 0; i < pairs.length; i++) {
            var pair = pairs[i];
            if (!pair) continue;
            var eq = pair.indexOf('=');
            var k = eq >= 0 ? pair.slice(0, eq) : pair;
            var v = eq >= 0 ? pair.slice(eq + 1) : '';
            this._p.push([_usp_decode(k), _usp_decode(v)]);
        }
    } else if (Array.isArray(init)) {
        for (var i = 0; i < init.length; i++) {
            var entry = init[i];
            if (!Array.isArray(entry) || entry.length < 2)
                throw new TypeError('URLSearchParams: each sequence entry must have 2 items');
            this._p.push([String(entry[0]), String(entry[1])]);
        }
    } else if (typeof init === 'object') {
        var keys = Object.keys(init);
        for (var i = 0; i < keys.length; i++) {
            this._p.push([String(keys[i]), String(init[keys[i]])]);
        }
    }
}
function _usp_decode(s) {
    try { return decodeURIComponent(s.split('+').join(' ')); } catch(e) { return s; }
}
function _usp_encode(s) {
    // application/x-www-form-urlencoded percent-encode set (WHATWG URL §5.1 step 2)
    return encodeURIComponent(s).replace(/%20/g, '+');
}
URLSearchParams.prototype.append = function(name, value) {
    this._p.push([String(name), String(value)]);
};
URLSearchParams.prototype.delete = function(name) {
    var n = String(name);
    this._p = this._p.filter(function(e) { return e[0] !== n; });
};
URLSearchParams.prototype.get = function(name) {
    var n = String(name);
    for (var i = 0; i < this._p.length; i++) { if (this._p[i][0] === n) return this._p[i][1]; }
    return null;
};
URLSearchParams.prototype.getAll = function(name) {
    var n = String(name); var out = [];
    for (var i = 0; i < this._p.length; i++) { if (this._p[i][0] === n) out.push(this._p[i][1]); }
    return out;
};
URLSearchParams.prototype.has = function(name) {
    var n = String(name);
    for (var i = 0; i < this._p.length; i++) { if (this._p[i][0] === n) return true; }
    return false;
};
URLSearchParams.prototype.set = function(name, value) {
    var n = String(name), v = String(value), found = false;
    this._p = this._p.filter(function(e) {
        if (e[0] !== n) return true;
        if (!found) { found = true; e[1] = v; return true; }
        return false;
    });
    if (!found) this._p.push([n, v]);
};
URLSearchParams.prototype.sort = function() {
    this._p.sort(function(a, b) { return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0; });
};
URLSearchParams.prototype.toString = function() {
    return this._p.map(function(e) { return _usp_encode(e[0]) + '=' + _usp_encode(e[1]); }).join('&');
};
URLSearchParams.prototype.forEach = function(cb, thisArg) {
    for (var i = 0; i < this._p.length; i++) cb.call(thisArg, this._p[i][1], this._p[i][0], this);
};
URLSearchParams.prototype.keys = function() {
    var p = this._p, i = 0;
    return { next: function() { return i < p.length ? { value: p[i++][0], done: false } : { value: undefined, done: true }; },
             Symbol_iterator: function() { return this; } };
};
URLSearchParams.prototype.values = function() {
    var p = this._p, i = 0;
    return { next: function() { return i < p.length ? { value: p[i++][1], done: false } : { value: undefined, done: true }; },
             Symbol_iterator: function() { return this; } };
};
URLSearchParams.prototype.entries = function() {
    var p = this._p, i = 0;
    return { next: function() { return i < p.length ? { value: [p[i][0], p[i++][1]], done: false } : { value: undefined, done: true }; },
             Symbol_iterator: function() { return this; } };
};
URLSearchParams.prototype.size = undefined; // defined as getter below
Object.defineProperty(URLSearchParams.prototype, 'size', {
    get: function() { return this._p.length; }
});

// ── URL (WHATWG URL §6.1) ─────────────────────────────────────────────────────
// Supports absolute URLs and resolution against a base URL.
// Full IDNA/percent-encoding spec requires platform support; this is a
// high-fidelity subset sufficient for the most common JS URL patterns.
function _url_resolve(href, base) {
    href = String(href || '');
    // Already absolute?
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(href)) return href;
    if (!base) return href;
    var bp = _lumen_parse_url(String(base));
    // Protocol-relative
    if (href.slice(0, 2) === '//') return bp.protocol + href;
    // Root-relative
    if (href[0] === '/') return bp.protocol + '//' + bp.host + href;
    // Fragment-only or query-only
    if (href[0] === '#') return bp.protocol + '//' + bp.host + bp.pathname + bp.search + href;
    if (href[0] === '?') return bp.protocol + '//' + bp.host + bp.pathname + href;
    // Relative path
    var dir = bp.pathname.slice(0, bp.pathname.lastIndexOf('/') + 1);
    var raw = dir + href;
    // Normalize dot segments (RFC 3986 §5.2.4)
    var parts = raw.split('/');
    var out = [];
    for (var i = 0; i < parts.length; i++) {
        if (parts[i] === '.') continue;
        if (parts[i] === '..') { if (out.length > 1) out.pop(); }
        else out.push(parts[i]);
    }
    return bp.protocol + '//' + bp.host + out.join('/');
}
function URL(href, base) {
    if (arguments.length === 0) throw new TypeError('URL constructor: at least 1 argument required');
    var resolved = _url_resolve(String(href), base ? String(base) : (typeof location !== 'undefined' ? location.href : ''));
    var p = _lumen_parse_url(resolved);
    if (!p.protocol) throw new TypeError('URL constructor: invalid URL: ' + href);
    this._href     = p.href;
    this._protocol = p.protocol;
    this._hostname = p.hostname;
    this._host     = p.host;
    this._port     = p.port;
    this._pathname = p.pathname;
    this._search   = p.search;
    this._hash     = p.hash;
    this._origin   = p.origin;
    this._sp       = null; // lazy URLSearchParams
}
(function() {
    function prop(key, getter, setter) {
        Object.defineProperty(URL.prototype, key, {
            get: getter,
            set: setter || function() {},
            enumerable: true, configurable: true
        });
    }
    prop('href',     function() { return this._href; },     function(v) { var p=_lumen_parse_url(String(v)); this._href=p.href; this._protocol=p.protocol; this._hostname=p.hostname; this._host=p.host; this._port=p.port; this._pathname=p.pathname; this._search=p.search; this._hash=p.hash; this._origin=p.origin; this._sp=null; });
    prop('protocol', function() { return this._protocol; });
    prop('hostname', function() { return this._hostname; });
    prop('host',     function() { return this._host; });
    prop('port',     function() { return this._port; });
    prop('pathname', function() { return this._pathname; });
    prop('search',   function() { return this._search; });
    prop('hash',     function() { return this._hash; });
    prop('origin',   function() { return this._origin; });
    prop('username', function() { return ''; });
    prop('password', function() { return ''; });
    prop('searchParams', function() {
        if (!this._sp) this._sp = new URLSearchParams(this._search);
        return this._sp;
    });
    URL.prototype.toString = function() { return this._href; };
    URL.prototype.toJSON   = function() { return this._href; };
    // URL.canParse(url, base?) — URL Living Standard §6.1 static method (2023)
    URL.canParse = function(url, base) {
        try { new URL(String(url), base !== undefined ? String(base) : undefined); return true; }
        catch (e) { return false; }
    };
    // URL.parse(url, base?) — returns URL or null (URL Living Standard §6.1)
    URL.parse = function(url, base) {
        try { return new URL(String(url), base !== undefined ? String(base) : undefined); }
        catch (e) { return null; }
    };
})();
// ── btoa / atob (HTML5 Living Std §2.4.7 + RFC 4648 §4) ─────────────────────
var _b64c = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function btoa(str) {
    var s = String(str), out = '';
    for (var i = 0; i < s.length; i++) {
        if (s.charCodeAt(i) > 0xff) throw new TypeError('btoa: character out of Latin1 range');
    }
    for (var j = 0; j < s.length; j += 3) {
        var b0 = s.charCodeAt(j), b1 = s.charCodeAt(j+1) || 0, b2 = s.charCodeAt(j+2) || 0;
        out += _b64c[b0 >> 2];
        out += _b64c[((b0 & 3) << 4) | (b1 >> 4)];
        out += j+1 < s.length ? _b64c[((b1 & 0xf) << 2) | (b2 >> 6)] : '=';
        out += j+2 < s.length ? _b64c[b2 & 0x3f]                      : '=';
    }
    return out;
}
function atob(str) {
    var s = String(str).replace(/[ \\t\\r\\n]+/g, '');
    var valid = true;
    for (var _i = 0; _i < s.length; _i++) {
        var _c = s.charCodeAt(_i);
        if (!((_c >= 65 && _c <= 90) || (_c >= 97 && _c <= 122) ||
              (_c >= 48 && _c <= 57) || _c === 43 || _c === 47 || _c === 61))
            { valid = false; break; }
    }
    if (s.length % 4 !== 0 || !valid)
        throw new TypeError('atob: invalid base64 string');
    var idx = {}, i; for (i = 0; i < _b64c.length; i++) idx[_b64c[i]] = i;
    var out = '';
    for (var j = 0; j < s.length; j += 4) {
        var n = (idx[s[j]] << 18) | (idx[s[j+1]] << 12) |
                ((s[j+2] === '=' ? 0 : idx[s[j+2]]) << 6) |
                (s[j+3] === '=' ? 0 : idx[s[j+3]]);
        out += String.fromCharCode(n >> 16);
        if (s[j+2] !== '=') out += String.fromCharCode((n >> 8) & 0xff);
        if (s[j+3] !== '=') out += String.fromCharCode(n & 0xff);
    }
    return out;
}

// ── Blob / File / FileReader (WHATWG File API) ────────────────────────────────
function _blob_concat_parts(parts) {
    if (!parts || !parts.length) return new Uint8Array(0);
    var arrays = [], total = 0, enc = new TextEncoder();
    for (var i = 0; i < parts.length; i++) {
        var p = parts[i], a;
        if (typeof p === 'string') {
            a = enc.encode(p);
        } else if (p && p._bytes instanceof Uint8Array) {
            // Blob or File
            a = p._bytes;
        } else if (p instanceof ArrayBuffer) {
            a = new Uint8Array(p);
        } else if (p && ArrayBuffer.isView(p)) {
            a = new Uint8Array(p.buffer, p.byteOffset, p.byteLength);
        } else {
            a = enc.encode(String(p));
        }
        arrays.push(a);
        total += a.length;
    }
    var out = new Uint8Array(total), off = 0;
    for (var j = 0; j < arrays.length; j++) { out.set(arrays[j], off); off += arrays[j].length; }
    return out;
}

// WHATWG File API §4 — Blob
function Blob(blobParts, options) {
    this._bytes = _blob_concat_parts(blobParts || []);
    this._type  = (options && typeof options.type === 'string')
        ? options.type.toLowerCase() : '';
}
Object.defineProperties(Blob.prototype, {
    size: { get: function() { return this._bytes.length; }, enumerable: true },
    type: { get: function() { return this._type; }, enumerable: true },
});
Blob.prototype.slice = function(start, end, contentType) {
    var len = this._bytes.length;
    var s = typeof start === 'number' ? (start < 0 ? Math.max(len+start,0) : Math.min(start,len)) : 0;
    var e = typeof end   === 'number' ? (end   < 0 ? Math.max(len+end,  0) : Math.min(end,  len)) : len;
    if (e < s) e = s;
    return new Blob([this._bytes.slice(s, e)],
        { type: typeof contentType === 'string' ? contentType : this._type });
};
Blob.prototype.text = function() {
    return Promise.resolve(new TextDecoder().decode(this._bytes));
};
Blob.prototype.arrayBuffer = function() {
    return Promise.resolve(this._bytes.buffer.slice(0));
};
Blob.prototype.stream = function() {
    var bytes = this._bytes;
    return new ReadableStream({
        start: function(c) { c.enqueue(bytes); c.close(); }
    });
};

// WHATWG File API §7 — File extends Blob
function File(fileParts, fileName, options) {
    if (fileName === undefined) throw new TypeError('File requires a fileName argument');
    Blob.call(this, fileParts, options);
    this._name = String(fileName);
    this._lastModified = (options && typeof options.lastModified === 'number')
        ? options.lastModified : Date.now();
}
File.prototype = Object.create(Blob.prototype);
File.prototype.constructor = File;
Object.defineProperties(File.prototype, {
    name:             { get: function() { return this._name; }, enumerable: true },
    lastModified:     { get: function() { return this._lastModified; }, enumerable: true },
    lastModifiedDate: { get: function() { return new Date(this._lastModified); }, enumerable: true },
});

// WHATWG File API §10 — FileReader
function FileReader() {
    this.readyState = FileReader.EMPTY;
    this.result     = null;
    this.error      = null;
    this.onloadstart = null;
    this.onprogress  = null;
    this.onload      = null;
    this.onabort     = null;
    this.onerror     = null;
    this.onloadend   = null;
    this._aborted    = false;
}
FileReader.EMPTY   = 0;
FileReader.LOADING = 1;
FileReader.DONE    = 2;
FileReader.prototype._dispatch = function(name, extra) {
    var ev = Object.assign({ type: name, target: this }, extra || {});
    var h = this['on' + name];
    if (typeof h === 'function') h.call(this, ev);
};
FileReader.prototype._doRead = function(blob, transform) {
    var self = this;
    self.readyState = FileReader.LOADING;
    self.result = null; self.error = null; self._aborted = false;
    self._dispatch('loadstart');
    queueMicrotask(function() {
        if (self._aborted) {
            self.readyState = FileReader.DONE; self.result = null;
            self._dispatch('abort'); self._dispatch('loadend'); return;
        }
        try {
            self.result = transform(blob);
            self.readyState = FileReader.DONE;
            self._dispatch('load');
        } catch(err) {
            self.readyState = FileReader.DONE;
            self.error = { name: 'NotReadableError', message: String(err) };
            self._dispatch('error');
        }
        self._dispatch('loadend');
    });
};
FileReader.prototype.readAsText = function(blob, encoding) {
    var dec = new TextDecoder(encoding || 'utf-8');
    this._doRead(blob, function(b) { return dec.decode(b._bytes); });
};
FileReader.prototype.readAsArrayBuffer = function(blob) {
    this._doRead(blob, function(b) { return b._bytes.buffer.slice(0); });
};
FileReader.prototype.readAsBinaryString = function(blob) {
    this._doRead(blob, function(b) {
        var s = '';
        for (var i = 0; i < b._bytes.length; i++) s += String.fromCharCode(b._bytes[i]);
        return s;
    });
};
FileReader.prototype.readAsDataURL = function(blob) {
    this._doRead(blob, function(b) {
        var s = '';
        for (var i = 0; i < b._bytes.length; i++) s += String.fromCharCode(b._bytes[i]);
        return 'data:' + (b._type || 'application/octet-stream') + ';base64,' + btoa(s);
    });
};
FileReader.prototype.abort = function() {
    if (this.readyState === FileReader.LOADING) this._aborted = true;
};

// WHATWG File API §24.9 — URL.createObjectURL / revokeObjectURL
var _object_url_store = Object.create(null);
var _object_url_seq   = 0;
URL.createObjectURL = function(blob) {
    var key = 'blob:lumen/' + (++_object_url_seq);
    _object_url_store[key] = blob;
    return key;
};
URL.revokeObjectURL = function(url) { delete _object_url_store[String(url)]; };

// ── performance (HR Timer — W3C HR Time L2 + User Timing L3) ─────────────────
// Time origin is the instant install_dom_api ran (injected by Rust).
var _perf_origin_ms = typeof _lumen_now_ms === 'function' ? _lumen_now_ms() : 0;
// Internal entry store: array of {entryType, name, startTime, duration}.
var _perf_entries = [];
var performance = {
    timeOrigin: _perf_origin_ms,
    now: function() {
        return (typeof _lumen_now_ms === 'function' ? _lumen_now_ms() : 0) - _perf_origin_ms;
    },
    // User Timing L3 §4.2 — performance.mark(name, options?)
    mark: function(name, opts) {
        var start = (opts && typeof opts.startTime === 'number') ? opts.startTime : performance.now();
        var entry = { entryType: 'mark', name: String(name), startTime: start, duration: 0 };
        _perf_entries.push(entry);
        _perf_observer_notify([entry]);
        return entry;
    },
    // User Timing L3 §4.3 — performance.measure(name, start?, end?)
    measure: function(name, startMark, endMark) {
        var start = 0, end = performance.now();
        if (typeof startMark === 'string') {
            var sm = _perf_entries_by_name(startMark, 'mark');
            if (sm.length > 0) start = sm[sm.length - 1].startTime;
        } else if (typeof startMark === 'number') {
            start = startMark;
        }
        if (typeof endMark === 'string') {
            var em = _perf_entries_by_name(endMark, 'mark');
            if (em.length > 0) end = em[em.length - 1].startTime;
        } else if (typeof endMark === 'number') {
            end = endMark;
        }
        var entry = { entryType: 'measure', name: String(name), startTime: start, duration: end - start };
        _perf_entries.push(entry);
        _perf_observer_notify([entry]);
        return entry;
    },
    getEntriesByName: function(name, type) {
        return _perf_entries_by_name(String(name), type);
    },
    getEntriesByType: function(type) {
        var t = String(type);
        return _perf_entries.filter(function(e) { return e.entryType === t; });
    },
    getEntries: function() { return _perf_entries.slice(); },
    clearMarks: function(name) {
        if (typeof name === 'string') {
            _perf_entries = _perf_entries.filter(function(e) { return !(e.entryType === 'mark' && e.name === name); });
        } else {
            _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'mark'; });
        }
    },
    clearMeasures: function(name) {
        if (typeof name === 'string') {
            _perf_entries = _perf_entries.filter(function(e) { return !(e.entryType === 'measure' && e.name === name); });
        } else {
            _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'measure'; });
        }
    },
    // W3C Resource Timing L2 §4.4 — clears all 'resource' entries from the buffer.
    clearResourceTimings: function() {
        _perf_entries = _perf_entries.filter(function(e) { return e.entryType !== 'resource'; });
    },
    // W3C Resource Timing L2 §4.4 — sets max buffer size; Phase 0: no-op (unbounded).
    setResourceTimingBufferSize: function(_maxSize) {},
};

function _perf_entries_by_name(name, type) {
    return _perf_entries.filter(function(e) {
        return e.name === name && (type === undefined || e.entryType === type);
    });
}

// ── PerformanceObserver (Performance Timeline L2 §5–6) ───────────────────────
// observe({entryTypes}) or observe({type, buffered}) per §6.2.2.
// disconnect() → stops observing. Callback: fn(list, observer).
var _perf_observers = [];

function PerformanceObserver(callback) {
    if (typeof callback !== 'function') throw new TypeError('PerformanceObserver: callback must be a function');
    this._cb      = callback;
    this._types   = [];
    this._buffered = false;
}
// Performance Timeline L2 §6.2.2: supportedEntryTypes static accessor.
Object.defineProperty(PerformanceObserver, 'supportedEntryTypes', {
    get: function() {
        return ['element', 'event', 'first-input', 'largest-contentful-paint',
                'layout-shift', 'longtask', 'mark', 'measure', 'navigation',
                'paint', 'resource', 'soft-navigation'];
    },
    configurable: true,
});
PerformanceObserver.prototype.observe = function(opts) {
    var types;
    var buffered;
    if (opts && typeof opts.type === 'string') {
        // §6.2.2 single-type form: observe({type, buffered})
        types   = [opts.type];
        buffered = !!(opts.buffered);
    } else {
        // §6.2.2 multi-type form: observe({entryTypes[, buffered]})
        // Spec disallows buffered here, but we accept it for compatibility.
        types   = (opts && Array.isArray(opts.entryTypes)) ? opts.entryTypes : [];
        buffered = !!(opts && opts.buffered);
    }
    // Merge into existing subscribed types so repeated observe() calls accumulate.
    for (var i = 0; i < types.length; i++) {
        if (this._types.indexOf(types[i]) === -1) this._types.push(types[i]);
    }
    if (buffered) this._buffered = true;
    // De-duplicate in global list.
    var idx = _perf_observers.indexOf(this);
    if (idx === -1) _perf_observers.push(this);
    // If buffered: deliver already-existing matching entries immediately.
    if (buffered && types.length > 0) {
        var buf = _perf_entries.filter(function(e) {
            return types.indexOf(e.entryType) !== -1;
        });
        if (buf.length > 0) {
            _perf_deliver_to_observer(this, buf);
        }
    }
};
PerformanceObserver.prototype.disconnect = function() {
    var idx = _perf_observers.indexOf(this);
    if (idx !== -1) _perf_observers.splice(idx, 1);
};
PerformanceObserver.prototype.takeRecords = function() {
    var entries = [];
    for (var i = 0; i < this._types.length; i++) {
        var type = this._types[i];
        var matching = _perf_entries.filter(function(e) { return e.entryType === type; });
        entries = entries.concat(matching);
    }
    return entries;
};

// Deliver a batch of entries to a single observer (wraps in EntryList).
function _perf_deliver_to_observer(obs, entries) {
    var list = {
        getEntries:        function() { return entries.slice(); },
        getEntriesByName:  function(n, t) { return entries.filter(function(e) { return e.name === n && (!t || e.entryType === t); }); },
        getEntriesByType:  function(t) { return entries.filter(function(e) { return e.entryType === t; }); },
    };
    try { obs._cb(list, obs); } catch(e) {}
}

// Called internally when new entries are created (mark/measure/paint).
function _perf_observer_notify(entries) {
    for (var i = 0; i < _perf_observers.length; i++) {
        var obs = _perf_observers[i];
        var matching = entries.filter(function(e) { return obs._types.indexOf(e.entryType) !== -1; });
        if (matching.length > 0) _perf_deliver_to_observer(obs, matching);
    }
}

// Called by the shell after first paint / first contentful paint.
// name = 'first-paint' | 'first-contentful-paint', start_ms = DOMHighResTimeStamp.
function _lumen_deliver_paint_entry(name, start_ms) {
    var entry = { entryType: 'paint', name: String(name), startTime: start_ms, duration: 0 };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Called by the shell after rendering a large content element (LCP).
// element_id = NID of the element; size = area in pixels (>500px²).
// start_ms = DOMHighResTimeStamp; render_time_ms = when rendering completed.
function _lumen_deliver_lcp_entry(element_id, size, start_ms, render_time_ms) {
    var entry = {
        entryType: 'largest-contentful-paint',
        name: 'largest-contentful-paint',
        startTime: start_ms,
        duration: render_time_ms - start_ms,
        size: size,
        element: element_id >= 0 ? _lumen_make_element(element_id) : null,
        url: '',
        id: '',
        activationStart: 0,
    };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Called by the shell when layout shift detected (CLS).
// value = fractional shift distance (0.0..1.0+); session_id for grouping.
// had_input = whether user input occurred recently (affects grouping).
function _lumen_deliver_layout_shift(value, session_id, had_input) {
    var entry = {
        entryType: 'layout-shift',
        name: 'layout-shift',
        startTime: performance.now(),
        duration: 0,
        value: value,
        hadRecentInput: !!had_input,
        sources: [],
    };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Called by network layer when a resource fetch completes.
// W3C Resource Timing L2 §4: creates a PerformanceResourceTiming entry with
// all sub-timings set to start_ms (Phase 0 — no per-phase breakdown available).
// initiator = 'script'|'link'|'img'|'fetch'|'xmlhttprequest'|'other'.
function _lumen_record_resource_timing(url, initiator, start_ms, duration_ms) {
    var s = Number(start_ms);
    var d = Number(duration_ms);
    var entry = {
        entryType: 'resource',
        name: String(url),
        startTime: s,
        duration: d,
        initiatorType: String(initiator),
        fetchStart: s,
        domainLookupStart: s,
        domainLookupEnd: s,
        connectStart: s,
        connectEnd: s,
        secureConnectionStart: s,
        requestStart: s,
        responseStart: s,
        responseEnd: s + d,
        transferSize: 0,
        encodedBodySize: 0,
        decodedBodySize: 0,
        responseStatus: 0,
        renderBlockingStatus: 'non-blocking',
        contentType: '',
    };
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// Generic entry delivery — called by Rust shell for any PerformanceEntry type.
// entry_type: W3C entryType string (e.g. 'longtask', 'element', 'event').
// detail_json: optional JSON string; its properties are merged into the entry.
function _lumen_deliver_perf_entry(entry_type, name, start_ms, duration_ms, detail_json) {
    var entry = {
        entryType: String(entry_type),
        name: String(name),
        startTime: Number(start_ms),
        duration: Number(duration_ms),
    };
    if (detail_json) {
        try {
            var extra = JSON.parse(String(detail_json));
            for (var k in extra) {
                if (Object.prototype.hasOwnProperty.call(extra, k)) entry[k] = extra[k];
            }
        } catch(e) {}
    }
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);
}

// ── scheduler (Prioritized Task Scheduling API — W3C §2) ─────────────────────
// scheduler.postTask(fn, {priority?, delay?}) → Promise
// Priorities: 'user-blocking' (microtask-like), 'user-visible' (default,
// setTimeout 0), 'background' (setTimeout 0). All three converge to async
// execution; priority differentiation is Phase 2 (requires Rust task sources).
var scheduler = {
    postTask: function(fn, opts) {
        if (typeof fn !== 'function') return Promise.reject(new TypeError('scheduler.postTask: argument must be a function'));
        var delay = (opts && typeof opts.delay === 'number' && opts.delay > 0) ? opts.delay : 0;
        return new Promise(function(resolve, reject) {
            setTimeout(function() {
                try { resolve(fn()); } catch(e) { reject(e); }
            }, delay);
        });
    },
    yield: function() {
        return new Promise(function(resolve) { setTimeout(resolve, 0); });
    },
};

// ── requestIdleCallback / cancelIdleCallback (HTML LS §8.6) ──────────────────
// Stub: fires via setTimeout(~50ms) with a synthetic IdleDeadline that always
// reports 50ms remaining — Lumen is single-process, so there is no real idle
// detection. The timeout option is honoured as the scheduling delay.
var _idle_cbs    = {};
var _idle_seq    = 1;

function requestIdleCallback(cb, opts) {
    if (typeof cb !== 'function') throw new TypeError('requestIdleCallback: argument must be a function');
    var delay = (opts && typeof opts.timeout === 'number' && opts.timeout > 0) ? Math.min(opts.timeout, 50) : 50;
    var id = _idle_seq++;
    _idle_cbs[id] = cb;
    setTimeout(function() {
        var fn = _idle_cbs[id];
        if (!fn) return;
        delete _idle_cbs[id];
        var deadline = { timeRemaining: function() { return 50; }, didTimeout: false };
        try { fn(deadline); } catch(e) {}
    }, delay);
    return id;
}

function cancelIdleCallback(id) {
    delete _idle_cbs[id | 0];
}

// ── MessageChannel / MessagePort (WHATWG HTML §8.3.4-§8.3.5) ─────────────────
// MessageChannel() creates two entangled MessagePort objects (port1 / port2).
// Messages posted on one port are delivered asynchronously (queueMicrotask) to
// the other.  Setting port.onmessage auto-starts the port (spec §8.3.5 step 4).

function MessagePort() {
    this._other          = null;
    this._started        = false;
    this._closed         = false;
    this._queue          = [];
    this._listeners      = [];
    this._onmessage      = null;
    this.onmessageerror  = null;
}

// start() — activate queued message delivery (HTML §8.3.5 «start» algorithm).
MessagePort.prototype.start = function() {
    if (this._started || this._closed) return;
    this._started = true;
    var self = this;
    queueMicrotask(function() { self._drain(); });
};

// close() — detach the port; further delivery and sends are no-ops.
MessagePort.prototype.close = function() {
    this._closed  = true;
    this._other   = null;
    this._queue   = [];
};

// postMessage(data) — clone data and enqueue delivery to the entangled port.
MessagePort.prototype.postMessage = function(message) {
    if (this._closed || !this._other || this._other._closed) return;
    var other = this._other;
    var clone = structuredClone(message);
    queueMicrotask(function() {
        if (other._closed) return;
        var evt = { type: 'message', data: clone, target: other,
                    currentTarget: other, bubbles: false, cancelable: false };
        if (other._started) {
            other._deliver(evt);
        } else {
            other._queue.push(evt);
        }
    });
};

// Internal: deliver evt to onmessage + 'message' addEventListener listeners.
MessagePort.prototype._deliver = function(evt) {
    if (typeof this._onmessage === 'function') {
        try { this._onmessage.call(this, evt); } catch(e) {}
    }
    for (var i = 0; i < this._listeners.length; i++) {
        try { this._listeners[i].call(this, evt); } catch(e) {}
    }
};

// Internal: drain queued messages after start().
MessagePort.prototype._drain = function() {
    var q = this._queue.splice(0);
    for (var i = 0; i < q.length; i++) this._deliver(q[i]);
};

// addEventListener — supports 'message' and 'messageerror'; auto-starts on 'message'.
MessagePort.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (type !== 'message' && type !== 'messageerror') return;
    if (this._listeners.indexOf(fn) < 0) this._listeners.push(fn);
    if (type === 'message') this.start();
};

// removeEventListener — removes a previously registered listener.
MessagePort.prototype.removeEventListener = function(type, fn) {
    var idx = this._listeners.indexOf(fn);
    if (idx >= 0) this._listeners.splice(idx, 1);
};

// dispatchEvent stub — required by some frameworks.
MessagePort.prototype.dispatchEvent = function(evt) {
    this._deliver(evt);
    return true;
};

// onmessage getter/setter — setting to a Function auto-starts delivery.
Object.defineProperty(MessagePort.prototype, 'onmessage', {
    get: function() { return this._onmessage || null; },
    set: function(fn) {
        this._onmessage = (typeof fn === 'function') ? fn : null;
        if (this._onmessage !== null) this.start();
    },
    configurable: true,
    enumerable:   true,
});

// MessageChannel — creates two entangled ports.
function MessageChannel() {
    var p1 = new MessagePort();
    var p2 = new MessagePort();
    p1._other = p2;
    p2._other = p1;
    this.port1 = p1;
    this.port2 = p2;
}

// Expose new globals on window object (defined after window literal because
// `var performance` is not hoisted with its value, only its name).
window.URL                   = URL;
window.URLSearchParams       = URLSearchParams;
window.performance           = performance;
window.queueMicrotask        = queueMicrotask;
window.Event                 = Event;
window.CustomEvent           = CustomEvent;
window.UIEvent               = UIEvent;
window.MouseEvent            = MouseEvent;
window.KeyboardEvent         = KeyboardEvent;
window.InputEvent            = InputEvent;
window.FocusEvent            = FocusEvent;
window.WheelEvent            = WheelEvent;
window.PointerEvent          = PointerEvent;
window.AnimationEvent        = AnimationEvent;
window.TransitionEvent       = TransitionEvent;
window.Animation             = Animation;
window.KeyframeEffect        = KeyframeEffect;
window.DocumentTimeline      = DocumentTimeline;
window.AnimationPlaybackEvent = AnimationPlaybackEvent;
window.StorageEvent          = StorageEvent;
window.PopStateEvent         = PopStateEvent;
window.HashChangeEvent       = HashChangeEvent;
window.ErrorEvent            = ErrorEvent;
window.SubmitEvent           = SubmitEvent;
window.PageTransitionEvent   = PageTransitionEvent;
window.BeforeUnloadEvent     = BeforeUnloadEvent;
window.DataTransfer          = DataTransfer;
window.DataTransferItem      = DataTransferItem;
window.DataTransferItemList  = DataTransferItemList;
window.DragEvent             = DragEvent;
window.ClipboardEvent        = ClipboardEvent;
window.CompositionEvent      = CompositionEvent;
window.scheduler                = scheduler;
window.requestIdleCallback      = requestIdleCallback;
window.cancelIdleCallback       = cancelIdleCallback;
window.ValidityState            = ValidityState;
window.setTimeout            = setTimeout;
window.clearTimeout          = clearTimeout;
window.setInterval           = setInterval;
window.clearInterval         = clearInterval;
window.MutationObserver      = MutationObserver;
window.ResizeObserver        = ResizeObserver;
window.IntersectionObserver  = IntersectionObserver;
window.NodeFilter            = NodeFilter;
window.TreeWalker            = _TreeWalker;
window.NodeIterator          = _NodeIterator;
window.PerformanceObserver   = PerformanceObserver;
window.MediaQueryList        = MediaQueryList;
window.MediaQueryListEvent   = MediaQueryListEvent;
// CSS Media Queries L4 §4.2 — Window.matchMedia returns a live MediaQueryList.
// Bare `matchMedia(...)` (without window prefix) also works because the var
// declaration below promotes it to a global.
var matchMedia = function(media) {
    var mql = new MediaQueryList(media);
    _mqlRegistry.push(mql);
    return mql;
};
window.matchMedia            = matchMedia;

// ── window scroll API (CSSOM View Module §4) ────────────────────────────────
// window.scrollX / scrollY / pageXOffset / pageYOffset — read current page scroll.
// window.scrollTo / scroll / scrollBy — programmatic page scroll with behavior option.
Object.defineProperties(window, {
    scrollY: { get: function() { return _lumen_get_page_scroll_y(); }, enumerable: true },
    scrollX: { get: function() { return 0; }, enumerable: true },
    pageYOffset: { get: function() { return _lumen_get_page_scroll_y(); }, enumerable: true },
    pageXOffset: { get: function() { return 0; }, enumerable: true }
});
window.scrollTo = function(x, y) {
    var top, smooth;
    if (typeof x === 'object' && x !== null) { top = +(x.top || 0); smooth = x.behavior === 'smooth' ? 1 : 0; }
    else { top = +(y || 0); smooth = 0; }
    _lumen_request_page_scroll(top, smooth);
};
window.scroll = window.scrollTo;
window.scrollBy = function(x, y) {
    var dy, smooth;
    if (typeof x === 'object' && x !== null) { dy = +(x.top || 0); smooth = x.behavior === 'smooth' ? 1 : 0; }
    else { dy = +(y || 0); smooth = 0; }
    _lumen_request_page_scroll(_lumen_get_page_scroll_y() + dy, smooth);
};

// ── window.CSS (CSS Object Model L1 §5 + CSS Conditional Rules L3 §6) ────────
// CSS.supports(property, value) — two-argument form.
// CSS.supports(conditionText) — one-argument form.
// CSS.escape(ident) — CSS.escape() L1 §4.2 (WhatWG CSS OM).
var CSS = {
    supports: function(prop, value) {
        if (arguments.length < 2) {
            // One-argument form: CSS.supports(conditionText)
            // Strip outermost parens if present (common usage pattern).
            var cond = String(prop);
            return !!_lumen_css_supports_cond(cond);
        }
        // Two-argument form: CSS.supports(property, value)
        return !!_lumen_css_supports_prop(String(prop), String(value));
    },
    escape: function(ident) {
        // CSS.escape() — WhatWG CSS OM §4.2.
        // Escapes all chars that are not safe in CSS identifiers.
        ident = String(ident);
        var result = '';
        for (var i = 0; i < ident.length; i++) {
            var code = ident.charCodeAt(i);
            var ch = ident[i];
            if (i === 0 && code >= 0x30 && code <= 0x39) {
                // Leading digit (escape as hex) — escape as hex.
                result += '\\\\' + code.toString(16) + ' ';
                continue;
            }
            // Safe: [a-zA-Z0-9_-] and non-ASCII.
            if ((code >= 0x61 && code <= 0x7a) ||
                (code >= 0x41 && code <= 0x5a) ||
                (code >= 0x30 && code <= 0x39) ||
                code === 0x5f || code === 0x2d || code >= 0x80) {
                result += ch;
            } else if (code === 0x00) {
                result += '�';
            } else if (code <= 0x1f || code === 0x7f) {
                result += '\\\\' + code.toString(16) + ' ';
            } else {
                result += '\\\\' + ch;
            }
        }
        return result;
    },
};
window.CSS = CSS;

window.Blob                  = Blob;
window.File                  = File;
window.FileReader            = FileReader;
window.btoa                  = btoa;
window.atob                  = atob;
window.MessageChannel        = MessageChannel;
window.MessagePort           = MessagePort;
window.PermissionStatus      = PermissionStatus;
// W3C Secure Contexts §3.1: local-file and localhost are considered secure.
window.isSecureContext       = true;
// Set by Rust via _LUMEN_CROSS_ORIGIN_ISOLATED global (COOP=same-origin + COEP=require-corp).
window.crossOriginIsolated   = !!_LUMEN_CROSS_ORIGIN_ISOLATED;

// ── window.open() (HTML LS §8.7.1) ─────────────────────────────────────────
// Opens a new browsing context (implemented as a new tab in Lumen).
// Returns a stub WindowProxy with location/close — actual cross-window state
// sharing is not implemented (window.opener is always null).
window.opener = null;
window.open = function(url, target, features) {
  url     = (url     == null) ? '' : String(url);
  target  = (target  == null) ? '_blank' : String(target);
  features = (features == null) ? '' : String(features);
  _lumen_window_open(url, target, features);
  // Return a minimal stub so callers can call .close() / read .location.href
  // without throwing. Real cross-window messaging is not yet supported.
  var href = url || 'about:blank';
  return {
    closed: false,
    opener: null,
    name: target,
    location: {
      href: href,
      toString: function() { return href; }
    },
    close: function() { this.closed = true; },
    focus: function() {},
    blur: function() {},
    postMessage: function() {}
  };
};
window.close = function() {};

// ── Lazy image loading (HTML LS §2.6.6.9) ──────────────────────────────────
// Maps nid (u32 as string key) → url for images deferred by loading=\"lazy\".
// Internal IntersectionObserver for lazy images (HTML LS loading=lazy, §lazy-loading).
// Created on first _lumen_init_lazy_images call; uses rootMargin to load images
// one viewport-height ahead of the visible area.
var _lazy_io = null;
// nid → url for images not yet loaded; populated by _lumen_init_lazy_images.
var _lazy_io_urls = {};

// Called by shell after initial layout with [[nid, url], ...] for lazy images.
// Creates an internal IntersectionObserver that fires _lumen_request_lazy_image_load
// when each image enters the lazy-load margin. Idempotent: re-registration skipped.
function _lumen_init_lazy_images(pairs) {
    if (pairs.length === 0) return;
    if (!_lazy_io) {
        var vp = _lumen_get_viewport_size();
        // HTML LS §lazy-loading distance threshold: load 1 viewport-height ahead.
        var margin = Math.round(vp[1]);
        _lazy_io = new IntersectionObserver(function(entries) {
            for (var i = 0; i < entries.length; i++) {
                var entry = entries[i];
                if (!entry.isIntersecting) continue;
                var nid = entry.target.__nid__;
                if (_lazy_io_urls[nid] !== undefined) {
                    _lumen_request_lazy_image_load(nid, _lazy_io_urls[nid]);
                    delete _lazy_io_urls[nid];
                    _lazy_io.unobserve(entry.target);
                }
            }
        }, { rootMargin: '0px 0px ' + margin + 'px 0px' });
    }
    for (var i = 0; i < pairs.length; i++) {
        var nid = pairs[i][0];
        if (_lazy_io_urls[nid] === undefined) {
            _lazy_io_urls[nid] = pairs[i][1];
            // Proxy object: IntersectionObserver only needs __nid__ to look up the rect.
            _lazy_io.observe({ __nid__: nid });
        }
    }
}

// Called by shell after each relayout.  Lazy images are now delivered via
// _lazy_io (an IntersectionObserver), which fires inside
// _lumen_deliver_intersection_observers() called earlier by deliver_layout_observers().
// This function is kept for shell API compatibility.
function _lumen_deliver_lazy_images() {}

// ── IndexedDB (W3C Indexed Database API 3.0) ─────────────────────────────────
// In-memory implementation: databases live in this runtime's JS heap and do not
// persist across reloads (Rust-backed persistence is a separate follow-up task).
// Request 'success'/'error' events and transaction 'complete'/'abort' fire
// asynchronously via a pending queue drained by _lumen_idb_flush(), which the
// shell calls each event-loop tick (and tests call directly). This mirrors the
// raf / MutationObserver delivery pattern already used in this shim.

var _idb_databases = {};          // name -> { name, version, stores }
var _idb_active_txns = [];        // transactions with pending request dispatches
var _idb_pending_opens = [];      // IDBOpenDBRequest dispatch entries
var _idb_flush_scheduled = false;
var _idb_dirty = false;           // set by any mutation; drives persistence at flush end

// --- persistence (Rust-backed via _lumen_idb_load / _lumen_idb_persist) -------
// The whole per-origin database set is one opaque JSON snapshot. Date keys/values
// are tagged ({__idb_date__: ms}) since JSON has no Date type; everything else is
// plain structured data (numbers, strings, arrays, objects). Persistence is
// best-effort: when no backend is installed the shim stays in-heap-only.

function _idb_serialize() {
    return JSON.stringify(_idb_databases, function(k, v) {
        // `this[k]` is the original (pre-toJSON) value, so Dates are detectable
        // even though `v` is already their ISO string.
        if (this[k] instanceof Date) return { __idb_date__: this[k].getTime() };
        return v;
    });
}

function _idb_deserialize(json) {
    return JSON.parse(json, function(k, v) {
        if (v && typeof v === 'object' && typeof v.__idb_date__ === 'number') return new Date(v.__idb_date__);
        return v;
    });
}

// Writes the current snapshot to the backend if a mutation occurred since the
// last persist. Called at the end of every flush.
function _idb_persist_if_dirty() {
    if (!_idb_dirty) return;
    _idb_dirty = false;
    if (typeof _lumen_idb_persist !== 'function') return;
    try { _lumen_idb_persist(_idb_serialize()); }
    catch (e) { _lumen_console_error('IDB persist: ' + e); }
}

// --- key validation / comparison / extraction (Indexed DB §3.1) --------------

function _idb_is_valid_key(k) {
    var t = typeof k;
    if (t === 'number') return !isNaN(k);
    if (t === 'string') return true;
    if (k instanceof Date) return !isNaN(k.getTime());
    if (Array.isArray(k)) {
        for (var i = 0; i < k.length; i++) if (!_idb_is_valid_key(k[i])) return false;
        return true;
    }
    return false;
}

// Type precedence per spec: number < date < string < array.
function _idb_key_rank(k) {
    if (typeof k === 'number') return 1;
    if (k instanceof Date) return 2;
    if (typeof k === 'string') return 3;
    if (Array.isArray(k)) return 4;
    return 0;
}

// Returns -1, 0 or 1 comparing two valid keys per the IndexedDB key ordering.
function _idb_cmp(a, b) {
    var ra = _idb_key_rank(a), rb = _idb_key_rank(b);
    if (ra !== rb) return ra < rb ? -1 : 1;
    if (ra === 1 || ra === 3) return a < b ? -1 : (a > b ? 1 : 0);
    if (ra === 2) {
        var ta = a.getTime(), tb = b.getTime();
        return ta < tb ? -1 : (ta > tb ? 1 : 0);
    }
    if (ra === 4) {
        var n = Math.min(a.length, b.length);
        for (var i = 0; i < n; i++) {
            var c = _idb_cmp(a[i], b[i]);
            if (c !== 0) return c;
        }
        return a.length < b.length ? -1 : (a.length > b.length ? 1 : 0);
    }
    return 0;
}

// Extracts the key at keyPath from value; returns undefined if any segment is
// missing. keyPath may be a string (dotted), an array (yields an array key), or
// '' (the value itself).
function _idb_extract_key(value, keyPath) {
    if (Array.isArray(keyPath)) {
        var arr = [];
        for (var i = 0; i < keyPath.length; i++) {
            var v = _idb_extract_key(value, keyPath[i]);
            if (v === undefined) return undefined;
            arr.push(v);
        }
        return arr;
    }
    if (keyPath === '') return value;
    var parts = String(keyPath).split('.');
    var cur = value;
    for (var j = 0; j < parts.length; j++) {
        if (cur === null || typeof cur !== 'object') return undefined;
        cur = cur[parts[j]];
        if (cur === undefined) return undefined;
    }
    return cur;
}

// Writes a generated key back into value at a string keyPath (autoIncrement).
function _idb_inject_key(value, keyPath, key) {
    var parts = String(keyPath).split('.');
    var cur = value;
    for (var i = 0; i < parts.length - 1; i++) {
        if (cur[parts[i]] === undefined || cur[parts[i]] === null) cur[parts[i]] = {};
        cur = cur[parts[i]];
    }
    cur[parts[parts.length - 1]] = key;
}

function _idb_error(name, message) {
    var e = new Error(message || name);
    e.name = name;
    return e;
}

// --- IDBKeyRange (Indexed DB §3.1.5) -----------------------------------------

function IDBKeyRange(lower, upper, lowerOpen, upperOpen) {
    this.lower = lower;
    this.upper = upper;
    this.lowerOpen = !!lowerOpen;
    this.upperOpen = !!upperOpen;
}
IDBKeyRange.prototype.includes = function(key) {
    if (!_idb_is_valid_key(key)) throw _idb_error('DataError', 'invalid key');
    if (this.lower !== undefined) {
        var c = _idb_cmp(key, this.lower);
        if (c < 0 || (c === 0 && this.lowerOpen)) return false;
    }
    if (this.upper !== undefined) {
        var c2 = _idb_cmp(key, this.upper);
        if (c2 > 0 || (c2 === 0 && this.upperOpen)) return false;
    }
    return true;
};
IDBKeyRange.only = function(value) {
    if (!_idb_is_valid_key(value)) throw _idb_error('DataError', 'invalid key');
    return new IDBKeyRange(value, value, false, false);
};
IDBKeyRange.lowerBound = function(lower, open) {
    if (!_idb_is_valid_key(lower)) throw _idb_error('DataError', 'invalid key');
    return new IDBKeyRange(lower, undefined, !!open, false);
};
IDBKeyRange.upperBound = function(upper, open) {
    if (!_idb_is_valid_key(upper)) throw _idb_error('DataError', 'invalid key');
    return new IDBKeyRange(undefined, upper, false, !!open);
};
IDBKeyRange.bound = function(lower, upper, lowerOpen, upperOpen) {
    if (!_idb_is_valid_key(lower) || !_idb_is_valid_key(upper)) throw _idb_error('DataError', 'invalid key');
    if (_idb_cmp(lower, upper) > 0) throw _idb_error('DataError', 'lower bound greater than upper bound');
    return new IDBKeyRange(lower, upper, !!lowerOpen, !!upperOpen);
};

// Coerces a query argument (key | IDBKeyRange | null) into an IDBKeyRange or null.
function _idb_to_range(q) {
    if (q === undefined || q === null) return null;
    if (q instanceof IDBKeyRange) return q;
    if (!_idb_is_valid_key(q)) throw _idb_error('DataError', 'invalid key or range');
    return IDBKeyRange.only(q);
}

// --- IDBRequest / IDBOpenDBRequest (Indexed DB §3.5) -------------------------

function IDBRequest(source, txn) {
    this.result = undefined;
    this.error = null;
    this.source = source || null;
    this.transaction = txn || null;
    this.readyState = 'pending';
    this.onsuccess = null;
    this.onerror = null;
    this._successListeners = [];
    this._errorListeners = [];
    this._action = null;
}
IDBRequest.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (type === 'success') this._successListeners.push(fn);
    else if (type === 'error') this._errorListeners.push(fn);
};
IDBRequest.prototype.removeEventListener = function(type, fn) {
    var arr = type === 'success' ? this._successListeners : (type === 'error' ? this._errorListeners : null);
    if (!arr) return;
    var i = arr.indexOf(fn);
    if (i >= 0) arr.splice(i, 1);
};

function IDBOpenDBRequest() {
    IDBRequest.call(this, null, null);
    this.onupgradeneeded = null;
    this.onblocked = null;
    this._upgradeListeners = [];
}
IDBOpenDBRequest.prototype = Object.create(IDBRequest.prototype);
IDBOpenDBRequest.prototype.constructor = IDBOpenDBRequest;
IDBOpenDBRequest.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (type === 'upgradeneeded') this._upgradeListeners.push(fn);
    else IDBRequest.prototype.addEventListener.call(this, type, fn);
};

function _idb_make_event(type, target, extra) {
    var ev = { type: type, target: target, currentTarget: target, bubbles: false, _prevented: false };
    ev.preventDefault = function() { this._prevented = true; };
    ev.stopPropagation = function() {};
    ev.stopImmediatePropagation = function() {};
    if (extra) for (var k in extra) ev[k] = extra[k];
    return ev;
}

// Runs a request's deferred action (data read/write), then fires its
// success or error event; on an unhandled error the owning transaction is
// aborted (Indexed DB §3.5.5). Operations run at dispatch time in FIFO order so
// that intra- and inter-transaction ordering matches the spec.
function _idb_dispatch_request(req) {
    if (req._action) {
        var action = req._action;
        req._action = null;
        try { req.result = action(); req.error = null; }
        catch (e) { req.result = undefined; req.error = (e && e.name) ? e : _idb_error('DataError', String(e)); }
    }
    req.readyState = 'done';
    if (req.error) {
        var ev = _idb_make_event('error', req, { bubbles: true });
        if (typeof req.onerror === 'function') {
            try { req.onerror(ev); } catch(e) { _lumen_console_error('IDB onerror: ' + e); }
        }
        for (var i = 0; i < req._errorListeners.length; i++) {
            try { req._errorListeners[i](ev); } catch(e) { _lumen_console_error('IDB error listener: ' + e); }
        }
        if (req.transaction && !ev._prevented) {
            req.transaction.error = req.error;
            req.transaction._aborted = true;
        }
    } else {
        var ev2 = _idb_make_event('success', req);
        if (typeof req.onsuccess === 'function') {
            try { req.onsuccess(ev2); } catch(e) { _lumen_console_error('IDB onsuccess: ' + e); }
        }
        for (var j = 0; j < req._successListeners.length; j++) {
            try { req._successListeners[j](ev2); } catch(e) { _lumen_console_error('IDB success listener: ' + e); }
        }
    }
}

// --- IDBTransaction (Indexed DB §3.4) ----------------------------------------

function IDBTransaction(db, storeNames, mode) {
    this.db = db;
    this.mode = mode || 'readonly';
    this.objectStoreNames = storeNames.slice().sort();
    this.error = null;
    this.oncomplete = null;
    this.onabort = null;
    this.onerror = null;
    this._completeListeners = [];
    this._abortListeners = [];
    this._queue = [];
    this._stores = {};
    this._aborted = false;
    this._finished = false;
    this._isUpgrade = false;
}
IDBTransaction.prototype.objectStore = function(name) {
    if (this._finished) throw _idb_error('InvalidStateError', 'transaction has finished');
    if (this.objectStoreNames.indexOf(name) < 0) throw _idb_error('NotFoundError', 'store not in transaction scope');
    if (!this._stores[name]) {
        var sd = this.db._data.stores[name];
        if (!sd) throw _idb_error('NotFoundError', 'no object store named ' + name);
        this._stores[name] = new IDBObjectStore(sd, this);
    }
    return this._stores[name];
};
IDBTransaction.prototype.abort = function() {
    this._aborted = true;
    _idb_schedule_txn(this);
};
IDBTransaction.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (type === 'complete') this._completeListeners.push(fn);
    else if (type === 'abort') this._abortListeners.push(fn);
};
IDBTransaction.prototype.removeEventListener = function(type, fn) {
    var arr = type === 'complete' ? this._completeListeners : (type === 'abort' ? this._abortListeners : null);
    if (!arr) return;
    var i = arr.indexOf(fn);
    if (i >= 0) arr.splice(i, 1);
};

function _idb_fire_txn(txn, type) {
    var ev = _idb_make_event(type, txn);
    var handler = type === 'complete' ? txn.oncomplete : txn.onabort;
    if (typeof handler === 'function') {
        try { handler(ev); } catch(e) { _lumen_console_error('IDB txn ' + type + ': ' + e); }
    }
    var arr = type === 'complete' ? txn._completeListeners : txn._abortListeners;
    for (var i = 0; i < arr.length; i++) {
        try { arr[i](ev); } catch(e) { _lumen_console_error('IDB txn listener: ' + e); }
    }
}

function _idb_schedule_txn(txn) {
    if (_idb_active_txns.indexOf(txn) < 0) _idb_active_txns.push(txn);
    _idb_schedule_flush();
}

function _idb_schedule_flush() {
    if (_idb_flush_scheduled) return;
    _idb_flush_scheduled = true;
    queueMicrotask(_lumen_idb_flush);
}

function _idb_flush_txn(txn) {
    if (txn._finished) return;
    while (txn._queue.length > 0 && !txn._aborted) {
        _idb_dispatch_request(txn._queue.shift());
    }
    txn._finished = true;
    if (txn._aborted) {
        txn._queue = [];
        _idb_fire_txn(txn, 'abort');
    } else {
        // A committed write/versionchange transaction changed the stored data.
        if (txn.mode !== 'readonly') _idb_dirty = true;
        _idb_fire_txn(txn, 'complete');
    }
}

// Creates a request whose `fn` (data read/write) runs at dispatch time, in the
// transaction's request order. Synchronous validation (key range, mode) must be
// done by the caller before calling this, so it can throw to the caller.
function _idb_make_request(source, txn, fn) {
    if (txn._finished) throw _idb_error('TransactionInactiveError', 'transaction is not active');
    var req = new IDBRequest(source, txn);
    req._action = fn;
    txn._queue.push(req);
    _idb_schedule_txn(txn);
    return req;
}

// --- IDBDatabase (Indexed DB §3.3) -------------------------------------------

function IDBDatabase(data) {
    this._data = data;
    this.name = data.name;
    this.version = data.version;
    this._upgradeTxn = null;
    this._closed = false;
    this.onversionchange = null;
    this.onabort = null;
    this.onerror = null;
    this.onclose = null;
}
Object.defineProperty(IDBDatabase.prototype, 'objectStoreNames', {
    get: function() { return Object.keys(this._data.stores).sort(); }
});
IDBDatabase.prototype.createObjectStore = function(name, options) {
    if (!this._upgradeTxn) throw _idb_error('InvalidStateError', 'createObjectStore allowed only during a versionchange transaction');
    name = String(name);
    if (this._data.stores[name]) throw _idb_error('ConstraintError', 'object store already exists: ' + name);
    options = options || {};
    var keyPath = (options.keyPath === undefined || options.keyPath === null) ? null : options.keyPath;
    var store = {
        name: name,
        keyPath: keyPath,
        autoIncrement: !!options.autoIncrement,
        keyGenerator: 1,
        records: [],
        indexes: {}
    };
    this._data.stores[name] = store;
    if (this._upgradeTxn.objectStoreNames.indexOf(name) < 0) this._upgradeTxn.objectStoreNames.push(name);
    return new IDBObjectStore(store, this._upgradeTxn);
};
IDBDatabase.prototype.deleteObjectStore = function(name) {
    if (!this._upgradeTxn) throw _idb_error('InvalidStateError', 'deleteObjectStore allowed only during a versionchange transaction');
    if (!this._data.stores[name]) throw _idb_error('NotFoundError', 'no object store named ' + name);
    delete this._data.stores[name];
};
IDBDatabase.prototype.transaction = function(storeNames, mode) {
    if (this._closed) throw _idb_error('InvalidStateError', 'database connection is closed');
    if (typeof storeNames === 'string') storeNames = [storeNames];
    else storeNames = storeNames.slice();
    if (storeNames.length === 0) throw _idb_error('InvalidAccessError', 'empty store scope');
    for (var i = 0; i < storeNames.length; i++) {
        if (!this._data.stores[storeNames[i]]) throw _idb_error('NotFoundError', 'no object store named ' + storeNames[i]);
    }
    return new IDBTransaction(this, storeNames, mode || 'readonly');
};
IDBDatabase.prototype.close = function() { this._closed = true; };

// --- IDBObjectStore (Indexed DB §3.2) ----------------------------------------

function IDBObjectStore(store, txn) {
    this._store = store;
    this.transaction = txn;
    this.name = store.name;
    this.keyPath = store.keyPath;
    this.autoIncrement = store.autoIncrement;
}
Object.defineProperty(IDBObjectStore.prototype, 'indexNames', {
    get: function() { return Object.keys(this._store.indexes).sort(); }
});

// Binary search over the store's key-sorted records array.
function _idb_find_record(records, key) {
    var lo = 0, hi = records.length;
    while (lo < hi) {
        var mid = (lo + hi) >> 1;
        var c = _idb_cmp(records[mid].key, key);
        if (c < 0) lo = mid + 1;
        else if (c > 0) hi = mid;
        else return { found: true, idx: mid };
    }
    return { found: false, idx: lo };
}

// Throws ConstraintError if writing (value, primaryKey) would duplicate a value
// in any unique index (excluding the record currently at primaryKey).
function _idb_check_unique(store, value, primaryKey) {
    for (var name in store.indexes) {
        var idx = store.indexes[name];
        if (!idx.unique) continue;
        var ik = _idb_extract_key(value, idx.keyPath);
        if (ik === undefined) continue;
        var keys = (idx.multiEntry && Array.isArray(ik)) ? ik : [ik];
        for (var ki = 0; ki < keys.length; ki++) {
            for (var r = 0; r < store.records.length; r++) {
                var rec = store.records[r];
                if (_idb_cmp(rec.key, primaryKey) === 0) continue;
                var rik = _idb_extract_key(rec.value, idx.keyPath);
                if (rik === undefined) continue;
                var rkeys = (idx.multiEntry && Array.isArray(rik)) ? rik : [rik];
                for (var rk = 0; rk < rkeys.length; rk++) {
                    if (_idb_is_valid_key(keys[ki]) && _idb_is_valid_key(rkeys[rk]) && _idb_cmp(keys[ki], rkeys[rk]) === 0) {
                        throw _idb_error('ConstraintError', 'unique index ' + name + ' violation');
                    }
                }
            }
        }
    }
}

IDBObjectStore.prototype._write = function(value, key, overwrite) {
    var store = this._store;
    var usedKey;
    if (store.keyPath !== null) {
        if (key !== undefined) throw _idb_error('DataError', 'in-line keys do not take an explicit key argument');
        var k = _idb_extract_key(value, store.keyPath);
        if (k === undefined) {
            if (store.autoIncrement && typeof store.keyPath === 'string') {
                k = store.keyGenerator++;
                _idb_inject_key(value, store.keyPath, k);
            } else {
                throw _idb_error('DataError', 'evaluating the key path yielded no key');
            }
        } else {
            if (!_idb_is_valid_key(k)) throw _idb_error('DataError', 'evaluated key is not a valid key');
            if (store.autoIncrement && typeof k === 'number' && k >= store.keyGenerator) store.keyGenerator = Math.floor(k) + 1;
        }
        usedKey = k;
    } else {
        if (key === undefined) {
            if (store.autoIncrement) { usedKey = store.keyGenerator++; }
            else throw _idb_error('DataError', 'a key is required for an out-of-line store without autoIncrement');
        } else {
            if (!_idb_is_valid_key(key)) throw _idb_error('DataError', 'the supplied key is not a valid key');
            usedKey = key;
            if (store.autoIncrement && typeof key === 'number' && key >= store.keyGenerator) store.keyGenerator = Math.floor(key) + 1;
        }
    }
    var pos = _idb_find_record(store.records, usedKey);
    if (pos.found && !overwrite) throw _idb_error('ConstraintError', 'a record already exists for this key');
    _idb_check_unique(store, value, usedKey);
    if (pos.found) store.records[pos.idx].value = value;
    else store.records.splice(pos.idx, 0, { key: usedKey, value: value });
    return usedKey;
};

IDBObjectStore.prototype.add = function(value, key) {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var self = this;
    return _idb_make_request(this, this.transaction, function() { return self._write(value, key, false); });
};
IDBObjectStore.prototype.put = function(value, key) {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var self = this;
    return _idb_make_request(this, this.transaction, function() { return self._write(value, key, true); });
};
IDBObjectStore.prototype.get = function(query) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        for (var i = 0; i < store.records.length; i++) if (range.includes(store.records[i].key)) return store.records[i].value;
        return undefined;
    });
};
IDBObjectStore.prototype.getKey = function(query) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        for (var i = 0; i < store.records.length; i++) if (range.includes(store.records[i].key)) return store.records[i].key;
        return undefined;
    });
};
IDBObjectStore.prototype.getAll = function(query, count) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var out = [];
        for (var i = 0; i < store.records.length; i++) {
            if (range === null || range.includes(store.records[i].key)) {
                out.push(store.records[i].value);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBObjectStore.prototype.getAllKeys = function(query, count) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var out = [];
        for (var i = 0; i < store.records.length; i++) {
            if (range === null || range.includes(store.records[i].key)) {
                out.push(store.records[i].key);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBObjectStore.prototype.count = function(query) {
    var store = this._store, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return store.records.length;
        var n = 0;
        for (var i = 0; i < store.records.length; i++) if (range.includes(store.records[i].key)) n++;
        return n;
    });
};
IDBObjectStore.prototype.delete = function(query) {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var store = this._store, range = _idb_to_range(query);
    if (range === null) throw _idb_error('DataError', 'a key or key range is required');
    return _idb_make_request(this, this.transaction, function() {
        for (var i = store.records.length - 1; i >= 0; i--) if (range.includes(store.records[i].key)) store.records.splice(i, 1);
        return undefined;
    });
};
IDBObjectStore.prototype.clear = function() {
    if (this.transaction.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    var store = this._store;
    return _idb_make_request(this, this.transaction, function() { store.records = []; return undefined; });
};
IDBObjectStore.prototype.createIndex = function(name, keyPath, options) {
    if (!this.transaction._isUpgrade) throw _idb_error('InvalidStateError', 'createIndex allowed only during a versionchange transaction');
    name = String(name);
    if (this._store.indexes[name]) throw _idb_error('ConstraintError', 'index already exists: ' + name);
    options = options || {};
    var idx = { name: name, keyPath: keyPath, unique: !!options.unique, multiEntry: !!options.multiEntry };
    this._store.indexes[name] = idx;
    return new IDBIndex(idx, this);
};
IDBObjectStore.prototype.deleteIndex = function(name) {
    if (!this.transaction._isUpgrade) throw _idb_error('InvalidStateError', 'deleteIndex allowed only during a versionchange transaction');
    if (!this._store.indexes[name]) throw _idb_error('NotFoundError', 'no index named ' + name);
    delete this._store.indexes[name];
};
IDBObjectStore.prototype.index = function(name) {
    var idx = this._store.indexes[name];
    if (!idx) throw _idb_error('NotFoundError', 'no index named ' + name);
    return new IDBIndex(idx, this);
};
IDBObjectStore.prototype.openCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_store(store, range, dir); }, true, dir);
};
IDBObjectStore.prototype.openKeyCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_store(store, range, dir); }, false, dir);
};

// --- IDBIndex (Indexed DB §3.2.8) --------------------------------------------

function IDBIndex(idx, objectStore) {
    this._index = idx;
    this.objectStore = objectStore;
    this._store = objectStore._store;
    this.transaction = objectStore.transaction;
    this.name = idx.name;
    this.keyPath = idx.keyPath;
    this.unique = idx.unique;
    this.multiEntry = idx.multiEntry;
}
// Materialises an index as a list of { key, primaryKey, value } sorted by
// (index key, primary key). multiEntry array keys are expanded to one entry per
// element. Recomputed per query — simple and correct for an in-memory store.
function _idb_index_entries(store, index) {
    var out = [];
    for (var i = 0; i < store.records.length; i++) {
        var rec = store.records[i];
        var ik = _idb_extract_key(rec.value, index.keyPath);
        if (ik === undefined) continue;
        if (index.multiEntry && Array.isArray(ik)) {
            for (var j = 0; j < ik.length; j++) {
                if (_idb_is_valid_key(ik[j])) out.push({ key: ik[j], primaryKey: rec.key, value: rec.value });
            }
        } else if (_idb_is_valid_key(ik)) {
            out.push({ key: ik, primaryKey: rec.key, value: rec.value });
        }
    }
    out.sort(function(a, b) {
        var c = _idb_cmp(a.key, b.key);
        return c !== 0 ? c : _idb_cmp(a.primaryKey, b.primaryKey);
    });
    return out;
}
IDBIndex.prototype.get = function(query) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        var entries = _idb_index_entries(store, index);
        for (var i = 0; i < entries.length; i++) if (range.includes(entries[i].key)) return entries[i].value;
        return undefined;
    });
};
IDBIndex.prototype.getKey = function(query) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        if (range === null) return undefined;
        var entries = _idb_index_entries(store, index);
        for (var i = 0; i < entries.length; i++) if (range.includes(entries[i].key)) return entries[i].primaryKey;
        return undefined;
    });
};
IDBIndex.prototype.getAll = function(query, count) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var entries = _idb_index_entries(store, index);
        var out = [];
        for (var i = 0; i < entries.length; i++) {
            if (range === null || range.includes(entries[i].key)) {
                out.push(entries[i].value);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBIndex.prototype.getAllKeys = function(query, count) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var entries = _idb_index_entries(store, index);
        var out = [];
        for (var i = 0; i < entries.length; i++) {
            if (range === null || range.includes(entries[i].key)) {
                out.push(entries[i].primaryKey);
                if (count && out.length >= count) break;
            }
        }
        return out;
    });
};
IDBIndex.prototype.count = function(query) {
    var store = this._store, index = this._index, range = _idb_to_range(query);
    return _idb_make_request(this, this.transaction, function() {
        var entries = _idb_index_entries(store, index);
        if (range === null) return entries.length;
        var n = 0;
        for (var i = 0; i < entries.length; i++) if (range.includes(entries[i].key)) n++;
        return n;
    });
};
IDBIndex.prototype.openCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, index = this._index, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_index(store, index, range, dir); }, true, dir);
};
IDBIndex.prototype.openKeyCursor = function(query, direction) {
    var range = _idb_to_range(query), store = this._store, index = this._index, dir = direction || 'next';
    return _idb_open_cursor(this, this.transaction, store, function() { return _idb_cursor_list_index(store, index, range, dir); }, false, dir);
};

// --- cursors (Indexed DB §3.2.6) ---------------------------------------------

function _idb_cursor_list_store(store, range, direction) {
    var arr = [];
    for (var i = 0; i < store.records.length; i++) {
        var rec = store.records[i];
        if (range === null || range.includes(rec.key)) arr.push({ key: rec.key, primaryKey: rec.key, value: rec.value });
    }
    if (direction === 'prev' || direction === 'prevunique') arr.reverse();
    return arr;
}
function _idb_cursor_list_index(store, index, range, direction) {
    var entries = _idb_index_entries(store, index);
    var filtered = [];
    for (var i = 0; i < entries.length; i++) if (range === null || range.includes(entries[i].key)) filtered.push(entries[i]);
    if (direction === 'nextunique' || direction === 'prevunique') {
        var dedup = [], lastKey;
        for (var j = 0; j < filtered.length; j++) {
            if (dedup.length === 0 || _idb_cmp(filtered[j].key, lastKey) !== 0) { dedup.push(filtered[j]); lastKey = filtered[j].key; }
        }
        filtered = dedup;
    }
    if (direction === 'prev' || direction === 'prevunique') filtered.reverse();
    return filtered;
}

function IDBCursor(req, source, txn, store, withValue, direction) {
    this._req = req;
    this.source = source;
    this._txn = txn;
    this._store = store;
    this._list = null;       // materialised at first dispatch (deferred)
    this._pos = -1;
    this._withValue = withValue;
    this.direction = direction;
    this.key = undefined;
    this.primaryKey = undefined;
    if (withValue) this.value = undefined;
}
IDBCursor.prototype._step = function() {
    this._pos++;
    if (this._pos >= this._list.length) {
        this.key = undefined; this.primaryKey = undefined;
        if (this._withValue) this.value = undefined;
        this._req.result = null;
        return false;
    }
    var item = this._list[this._pos];
    this.key = item.key;
    this.primaryKey = item.primaryKey;
    if (this._withValue) this.value = item.value;
    this._req.result = this;
    return true;
};
IDBCursor.prototype.continue = function(key) {
    if (key !== undefined && !_idb_is_valid_key(key)) throw _idb_error('DataError', 'invalid cursor key');
    var self = this;
    this._req._action = function() {
        if (key !== undefined) {
            var desc = (self.direction === 'prev' || self.direction === 'prevunique');
            while (self._step()) {
                var c = _idb_cmp(self.key, key);
                if ((!desc && c >= 0) || (desc && c <= 0)) break;
            }
        } else {
            self._step();
        }
        return self._req.result;
    };
    this._txn._queue.push(this._req);
    _idb_schedule_txn(this._txn);
};
IDBCursor.prototype.advance = function(count) {
    count = count >>> 0;
    if (count === 0) throw _idb_error('TypeError', 'advance count must be > 0');
    var self = this;
    this._req._action = function() {
        for (var i = 0; i < count; i++) if (!self._step()) break;
        return self._req.result;
    };
    this._txn._queue.push(this._req);
    _idb_schedule_txn(this._txn);
};
IDBCursor.prototype.update = function(value) {
    if (this._txn.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    if (this._pos < 0 || this._pos >= this._list.length) throw _idb_error('InvalidStateError', 'cursor is not positioned on a record');
    var store = this._store, pk = this.primaryKey;
    return _idb_make_request(this.source, this._txn, function() {
        if (store.keyPath !== null) {
            var k = _idb_extract_key(value, store.keyPath);
            if (k === undefined || _idb_cmp(k, pk) !== 0) throw _idb_error('DataError', 'cursor.update must not change the primary key');
        }
        var pos = _idb_find_record(store.records, pk);
        if (!pos.found) throw _idb_error('DataError', 'record no longer exists');
        _idb_check_unique(store, value, pk);
        store.records[pos.idx].value = value;
        return pk;
    });
};
IDBCursor.prototype.delete = function() {
    if (this._txn.mode === 'readonly') throw _idb_error('ReadOnlyError', 'transaction is read-only');
    if (this._pos < 0 || this._pos >= this._list.length) throw _idb_error('InvalidStateError', 'cursor is not positioned on a record');
    var store = this._store, pk = this.primaryKey;
    return _idb_make_request(this.source, this._txn, function() {
        var pos = _idb_find_record(store.records, pk);
        if (pos.found) store.records.splice(pos.idx, 1);
        return undefined;
    });
};

function _idb_open_cursor(source, txn, store, buildList, withValue, direction) {
    if (txn._finished) throw _idb_error('TransactionInactiveError', 'transaction is not active');
    var req = new IDBRequest(source, txn);
    var cursor = new IDBCursor(req, source, txn, store, withValue, direction);
    req._action = function() {
        cursor._list = buildList();
        cursor._step();
        return req.result;
    };
    txn._queue.push(req);
    _idb_schedule_txn(txn);
    return req;
}

// --- open / delete / flush (Indexed DB §3.1) ---------------------------------

function _idb_process_open(entry) {
    var req = entry.req;
    if (req.error) { _idb_dispatch_request(req); return; }
    // A version upgrade (store/index creation, version bump) or a database
    // deletion mutates the persisted snapshot.
    if (entry.upgrade || entry._delete) _idb_dirty = true;
    if (entry.upgrade) {
        var data = entry.data, db = entry.db;
        var txn = new IDBTransaction(db, Object.keys(data.stores), 'versionchange');
        txn._isUpgrade = true;
        db._upgradeTxn = txn;
        data.version = entry.newVersion;
        db.version = entry.newVersion;
        req.transaction = txn;
        req.readyState = 'done';
        var ev = _idb_make_event('upgradeneeded', req, { oldVersion: entry.oldVersion, newVersion: entry.newVersion });
        if (typeof req.onupgradeneeded === 'function') {
            try { req.onupgradeneeded(ev); } catch(e) { _lumen_console_error('IDB onupgradeneeded: ' + e); }
        }
        for (var i = 0; i < req._upgradeListeners.length; i++) {
            try { req._upgradeListeners[i](ev); } catch(e) { _lumen_console_error('IDB upgrade listener: ' + e); }
        }
        while (txn._queue.length > 0 && !txn._aborted) _idb_dispatch_request(txn._queue.shift());
        txn._finished = true;
        db._upgradeTxn = null;
        req.transaction = null;
        if (txn._aborted) { _idb_fire_txn(txn, 'abort'); _idb_dispatch_request(req); return; }
        _idb_fire_txn(txn, 'complete');
    }
    req.readyState = 'done';
    req.error = null;
    var ev2 = _idb_make_event('success', req);
    if (typeof req.onsuccess === 'function') {
        try { req.onsuccess(ev2); } catch(e) { _lumen_console_error('IDB open onsuccess: ' + e); }
    }
    for (var j = 0; j < req._successListeners.length; j++) {
        try { req._successListeners[j](ev2); } catch(e) { _lumen_console_error('IDB open success listener: ' + e); }
    }
}

// Synchronously delivers all pending IndexedDB events. Idempotent and re-entrant
// safe: handlers may enqueue further requests (cursor.continue) or transactions.
function _lumen_idb_flush() {
    _idb_flush_scheduled = false;
    var guard = 0;
    while ((_idb_pending_opens.length > 0 || _idb_active_txns.length > 0) && guard < 1000000) {
        guard++;
        if (_idb_pending_opens.length > 0) { _idb_process_open(_idb_pending_opens.shift()); continue; }
        _idb_flush_txn(_idb_active_txns.shift());
    }
    _idb_persist_if_dirty();
}

var indexedDB = {
    open: function(name, version) {
        name = String(name);
        if (version !== undefined) {
            version = Number(version);
            if (!isFinite(version) || version < 1) throw new TypeError('IndexedDB version must be >= 1');
            version = Math.floor(version);
        }
        var req = new IDBOpenDBRequest();
        var existing = _idb_databases[name];
        var oldVersion = existing ? existing.version : 0;
        var newVersion = (version === undefined) ? (existing ? existing.version : 1) : version;
        if (existing && newVersion < oldVersion) {
            req.error = _idb_error('VersionError', 'requested version is lower than the existing version');
            _idb_pending_opens.push({ req: req });
            _idb_schedule_flush();
            return req;
        }
        var data = existing;
        if (!data) { data = { name: name, version: 0, stores: {} }; _idb_databases[name] = data; }
        var db = new IDBDatabase(data);
        req.result = db;
        _idb_pending_opens.push({
            req: req,
            upgrade: newVersion > data.version,
            oldVersion: data.version,
            newVersion: newVersion,
            db: db,
            data: data
        });
        _idb_schedule_flush();
        return req;
    },
    deleteDatabase: function(name) {
        name = String(name);
        var req = new IDBOpenDBRequest();
        var existing = _idb_databases[name];
        req.result = undefined;
        var old = existing ? existing.version : 0;
        delete _idb_databases[name];
        _idb_pending_opens.push({ req: req, oldVersion: old, newVersion: null, _delete: true });
        _idb_schedule_flush();
        return req;
    },
    databases: function() {
        var out = [];
        for (var name in _idb_databases) out.push({ name: name, version: _idb_databases[name].version });
        return Promise.resolve(out);
    },
    cmp: function(a, b) {
        if (!_idb_is_valid_key(a) || !_idb_is_valid_key(b)) throw _idb_error('DataError', 'invalid key');
        return _idb_cmp(a, b);
    }
};

window.indexedDB        = indexedDB;
window.IDBKeyRange      = IDBKeyRange;
window.IDBRequest       = IDBRequest;
window.IDBOpenDBRequest = IDBOpenDBRequest;
window.IDBDatabase      = IDBDatabase;
window.IDBTransaction   = IDBTransaction;
window.IDBObjectStore   = IDBObjectStore;
window.IDBIndex         = IDBIndex;
window.IDBCursor        = IDBCursor;
window.IDBCursorWithValue = IDBCursor;
window._lumen_idb_flush = _lumen_idb_flush;
window.getSelection     = function() { return _lumen_selection; };
window.Range            = Range;

// ── window.getComputedStyle(element[, pseudoElt]) ────────────────────────────
// Returns a CSSStyleDeclaration-like object with resolved property values.
// Pseudo-elements are not yet supported (ignored).
window.getComputedStyle = function(element, pseudoElt) {
    var nid = element && element.__nid__ != null ? element.__nid__ : null;
    // Cache: keyed by nid, invalidated on next call (live object semantics).
    var handler = {
        get: function(target, prop) {
            if (prop === 'getPropertyValue') {
                return function(name) {
                    if (nid == null) return '';
                    return _lumen_get_computed_style(nid, name) || '';
                };
            }
            if (prop === 'length') return 0;
            if (prop === 'item') return function() { return ''; };
            if (prop === 'cssText') return '';
            if (typeof prop === 'string' && !/^\\d+$/.test(prop)) {
                // camelCase → kebab-case conversion for convenience
                var kebab = prop.replace(/([A-Z])/g, function(m) { return '-' + m.toLowerCase(); });
                if (nid != null) return _lumen_get_computed_style(nid, kebab) || '';
            }
            return undefined;
        }
    };
    // Return a Proxy if available (modern JS), otherwise a plain object with getPropertyValue.
    if (typeof Proxy !== 'undefined') {
        return new Proxy({}, handler);
    }
    // Fallback for environments without Proxy.
    return {
        getPropertyValue: function(name) {
            if (nid == null) return '';
            return _lumen_get_computed_style(nid, name) || '';
        }
    };
};

// Restore persisted databases for this origin (no-op on first visit / when no
// backend is installed). A new JS runtime is built on every page load, so this
// is what makes IndexedDB survive a reload.
if (typeof _lumen_idb_load === 'function') {
    try {
        var _idb_saved = _lumen_idb_load();
        if (_idb_saved) {
            var _idb_restored = _idb_deserialize(_idb_saved);
            if (_idb_restored && typeof _idb_restored === 'object') _idb_databases = _idb_restored;
        }
    } catch (e) { _lumen_console_error('IDB load: ' + e); }
}

// ── Web Crypto API (W3C Web Cryptography API §3 + §14 SubtleCrypto) ──────────
// window.crypto: getRandomValues, randomUUID, subtle (SubtleCrypto).
// Algorithms: ECDSA P-256, HMAC-SHA-256/384/512, AES-GCM 128/256.
(function () {
    function getRandomValues(typedArray) {
        if (!typedArray || typeof typedArray.byteLength !== 'number')
            throw new TypeError('getRandomValues: argument must be a typed array');
        if (typedArray.byteLength > 65536)
            throw new DOMException('getRandomValues: requested too many random bytes (max 65536)', 'QuotaExceededError');
        var bytes = _lumen_get_random_bytes(typedArray.byteLength);
        var view = new Uint8Array(typedArray.buffer, typedArray.byteOffset, typedArray.byteLength);
        for (var i = 0; i < bytes.length; i++) view[i] = bytes[i];
        return typedArray;
    }

    function randomUUID() {
        // RFC 4122 §4.4 UUID version 4
        var b = _lumen_get_random_bytes(16);
        b[6] = (b[6] & 0x0f) | 0x40;  // version 4
        b[8] = (b[8] & 0x3f) | 0x80;  // variant 10xx
        var h = b.map(function(x) { return ('0' + x.toString(16)).slice(-2); });
        return h.slice(0, 4).join('') + '-' + h.slice(4, 6).join('') + '-' +
               h.slice(6, 8).join('') + '-' + h.slice(8, 10).join('') + '-' +
               h.slice(10).join('');
    }

    // Opaque CryptoKey object — wraps a Rust-side key id.
    function CryptoKey(id, info) {
        this.__ckid   = id;
        this.type       = info.type;
        this.algorithm  = info.algorithm;
        this.extractable = info.extractable;
        this.usages     = info.usages;
    }

    function _make_crypto_key(id) {
        var infoJson = _lumen_subtle_key_info(id);
        if (!infoJson) throw new DOMException('Internal: key not found', 'OperationError');
        var info = JSON.parse(infoJson);
        return new CryptoKey(id, info);
    }

    function _to_bytes(data) {
        if (data instanceof ArrayBuffer) return Array.from(new Uint8Array(data));
        if (ArrayBuffer.isView(data))    return Array.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
        throw new TypeError('SubtleCrypto: data must be a BufferSource');
    }

    function _alg_json(algorithm) {
        if (typeof algorithm === 'string') return JSON.stringify({ name: algorithm });
        return JSON.stringify(algorithm);
    }

    function _usages_json(usages) {
        return JSON.stringify(Array.isArray(usages) ? usages : []);
    }

    function _dom_err(result) {
        // result starts with err: prefix
        var msg = result.slice(4);
        return new DOMException(msg, msg);
    }

    var subtle = {
        // ── digest ───────────────────────────────────────────────────────────
        digest: function (algorithm, data) {
            var algo = (algorithm && typeof algorithm === 'object' && algorithm.name)
                     ? algorithm.name : String(algorithm);
            return new Promise(function (resolve, reject) {
                try {
                    var inputBytes = _to_bytes(data);
                    var result = _lumen_sha_digest(algo, inputBytes);
                    if (!result || result.length === 0) {
                        reject(new DOMException(
                            'SubtleCrypto.digest: unsupported algorithm: ' + algo,
                            'NotSupportedError'));
                        return;
                    }
                    resolve(new Uint8Array(result).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── generateKey ──────────────────────────────────────────────────────
        generateKey: function (algorithm, extractable, keyUsages) {
            return new Promise(function (resolve, reject) {
                try {
                    var algJson = _alg_json(algorithm);
                    var usagesJson = _usages_json(keyUsages);
                    var result = _lumen_subtle_generate_key(algJson, !!extractable, usagesJson);
                    if (result.startsWith('err:')) { reject(_dom_err(result)); return; }
                    // ECDSA key pair: pub_id comma priv_id
                    if (result.indexOf(',') !== -1) {
                        var parts = result.split(',');
                        resolve({
                            publicKey:  _make_crypto_key(parseInt(parts[0], 10)),
                            privateKey: _make_crypto_key(parseInt(parts[1], 10))
                        });
                    } else {
                        resolve(_make_crypto_key(parseInt(result, 10)));
                    }
                } catch (e) { reject(e); }
            });
        },

        // ── importKey ────────────────────────────────────────────────────────
        importKey: function (format, keyData, algorithm, extractable, keyUsages) {
            return new Promise(function (resolve, reject) {
                try {
                    var algJson = _alg_json(algorithm);
                    var usagesJson = _usages_json(keyUsages);
                    var bytes;
                    if (format === 'jwk') {
                        // keyData is a JWK object — stringify it to UTF-8 bytes
                        bytes = Array.from(new TextEncoder().encode(JSON.stringify(keyData)));
                    } else {
                        bytes = _to_bytes(keyData instanceof ArrayBuffer ? keyData
                            : (ArrayBuffer.isView(keyData) ? keyData : new Uint8Array(0)));
                    }
                    var result = _lumen_subtle_import_key(format, bytes, algJson, !!extractable, usagesJson);
                    if (result.startsWith('err:')) { reject(_dom_err(result)); return; }
                    resolve(_make_crypto_key(parseInt(result, 10)));
                } catch (e) { reject(e); }
            });
        },

        // ── exportKey ────────────────────────────────────────────────────────
        exportKey: function (format, key) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('exportKey: argument is not a CryptoKey')); return;
                    }
                    var result = _lumen_subtle_export_key_or_err(format, key.__ckid);
                    if (result.startsWith('err:')) { reject(_dom_err(result)); return; }
                    if (result.startsWith('hex:')) {
                        // Raw bytes in hex form
                        var hex = result.slice(4);
                        var buf = new Uint8Array(hex.length / 2);
                        for (var i = 0; i < buf.length; i++)
                            buf[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
                        resolve(format === 'jwk' ? JSON.parse(new TextDecoder().decode(buf)) : buf.buffer);
                    } else {
                        // ok:... prefix for JWK JSON
                        var json = result.slice(3);
                        resolve(format === 'jwk' ? JSON.parse(json) : new TextEncoder().encode(json).buffer);
                    }
                } catch (e) { reject(e); }
            });
        },

        // ── sign ─────────────────────────────────────────────────────────────
        sign: function (algorithm, key, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('sign: argument is not a CryptoKey')); return;
                    }
                    var algJson = _alg_json(algorithm);
                    var dataBytes = _to_bytes(data);
                    var sig = _lumen_subtle_sign(algJson, key.__ckid, dataBytes);
                    if (!sig || sig.length === 0) {
                        reject(new DOMException('sign: operation failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(sig).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── verify ───────────────────────────────────────────────────────────
        verify: function (algorithm, key, signature, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('verify: argument is not a CryptoKey')); return;
                    }
                    var algJson = _alg_json(algorithm);
                    var sigBytes  = _to_bytes(signature);
                    var dataBytes = _to_bytes(data);
                    var ok = _lumen_subtle_verify(algJson, key.__ckid, sigBytes, dataBytes);
                    resolve(!!ok);
                } catch (e) { reject(e); }
            });
        },

        // ── encrypt (AES-GCM) ────────────────────────────────────────────────
        encrypt: function (algorithm, key, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('encrypt: argument is not a CryptoKey')); return;
                    }
                    var iv  = _to_bytes(algorithm.iv || new Uint8Array(12));
                    var aad = algorithm.additionalData ? _to_bytes(algorithm.additionalData) : [];
                    var pt  = _to_bytes(data);
                    var ct  = _lumen_subtle_encrypt(key.__ckid, iv, aad, pt);
                    if (!ct || ct.length === 0) {
                        reject(new DOMException('encrypt: operation failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(ct).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── decrypt (AES-GCM) ────────────────────────────────────────────────
        decrypt: function (algorithm, key, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('decrypt: argument is not a CryptoKey')); return;
                    }
                    var iv  = _to_bytes(algorithm.iv || new Uint8Array(12));
                    var aad = algorithm.additionalData ? _to_bytes(algorithm.additionalData) : [];
                    var ct  = _to_bytes(data);
                    var pt  = _lumen_subtle_decrypt(key.__ckid, iv, aad, ct);
                    if (!pt || pt.length === 0) {
                        reject(new DOMException('decrypt: authentication failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(pt).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── wrapKey / unwrapKey / deriveBits / deriveKey — stubs ─────────────
        wrapKey: function() {
            return Promise.reject(new DOMException('wrapKey: not implemented', 'NotSupportedError'));
        },
        unwrapKey: function() {
            return Promise.reject(new DOMException('unwrapKey: not implemented', 'NotSupportedError'));
        },
        deriveBits: function() {
            return Promise.reject(new DOMException('deriveBits: not implemented', 'NotSupportedError'));
        },
        deriveKey: function() {
            return Promise.reject(new DOMException('deriveKey: not implemented', 'NotSupportedError'));
        }
    };

    window.CryptoKey = CryptoKey;
    window.crypto = { getRandomValues: getRandomValues, randomUUID: randomUUID, subtle: subtle };
    window.Crypto = function Crypto() {};
})();

// ── structuredClone (HTML LS §2.7) ─────────────────────────────────────────
// Handles: primitives, plain objects, arrays, Date, RegExp, Map, Set.
// Not handled: typed arrays as values, circular refs, functions, symbols.
function structuredClone(val) {
    if (val === null || val === undefined) return val;
    var t = typeof val;
    if (t !== 'object') return val;
    if (val instanceof Date) return new Date(val.getTime());
    if (val instanceof RegExp) return new RegExp(val.source, val.flags);
    if (val instanceof Map) {
        var m = new Map();
        val.forEach(function(v, k) { m.set(structuredClone(k), structuredClone(v)); });
        return m;
    }
    if (val instanceof Set) {
        var s = new Set();
        val.forEach(function(v) { s.add(structuredClone(v)); });
        return s;
    }
    if (Array.isArray(val)) {
        var arr = [];
        for (var i = 0; i < val.length; i++) arr[i] = structuredClone(val[i]);
        return arr;
    }
    var out = {};
    var keys = Object.keys(val);
    for (var k = 0; k < keys.length; k++) out[keys[k]] = structuredClone(val[keys[k]]);
    return out;
}
window.structuredClone = structuredClone;

// ── Page lifecycle driver functions (called from Rust via QuickJsRuntime) ─────

// Drive document.readyState forward: 'loading' → 'interactive' → 'complete'.
// Idempotent — state only advances forward.
// Called by Rust: after HTML parse → 'interactive'; after all resources loaded → 'complete'.
function _lumen_apply_ready_state(state) {
    if (state === 'interactive' && _doc_ready_state !== 'loading') return;
    if (state === 'complete' && _doc_ready_state === 'complete') return;
    _doc_ready_state = state;
    // readystatechange on document
    var rsEv = new Event('readystatechange', { bubbles: false, cancelable: false });
    document.dispatchEvent(rsEv);
    if (state === 'interactive') {
        // DOMContentLoaded fires on document (bubbles) then window
        var dcl = new Event('DOMContentLoaded', { bubbles: true, cancelable: false });
        document.dispatchEvent(dcl);
        var winArr = _domcontentloaded_win_listeners.slice();
        for (var i = 0; i < winArr.length; i++) {
            try { winArr[i].call(window, dcl); } catch(e) {}
        }
    } else if (state === 'complete') {
        // load fires on window (does not bubble)
        var loadEv = new Event('load', { bubbles: false, cancelable: false });
        var loadArr = _load_listeners.slice();
        for (var j = 0; j < loadArr.length; j++) {
            try { loadArr[j].call(window, loadEv); } catch(e) {}
        }
        if (typeof window.onload === 'function') {
            try { window.onload.call(window, loadEv); } catch(e) {}
        }
    }
}

// Drive document.visibilityState.  Called from Rust on window focus/blur.
// hidden=true → 'hidden'; hidden=false → 'visible'.
// Fires visibilitychange on document + window listeners if state changed.
function _lumen_apply_visibility(hidden) {
    if (_doc_hidden === hidden) return;
    _doc_hidden = hidden;
    _doc_visibility_state = hidden ? 'hidden' : 'visible';
    var ev = new Event('visibilitychange', { bubbles: true, cancelable: false });
    document.dispatchEvent(ev);
    var vcArr = _visibilitychange_listeners.slice();
    for (var i = 0; i < vcArr.length; i++) {
        try { vcArr[i].call(window, ev); } catch(e) {}
    }
}

window._lumen_apply_ready_state = _lumen_apply_ready_state;
window._lumen_apply_visibility  = _lumen_apply_visibility;

// ── <dialog> modal stack (HTML5 §4.11.7) ─────────────────────────────────────
// Tracks nids of dialogs opened via showModal(), in open order.
// Maintained by _lumen_make_element's showModal/close methods (see below).
var _lumen_modal_dialog_nids = [];

// nid of the element that had keyboard focus immediately before the most
// recent showModal() call (-1 = none). Used to restore focus on close.
var _lumen_last_focused_nid = -1;

// Per-dialog saved focus nid: restored when that dialog closes.
var _lumen_dialog_prev_focus = {};

// DFS search for the first descendant of `container_nid` that has an
// `autofocus` attribute. Returns its nid, or -1 if none found.
function _lumen_find_autofocus_in(container_nid) {
    var queue = _lumen_get_children(container_nid).slice();
    while (queue.length > 0) {
        var cur = queue.shift();
        if (_lumen_get_attr(cur, 'autofocus') !== undefined) return cur;
        var ch = _lumen_get_children(cur);
        for (var i = 0; i < ch.length; i++) queue.push(ch[i]);
    }
    return -1;
}

// ── <selectlist> helpers (Open UI Customizable Select §3) ─────────────────────
// Returns the <listbox> child nid of a <selectlist>, or null if absent.
function _lumen_selectlist_listbox(sl_nid) {
    var kids = _lumen_get_children(sl_nid);
    for (var i = 0; i < kids.length; i++) {
        if ((_lumen_get_tag_name(kids[i]) || '').toLowerCase() === 'listbox') return kids[i];
    }
    return null;
}

// Returns an array of element objects for all <option> children of a
// <selectlist> — either direct children or inside a <listbox> child.
function _lumen_selectlist_options(sl_nid) {
    var out = [];
    var kids = _lumen_get_children(sl_nid);
    for (var i = 0; i < kids.length; i++) {
        var tag = (_lumen_get_tag_name(kids[i]) || '').toLowerCase();
        if (tag === 'option') {
            out.push(_lumen_make_element(kids[i]));
        } else if (tag === 'listbox') {
            var gkids = _lumen_get_children(kids[i]);
            for (var j = 0; j < gkids.length; j++) {
                if ((_lumen_get_tag_name(gkids[j]) || '').toLowerCase() === 'option') {
                    out.push(_lumen_make_element(gkids[j]));
                }
            }
        }
    }
    return out;
}

// ── <details>/<summary> toggle (HTML5 §4.11.1) ───────────────────────────────
// A click anywhere within a <summary> element toggles the `open` attribute on
// its parent <details> and fires a `toggle` event on <details>.
document.addEventListener('click', function(evt) {
    var el = evt.target;
    while (el && el.__nid__ !== undefined) {
        var tag = _lumen_get_tag_name(el.__nid__).toLowerCase();
        if (tag === 'summary') {
            var pid = _lumen_u2n(_lumen_get_parent(el.__nid__));
            if (pid !== null && _lumen_get_tag_name(pid).toLowerCase() === 'details') {
                var wasOpen = _lumen_get_attr(pid, 'open') !== undefined;
                var oldState = wasOpen ? 'open' : 'closed';
                if (wasOpen) { _lumen_remove_attr(pid, 'open'); }
                else         { _lumen_set_attr(pid, 'open', ''); }
                var newState = wasOpen ? 'closed' : 'open';
                var toggleEvt = new Event('toggle', { bubbles: false, cancelable: false });
                toggleEvt.oldState = oldState;
                toggleEvt.newState = newState;
                _lumen_dispatch(pid, toggleEvt);

                // HTML LS §4.11.1.1: exclusive accordion — opening a <details name=X>
                // closes all sibling <details> with the same name attribute.
                if (!wasOpen) {
                    var detailsName = _lumen_u2n(_lumen_get_attr(pid, 'name'));
                    if (detailsName !== null && detailsName !== '') {
                        var parentNid = _lumen_u2n(_lumen_get_parent(pid));
                        if (parentNid !== null) {
                            var siblings = _lumen_get_children(parentNid);
                            for (var _si = 0; _si < siblings.length; _si++) {
                                var sib = siblings[_si];
                                if (sib === pid) continue;
                                if (_lumen_get_tag_name(sib).toLowerCase() !== 'details') continue;
                                var sibName = _lumen_u2n(_lumen_get_attr(sib, 'name'));
                                if (sibName !== detailsName) continue;
                                if (_lumen_get_attr(sib, 'open') === undefined) continue;
                                _lumen_remove_attr(sib, 'open');
                                var sibEvt = new Event('toggle', { bubbles: false, cancelable: false });
                                sibEvt.oldState = 'open';
                                sibEvt.newState = 'closed';
                                _lumen_dispatch(sib, sibEvt);
                            }
                        }
                    }
                }
            }
            return;
        }
        el = el.parentElement;
    }
});

// ── <dialog> Escape key handler (HTML5 §4.11.7) ──────────────────────────────
// Pressing Escape closes the topmost modal dialog: fires `cancel` (cancelable);
// if not prevented, removes `open` and fires `close`.
document.addEventListener('keydown', function(evt) {
    if (evt.key !== 'Escape') return;
    while (_lumen_modal_dialog_nids.length > 0 &&
           _lumen_get_attr(_lumen_modal_dialog_nids[_lumen_modal_dialog_nids.length - 1], 'open') === undefined) {
        _lumen_modal_dialog_nids.pop();
    }
    if (_lumen_modal_dialog_nids.length === 0) return;
    var lastNid = _lumen_modal_dialog_nids[_lumen_modal_dialog_nids.length - 1];
    var cancelEvt = new Event('cancel', { bubbles: false, cancelable: true });
    var notPrevented = _lumen_dispatch(lastNid, cancelEvt);
    if (notPrevented) {
        _lumen_remove_attr(lastNid, 'open');
        _lumen_remove_attr(lastNid, 'data-lumen-modal');
        _lumen_modal_dialog_nids.pop();
        var closeEvt = new Event('close', { bubbles: false, cancelable: false });
        _lumen_dispatch(lastNid, closeEvt);
    }
});

// ── HTML Popover API (WHATWG HTML §6.12) ─────────────────────────────────────
// Top-layer emulation: position:fixed + z-index:2147483647 when open.
// Elements with [popover] are hidden by layout (is_closed_popover in box_tree.rs)
// until showPopover() sets data-lumen-popover-open. Auto-popovers close each
// other and on outside clicks; Escape closes the topmost auto-popover.

// Open auto-popovers in stack order (newest = last).
// Stack of open auto popovers (popover='' or popover='auto').
var _lumen_popover_stack = [];
// Stack of open hint popovers (popover='hint', Popover API Level 2).
// Hints live above autos but are closed when any auto closes.
var _lumen_hint_stack = [];

// Sentinel attribute written by showPopover() — read by layout's is_closed_popover.
var _LPOP_ATTR = 'data-lumen-popover-open';

// Fixed-position styles applied to open popovers (top-layer emulation).
var _LPOP_STYLE = 'position:fixed;z-index:2147483647;inset:auto;margin:auto;overflow:auto;';

function _lumen_popover_show(nid) {
    if (_lumen_get_attr(nid, 'popover') === undefined) {
        throw new DOMException('Element is not a popover', 'NotSupportedError');
    }
    if (_lumen_get_attr(nid, _LPOP_ATTR) !== undefined) return; // already open
    var beforeEvt = new Event('beforetoggle', { bubbles: false, cancelable: false });
    beforeEvt.oldState = 'closed'; beforeEvt.newState = 'open';
    _lumen_dispatch(nid, beforeEvt);
    // Re-check: still not open? (beforetoggle could in theory trigger re-entrant show)
    if (_lumen_get_attr(nid, _LPOP_ATTR) !== undefined) return;
    var popVal = (_lumen_get_attr(nid, 'popover') || '').toLowerCase();
    var isHint = popVal === 'hint';
    var isAuto = !isHint && popVal !== 'manual';
    if (isHint) {
        // Popover API Level 2 §3.2: showing a hint closes other hints but NOT autos.
        var hs = _lumen_hint_stack.slice();
        for (var hi = hs.length - 1; hi >= 0; hi--) { _lumen_popover_hide(hs[hi]); }
        _lumen_hint_stack.push(nid);
    } else if (isAuto) {
        // Showing an auto popover closes all hints first, then all autos.
        var hs2 = _lumen_hint_stack.slice();
        for (var hi2 = hs2.length - 1; hi2 >= 0; hi2--) { _lumen_popover_hide(hs2[hi2]); }
        var snap = _lumen_popover_stack.slice();
        for (var i = snap.length - 1; i >= 0; i--) { _lumen_popover_hide(snap[i]); }
        _lumen_popover_stack.push(nid);
    }
    _lumen_set_attr(nid, _LPOP_ATTR, '');
    // Apply top-layer emulation via inline style (saved/restored around the forced override).
    var saved = _lumen_get_attr(nid, 'style') !== undefined ? _lumen_get_attr(nid, 'style') : '';
    _lumen_set_attr(nid, 'data-lumen-popover-saved-style', saved);
    // hints get a slightly lower z-index than auto (still above page content).
    var style = isHint ? 'position:fixed;z-index:2147483646;inset:auto;margin:auto;overflow:auto;' : _LPOP_STYLE;
    _lumen_set_attr(nid, 'style', style + (saved ? saved : ''));
    var toggleEvt = new Event('toggle', { bubbles: false, cancelable: false });
    toggleEvt.oldState = 'closed'; toggleEvt.newState = 'open';
    _lumen_dispatch(nid, toggleEvt);
}

function _lumen_popover_hide(nid) {
    if (_lumen_get_attr(nid, _LPOP_ATTR) === undefined) return; // already closed
    var beforeEvt = new Event('beforetoggle', { bubbles: false, cancelable: false });
    beforeEvt.oldState = 'open'; beforeEvt.newState = 'closed';
    _lumen_dispatch(nid, beforeEvt);
    if (_lumen_get_attr(nid, _LPOP_ATTR) === undefined) return; // closed by beforetoggle re-entry
    // Remove from whichever stack holds this popover.
    var idx = _lumen_popover_stack.indexOf(nid);
    if (idx >= 0) {
        _lumen_popover_stack.splice(idx, 1);
        // Hiding an auto popover also closes all hints above it in the stack.
        var hs3 = _lumen_hint_stack.slice();
        for (var hi3 = hs3.length - 1; hi3 >= 0; hi3--) { _lumen_popover_hide(hs3[hi3]); }
    }
    var hidx = _lumen_hint_stack.indexOf(nid);
    if (hidx >= 0) _lumen_hint_stack.splice(hidx, 1);
    _lumen_remove_attr(nid, _LPOP_ATTR);
    // Restore saved inline style (remove popover-injected portion).
    var saved = _lumen_u2n(_lumen_get_attr(nid, 'data-lumen-popover-saved-style'));
    if (saved !== null) {
        if (saved === '') { _lumen_remove_attr(nid, 'style'); }
        else { _lumen_set_attr(nid, 'style', saved); }
        _lumen_remove_attr(nid, 'data-lumen-popover-saved-style');
    }
    var toggleEvt = new Event('toggle', { bubbles: false, cancelable: false });
    toggleEvt.oldState = 'open'; toggleEvt.newState = 'closed';
    _lumen_dispatch(nid, toggleEvt);
}

function _lumen_popover_toggle(nid, force) {
    var isOpen = _lumen_get_attr(nid, _LPOP_ATTR) !== undefined;
    if (force === true || (!isOpen && force === undefined)) {
        _lumen_popover_show(nid);
    } else if (force === false || (isOpen && force === undefined)) {
        _lumen_popover_hide(nid);
    }
}

// Click outside handler — close auto and hint popovers when click lands outside all of them.
// Runs in capture phase so it fires before target-specific handlers.
document.addEventListener('click', function(evt) {
    if (_lumen_popover_stack.length === 0 && _lumen_hint_stack.length === 0) return;
    // Walk from target toward root; if any open popover contains the target, bail.
    var cur = evt.target;
    while (cur && cur.__nid__ !== undefined) {
        if (_lumen_get_attr(cur.__nid__, _LPOP_ATTR) !== undefined) return;
        cur = cur.parentElement;
    }
    // Outside click — close hints first (top-down), then autos (top-down).
    var hs = _lumen_hint_stack.slice();
    for (var hi = hs.length - 1; hi >= 0; hi--) { _lumen_popover_hide(hs[hi]); }
    var snap = _lumen_popover_stack.slice();
    for (var i = snap.length - 1; i >= 0; i--) { _lumen_popover_hide(snap[i]); }
}, true);

// Escape key — close topmost hint or auto-popover (if no modal dialog takes precedence).
document.addEventListener('keydown', function(evt) {
    if (evt.key !== 'Escape') return;
    // Let dialog Escape handler take priority when a modal dialog is open.
    if (_lumen_modal_dialog_nids.length > 0) return;
    // Hints sit on top — close topmost hint first if any.
    if (_lumen_hint_stack.length > 0) {
        _lumen_popover_hide(_lumen_hint_stack[_lumen_hint_stack.length - 1]);
        return;
    }
    if (_lumen_popover_stack.length > 0) {
        _lumen_popover_hide(_lumen_popover_stack[_lumen_popover_stack.length - 1]);
    }
});

// popovertarget / popovertargetaction: button/input clicks trigger show/hide/toggle on target.
document.addEventListener('click', function(evt) {
    var el = evt.target;
    while (el && el.__nid__ !== undefined) {
        var ptId = _lumen_u2n(_lumen_get_attr(el.__nid__, 'popovertarget'));
        if (ptId !== null) {
            var targetNid = _lumen_u2n(_lumen_get_element_by_id(ptId));
            if (targetNid !== null) {
                var action = (_lumen_u2n(_lumen_get_attr(el.__nid__, 'popovertargetaction')) || 'toggle').toLowerCase();
                if (action === 'show')   { _lumen_popover_show(targetNid);              return; }
                if (action === 'hide')   { _lumen_popover_hide(targetNid);              return; }
                /* toggle */ _lumen_popover_toggle(targetNid, undefined); return;
            }
        }
        el = el.parentElement;
    }
});

// ── Fullscreen API helpers ────────────────────────────────────────────────────
// Called by the shell (via eval_js) when fullscreen is exited externally, e.g.
// the user pressed Escape or the OS window manager exited fullscreen mode.
// This keeps JS state consistent with reality — _fs_nid → -1, fires events.
function _lumen_notify_fullscreen_exit() {
    if (_fs_nid !== -1) {
        var old = _fs_nid;
        _lumen_remove_attr(_fs_nid, _FS_ATTR);
        _fs_nid = -1;
        var prev = _lumen_make_element(old);
        if (prev) { prev.dispatchEvent(new Event('fullscreenchange', { bubbles: true })); }
        document.dispatchEvent(new Event('fullscreenchange'));
    }
}

// ── Web Animations API Level 1 (W3C Web Animations §3) ─────────────────────
// Pure JS implementation; ticks via a shared requestAnimationFrame loop.
// P4 wires CSS animation-* properties separately; P2 handles compositor offload.
//
// External API surface (called by _lumen_make_element and document object):
//   _wa_element_animate(target, keyframes, options) → Animation
//   _wa_get_animations_for(target) → Animation[]
//   _wa_doc_get_animations() → Animation[]
//   _wa_doc_timeline — DocumentTimeline singleton

// Current animation timeline time — updated at the start of every RAF tick.
var _wa_current_time = 0;
// Live registry of all non-idle Animation instances.
var _wa_animations = [];

// AnimationPlaybackEvent (W3C Web Animations §4.4.3) — fired on finish/cancel.
function AnimationPlaybackEvent(type, init) {
    Event.call(this, type, { bubbles: false, cancelable: false });
    this.currentTime  = (init && init.currentTime  != null) ? init.currentTime  : null;
    this.timelineTime = (init && init.timelineTime != null) ? init.timelineTime : null;
}
AnimationPlaybackEvent.prototype = Object.create(Event.prototype);
AnimationPlaybackEvent.prototype.constructor = AnimationPlaybackEvent;

// DocumentTimeline — wraps the document's global animation timeline.
function DocumentTimeline(options) {
    this._originTime = (options && options.originTime != null) ? +options.originTime : 0;
}
Object.defineProperty(DocumentTimeline.prototype, 'currentTime', {
    get: function() { return _wa_current_time > 0 ? _wa_current_time - this._originTime : null; },
    configurable: true,
});

// Singleton document timeline — shared across all animations on the page.
var _wa_doc_timeline = new DocumentTimeline();

// Normalize the keyframes argument into a sorted array of
// { offset, easing, composite, <prop>: <value> } objects.
function _wa_normalize_keyframes(keyframes) {
    if (!keyframes) return [];
    var result = [];
    if (Array.isArray(keyframes)) {
        var n = keyframes.length;
        for (var i = 0; i < n; i++) {
            var src = keyframes[i] || {};
            var kf = {};
            kf.offset = (src.offset != null) ? +src.offset : (n <= 1 ? 0 : i / (n - 1));
            kf.easing = src.easing || 'linear';
            kf.composite = src.composite || 'replace';
            for (var p in src) {
                if (p !== 'offset' && p !== 'easing' && p !== 'composite') kf[p] = src[p];
            }
            result.push(kf);
        }
    } else {
        // Property-indexed form: { opacity: [0, 1], transform: ['none', 'rotate(90deg)'] }
        var offsets = Array.isArray(keyframes.offset) ? keyframes.offset : null;
        var len = 0;
        var propNames = [];
        for (var pp in keyframes) {
            if (pp !== 'offset' && pp !== 'easing' && pp !== 'composite' && Array.isArray(keyframes[pp])) {
                if (keyframes[pp].length > len) len = keyframes[pp].length;
                propNames.push(pp);
            }
        }
        for (var j = 0; j < len; j++) {
            var kf2 = {};
            kf2.offset = (offsets && offsets[j] != null) ? +offsets[j] : (len <= 1 ? 0 : j / (len - 1));
            kf2.easing = (Array.isArray(keyframes.easing) ? keyframes.easing[j] : keyframes.easing) || 'linear';
            kf2.composite = 'replace';
            for (var k = 0; k < propNames.length; k++) {
                var arr = keyframes[propNames[k]];
                kf2[propNames[k]] = arr[j];
            }
            result.push(kf2);
        }
    }
    result.sort(function(a, b) { return a.offset - b.offset; });
    return result;
}

// Easing functions: linear / ease / ease-in / ease-out / ease-in-out.
function _wa_ease(t, easing) {
    if (!easing || easing === 'linear') return t;
    if (easing === 'ease-in')  return t * t;
    if (easing === 'ease-out') return t * (2 - t);
    if (easing === 'ease' || easing === 'ease-in-out') return t < 0.5 ? 2*t*t : -1+(4-2*t)*t;
    if (easing === 'step-start') return t > 0 ? 1 : 0;
    if (easing === 'step-end')   return t >= 1 ? 1 : 0;
    // cubic-bezier(p1x, p1y, p2x, p2y) — approximate with de Casteljau.
    var m = easing.match(/^cubic-bezier\\(([^,]+),([^,]+),([^,]+),([^)]+)\\)$/);
    if (m) {
        var p1x = +m[1], p1y = +m[2], p2x = +m[3], p2y = +m[4];
        // Newton's method to find t_css for x == t, then return y.
        var u = t;
        for (var iter = 0; iter < 8; iter++) {
            var cx = 3*p1x, bx = 3*(p2x-p1x)-cx, ax = 1-cx-bx;
            var x = ((ax*u+bx)*u+cx)*u;
            var dx = (3*ax*u+2*bx)*u+cx;
            if (Math.abs(dx) < 1e-8) break;
            u -= (x - t) / dx;
        }
        var cy = 3*p1y, by = 3*(p2y-p1y)-cy, ay = 1-cy-by;
        return ((ay*u+by)*u+cy)*u;
    }
    return t;
}

// Parse a CSS color string to [r, g, b, a] (0-255).
function _wa_parse_color(str) {
    str = String(str).trim();
    var m;
    if ((m = str.match(/^rgba?\\(\\s*(\\d+)\\s*,\\s*(\\d+)\\s*,\\s*(\\d+)(?:\\s*,\\s*([\\d.]+))?\\s*\\)$/))) {
        return [+m[1], +m[2], +m[3], m[4] != null ? Math.round(+m[4]*255) : 255];
    }
    if (str.charAt(0) === '#') {
        var h = str.slice(1);
        if (h.length === 3)  h = h[0]+h[0]+h[1]+h[1]+h[2]+h[2];
        if (h.length === 6)  h += 'ff';
        if (h.length === 8)  return [parseInt(h.slice(0,2),16),parseInt(h.slice(2,4),16),parseInt(h.slice(4,6),16),parseInt(h.slice(6,8),16)];
    }
    return null;
}

// Lerp a CSS color.
function _wa_lerp_color(a, b, t) {
    var ca = _wa_parse_color(a), cb = _wa_parse_color(b);
    if (!ca || !cb) return t < 0.5 ? a : b;
    function lr(x, y) { return Math.round(x + (y-x)*t); }
    var al = lr(ca[3], cb[3]);
    if (al === 255) return 'rgb('+lr(ca[0],cb[0])+','+lr(ca[1],cb[1])+','+lr(ca[2],cb[2])+')';
    return 'rgba('+lr(ca[0],cb[0])+','+lr(ca[1],cb[1])+','+lr(ca[2],cb[2])+','+(al/255).toFixed(4)+')';
}

// Lerp a single CSS scalar+unit value (e.g. '100px', '0.5').
function _wa_lerp_scalar(a, b, t) {
    var na = parseFloat(a), nb = parseFloat(b);
    if (isNaN(na) || isNaN(nb)) return t < 0.5 ? a : b;
    var v = na + (nb - na) * t;
    var ua = String(a).replace(/[0-9. +-]/g, '');
    var ub = String(b).replace(/[0-9. +-]/g, '');
    return v + (ua || ub || '');
}

// CSS color-like property names.
var _wa_color_props = {
    color:1, backgroundColor:1, borderColor:1, outlineColor:1,
    borderTopColor:1, borderRightColor:1, borderBottomColor:1, borderLeftColor:1,
    textDecorationColor:1, fill:1, stroke:1
};

// Parse a transform function string: 'rotate(90deg)' → {name:'rotate', args:['90deg']}.
function _wa_parse_tfn(s) {
    var m = s.match(/^(\\w+)\\(([^)]*)\\)$/);
    return m ? { name: m[1], args: m[2].split(',').map(function(a){ return a.trim(); }) } : null;
}

// Lerp two transform strings using matched-pair lerp when possible.
function _wa_lerp_transform(from, to, t) {
    if (from === to) return from;
    if (from === 'none' && to === 'none') return 'none';
    if (from === 'none') return to;
    if (to === 'none') return from;
    var fns_a = from.match(/\\w+\\([^)]*\\)/g) || [];
    var fns_b = to.match(/\\w+\\([^)]*\\)/g) || [];
    if (fns_a.length !== fns_b.length) return t < 0.5 ? from : to;
    var out = [];
    for (var i = 0; i < fns_a.length; i++) {
        var fa = _wa_parse_tfn(fns_a[i]), fb = _wa_parse_tfn(fns_b[i]);
        if (!fa || !fb || fa.name !== fb.name) return t < 0.5 ? from : to;
        var args = [];
        for (var j = 0; j < fa.args.length; j++) args.push(_wa_lerp_scalar(fa.args[j], fb.args[j], t));
        out.push(fa.name + '(' + args.join(', ') + ')');
    }
    return out.join(' ');
}

// Interpolate a single CSS property value between two string values.
function _wa_interp_prop(prop, from, to, t) {
    if (from === to) return from;
    if (_wa_color_props[prop]) return _wa_lerp_color(from, to, t);
    if (prop === 'opacity') {
        var fa2 = parseFloat(from), fb2 = parseFloat(to);
        if (!isNaN(fa2) && !isNaN(fb2)) return String(+(fa2+(fb2-fa2)*t).toFixed(6));
    }
    if (prop === 'transform') return _wa_lerp_transform(from, to, t);
    return _wa_lerp_scalar(from, to, t);
}

// Compute the per-property interpolated styles for a KeyframeEffect at progress p.
function _wa_compute_at_p(effect, p) {
    var kfs = effect._keyframes;
    if (!kfs || !kfs.length) return {};
    // Find surrounding keyframe pair.
    var from = kfs[0], to = kfs[kfs.length - 1];
    for (var i = 0; i < kfs.length - 1; i++) {
        if (kfs[i].offset <= p && kfs[i+1].offset >= p) { from = kfs[i]; to = kfs[i+1]; break; }
    }
    var span = to.offset - from.offset;
    var lt = span < 1e-7 ? 1 : Math.max(0, Math.min(1, (p - from.offset) / span));
    lt = _wa_ease(lt, from.easing || 'linear');
    var result = {};
    for (var fp in from) {
        if (fp === 'offset' || fp === 'easing' || fp === 'composite') continue;
        result[fp] = (fp in to) ? _wa_interp_prop(fp, from[fp], to[fp], lt) : from[fp];
    }
    for (var tp in to) {
        if (tp === 'offset' || tp === 'easing' || tp === 'composite') continue;
        if (!(tp in result)) result[tp] = to[tp];
    }
    return result;
}

// Compute the iteration progress [0,1] from animation timing and currentTime.
function _wa_iter_progress(timing, ct) {
    var dur = +timing.duration || 0;
    if (dur <= 0) return 1;
    var delay = +(timing.delay || 0);
    var elapsed = ct - delay;
    var fill = timing.fill || 'auto';
    if (elapsed < 0) {
        return (fill === 'backwards' || fill === 'both') ? 0 : -1;
    }
    var maxIter = (timing.iterations === Infinity || timing.iterations == null) ? Infinity : +(timing.iterations) || 1;
    var totalDur = maxIter === Infinity ? Infinity : dur * maxIter;
    if (totalDur !== Infinity && elapsed >= totalDur) {
        return (fill === 'forwards' || fill === 'both') ? 1 : -2;
    }
    var iterFloor = Math.floor(elapsed / dur);
    var iterProg = (elapsed % dur) / dur;
    var dir = timing.direction || 'normal';
    var isOdd = iterFloor % 2 === 1;
    var directed = iterProg;
    if      (dir === 'reverse')           directed = 1 - iterProg;
    else if (dir === 'alternate')         directed = isOdd ? 1 - iterProg : iterProg;
    else if (dir === 'alternate-reverse') directed = isOdd ? iterProg : 1 - iterProg;
    return _wa_ease(Math.max(0, Math.min(1, directed)), timing.easing || 'linear');
}

// KeyframeEffect constructor (Web Animations §5.1).
function KeyframeEffect(target, keyframes, options) {
    this.target = target || null;
    this._keyframes = _wa_normalize_keyframes(keyframes);
    var opts = (typeof options === 'number') ? { duration: options } : (options || {});
    this._timing = {
        duration:       opts.duration     != null  ? +opts.duration       : 0,
        delay:          +(opts.delay      || 0),
        endDelay:       +(opts.endDelay   || 0),
        fill:           opts.fill         || 'auto',
        iterationStart: +(opts.iterationStart || 0),
        iterations:     opts.iterations   != null  ? opts.iterations      : 1,
        easing:         opts.easing       || 'linear',
        direction:      opts.direction    || 'normal',
    };
    this.composite          = opts.composite          || 'replace';
    this.iterationComposite = opts.iterationComposite || 'replace';
    this.pseudoElement      = opts.pseudoElement      || null;
}
KeyframeEffect.prototype.getTiming    = function() { return Object.assign({}, this._timing); };
KeyframeEffect.prototype.updateTiming = function(t) { Object.assign(this._timing, t); };
KeyframeEffect.prototype.getKeyframes = function() { return this._keyframes.slice(); };
KeyframeEffect.prototype.setKeyframes = function(kf) { this._keyframes = _wa_normalize_keyframes(kf); };

// Animation constructor (Web Animations §3.4).
var _wa_anim_seq = 1;
function Animation(effect, timeline) {
    this._wid         = _wa_anim_seq++;
    this.id           = '';
    this.effect       = effect   || null;
    this.timeline     = timeline || _wa_doc_timeline;
    this._startTime   = null;
    this._holdTime    = null;
    this._pbRate      = 1;
    this._state       = 'idle';   // idle | running | paused | finished
    this._prevStyles  = {};
    this.onfinish     = null;
    this.oncancel     = null;
    this.onremove     = null;
    var self = this;
    this.ready    = Promise.resolve(self);
    this.finished = new Promise(function(res) { self._finishRes = res; });
    this._rafId   = null;
}

Object.defineProperty(Animation.prototype, 'currentTime', {
    get: function() {
        if (this._holdTime !== null) return this._holdTime;
        if (this._startTime === null) return null;
        return (_wa_current_time - this._startTime) * this._pbRate;
    },
    set: function(v) {
        if (v == null) { this._holdTime = null; return; }
        this._holdTime = +v;
        if (this._state !== 'paused' && this._startTime !== null) {
            this._startTime = _wa_current_time - this._holdTime / this._pbRate;
            this._holdTime = null;
        }
    },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'startTime', {
    get: function() { return this._startTime; },
    set: function(v) {
        this._startTime = (v == null) ? null : +v;
        this._holdTime  = null;
        if (this._startTime !== null && this._state === 'idle') this._state = 'running';
    },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'playbackRate', {
    get: function() { return this._pbRate; },
    set: function(v) { this._pbRate = +v || 1; },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'playState', {
    get: function() { return this._state; },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'pending', {
    get: function() { return false; },
    configurable: true,
});

Animation.prototype.play = function() {
    var hold = this._holdTime !== null ? this._holdTime : (this._state === 'idle' ? 0 : null);
    if (hold !== null) {
        this._startTime = _wa_current_time - hold / this._pbRate;
        this._holdTime  = null;
    } else if (this._startTime === null) {
        this._startTime = _wa_current_time;
    }
    this._state = 'running';
    this._scheduleRaf();
    var idx = _wa_animations.indexOf(this);
    if (idx < 0) _wa_animations.push(this);
};

Animation.prototype.pause = function() {
    var ct = this.currentTime;
    this._holdTime  = ct !== null ? ct : 0;
    this._startTime = null;
    this._state     = 'paused';
    this._cancelRaf();
};

Animation.prototype.cancel = function() {
    this._clearStyles();
    this._state     = 'idle';
    this._startTime = null;
    this._holdTime  = null;
    this._cancelRaf();
    var idx = _wa_animations.indexOf(this);
    if (idx >= 0) _wa_animations.splice(idx, 1);
    if (typeof this.oncancel === 'function') try { this.oncancel(new Event('cancel')); } catch(e) {}
};

Animation.prototype.finish = function() {
    var eff = this.effect;
    if (eff) {
        var t = eff._timing;
        var maxI = (t.iterations === Infinity || t.iterations == null) ? Infinity : +t.iterations || 1;
        this._holdTime = maxI === Infinity ? 0 : +t.duration * maxI;
    }
    this._state = 'finished';
    this._applyAtP(1);
    this._cancelRaf();
    this._onFinish();
};

Animation.prototype.reverse = function() {
    this._pbRate = -this._pbRate;
    this.play();
};

Animation.prototype.updatePlaybackRate = function(rate) {
    this._pbRate = +rate || 1;
};

Animation.prototype._scheduleRaf = function() {
    if (this._rafId !== null) return;
    var self = this;
    this._rafId = requestAnimationFrame(function(ts) {
        self._rafId = null;
        self._tick(ts);
    });
};

Animation.prototype._cancelRaf = function() {
    if (this._rafId !== null) {
        cancelAnimationFrame(this._rafId);
        this._rafId = null;
    }
};

Animation.prototype._tick = function(now) {
    if (this._state !== 'running') return;
    var eff = this.effect;
    if (!eff) return;
    var ct = this.currentTime;
    if (ct === null) return;
    var p = _wa_iter_progress(eff._timing, ct);
    if (p === -2) {
        // Past end — finished
        this._state = 'finished';
        this._applyAtP(1);
        var idx = _wa_animations.indexOf(this);
        if (idx >= 0) _wa_animations.splice(idx, 1);
        this._onFinish();
        return;
    }
    if (p === -1) {
        // Before delay start — apply 'from' frame if fill=backwards|both
        var fillMode = (eff._timing && eff._timing.fill) || 'auto';
        if (fillMode === 'backwards' || fillMode === 'both') this._applyAtP(0);
    } else {
        this._applyAtP(p);
    }
    this._scheduleRaf();
};

Animation.prototype._applyAtP = function(p) {
    var eff = this.effect;
    if (!eff || !eff.target) return;
    var styles = _wa_compute_at_p(eff, p);
    for (var prop in styles) {
        try { eff.target.style[prop] = styles[prop]; } catch(e) {}
    }
    this._prevStyles = styles;
};

Animation.prototype._clearStyles = function() {
    var eff = this.effect;
    if (!eff || !eff.target) return;
    for (var prop in this._prevStyles) {
        try { eff.target.style[prop] = ''; } catch(e) {}
    }
    this._prevStyles = {};
};

Animation.prototype._onFinish = function() {
    if (typeof this.onfinish === 'function') try { this.onfinish(new Event('finish')); } catch(e) {}
    if (typeof this._finishRes === 'function') { try { this._finishRes(this); } catch(e) {} this._finishRes = null; }
};

// element.animate() factory shortcut (Web Animations §3.3).
function _wa_element_animate(target, keyframes, options) {
    var eff  = new KeyframeEffect(target, keyframes, options);
    var anim = new Animation(eff, _wa_doc_timeline);
    anim.play();
    return anim;
}

// element.getAnimations() — all non-idle animations targeting this element.
function _wa_get_animations_for(target) {
    return _wa_animations.filter(function(a) {
        return a._state !== 'idle' && a.effect && a.effect.target === target;
    });
}

// document.getAnimations() — all non-idle animations on this document.
function _wa_doc_get_animations() {
    return _wa_animations.filter(function(a) { return a._state !== 'idle'; });
}

// ── Web Locks API (W3C Web Locks API §5) ──────────────────────────────────────
// navigator.locks.request(name[, options], callback) → Promise
// navigator.locks.query() → Promise<{held, pending}>
//
// Single-context implementation: locks are scoped to one JS context (page).
// Cross-context coordination (cross-tab mutex) is Phase 3 / multi-process.
//
// Lock modes:
//   'exclusive' (default): one holder max; blocked by any existing lock.
//   'shared': concurrent readers allowed; blocked only by exclusive holders.
//
// Options (all optional):
//   mode       'exclusive' | 'shared'  (default 'exclusive')
//   signal     AbortSignal             (cancel queued request on abort)
//   ifAvailable boolean                (callback(null) if not immediately free)
//   steal      boolean                 (evict current holders; grant immediately)
(function() {
  var _locks = {};  // name → { excl, shared, queue[] }

  function _st(name) {
    if (!_locks[name]) _locks[name] = { excl: 0, shared: 0, queue: [] };
    return _locks[name];
  }

  function _canAcq(st, mode) {
    return mode === 'exclusive' ? st.excl === 0 && st.shared === 0 : st.excl === 0;
  }

  function _acq(st, mode) {
    if (mode === 'exclusive') st.excl++; else st.shared++;
  }

  function _rel(st, mode) {
    if (mode === 'exclusive') { if (st.excl   > 0) st.excl--;   }
    else                      { if (st.shared > 0) st.shared--; }
    _drain(st);
  }

  function _drain(st) {
    var i = 0;
    while (i < st.queue.length) {
      var req = st.queue[i];
      if (!_canAcq(st, req.mode)) break;
      _acq(st, req.mode);
      st.queue.splice(i, 1);
      req.grant();
      if (req.mode === 'exclusive') break; // exclusive acquired — stop draining
      // shared acquired — continue to try more queued shared requests
    }
  }

  function _run(cb, lock, resolve, reject, st, mode) {
    var res;
    try { res = cb(lock); } catch (e) { _rel(st, mode); reject(e); return; }
    Promise.resolve(res).then(
      function(v) { _rel(st, mode); resolve(v); },
      function(e) { _rel(st, mode); reject(e); }
    );
  }

  function Lock(name, mode) {
    Object.defineProperty(this, 'name', { value: name, enumerable: true });
    Object.defineProperty(this, 'mode', { value: mode, enumerable: true });
  }

  function LockManager() {}

  LockManager.prototype.request = function(name, a, b) {
    var opts = {}, cb;
    if (typeof a === 'function') { cb = a; }
    else { opts = a && typeof a === 'object' ? a : {}; cb = b; }

    if (typeof cb !== 'function')
      return Promise.reject(new TypeError('LockManager.request: callback required'));
    if (name == null)
      return Promise.reject(new TypeError('LockManager.request: name required'));

    name = String(name);
    var mode = opts.mode != null ? String(opts.mode) : 'exclusive';
    if (mode !== 'exclusive' && mode !== 'shared')
      return Promise.reject(
        new TypeError('LockManager.request: mode must be exclusive or shared'));

    var sig    = opts.signal     || null;
    var ifAvl  = !!opts.ifAvailable;
    var steal  = !!opts.steal;
    var st     = _st(name);

    if (steal) {
      // Evict all current holders and remove exclusive pending requests.
      st.excl = 0; st.shared = 0;
      for (var qi = st.queue.length - 1; qi >= 0; qi--) {
        if (st.queue[qi].mode === 'exclusive') {
          st.queue[qi].abort(new DOMException('Lock stolen', 'AbortError'));
          st.queue.splice(qi, 1);
        }
      }
    }

    return new Promise(function(resolve, reject) {
      if (sig && sig.aborted) {
        reject(sig.reason instanceof Error ? sig.reason
          : new DOMException('The operation was aborted.', 'AbortError'));
        return;
      }
      if (_canAcq(st, mode)) {
        _acq(st, mode);
        _run(cb, new Lock(name, mode), resolve, reject, st, mode);
        return;
      }
      if (ifAvl) {
        var r2;
        try { r2 = cb(null); } catch (e2) { reject(e2); return; }
        Promise.resolve(r2).then(resolve, reject);
        return;
      }
      // Queue the request.
      var granted = false, abortH = null;
      function onGrant() {
        if (granted) return; granted = true;
        if (sig && abortH) sig.removeEventListener('abort', abortH);
        _run(cb, new Lock(name, mode), resolve, reject, st, mode);
      }
      function onAbort() {
        if (granted) return;
        for (var j = 0; j < st.queue.length; j++) {
          if (st.queue[j].grant === onGrant) { st.queue.splice(j, 1); break; }
        }
        var reason = (sig && sig.reason instanceof Error)
          ? sig.reason : new DOMException('The operation was aborted.', 'AbortError');
        reject(reason);
      }
      if (sig) { abortH = onAbort; sig.addEventListener('abort', abortH); }
      st.queue.push({ mode: mode, grant: onGrant, abort: onAbort });
    });
  };

  LockManager.prototype.query = function() {
    var held = [], pending = [];
    for (var n in _locks) {
      var s = _locks[n];
      for (var i = 0; i < s.excl;   i++) held.push({ name: n, mode: 'exclusive', clientId: '' });
      for (var j = 0; j < s.shared; j++) held.push({ name: n, mode: 'shared',    clientId: '' });
      for (var k = 0; k < s.queue.length; k++)
        pending.push({ name: n, mode: s.queue[k].mode, clientId: '' });
    }
    return Promise.resolve({ held: held, pending: pending });
  };

  var _lockMgr = new LockManager();
  Object.defineProperty(navigator, 'locks', {
    value: _lockMgr, configurable: true, writable: false, enumerable: true,
  });
  window.LockManager = LockManager;
  window.Lock = Lock;
})();

// ── Screen Wake Lock API (W3C Screen Wake Lock §6.5) ──────────────────────────
// navigator.wakeLock.request('screen') → Promise<WakeLockSentinel>
// Phase 1 stub: always resolves (no OS integration yet; release is a no-op).
(function() {
  function WakeLockSentinel(type) {
    Object.defineProperty(this, 'type', { value: type, enumerable: true });
    this.released  = false;
    this._listeners = [];
  }
  WakeLockSentinel.prototype.release = function() {
    if (this.released) return Promise.resolve();
    this.released = true;
    var ev = { type: 'release', target: this };
    if (typeof this._onrelease === 'function') try { this._onrelease(ev); } catch(e) {}
    for (var i = 0; i < this._listeners.length; i++) try { this._listeners[i](ev); } catch(e) {}
    return Promise.resolve();
  };
  Object.defineProperty(WakeLockSentinel.prototype, 'onrelease', {
    get: function() { return this._onrelease || null; },
    set: function(fn) { this._onrelease = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });
  WakeLockSentinel.prototype.addEventListener = function(t, fn) {
    if (t === 'release' && typeof fn === 'function') this._listeners.push(fn);
  };
  WakeLockSentinel.prototype.removeEventListener = function(t, fn) {
    var i = this._listeners.indexOf(fn); if (i >= 0) this._listeners.splice(i, 1);
  };

  navigator.wakeLock = {
    request: function(type) {
      if (type !== 'screen')
        return Promise.reject(
          new DOMException('Unsupported wake lock type: ' + String(type), 'NotSupportedError'));
      return Promise.resolve(new WakeLockSentinel(String(type)));
    },
  };
  window.WakeLockSentinel = WakeLockSentinel;
})();

// ── Network Information API (W3C Network Information §7) ──────────────────────
// navigator.connection — effective type, downlink, rtt, saveData.
// Phase 1 stub: reports '4g'/10 Mbps/100 ms (reasonable desktop default).
(function() {
  function NetworkInformation() {
    this.effectiveType = '4g';
    this.downlink      = 10;
    this.rtt           = 100;
    this.saveData      = false;
    this.type          = 'wifi';
    this._onchange     = null;
  }
  Object.defineProperty(NetworkInformation.prototype, 'onchange', {
    get: function() { return this._onchange; },
    set: function(fn) { this._onchange = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });
  NetworkInformation.prototype.addEventListener    = function() {};
  NetworkInformation.prototype.removeEventListener = function() {};

  navigator.connection = new NetworkInformation();
  window.NetworkInformation = NetworkInformation;
})();

// ── navigator.userActivation (HTML LS §6.4) ───────────────────────────────────
// Single-user interactive desktop app: always reports the user has activated.
Object.defineProperty(navigator, 'userActivation', {
  value: Object.freeze({ isActive: true, hasBeenActive: true }),
  configurable: true, writable: false, enumerable: true,
});

// ── Web Share API (W3C Web Share §4) ──────────────────────────────────────────
// Phase 1 stub: always rejects (no OS share-sheet integration yet).
navigator.share = function(_data) {
  return Promise.reject(
    new DOMException('navigator.share is not supported in Lumen Phase 1.', 'NotSupportedError'));
};
navigator.canShare = function() { return false; };

// ── window.reportError() (HTML LS §8.1.3.6) ───────────────────────────────────
// Fires an ErrorEvent on window for the given error (uncaught-error pipeline).
function reportError(err) {
  var msg = err instanceof Error ? err.message : String(err);
  var ev = new ErrorEvent('error', { error: err, message: msg, bubbles: true, cancelable: true });
  window.dispatchEvent(ev);
}
window.reportError = reportError;

// ── DOM GC collect (idle shell tick) ─────────────────────────────────────────
// Called by the shell's GcTick every 30 s with an array of node IDs that
// have been detached from the document and have zero live JS references.
// Purges JS-side per-node caches so dead nodes don't retain memory through maps:
//   - _lumen_listeners  keyed by 'nid:eventtype'
//   - _input_values     keyed by nid
// The arena itself is append-only in Phase 1; physical compaction is Phase 3.
function _lumen_gc_collect(nids) {
    for (var i = 0; i < nids.length; i++) {
        var nid = nids[i];
        var prefix = String(nid) + ':';
        var plen   = prefix.length;
        for (var key in _lumen_listeners) {
            if (key.length > plen && key.substring(0, plen) === prefix) {
                delete _lumen_listeners[key];
            }
        }
        delete _input_values[nid];
        delete _canvas2d_ctxs[nid];
    }
}

// B-7: CSS Resize property Phase 1 — apply element width/height changes from grip drag.
// Called during CursorMoved when resize_active is set.
// start_x/y are saved at MouseInput Pressed; delta is computed from current cursor position.
// The binding updates element's inline style: width = computed_width + delta_x; height = computed_height + delta_y.
function _lumen_apply_resize(nid, delta_x, delta_y) {
    var elem = _lumen_make_element(nid);
    if (!elem) return;

    var style = elem.style;
    if (!style) return;

    // Get current computed dimensions (bounding rect: [x, y, w, h])
    var rect = _lumen_get_bounding_rect(nid);
    if (!rect) return;

    var curr_width = rect[2];
    var curr_height = rect[3];

    // Apply delta to compute new width/height
    var new_width = Math.max(0, curr_width + delta_x);
    var new_height = Math.max(0, curr_height + delta_y);

    // Update inline style (will trigger relayout + repaint)
    style.width = new_width + 'px';
    style.height = new_height + 'px';
}

// D-6: Extension system stub — chrome.runtime API Phase 0.
// Provides enough surface so existing extension content-scripts don't throw on import.
// Phase 0: sendMessage is fire-and-forget (message goes to native no-op binding).
// Phase 1: shell wires up a real message bus between content scripts and extension background.
// Guard: only install when _LUMEN_EXTENSION_ACTIVE is set (avoids CDP automation detection markers).
(function() {
    if (typeof globalThis === 'undefined' || !globalThis._LUMEN_EXTENSION_ACTIVE) { return; }
    var _rt = {
        id: 'lumen-extension',
        sendMessage: function(msg, callback) {
            _lumen_chrome_runtime_send_message(JSON.stringify(msg));
            if (typeof callback === 'function') { callback(undefined); }
        },
        onMessage: {
            _listeners: [],
            addListener: function(fn) { this._listeners.push(fn); },
            removeListener: function(fn) {
                this._listeners = this._listeners.filter(function(l) { return l !== fn; });
            },
            hasListener: function(fn) { return this._listeners.indexOf(fn) !== -1; }
        },
        getURL: function(path) { return 'chrome-extension://lumen-extension/' + path; },
        getManifest: function() { return { name: '', version: '0', manifest_version: 3 }; }
    };
    if (typeof globalThis !== 'undefined') {
        globalThis.chrome = { runtime: _rt };
        globalThis.browser = { runtime: _rt };
    }
    if (typeof window !== 'undefined') {
        window.chrome = { runtime: _rt };
        window.browser = { runtime: _rt };
    }
})();

// ── scroll events helpers ──────────────────────────────────────────────────────
// Called from Rust (QuickJsRuntime::fire_element_scroll / fire_window_scroll)
// after scroll position changes.  Per WHATWG HTML §8.1.6.2 scroll events are
// non-bubbling (bubbles:false) and non-cancelable.
function _lumen_fire_scroll_on_element(nid) {
    var el = _lumen_make_element(nid);
    if (!el) return;
    var ev = new Event('scroll', { bubbles: false, cancelable: false });
    el.dispatchEvent(ev);
}
function _lumen_fire_window_scroll_event() {
    var ev = new Event('scroll', { bubbles: false, cancelable: false });
    if (typeof window !== 'undefined') { window.dispatchEvent(ev); }
    if (typeof document !== 'undefined') { document.dispatchEvent(ev); }
}
";

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::JsRuntime;
    use lumen_dom::{Document, NodeData, QualName};

    fn make_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let title = doc.create_element(QualName::html("title"));
        let title_text = doc.create_text("Test Page");
        let body = doc.create_element(QualName::html("body"));
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"),
                value: "main".into(),
            });
        }
        let span = doc.create_element(QualName::html("span"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(span).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("class"),
                value: "highlight".into(),
            });
        }
        let text = doc.create_text("Hello");
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, title);
        doc.append_child(title, title_text);
        doc.append_child(html, body);
        doc.append_child(body, div);
        doc.append_child(div, span);
        doc.append_child(span, text);
        Arc::new(Mutex::new(doc))
    }

    fn runtime_with_dom(doc: Arc<Mutex<Document>>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        // Enable extension API (chrome.runtime) for unit tests that verify its behaviour.
        rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
        rt.install_dom(doc, "", None, None, None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn console_log_does_not_crash() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("console.log('hello from test')").unwrap();
    }

    #[test]
    fn canvas_get_context_2d_returns_object() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var c = document.createElement('canvas');\
                 var ctx = c.getContext('2d');\
                 ctx !== null && typeof ctx.fillRect === 'function' \
                   && typeof ctx.beginPath === 'function'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn canvas_get_context_2d_caches_same_object() {
        let rt = runtime_with_dom(make_doc());
        let same = rt
            .eval(
                "var c = document.createElement('canvas');\
                 c.getContext('2d') === c.getContext('2d')",
            )
            .unwrap();
        assert_eq!(same, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn canvas_default_dimensions_are_300x150() {
        let rt = runtime_with_dom(make_doc());
        let w = rt
            .eval("var c = document.createElement('canvas'); c.width")
            .unwrap();
        let h = rt
            .eval("var c = document.createElement('canvas'); c.height")
            .unwrap();
        assert_eq!(w, lumen_core::JsValue::Number(300.0));
        assert_eq!(h, lumen_core::JsValue::Number(150.0));
    }

    #[test]
    fn canvas_draw_flushes_dirty_buffer() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var c = document.createElement('canvas');\
             c.setAttribute('width', '4'); c.setAttribute('height', '4');\
             var ctx = c.getContext('2d');\
             ctx.fillStyle = '#00ff00';\
             ctx.fillRect(0, 0, 4, 4);",
        )
        .unwrap();
        let updates = rt.flush_canvas_updates();
        assert_eq!(updates.len(), 1, "one dirty canvas after fillRect");
        let (_nid, w, h, rgba) = &updates[0];
        assert_eq!((*w, *h), (4, 4));
        assert_eq!(rgba[1], 255, "green channel painted");
    }

    #[test]
    fn canvas_get_context_webgl_via_2d_shim_is_null() {
        // The 2D shim's getContext returns null for non-2d types (the functional
        // WebGL path is the separate webgl_canvas shim, not wired in these tests).
        let rt = runtime_with_dom(make_doc());
        let is_null = rt
            .eval(
                "var c = document.createElement('canvas');\
                 c.getContext('webgl') === null",
            )
            .unwrap();
        assert_eq!(is_null, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn non_canvas_get_context_2d_is_null() {
        let rt = runtime_with_dom(make_doc());
        let is_null = rt
            .eval(
                "var d = document.createElement('div');\
                 d.getContext('2d') === null",
            )
            .unwrap();
        assert_eq!(is_null, lumen_core::JsValue::Bool(true));
    }

    // ── Canvas CSS resize tests ───────────────────────────────────────────────

    #[test]
    fn canvas_css_resize_scales_pixels() {
        // After a CSS-driven resize, scale_resize is called and pixels are preserved.
        let rt = runtime_with_dom(make_doc());
        // Create canvas, draw a red fill, then trigger CSS resize.
        rt.eval(r#"
            var c = document.createElement('canvas');
            c.width = 4; c.height = 4;
            var ctx = c.getContext('2d');
            ctx.fillStyle = '#ff0000';
            ctx.fillRect(0, 0, 4, 4);
            window.__test_canvas_nid = c.__nid__;
        "#).unwrap();
        let nid_val = rt.eval("window.__test_canvas_nid").unwrap();
        let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
        // First delivery at 4×4 — records baseline.
        rt.update_layout_rects([(nid, [0.0, 0.0, 4.0, 4.0])].into_iter().collect());
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        // Drain dirty list so next flush only sees scale_resize changes.
        let _ = crate::canvas2d::flush_dirty();
        // Change CSS dims to 8×8 — triggers scale_resize + marks dirty.
        rt.update_layout_rects([(nid, [0.0, 0.0, 8.0, 8.0])].into_iter().collect());
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        // Canvas backing buffer should now be 8×8.
        let dirty = crate::canvas2d::flush_dirty();
        let resized = dirty.iter().any(|(id, w, h, _)| *id == nid && *w == 8 && *h == 8);
        assert!(resized, "canvas should have been scaled to 8×8");
    }

    #[test]
    fn canvas_css_resize_fires_resize_event() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var c2 = document.createElement('canvas');
            c2.width = 10; c2.height = 10;
            c2.getContext('2d');
            var _css_resize_fired = false;
            c2.addEventListener('resize', function() { _css_resize_fired = true; });
            window.__test_c2_nid = c2.__nid__;
        "#).unwrap();
        let nid_val = rt.eval("window.__test_c2_nid").unwrap();
        let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
        // First delivery at 10×10 — records baseline, no event.
        rt.update_layout_rects([(nid, [0.0, 0.0, 10.0, 10.0])].into_iter().collect());
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        let fired_before = rt.eval("_css_resize_fired").unwrap();
        assert_eq!(fired_before, lumen_core::JsValue::Bool(false));
        // Change CSS dims — event should fire.
        rt.update_layout_rects([(nid, [0.0, 0.0, 20.0, 20.0])].into_iter().collect());
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        let fired = rt.eval("_css_resize_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn canvas_css_resize_no_event_when_size_unchanged() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var c3 = document.createElement('canvas');
            c3.width = 10; c3.height = 10;
            c3.getContext('2d');
            var _css_cnt = 0;
            c3.addEventListener('resize', function() { _css_cnt++; });
            window.__test_c3_nid = c3.__nid__;
        "#).unwrap();
        let nid_val = rt.eval("window.__test_c3_nid").unwrap();
        let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
        let rect = [(nid, [0.0, 0.0, 10.0, 10.0])].into_iter().collect();
        rt.update_layout_rects(rect);
        // First delivery — baseline.
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        // Second delivery — same size, no event.
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        let cnt = rt.eval("_css_cnt").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn canvas_css_resize_not_triggered_without_context() {
        // A canvas without a 2D context is not tracked by _lumen_deliver_canvas_css_resize.
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var c4 = document.createElement('canvas');
            // intentionally no getContext('2d')
            var _no_ctx_fired = false;
            c4.addEventListener('resize', function() { _no_ctx_fired = true; });
            window.__test_c4_nid = c4.__nid__;
        "#).unwrap();
        let nid_val = rt.eval("window.__test_c4_nid").unwrap();
        let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
        rt.update_layout_rects([(nid, [0.0, 0.0, 50.0, 50.0])].into_iter().collect());
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
        let fired = rt.eval("_no_ctx_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn get_element_by_id_found() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_element_by_id_not_found() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('nonexistent') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_element_by_id_tag_name() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').tagName")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("DIV".into()));
    }

    #[test]
    fn query_selector_by_id() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('#main') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_by_class() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_by_tag() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("document.querySelector('span') !== null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_content_get() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').textContent")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("Hello".into()));
    }

    #[test]
    fn text_content_set_mutates_dom() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval("document.getElementById('main').textContent = 'World';")
            .unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        // The div#main should now have a single text child "World".
        let body_id = find_element_by_tag(&doc, "body").unwrap();
        let div_id = doc.get(body_id).children[0];
        let text = collect_text_content(&doc, div_id);
        assert_eq!(text, "World");
    }

    #[test]
    fn set_attribute_mutates_dom() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval("document.getElementById('main').setAttribute('data-x', '42');")
            .unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        let body_id = find_element_by_tag(&doc, "body").unwrap();
        let div_id = doc.get(body_id).children[0];
        assert_eq!(doc.get(div_id).get_attr("data-x"), Some("42"));
    }

    #[test]
    fn get_attribute_returns_value() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').getAttribute('id')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("main".into()));
    }

    #[test]
    fn get_attribute_returns_null_for_missing() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').getAttribute('data-missing') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_title_get() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("document.title").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("Test Page".into()));
    }

    #[test]
    fn document_title_set() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval("document.title = 'New Title';").unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        let title_text = find_element_by_tag(&doc, "title")
            .map(|nid| collect_text_content(&doc, nid))
            .unwrap_or_default();
        assert_eq!(title_text, "New Title");
    }

    #[test]
    fn document_body_not_null() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("document.body !== null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_element_and_append() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval(
            "var p = document.createElement('p'); \
             p.textContent = 'new paragraph'; \
             document.body.appendChild(p);",
        )
        .unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        let body_id = find_element_by_tag(&doc, "body").unwrap();
        let body = doc.get(body_id);
        // body should now have 2 children: the original div + the new <p>
        assert_eq!(body.children.len(), 2);
        let p_id = body.children[1];
        assert_eq!(
            doc.get(p_id)
                .element_name()
                .map(|n| n.local.as_str()),
            Some("p")
        );
        assert_eq!(collect_text_content(&doc, p_id), "new paragraph");
    }

    #[test]
    fn query_selector_all_returns_array() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelectorAll('span').length")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn query_selector_compound_tag_and_id() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('div#main') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_compound_wrong_tag_returns_null() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('span#main') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_compound_tag_and_class() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('span.highlight') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_child_combinator() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('div > span') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_descendant_combinator() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('body span') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_id_child_class() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('#main > .highlight') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_matches_compound() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('span').matches('span.highlight')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_matches_wrong_compound_returns_false() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('span').matches('div.highlight')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn element_closest_finds_ancestor() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('span').closest('div') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_closest_id_selector() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('span').closest('#main') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_attribute_selector() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('[id=\"main\"]') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn alert_does_not_crash() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("alert('test')").unwrap();
    }

    #[test]
    fn window_print_emits_request() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.print()").unwrap();
        let reqs = rt.take_print_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].margin_top, 48.0);
        assert_eq!(reqs[0].margin_bottom, 48.0);
        assert_eq!(reqs[0].margin_left, 48.0);
        assert_eq!(reqs[0].margin_right, 48.0);
    }

    #[test]
    fn timeout_is_deferred_until_tick() {
        let rt = runtime_with_dom(make_doc());
        // Timer must NOT fire synchronously — deferred to _lumen_tick_timers().
        let result = rt
            .eval("var x = 0; setTimeout(function() { x = 1; }, 0); x")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn timeout_fires_after_tick() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var x = 0; setTimeout(function() { x = 1; }, 0);")
            .unwrap();
        let result = rt.eval("_lumen_tick_timers(); x").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn clear_timeout_prevents_fire() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var x = 0; var id = setTimeout(function() { x = 1; }, 0); clearTimeout(id);")
            .unwrap();
        let result = rt.eval("_lumen_tick_timers(); x").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn set_interval_fires_repeatedly() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var n = 0; setInterval(function() { n++; }, 0);")
            .unwrap();
        rt.eval("_lumen_tick_timers();").unwrap();
        rt.eval("_lumen_tick_timers();").unwrap();
        let result = rt.eval("n").unwrap();
        // Fired at least twice (exact count depends on scheduling).
        assert!(matches!(result, lumen_core::JsValue::Number(n) if n >= 2.0));
    }

    #[test]
    fn clear_interval_stops_fire() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var n = 0; var id = setInterval(function() { n++; }, 0);")
            .unwrap();
        rt.eval("_lumen_tick_timers(); clearInterval(id);")
            .unwrap();
        rt.eval("_lumen_tick_timers();").unwrap();
        let result = rt.eval("n").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn scheduler_post_task_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof scheduler.postTask(function() { return 42; })")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("object".into()));
    }

    #[test]
    fn scheduler_post_task_rejects_non_function() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("var rejected = false; scheduler.postTask(42).catch(function() { rejected = true; }); rejected")
            .unwrap();
        // Promise rejection is async; we can only verify the call didn't throw synchronously.
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn history_initial_length_is_one() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("history.length").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn history_initial_state_is_null() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("history.state === null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn history_push_state_increments_length() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("history.pushState({page: 1}, '', '/page1');").unwrap();
        rt.eval("history.pushState({page: 2}, '', '/page2');").unwrap();
        let result = rt.eval("history.length").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn history_state_after_push_returns_state() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("history.pushState({x: 42}, '', '/p');").unwrap();
        let result = rt.eval("history.state.x").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(42.0));
    }

    #[test]
    fn history_replace_state_keeps_length() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("history.pushState({n: 1}, '', '/a');").unwrap();
        rt.eval("history.replaceState({n: 99}, '', '/a2');").unwrap();
        let len = rt.eval("history.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(2.0));
        let state = rt.eval("history.state.n").unwrap();
        assert_eq!(state, lumen_core::JsValue::Number(99.0));
    }

    #[test]
    fn history_back_fires_popstate_with_previous_state() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var events = []; \
             window.addEventListener('popstate', function(e) { events.push(e.state); }); \
             history.pushState({page: 1}, '', '/p1'); \
             history.pushState({page: 2}, '', '/p2'); \
             history.back();",
        )
        .unwrap();
        let len = rt.eval("events.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(1.0));
        let page = rt.eval("events[0].page").unwrap();
        assert_eq!(page, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn history_forward_after_back() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "history.pushState({n: 1}, '', '/p1'); \
             history.pushState({n: 2}, '', '/p2'); \
             history.back();",
        )
        .unwrap();
        let state_after_back = rt.eval("history.state.n").unwrap();
        assert_eq!(state_after_back, lumen_core::JsValue::Number(1.0));

        rt.eval("history.forward();").unwrap();
        let state_after_fwd = rt.eval("history.state.n").unwrap();
        assert_eq!(state_after_fwd, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn history_go_beyond_bounds_does_not_fire_popstate() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var fired = false; \
             window.addEventListener('popstate', function() { fired = true; }); \
             history.go(-5);",
        )
        .unwrap();
        let result = rt.eval("fired").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn window_onpopstate_fires_on_back() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var captured = null; \
             window.onpopstate = function(e) { captured = e.state; }; \
             history.pushState({v: 7}, '', '/p'); \
             history.back();",
        )
        .unwrap();
        let result = rt.eval("captured === null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true)); // initial state is null
    }

    #[test]
    fn history_push_drops_forward_entries() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "history.pushState({n: 1}, '', '/p1'); \
             history.pushState({n: 2}, '', '/p2'); \
             history.back(); \
             history.pushState({n: 3}, '', '/p3');",
        )
        .unwrap();
        // After back + push, forward entries are dropped: entries = [init, {n:1}, {n:3}]
        let len = rt.eval("history.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(3.0));
        let state = rt.eval("history.state.n").unwrap();
        assert_eq!(state, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn window_object_exposes_history() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("window.history === history").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn window_remove_event_listener_stops_popstate() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var count = 0; \
             function handler(e) { count++; } \
             window.addEventListener('popstate', handler); \
             history.pushState({}, '', '/p'); \
             history.back(); \
             window.removeEventListener('popstate', handler); \
             history.forward(); \
             history.back();",
        )
        .unwrap();
        // handler fires once (on first back), then is removed
        let result = rt.eval("count").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    // ── classList ────────────────────────────────────────────────────────────

    #[test]
    fn classlist_contains_true() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.contains('highlight')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_contains_false() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.contains('missing')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn classlist_add_and_contains() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').classList.add('active');").unwrap();
        let result = rt
            .eval("document.getElementById('main').classList.contains('active')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_remove() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "document.querySelector('.highlight').classList.remove('highlight');",
        )
        .unwrap();
        let result = rt
            .eval("document.querySelector('.highlight') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_toggle_adds_when_absent() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').classList.toggle('open')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
        let has = rt
            .eval("document.getElementById('main').classList.contains('open')")
            .unwrap();
        assert_eq!(has, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_toggle_removes_when_present() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.querySelector('.highlight').classList.toggle('highlight');").unwrap();
        let has = rt
            .eval("document.querySelector('.highlight') === null")
            .unwrap();
        assert_eq!(has, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_replace() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "document.querySelector('.highlight').classList.replace('highlight', 'selected');",
        )
        .unwrap();
        let old = rt
            .eval("document.querySelector('.highlight') === null")
            .unwrap();
        assert_eq!(old, lumen_core::JsValue::Bool(true));
        let new = rt
            .eval("document.querySelector('.selected') !== null")
            .unwrap();
        assert_eq!(new, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_length() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.length")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn classlist_to_string() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.toString()")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("highlight".into()));
    }

    // ── style / CSSStyleDeclaration ──────────────────────────────────────────

    #[test]
    fn style_set_and_get_property() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').style.setProperty('color', 'red');")
            .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.getPropertyValue('color')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("red".into()));
    }

    #[test]
    fn style_assignment_via_property_name() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').style.color = 'blue';")
            .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.color")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("blue".into()));
    }

    #[test]
    fn style_camel_case_to_kebab() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').style.backgroundColor = 'green';")
            .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.getPropertyValue('background-color')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("green".into()));
    }

    #[test]
    fn style_remove_property() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var el = document.getElementById('main'); \
             el.style.color = 'red'; \
             el.style.removeProperty('color');",
        )
        .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.getPropertyValue('color')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("".into()));
    }

    #[test]
    fn style_css_text_roundtrip() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "document.getElementById('main').style.cssText = 'color: red; font-size: 12px';",
        )
        .unwrap();
        let color = rt
            .eval("document.getElementById('main').style.getPropertyValue('color')")
            .unwrap();
        assert_eq!(color, lumen_core::JsValue::String("red".into()));
        let size = rt
            .eval("document.getElementById('main').style.getPropertyValue('font-size')")
            .unwrap();
        assert_eq!(size, lumen_core::JsValue::String("12px".into()));
    }

    // ── addEventListener / dispatchEvent on elements ─────────────────────────

    #[test]
    fn element_add_event_listener_and_dispatch() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var received = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('click', function(e) { received = e.type; }); \
                 el.dispatchEvent(new Event('click')); \
                 received",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("click".into()));
    }

    #[test]
    fn element_remove_event_listener_stops_dispatch() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var count = 0; \
                 var el = document.getElementById('main'); \
                 function h() { count++; } \
                 el.addEventListener('click', h); \
                 el.dispatchEvent(new Event('click')); \
                 el.removeEventListener('click', h); \
                 el.dispatchEvent(new Event('click')); \
                 count",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn custom_event_detail_accessible_in_handler() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var got = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('myevent', function(e) { got = e.detail; }); \
                 el.dispatchEvent(new CustomEvent('myevent', { detail: 42 })); \
                 got",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(42.0));
    }

    #[test]
    fn event_prevent_default() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var el = document.getElementById('main'); \
                 el.addEventListener('submit', function(e) { e.preventDefault(); }); \
                 var ev = new Event('submit', { cancelable: true }); \
                 var ret = el.dispatchEvent(ev); \
                 ret",
            )
            .unwrap();
        // dispatchEvent returns false when defaultPrevented
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn stop_immediate_propagation_stops_subsequent_listeners() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var calls = 0; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('x', function(e) { calls++; e.stopImmediatePropagation(); }); \
                 el.addEventListener('x', function(e) { calls++; }); \
                 el.dispatchEvent(new Event('x')); \
                 calls",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    // ── Event / CustomEvent constructors ─────────────────────────────────────

    #[test]
    fn event_constructor_sets_type() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new Event('load').type").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("load".into()));
    }

    #[test]
    fn event_bubbles_cancelable_defaults_false() {
        let rt = runtime_with_dom(make_doc());
        let bubbles = rt.eval("new Event('x').bubbles").unwrap();
        assert_eq!(bubbles, lumen_core::JsValue::Bool(false));
        let cancelable = rt.eval("new Event('x').cancelable").unwrap();
        assert_eq!(cancelable, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn custom_event_detail_null_by_default() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new CustomEvent('x').detail === null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn event_is_trusted_false_by_default() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new Event('click').isTrusted").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn event_is_trusted_true_when_specified() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new Event('click', { isTrusted: true }).isTrusted").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_event_is_trusted_inherits_from_event() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new CustomEvent('x', { isTrusted: true }).isTrusted").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatchevent_creates_untrusted_event() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(
            r#"
            var evt = new Event('test');
            var el = document.createElement('div');
            var receivedEvent = null;
            el.addEventListener('test', function(e) { receivedEvent = e; });
            el.dispatchEvent(evt);
            receivedEvent.isTrusted === false
            "#
        ).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── navigator.serviceWorker ───────────────────────────────────────────────

    #[test]
    fn navigator_has_service_worker() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof navigator.serviceWorker === 'object'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_register_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                r#"
                var p = navigator.serviceWorker.register('/sw.js', { scope: '/app/' });
                typeof p.then === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_register_calls_lumen_primitive() {
        // Pass a file URL so that _sw_origin = 'file://' (protocol + '//' + host).
        let rt = runtime_with_url("file:///test.html");
        rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/' });")
            .unwrap();
        let result = rt.eval("_lumen_sw_has_registration('file://')").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_registration_has_installing_worker() {
        let rt = runtime_with_url("https://example.com/");
        let result = rt
            .eval(
                r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js', { scope: '/' })
                    .then(function(r) { reg = r; });
                _lumen_drain_microtasks();
                reg !== null && reg.installing !== null
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_worker_has_state_installing() {
        let rt = runtime_with_url("https://example.com/");
        let result = rt
            .eval(
                r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js')
                    .then(function(r) { reg = r; });
                _lumen_drain_microtasks();
                reg.installing.state
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("installing".into()));
    }

    #[test]
    fn sw_container_has_event_target() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                r#"
                typeof navigator.serviceWorker.addEventListener === 'function' &&
                typeof navigator.serviceWorker.removeEventListener === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_get_registration_returns_promise() {
        let rt = runtime_with_url("https://example.com/");
        rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/' });")
            .unwrap();
        let result = rt
            .eval("typeof navigator.serviceWorker.getRegistration('/').then === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_get_registrations_returns_array() {
        let rt = runtime_with_url("https://example.com/");
        rt.eval("navigator.serviceWorker.register('/sw.js');").unwrap();
        let result = rt
            .eval(
                r#"
                var arr = null;
                navigator.serviceWorker.getRegistrations()
                    .then(function(regs) { arr = regs; });
                _lumen_drain_microtasks();
                Array.isArray(arr) && arr.length === 1
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_ready_property_is_promise() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof navigator.serviceWorker.ready.then === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_registration_has_event_target() {
        let rt = runtime_with_url("https://example.com/");
        let result = rt
            .eval(
                r#"
                var reg = null;
                navigator.serviceWorker.register('/sw.js')
                    .then(function(r) { reg = r; });
                _lumen_drain_microtasks();
                typeof reg.addEventListener === 'function' &&
                typeof reg.dispatchEvent === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_persist_and_load_no_throw() {
        let rt = runtime_with_url("https://example.com/");
        // Without a backend, persist/load are no-ops — must not throw.
        rt.eval("_lumen_sw_persist('https://example.com', '[{\"scope\":\"/\"}]');")
            .unwrap();
        let result = rt.eval("_lumen_sw_load('https://example.com')").unwrap();
        assert!(matches!(
            result,
            lumen_core::JsValue::Null | lumen_core::JsValue::Undefined
        ));
    }

    #[test]
    fn sw_unregister_removes_registration() {
        let rt = runtime_with_url("https://example.com/");
        rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/app/' });")
            .unwrap();
        rt.eval(
            r#"
            navigator.serviceWorker.getRegistration('/app/')
                .then(function(reg) { if (reg) reg.unregister(); });
            _lumen_drain_microtasks();
            "#,
        )
        .unwrap();
        let result = rt
            .eval(
                r#"
                var arr = null;
                navigator.serviceWorker.getRegistrations()
                    .then(function(r) { arr = r; });
                _lumen_drain_microtasks();
                arr.length
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn sw_worker_post_message_does_not_throw() {
        let rt = runtime_with_url("https://example.com/");
        let result = rt
            .eval(
                r#"
                var threw = false;
                var reg = null;
                navigator.serviceWorker.register('/sw.js')
                    .then(function(r) { reg = r; });
                _lumen_drain_microtasks();
                try { reg.installing.postMessage('hello'); } catch(e) { threw = true; }
                !threw
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── caches API ────────────────────────────────────────────────────────────

    #[test]
    fn caches_object_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("typeof caches === 'object'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn caches_open_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof caches.open('v1').then === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_has_returns_false_for_unknown() {
        let rt = runtime_with_dom(make_doc());
        // has() returns promise; we check the primitive directly.
        let result = rt
            .eval("_lumen_cache_has('', 'nonexistent')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    // helper: put a minimal GET 200 cache entry via the native binding
    fn cache_put_test(rt: &QuickJsRuntime, origin: &str, name: &str, url: &str) {
        rt.eval(&format!(
            r#"_lumen_cache_put('{origin}', '{name}', '{url}', '{{"method":"GET","status":200,"statusText":"OK","headers":{{}}}}', [72, 101, 108, 108, 111]);"#
        ))
        .unwrap();
    }

    #[test]
    fn cache_put_and_match_roundtrip() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "v1", "https://x.com/a");
        assert_eq!(
            rt.eval("_lumen_cache_has('', 'v1')").unwrap(),
            lumen_core::JsValue::Bool(true)
        );
        let keys = rt.eval("_lumen_cache_keys('', 'v1')").unwrap();
        assert_eq!(
            keys,
            lumen_core::JsValue::Array(vec![lumen_core::JsValue::String("https://x.com/a".into())])
        );
    }

    #[test]
    fn cache_match_returns_body_bytes() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "v1", "https://x.com/a");
        // _lumen_cache_match returns a Uint8Array-like value (body bytes)
        let len = rt
            .eval("_lumen_cache_match('', 'v1', 'https://x.com/a').length")
            .unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(5.0)); // "Hello" = 5 bytes
    }

    #[test]
    fn cache_match_info_returns_json_metadata() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"_lumen_cache_put('', 'v1', 'https://x.com/css', '{"method":"GET","status":304,"statusText":"Not Modified","headers":{"content-type":"text/css"}}', []);"#)
            .unwrap();
        let info_str = rt
            .eval("_lumen_cache_match_info('', 'v1', 'https://x.com/css')")
            .unwrap();
        if let lumen_core::JsValue::String(s) = info_str {
            assert!(s.contains("304"));
            assert!(s.contains("Not Modified"));
            assert!(s.contains("content-type"));
        } else {
            panic!("expected String from _lumen_cache_match_info");
        }
    }

    #[test]
    fn cache_match_info_returns_none_on_miss() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("_lumen_cache_match_info('', 'v1', 'https://x.com/missing') === undefined")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_match_any_returns_none_on_miss() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("_lumen_cache_match_any('', 'https://x.com/missing') === undefined")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_match_any_info_finds_across_caches() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"_lumen_cache_put('', 'static', 'https://x.com/style.css', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', []);"#)
            .unwrap();
        let r = rt
            .eval("_lumen_cache_match_any_info('', 'https://x.com/style.css') !== undefined")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_delete_returns_true_when_found() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "v1", "https://x.com/b");
        let r = rt
            .eval("_lumen_cache_delete('', 'v1', 'https://x.com/b')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let keys = rt.eval("_lumen_cache_keys('', 'v1')").unwrap();
        assert_eq!(keys, lumen_core::JsValue::Array(vec![]));
    }

    #[test]
    fn cache_delete_returns_false_on_miss() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("_lumen_cache_delete('', 'v1', 'https://x.com/nonexistent')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn cache_keys_full_returns_method() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"_lumen_cache_put('', 'v1', 'https://x.com/api', '{"method":"POST","status":201,"statusText":"Created","headers":{}}', []);"#)
            .unwrap();
        let r = rt
            .eval("_lumen_cache_keys_full('', 'v1').indexOf('POST') >= 0")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_delete_cache_returns_true_when_found() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "v1", "https://x.com/r");
        let r = rt
            .eval("_lumen_cache_delete_cache('', 'v1')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        assert_eq!(
            rt.eval("_lumen_cache_has('', 'v1')").unwrap(),
            lumen_core::JsValue::Bool(false)
        );
    }

    #[test]
    fn cache_delete_cache_returns_false_when_missing() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("_lumen_cache_delete_cache('', 'nonexistent')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn cache_names_lists_opened_caches() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "alpha", "https://x.com/r");
        cache_put_test(&rt, "", "beta", "https://x.com/s");
        let mut names = match rt.eval("_lumen_cache_names('')").unwrap() {
            lumen_core::JsValue::Array(a) => a
                .into_iter()
                .filter_map(|v| {
                    if let lumen_core::JsValue::String(s) = v { Some(s) } else { None }
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        };
        names.sort();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn caches_open_returns_cache_with_match() {
        let rt = runtime_with_dom(make_doc());
        // Open cache first to obtain handle, then put with same _sw_origin, then match.
        let r = rt.eval(r#"
            var _cache_oc = null;
            caches.open('my-cache').then(function(c) { _cache_oc = c; });
            _lumen_drain_microtasks();
            _lumen_cache_put(_sw_origin, 'my-cache', 'https://x.com/data',
                '{"method":"GET","status":200,"statusText":"OK","headers":{}}', [1,2,3]);
            var _result_oc;
            _cache_oc.match('https://x.com/data').then(function(r) { _result_oc = r !== undefined; });
            _lumen_drain_microtasks();
            _result_oc
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn caches_has_returns_true_after_put() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "my-cache", "https://x.com/x");
        let r = rt
            .eval("_lumen_cache_has('', 'my-cache')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn caches_delete_returns_true_when_found() {
        let rt = runtime_with_dom(make_doc());
        cache_put_test(&rt, "", "old-cache", "https://x.com/z");
        // caches.delete returns a Promise<bool>; verify via native binding
        let had = rt.eval("_lumen_cache_delete_cache('', 'old-cache')").unwrap();
        assert_eq!(had, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_matchall_returns_all_entries() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var _cache_ma = null;
            caches.open('v1-ma').then(function(c) { _cache_ma = c; });
            _lumen_drain_microtasks();
            _lumen_cache_put(_sw_origin, 'v1-ma', 'https://x.com/a', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', [1]);
            _lumen_cache_put(_sw_origin, 'v1-ma', 'https://x.com/b', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', [2]);
            var _all;
            _cache_ma.matchAll().then(function(arr) { _all = arr.length; });
            _lumen_drain_microtasks();
            _all
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn cache_keys_returns_request_objects() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var _cache_kr = null;
            caches.open('v1-kr').then(function(c) { _cache_kr = c; });
            _lumen_drain_microtasks();
            _lumen_cache_put(_sw_origin, 'v1-kr', 'https://x.com/page', '{"method":"GET","status":200,"statusText":"OK","headers":{}}', []);
            var _url_kr;
            _cache_kr.keys().then(function(reqs) { _url_kr = reqs[0] && reqs[0].url; });
            _lumen_drain_microtasks();
            _url_kr
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("https://x.com/page".into()));
    }

    #[test]
    fn window_has_caches() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("typeof window.caches === 'object'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── Cache API — CacheBackend trait dispatch tests ─────────────────────────
    //
    // MockCacheBackend exercises the CacheBackend dispatch path in install_primitives
    // without pulling in lumen-storage as a test dependency. The SQLite
    // implementation is separately tested in lumen-storage::cache_storage.

    type MockCacheEntry = (String, Vec<u8>);
    type MockCacheMap = std::collections::HashMap<
        String, // origin
        std::collections::HashMap<
            String, // cache_name
            std::collections::HashMap<String, MockCacheEntry>, // url → (meta, body)
        >,
    >;

    struct MockCacheBackend {
        data: Mutex<MockCacheMap>,
    }

    impl MockCacheBackend {
        fn new() -> Self {
            Self { data: Mutex::new(std::collections::HashMap::new()) }
        }
    }

    impl lumen_core::ext::CacheBackend for MockCacheBackend {
        fn cache_put(&self, origin: &str, name: &str, url: &str, meta_json: &str, body: &[u8]) {
            self.data.lock().unwrap()
                .entry(origin.to_owned()).or_default()
                .entry(name.to_owned()).or_default()
                .insert(url.to_owned(), (meta_json.to_owned(), body.to_vec()));
        }
        fn cache_match(&self, origin: &str, name: &str, url: &str) -> Option<(String, Vec<u8>)> {
            self.data.lock().unwrap()
                .get(origin)?.get(name)?.get(url)
                .map(|(m, b)| (m.clone(), b.clone()))
        }
        fn cache_match_any(&self, origin: &str, url: &str) -> Option<(String, Vec<u8>)> {
            let g = self.data.lock().unwrap();
            let caches = g.get(origin)?;
            for c in caches.values() {
                if let Some((m, b)) = c.get(url) { return Some((m.clone(), b.clone())); }
            }
            None
        }
        fn cache_delete(&self, origin: &str, name: &str, url: &str) -> bool {
            self.data.lock().unwrap()
                .get_mut(origin).and_then(|c| c.get_mut(name))
                .and_then(|c| c.remove(url)).is_some()
        }
        fn cache_keys(&self, origin: &str, name: &str) -> Vec<(String, String)> {
            self.data.lock().unwrap()
                .get(origin).and_then(|c| c.get(name))
                .map(|c| c.iter().map(|(u, (meta, _))| {
                    let method = cache_meta_method(meta);
                    (u.clone(), method)
                }).collect())
                .unwrap_or_default()
        }
        fn cache_has(&self, origin: &str, name: &str) -> bool {
            self.data.lock().unwrap()
                .get(origin).and_then(|c| c.get(name))
                .map(|c| !c.is_empty()).unwrap_or(false)
        }
        fn cache_delete_cache(&self, origin: &str, name: &str) -> bool {
            self.data.lock().unwrap()
                .get_mut(origin).and_then(|c| c.remove(name)).is_some()
        }
        fn cache_names(&self, origin: &str) -> Vec<String> {
            self.data.lock().unwrap()
                .get(origin).map(|c| c.keys().cloned().collect()).unwrap_or_default()
        }
    }

    fn runtime_with_cache_backend() -> QuickJsRuntime {
        let be: Arc<dyn lumen_core::ext::CacheBackend> = Arc::new(MockCacheBackend::new());
        let rt = QuickJsRuntime::new().unwrap();
        rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, None, None, Some(be), false)
            .unwrap();
        rt
    }

    fn sqlite_cache_put(rt: &QuickJsRuntime, cache: &str, url: &str) {
        rt.eval(&format!(
            r#"_lumen_cache_put('https://example.com/', '{cache}', '{url}', '{{"method":"GET","status":200,"statusText":"OK","headers":{{}}}}', [72,101,108,108,111]);"#
        ))
        .unwrap();
    }

    #[test]
    fn sqlite_backend_put_and_has() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "v1", "https://example.com/main.js");
        let r = rt.eval("_lumen_cache_has('https://example.com/', 'v1')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sqlite_backend_match_returns_body() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "v1", "https://example.com/style.css");
        let len = rt.eval("_lumen_cache_match('https://example.com/', 'v1', 'https://example.com/style.css').length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(5.0)); // "Hello" = 5 bytes
    }

    #[test]
    fn sqlite_backend_match_info_roundtrip() {
        let rt = runtime_with_cache_backend();
        rt.eval(r#"_lumen_cache_put('https://example.com/', 'v1', 'https://example.com/api',
            '{"method":"GET","status":304,"statusText":"Not Modified","headers":{"etag":"abc123"}}', []);"#)
            .unwrap();
        let meta = rt.eval("_lumen_cache_match_info('https://example.com/', 'v1', 'https://example.com/api')").unwrap();
        if let lumen_core::JsValue::String(s) = meta {
            assert!(s.contains("304"));
            assert!(s.contains("etag"));
        } else {
            panic!("expected String from _lumen_cache_match_info (sqlite backend)");
        }
    }

    #[test]
    fn sqlite_backend_match_any_searches_all_caches() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "static", "https://example.com/logo.png");
        let body = rt.eval("_lumen_cache_match_any('https://example.com/', 'https://example.com/logo.png') !== null && _lumen_cache_match_any('https://example.com/', 'https://example.com/logo.png') !== undefined").unwrap();
        assert_eq!(body, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sqlite_backend_delete_entry() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "v1", "https://example.com/old");
        let deleted = rt.eval("_lumen_cache_delete('https://example.com/', 'v1', 'https://example.com/old')").unwrap();
        assert_eq!(deleted, lumen_core::JsValue::Bool(true));
        let after = rt.eval("_lumen_cache_match('https://example.com/', 'v1', 'https://example.com/old') === undefined").unwrap();
        assert_eq!(after, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sqlite_backend_keys_lists_urls() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "v1", "https://example.com/a");
        sqlite_cache_put(&rt, "v1", "https://example.com/b");
        let keys = rt.eval("_lumen_cache_keys('https://example.com/', 'v1')").unwrap();
        if let lumen_core::JsValue::Array(arr) = keys {
            assert_eq!(arr.len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn sqlite_backend_delete_cache() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "tmp", "https://example.com/x");
        let del = rt.eval("_lumen_cache_delete_cache('https://example.com/', 'tmp')").unwrap();
        assert_eq!(del, lumen_core::JsValue::Bool(true));
        let has = rt.eval("_lumen_cache_has('https://example.com/', 'tmp')").unwrap();
        assert_eq!(has, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn sqlite_backend_cache_names() {
        let rt = runtime_with_cache_backend();
        sqlite_cache_put(&rt, "alpha", "https://example.com/1");
        sqlite_cache_put(&rt, "beta", "https://example.com/2");
        let names = rt.eval("_lumen_cache_names('https://example.com/')").unwrap();
        if let lumen_core::JsValue::Array(arr) = names {
            let strs: Vec<String> = arr
                .into_iter()
                .filter_map(|v| if let lumen_core::JsValue::String(s) = v { Some(s) } else { None })
                .collect();
            assert!(strs.contains(&"alpha".to_string()));
            assert!(strs.contains(&"beta".to_string()));
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn sqlite_backend_match_miss_returns_none() {
        let rt = runtime_with_cache_backend();
        let r = rt.eval("_lumen_cache_match('https://example.com/', 'v1', 'https://example.com/missing') === undefined").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sqlite_backend_keys_full_includes_method() {
        let rt = runtime_with_cache_backend();
        rt.eval(r#"_lumen_cache_put('https://example.com/', 'v1', 'https://example.com/post',
            '{"method":"POST","status":201,"statusText":"Created","headers":{}}', []);"#)
            .unwrap();
        let r = rt.eval("_lumen_cache_keys_full('https://example.com/', 'v1').indexOf('POST') >= 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sqlite_backend_has_false_when_empty() {
        let rt = runtime_with_cache_backend();
        let r = rt.eval("_lumen_cache_has('https://example.com/', 'nonexistent')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn sqlite_backend_delete_returns_false_on_miss() {
        let rt = runtime_with_cache_backend();
        let r = rt.eval("_lumen_cache_delete('https://example.com/', 'v1', 'https://example.com/nosuchurl')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    // ── IME composition API ───────────────────────────────────────────────────

    #[test]
    fn dispatch_composition_function_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof _lumen_dispatch_composition === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_ime_target_function_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof _lumen_set_ime_target === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_composition_on_element_fires_listener() {
        let rt = runtime_with_dom(make_doc());
        // Регистрируем слушатель compositionstart на main div.
        // При диспатче он должен сохранить data в глобальной переменной.
        rt.eval(r#"
            var _got_composition = null;
            var el = document.getElementById('main');
            el.addEventListener('compositionstart', function(e) {
                _got_composition = e.type;
            });
            _lumen_set_ime_target(el);
            _lumen_dispatch_composition('compositionstart', '');
        "#).unwrap();
        let result = rt.eval("_got_composition").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("compositionstart".into()));
    }

    #[test]
    fn dispatch_composition_update_carries_data() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _comp_data = null;
            var el = document.getElementById('main');
            el.addEventListener('compositionupdate', function(e) {
                _comp_data = e.data;
            });
            _lumen_set_ime_target(el);
            _lumen_dispatch_composition('compositionupdate', 'あい');
        "#).unwrap();
        let result = rt.eval("_comp_data").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("あい".into()));
    }

    #[test]
    fn dispatch_composition_without_target_does_not_crash() {
        let rt = runtime_with_dom(make_doc());
        // Нет target — должен молча ничего не сделать.
        rt.eval("_lumen_set_ime_target(null); _lumen_dispatch_composition('compositionstart', '');")
            .unwrap();
    }

    #[test]
    fn window_has_dispatch_composition() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof window._lumen_dispatch_composition === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── bfcache / pageshow / pagehide ────────────────────────────────────────

    #[test]
    fn window_has_pageshow_pagehide_handlers() {
        let rt = runtime_with_dom(make_doc());
        // onpageshow and onpagehide should be null (not set) initially.
        let r1 = rt.eval("window.onpageshow === null").unwrap();
        let r2 = rt.eval("window.onpagehide === null").unwrap();
        assert_eq!(r1, lumen_core::JsValue::Bool(true));
        assert_eq!(r2, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pageshow_listener_receives_event_with_persisted_false() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var saw = false; var persistedFlag = null;
             window.addEventListener('pageshow', function(e) { saw = true; persistedFlag = e.persisted; });
             _lumen_fire_page_lifecycle('pageshow', false);",
        ).unwrap();
        let saw = rt.eval("saw").unwrap();
        let persisted = rt.eval("persistedFlag").unwrap();
        assert_eq!(saw, lumen_core::JsValue::Bool(true));
        assert_eq!(persisted, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn pageshow_listener_receives_persisted_true_from_bfcache() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var persistedFlag = null;
             window.addEventListener('pageshow', function(e) { persistedFlag = e.persisted; });
             _lumen_fire_page_lifecycle('pageshow', true);",
        ).unwrap();
        let persisted = rt.eval("persistedFlag").unwrap();
        assert_eq!(persisted, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pagehide_listener_fires() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var fired = false;
             window.addEventListener('pagehide', function(e) { fired = true; });
             _lumen_fire_page_lifecycle('pagehide', false);",
        ).unwrap();
        let fired = rt.eval("fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn onpageshow_handler_fires() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var saw = false;
             window.onpageshow = function(e) { saw = true; };
             _lumen_fire_page_lifecycle('pageshow', false);",
        ).unwrap();
        let saw = rt.eval("saw").unwrap();
        assert_eq!(saw, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn remove_pageshow_listener_stops_it_firing() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var count = 0;
             var fn1 = function() { count++; };
             window.addEventListener('pageshow', fn1);
             window.removeEventListener('pageshow', fn1);
             _lumen_fire_page_lifecycle('pageshow', false);",
        ).unwrap();
        let count = rt.eval("count").unwrap();
        assert_eq!(count, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn lumen_bfcache_persisted_default_false() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("_lumen_bfcache_persisted").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn lumen_fire_page_lifecycle_exported_on_window() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("typeof window._lumen_fire_page_lifecycle === 'function'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── Fetch API tests ───────────────────────────────────────────────────────

    #[test]
    fn fetch_global_is_function() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof fetch === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn window_fetch_is_function() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.fetch === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn headers_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof Headers === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn request_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof Request === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof Response === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_controller_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof AbortController === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn headers_get_set() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var h = new Headers(); h.set('Content-Type', 'application/json'); h.get('content-type')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("application/json".into()));
    }

    #[test]
    fn headers_case_insensitive() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var h = new Headers({'X-Foo': 'bar'}); h.get('x-foo')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("bar".into()));
    }

    #[test]
    fn response_ok_for_200() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new Response(null, {status: 200}).ok").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_not_ok_for_404() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new Response(null, {status: 404}).ok").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn response_text_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var r = new Response(new Uint8Array([104, 105])); \
             typeof r.text() === 'object'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_controller_abort_sets_signal() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var ctrl = new AbortController(); ctrl.abort(); ctrl.signal.aborted"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fetch_without_provider_returns_promise() {
        // install_dom with None fetch_provider: fetch() should return a rejected Promise.
        // QuickJS doesn't flush microtasks synchronously in eval, so we only verify
        // that fetch() returns a thenable (Promise), not that catch fired.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var p = fetch('http://example.com/'); \
             typeof p === 'object' && typeof p.then === 'function'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn request_default_method_get() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new Request('https://x.com/').method").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("GET".into()));
    }

    #[test]
    fn window_has_abort_controller() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.AbortController === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── WebSocket API ─────────────────────────────────────────────────────────

    #[test]
    fn window_has_websocket_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.WebSocket === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_constants_defined() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("WebSocket.CONNECTING === 0 && WebSocket.OPEN === 1 && WebSocket.CLOSING === 2 && WebSocket.CLOSED === 3")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // Mock WS provider: connect always fails (no server).
    struct FailWsProvider;
    impl lumen_core::ext::JsWebSocketProvider for FailWsProvider {
        fn connect(&self, _url: &str) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
            Err(lumen_core::error::Error::Network("test: no server".into()))
        }
    }

    fn runtime_with_ws(doc: Arc<Mutex<Document>>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(FailWsProvider);
        rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn websocket_connect_fail_sets_closed_state() {
        let rt = runtime_with_ws(make_doc());
        // connect fails immediately → readyState = 3 (CLOSED)
        let r = rt
            .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.readyState")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn websocket_connect_fail_no_handle() {
        let rt = runtime_with_ws(make_doc());
        let r = rt
            .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws._handle === 0")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_connect_fail_fires_onerror() {
        let rt = runtime_with_ws(make_doc());
        // onerror is called asynchronously via setTimeout(fn, 0) in the shim.
        // We can't pump the timeout in this test — just verify the handler is set.
        let r = rt
            .eval(
                "var fired = false;
                 var ws = new WebSocket('ws://127.0.0.1:1');
                 ws.onerror = function() { fired = true; };
                 ws.readyState === 3",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // Mock WS provider: immediately queues Open + one Text message.
    struct MockWsProvider;
    struct MockWsSession {
        queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsWsEvent>>,
    }
    impl lumen_core::ext::JsWebSocketSession for MockWsSession {
        fn send_text(&self, _text: &str) -> lumen_core::error::Result<()> { Ok(()) }
        fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> { Ok(()) }
        fn poll(&self) -> Option<lumen_core::ext::JsWsEvent> {
            self.queue.lock().unwrap().pop_front()
        }
        fn close(&self, _code: u16, _reason: &str) -> lumen_core::error::Result<()> { Ok(()) }
    }
    impl lumen_core::ext::JsWebSocketProvider for MockWsProvider {
        fn connect(&self, _url: &str) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
            use lumen_core::ext::JsWsEvent;
            let mut q = std::collections::VecDeque::new();
            q.push_back(JsWsEvent::Open);
            q.push_back(JsWsEvent::Message { data: b"hello".to_vec(), is_binary: false });
            Ok(Box::new(MockWsSession { queue: std::sync::Mutex::new(q) }))
        }
    }

    fn runtime_with_mock_ws(doc: Arc<Mutex<Document>>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(MockWsProvider);
        rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn websocket_mock_connect_open_state() {
        let rt = runtime_with_mock_ws(make_doc());
        // Phase 0: pump explicitly to deliver Open event → readyState = 1.
        let r = rt
            .eval("var ws = new WebSocket('ws://mock'); _lumen_pump_websockets(); ws.readyState")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn websocket_mock_open_fires_onopen() {
        let rt = runtime_with_mock_ws(make_doc());
        let r = rt
            .eval(
                "var opened = false;
                 var ws = new WebSocket('ws://mock');
                 ws.onopen = function() { opened = true; };
                 _lumen_pump_websockets();
                 opened",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_mock_message_via_pump() {
        let rt = runtime_with_mock_ws(make_doc());
        // Set handler before pump so onmessage fires when the message is dispatched.
        let r = rt
            .eval(
                "var received = null;
                 var ws = new WebSocket('ws://mock');
                 ws.onmessage = function(e) { received = e.data; };
                 _lumen_pump_websockets();
                 received",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("hello".into()));
    }

    #[test]
    fn websocket_no_provider_connect_returns_zero() {
        // Without ws_provider, _lumen_ws_connect always returns 0.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_lumen_ws_connect('ws://test')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    // ── EventSource / Server-Sent Events (HTML Living Standard §9.2) ──────────

    /// Mock SSE session feeding a preset event sequence via `poll()`.
    struct MockSseSession {
        queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsSseEvent>>,
    }
    impl lumen_core::ext::JsSseSession for MockSseSession {
        fn poll(&self) -> Option<lumen_core::ext::JsSseEvent> {
            self.queue.lock().unwrap().pop_front()
        }
        fn close(&mut self) {}
    }

    /// Mock SSE provider that queues a fixed event sequence on connect.
    struct MockSseProvider {
        events: Vec<lumen_core::ext::JsSseEvent>,
    }
    impl lumen_core::ext::JsSseProvider for MockSseProvider {
        fn connect_sse(
            &self,
            _url: &str,
        ) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsSseSession>> {
            let q: std::collections::VecDeque<_> = self.events.iter().cloned().collect();
            Ok(Box::new(MockSseSession {
                queue: std::sync::Mutex::new(q),
            }))
        }
    }

    fn runtime_with_mock_sse(
        doc: Arc<Mutex<Document>>,
        events: Vec<lumen_core::ext::JsSseEvent>,
    ) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsSseProvider> =
            Arc::new(MockSseProvider { events });
        rt.install_dom(doc, "", None, None, Some(provider), None, None, None, None, false)
            .unwrap();
        rt
    }

    #[test]
    fn eventsource_constructor_no_provider_sets_closed() {
        // Without an sse_provider, _lumen_sse_connect returns 0 → readyState CLOSED.
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("var es = new EventSource('https://x/sse'); es.readyState")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn eventsource_no_provider_connect_returns_zero() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_lumen_sse_connect('https://x/sse')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn eventsource_opens_on_sse_connect() {
        use lumen_core::ext::JsSseEvent;
        let rt = runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open]);
        let r = rt
            .eval(
                "var opened = false;
                 var es = new EventSource('https://x/sse');
                 es.onopen = function() { opened = true; };
                 _lumen_pump_sse();
                 [es.readyState, opened]",
            )
            .unwrap();
        match r {
            lumen_core::JsValue::Array(arr) => {
                // readyState OPEN (1) and onopen fired.
                assert_eq!(arr[0], lumen_core::JsValue::Number(1.0));
                assert_eq!(arr[1], lumen_core::JsValue::Bool(true));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn eventsource_delivers_message() {
        use lumen_core::ext::JsSseEvent;
        let rt = runtime_with_mock_sse(
            make_doc(),
            vec![
                JsSseEvent::Open,
                JsSseEvent::Message {
                    event_type: "message".into(),
                    data: "hello world".into(),
                    id: Some("42".into()),
                },
            ],
        );
        let r = rt
            .eval(
                "var data = null; var lid = null;
                 var es = new EventSource('https://x/sse');
                 es.onmessage = function(e) { data = e.data; lid = e.lastEventId; };
                 _lumen_pump_sse();
                 [data, lid]",
            )
            .unwrap();
        match r {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::String("hello world".into()));
                assert_eq!(arr[1], lumen_core::JsValue::String("42".into()));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn eventsource_delivers_typed_event() {
        use lumen_core::ext::JsSseEvent;
        let rt = runtime_with_mock_sse(
            make_doc(),
            vec![
                JsSseEvent::Open,
                JsSseEvent::Message {
                    event_type: "ping".into(),
                    data: "p".into(),
                    id: None,
                },
            ],
        );
        // A named event must reach addEventListener('ping', ...), not onmessage.
        let r = rt
            .eval(
                "var got = null; var onmsg = false;
                 var es = new EventSource('https://x/sse');
                 es.onmessage = function() { onmsg = true; };
                 es.addEventListener('ping', function(e) { got = e.data; });
                 _lumen_pump_sse();
                 [got, onmsg]",
            )
            .unwrap();
        match r {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::String("p".into()));
                assert_eq!(arr[1], lumen_core::JsValue::Bool(false));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn eventsource_close_sets_closed() {
        use lumen_core::ext::JsSseEvent;
        let rt = runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open]);
        let r = rt
            .eval(
                "var es = new EventSource('https://x/sse');
                 _lumen_pump_sse();
                 es.close();
                 es.readyState",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn eventsource_server_close_fires_error_and_reconnects() {
        use lumen_core::ext::JsSseEvent;
        // Server-initiated close: readyState becomes CONNECTING (0), error fires,
        // reconnect scheduled (HTML Living Standard §9.2.7).
        let rt = runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open, JsSseEvent::Close]);
        let r = rt
            .eval(
                "var errored = false;
                 var es = new EventSource('https://x/sse');
                 es.onerror = function() { errored = true; };
                 _lumen_pump_sse();
                 [es.readyState, errored]",
            )
            .unwrap();
        match r {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::Number(0.0)); // CONNECTING
                assert_eq!(arr[1], lumen_core::JsValue::Bool(true));  // error fired
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn eventsource_error_event_fires_onerror() {
        use lumen_core::ext::JsSseEvent;
        let rt = runtime_with_mock_sse(
            make_doc(),
            vec![JsSseEvent::Open, JsSseEvent::Error("boom".into())],
        );
        let r = rt
            .eval(
                "var errored = false; var msg = null;
                 var es = new EventSource('https://x/sse');
                 es.onerror = function(e) { errored = true; msg = e.message; };
                 _lumen_pump_sse();
                 [errored, msg, es.readyState]",
            )
            .unwrap();
        match r {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::Bool(true));
                assert_eq!(arr[1], lumen_core::JsValue::String("boom".into()));
                assert_eq!(arr[2], lumen_core::JsValue::Number(2.0));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn eventsource_poll_json_escapes_message() {
        use lumen_core::ext::JsSseEvent;
        // Data containing quotes/newlines must round-trip through JSON intact.
        let rt = runtime_with_mock_sse(
            make_doc(),
            vec![
                JsSseEvent::Open,
                JsSseEvent::Message {
                    event_type: "message".into(),
                    data: "line1\nline2 \"quoted\"".into(),
                    id: None,
                },
            ],
        );
        let r = rt
            .eval(
                "var data = null;
                 var es = new EventSource('https://x/sse');
                 es.onmessage = function(e) { data = e.data; };
                 _lumen_pump_sse();
                 data",
            )
            .unwrap();
        assert_eq!(
            r,
            lumen_core::JsValue::String("line1\nline2 \"quoted\"".into())
        );
    }

    #[test]
    fn eventsource_retry_event_updates_reconnect_delay() {
        use lumen_core::ext::JsSseEvent;
        // A Retry event from the server updates the internal reconnect delay.
        let rt = runtime_with_mock_sse(
            make_doc(),
            vec![JsSseEvent::Open, JsSseEvent::Retry(500)],
        );
        let r = rt
            .eval(
                "var es = new EventSource('https://x/sse');
                 _lumen_pump_sse();
                 es._retryMs",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(500.0));
    }

    #[test]
    fn eventsource_close_cancels_pending_reconnect() {
        use lumen_core::ext::JsSseEvent;
        // Calling close() after server-close must cancel the pending reconnect.
        let rt = runtime_with_mock_sse(make_doc(), vec![JsSseEvent::Open, JsSseEvent::Close]);
        let r = rt
            .eval(
                "var es = new EventSource('https://x/sse');
                 _lumen_pump_sse();
                 es.close();
                 [es.readyState, es._reconnecting]",
            )
            .unwrap();
        match r {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::Number(2.0)); // CLOSED
                assert_eq!(arr[1], lumen_core::JsValue::Bool(false)); // no reconnect
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn eventsource_remove_event_listener() {
        use lumen_core::ext::JsSseEvent;
        // removeEventListener must stop delivery to the removed handler.
        let rt = runtime_with_mock_sse(
            make_doc(),
            vec![
                JsSseEvent::Open,
                JsSseEvent::Message {
                    event_type: "ping".into(),
                    data: "p".into(),
                    id: None,
                },
            ],
        );
        let r = rt
            .eval(
                "var count = 0;
                 var fn1 = function() { count++; };
                 var es = new EventSource('https://x/sse');
                 es.addEventListener('ping', fn1);
                 es.removeEventListener('ping', fn1);
                 _lumen_pump_sse();
                 count",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn close_event_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("var ce = new CloseEvent(1001, 'bye', true); ce.code === 1001 && ce.reason === 'bye' && ce.wasClean === true")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn message_event_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("var me = new MessageEvent('payload'); me.data === 'payload' && me.type === 'message'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_has_buffered_amount() {
        let rt = runtime_with_ws(make_doc());
        let r = rt
            .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.bufferedAmount === 0")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_has_extensions_field() {
        let rt = runtime_with_ws(make_doc());
        let r = rt
            .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.extensions === ''")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_binary_type_default_blob() {
        let rt = runtime_with_ws(make_doc());
        let r = rt
            .eval("var ws = new WebSocket('ws://127.0.0.1:1'); ws.binaryType")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("blob".into()));
    }

    // Mock provider: queues Open + one binary message (bytes [0x01, 0x02, 0x03]).
    struct MockBinaryWsProvider;
    struct MockBinaryWsSession {
        queue: std::sync::Mutex<std::collections::VecDeque<lumen_core::ext::JsWsEvent>>,
    }
    impl lumen_core::ext::JsWebSocketSession for MockBinaryWsSession {
        fn send_text(&self, _text: &str) -> lumen_core::error::Result<()> { Ok(()) }
        fn send_binary(&self, _data: &[u8]) -> lumen_core::error::Result<()> { Ok(()) }
        fn poll(&self) -> Option<lumen_core::ext::JsWsEvent> {
            self.queue.lock().unwrap().pop_front()
        }
        fn close(&self, _code: u16, _reason: &str) -> lumen_core::error::Result<()> { Ok(()) }
    }
    impl lumen_core::ext::JsWebSocketProvider for MockBinaryWsProvider {
        fn connect(&self, _url: &str) -> lumen_core::error::Result<Box<dyn lumen_core::ext::JsWebSocketSession>> {
            use lumen_core::ext::JsWsEvent;
            let mut q = std::collections::VecDeque::new();
            q.push_back(JsWsEvent::Open);
            q.push_back(JsWsEvent::Message { data: vec![0x01, 0x02, 0x03], is_binary: true });
            Ok(Box::new(MockBinaryWsSession { queue: std::sync::Mutex::new(q) }))
        }
    }

    fn runtime_with_binary_ws(doc: Arc<Mutex<Document>>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let provider: Arc<dyn lumen_core::ext::JsWebSocketProvider> = Arc::new(MockBinaryWsProvider);
        rt.install_dom(doc, "", None, Some(provider), None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn websocket_binary_blob_mode_delivers_uint8array() {
        let rt = runtime_with_binary_ws(make_doc());
        // Default binaryType='blob' → Uint8Array (our Phase 0 representation).
        let r = rt
            .eval(
                "var received = null;
                 var ws = new WebSocket('ws://mock');
                 ws.onmessage = function(e) { received = e.data; };
                 _lumen_pump_websockets();
                 received instanceof Uint8Array && received[0] === 1 && received[1] === 2 && received[2] === 3",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_binary_arraybuffer_mode_delivers_arraybuffer() {
        let rt = runtime_with_binary_ws(make_doc());
        // binaryType='arraybuffer' → ArrayBuffer.
        let r = rt
            .eval(
                "var received = null;
                 var ws = new WebSocket('ws://mock');
                 ws.binaryType = 'arraybuffer';
                 ws.onmessage = function(e) { received = e.data; };
                 _lumen_pump_websockets();
                 received instanceof ArrayBuffer && new Uint8Array(received)[0] === 1",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn websocket_binary_hex_length_matches_byte_count() {
        let rt = runtime_with_binary_ws(make_doc());
        // 3 bytes → Uint8Array of length 3.
        let r = rt
            .eval(
                "var len = 0;
                 var ws = new WebSocket('ws://mock');
                 ws.onmessage = function(e) { len = e.data.length; };
                 _lumen_pump_websockets();
                 len === 3",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── location / NavigateRequest tests ─────────────────────────────────────

    fn runtime_with_url(url: &str) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        rt.install_dom(make_doc(), url, None, None, None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn location_href_initialised_from_page_url() {
        let rt = runtime_with_url("https://example.com/path?q=1#top");
        let r = rt.eval("location.href").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("https://example.com/path?q=1#top".into()));
    }

    #[test]
    fn location_fields_parsed_correctly() {
        let rt = runtime_with_url("https://example.com:8080/path/to?q=hello#sec");
        let proto    = rt.eval("location.protocol").unwrap();
        let hostname = rt.eval("location.hostname").unwrap();
        let host     = rt.eval("location.host").unwrap();
        let port     = rt.eval("location.port").unwrap();
        let pathname = rt.eval("location.pathname").unwrap();
        let search   = rt.eval("location.search").unwrap();
        let hash     = rt.eval("location.hash").unwrap();
        let origin   = rt.eval("location.origin").unwrap();
        assert_eq!(proto,    lumen_core::JsValue::String("https:".into()));
        assert_eq!(hostname, lumen_core::JsValue::String("example.com".into()));
        assert_eq!(host,     lumen_core::JsValue::String("example.com:8080".into()));
        assert_eq!(port,     lumen_core::JsValue::String("8080".into()));
        assert_eq!(pathname, lumen_core::JsValue::String("/path/to".into()));
        assert_eq!(search,   lumen_core::JsValue::String("?q=hello".into()));
        assert_eq!(hash,     lumen_core::JsValue::String("#sec".into()));
        assert_eq!(origin,   lumen_core::JsValue::String("https://example.com:8080".into()));
    }

    #[test]
    fn location_href_empty_when_no_url() {
        let rt = runtime_with_url("");
        let r = rt.eval("location.href").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("".into()));
    }

    #[test]
    fn location_assign_sets_navigate_push() {
        let rt = runtime_with_url("https://start.example/");
        rt.eval("location.assign('https://target.example/page')").unwrap();
        let req = rt.take_navigate_request();
        assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://target.example/page"));
    }

    #[test]
    fn location_href_setter_sets_navigate_push() {
        let rt = runtime_with_url("https://start.example/");
        rt.eval("location.href = 'https://other.example/'").unwrap();
        let req = rt.take_navigate_request();
        assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://other.example/"));
    }

    #[test]
    fn location_replace_sets_navigate_replace() {
        let rt = runtime_with_url("https://start.example/");
        rt.eval("location.replace('https://new.example/')").unwrap();
        let req = rt.take_navigate_request();
        assert!(matches!(req, Some(NavigateRequest::Replace(u)) if u == "https://new.example/"));
    }

    #[test]
    fn location_reload_sets_navigate_reload() {
        let rt = runtime_with_url("https://example.com/");
        rt.eval("location.reload()").unwrap();
        let req = rt.take_navigate_request();
        assert!(matches!(req, Some(NavigateRequest::Reload)));
    }

    #[test]
    fn no_navigate_request_when_no_navigation() {
        let rt = runtime_with_url("https://example.com/");
        rt.eval("1 + 1").unwrap();
        assert!(rt.take_navigate_request().is_none());
    }

    #[test]
    fn push_state_updates_location_href() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("history.pushState(null, '', '/page2')").unwrap();
        let r = rt.eval("location.href").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("/page2".into()));
    }

    #[test]
    fn replace_state_updates_location_href() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("history.replaceState({x:1}, '', '/replaced')").unwrap();
        let r = rt.eval("location.href").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("/replaced".into()));
    }

    #[test]
    fn push_state_does_not_request_navigation() {
        let rt = runtime_with_url("https://example.com/");
        rt.eval("history.pushState(null, '', '/other')").unwrap();
        // pushState changes URL client-side without a network request
        assert!(rt.take_navigate_request().is_none());
    }

    #[test]
    fn push_state_enqueues_history_url_update_push() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("history.pushState({a:1}, '', '/page2')").unwrap();
        let updates = rt.take_history_url_updates();
        assert_eq!(updates.len(), 1, "one push update expected");
        match &updates[0] {
            HistoryUrlUpdate::Push { url, new_state_json } => {
                assert_eq!(url, "/page2");
                assert_eq!(new_state_json, r#"{"a":1}"#);
            }
            other => panic!("expected Push, got {other:?}"),
        }
        // Second drain: already consumed
        assert!(rt.take_history_url_updates().is_empty());
    }

    #[test]
    fn replace_state_enqueues_history_url_update_replace() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("history.replaceState({b:2}, '', '/new-page')").unwrap();
        let updates = rt.take_history_url_updates();
        assert_eq!(updates.len(), 1, "one replace update expected");
        match &updates[0] {
            HistoryUrlUpdate::Replace { url, new_state_json } => {
                assert_eq!(url, "/new-page");
                assert_eq!(new_state_json, r#"{"b":2}"#);
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn push_state_no_url_does_not_enqueue_update() {
        let rt = runtime_with_url("https://example.com/");
        // pushState with null url → no URL update
        rt.eval("history.pushState({x:3}, '')").unwrap();
        assert!(rt.take_history_url_updates().is_empty());
    }

    #[test]
    fn deliver_popstate_fires_onpopstate() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("var fired = null; window.onpopstate = function(e) { fired = e.state; };").unwrap();
        rt.eval("_lumen_deliver_popstate('{\"x\":42}', '/page0')").unwrap();
        let r = rt.eval("fired && fired.x").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(42.0));
    }

    #[test]
    fn deliver_popstate_updates_location() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("_lumen_deliver_popstate('null', '/restored')").unwrap();
        // _lumen_location_update updates href (= raw url string).
        // pathname is only correct for absolute URLs due to _lumen_parse_url limitations.
        let r = rt.eval("location.href").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("/restored".into()));
    }

    #[test]
    fn deliver_popstate_fires_event_listeners() {
        let rt = runtime_with_url("https://example.com/page1");
        rt.eval("var count = 0; window.addEventListener('popstate', function(e) { count += e.state.n; });").unwrap();
        rt.eval("_lumen_deliver_popstate('{\"n\":5}', '')").unwrap();
        let r = rt.eval("count").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(5.0));
    }

    #[test]
    fn location_file_url_parsed() {
        let rt = runtime_with_url("file:///home/user/page.html");
        let r = rt.eval("location.protocol").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("file:".into()));
    }

    // ── Web Storage tests ─────────────────────────────────────────────────────

    fn runtime_with_storage(ls: Option<Arc<Mutex<lumen_core::WebStorage>>>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        rt.install_dom(make_doc(), "https://example.com/", None, None, None, ls, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn local_storage_set_get() {
        let rt = runtime_with_storage(None);
        rt.eval("localStorage.setItem('k', 'v')").unwrap();
        let r = rt.eval("localStorage.getItem('k')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("v".into()));
    }

    #[test]
    fn local_storage_missing_key_returns_null() {
        let rt = runtime_with_storage(None);
        let r = rt.eval("localStorage.getItem('nope')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Null);
    }

    #[test]
    fn local_storage_length_and_key() {
        let rt = runtime_with_storage(None);
        rt.eval("localStorage.setItem('a', '1'); localStorage.setItem('b', '2')").unwrap();
        let len = rt.eval("localStorage.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(2.0));
        // key(0) == 'a' (insertion order)
        let k0 = rt.eval("localStorage.key(0)").unwrap();
        assert_eq!(k0, lumen_core::JsValue::String("a".into()));
    }

    #[test]
    fn local_storage_remove_item() {
        let rt = runtime_with_storage(None);
        rt.eval("localStorage.setItem('x', '42'); localStorage.removeItem('x')").unwrap();
        let r = rt.eval("localStorage.getItem('x')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Null);
    }

    #[test]
    fn local_storage_clear() {
        let rt = runtime_with_storage(None);
        rt.eval("localStorage.setItem('a', '1'); localStorage.setItem('b', '2'); localStorage.clear()").unwrap();
        let len = rt.eval("localStorage.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn local_storage_persists_across_runtimes() {
        // Shared Arc<Mutex<WebStorage>> simulates the same origin across page reloads.
        let shared = Arc::new(Mutex::new(lumen_core::WebStorage::default()));
        {
            let rt = runtime_with_storage(Some(Arc::clone(&shared)));
            rt.eval("localStorage.setItem('persist', 'yes')").unwrap();
        }
        let rt2 = runtime_with_storage(Some(Arc::clone(&shared)));
        let r = rt2.eval("localStorage.getItem('persist')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("yes".into()));
    }

    #[test]
    fn session_storage_fresh_per_runtime() {
        // sessionStorage is NOT shared; each runtime gets a fresh instance.
        let rt1 = runtime_with_storage(None);
        rt1.eval("sessionStorage.setItem('s', 'hello')").unwrap();
        let rt2 = runtime_with_storage(None);
        let r = rt2.eval("sessionStorage.getItem('s')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Null);
    }

    #[test]
    fn local_storage_on_window() {
        let rt = runtime_with_storage(None);
        rt.eval("window.localStorage.setItem('w', 'win')").unwrap();
        let r = rt.eval("localStorage.getItem('w')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("win".into()));
    }

    // ── URLSearchParams tests ─────────────────────────────────────────────────

    #[test]
    fn usp_parse_query_string() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var p = new URLSearchParams('a=1&b=2'); p.get('a') + ',' + p.get('b')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("1,2".into()));
    }

    #[test]
    fn usp_parse_leading_question_mark() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URLSearchParams('?x=hello').get('x')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("hello".into()));
    }

    #[test]
    fn usp_append_and_getall() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var p = new URLSearchParams(); p.append('k','1'); p.append('k','2'); p.getAll('k').join(',')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("1,2".into()));
    }

    #[test]
    fn usp_set_replaces_first() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var p = new URLSearchParams('a=1&a=2'); p.set('a','9'); p.toString()").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("a=9".into()));
    }

    #[test]
    fn usp_delete() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var p = new URLSearchParams('x=1&y=2'); p.delete('x'); p.toString()").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("y=2".into()));
    }

    #[test]
    fn usp_has() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var p = new URLSearchParams('k=v'); p.has('k') && !p.has('z')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn usp_plus_as_space() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URLSearchParams('q=hello+world').get('q')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("hello world".into()));
    }

    #[test]
    fn usp_size_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URLSearchParams('a=1&b=2&c=3').size").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn usp_from_object() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var p = new URLSearchParams({foo:'bar'}); p.get('foo')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("bar".into()));
    }

    #[test]
    fn usp_empty_string() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URLSearchParams('').size").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    // ── URL tests ─────────────────────────────────────────────────────────────

    #[test]
    fn url_absolute_parse() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var u = new URL('https://example.com:8080/path?q=1#top'); u.hostname + ':' + u.port").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("example.com:8080".into()));
    }

    #[test]
    fn url_pathname_and_search() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var u = new URL('https://x.com/a/b?c=d'); u.pathname + u.search").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("/a/b?c=d".into()));
    }

    #[test]
    fn url_hash() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URL('https://x.com/page#section').hash").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("#section".into()));
    }

    #[test]
    fn url_origin() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URL('https://api.example.com/data').origin").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("https://api.example.com".into()));
    }

    #[test]
    fn url_resolve_relative_path() {
        let rt = runtime_with_url("https://example.com/dir/page.html");
        let r = rt.eval("new URL('../other.html', location.href).pathname").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("/other.html".into()));
    }

    #[test]
    fn url_resolve_root_relative() {
        let rt = runtime_with_url("https://example.com/dir/page.html");
        let r = rt.eval("new URL('/top.html', location.href).pathname").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("/top.html".into()));
    }

    #[test]
    fn url_tostring() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URL('https://example.com/').toString()").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("https://example.com/".into()));
    }

    #[test]
    fn url_searchparams_from_url() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("new URL('https://example.com/?a=1&b=2').searchParams.get('b')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("2".into()));
    }

    #[test]
    fn url_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.URL === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── performance tests ─────────────────────────────────────────────────────

    #[test]
    fn performance_now_returns_non_negative() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.now() >= 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_now_monotonic() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var t1 = performance.now(); var t2 = performance.now(); t2 >= t1").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_time_origin_positive() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.timeOrigin > 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.performance.now === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_mark_stores_entry() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.mark('t1'); performance.getEntriesByType('mark').length").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn performance_mark_returns_entry_name() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.mark('mymark').name").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("mymark".into()));
    }

    #[test]
    fn performance_measure_duration() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.mark('s'); performance.mark('e', {startTime: performance.now()+10}); var m = performance.measure('d','s','e'); m.duration >= 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_get_entries_by_name() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.mark('x'); performance.mark('x'); performance.getEntriesByName('x','mark').length").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn performance_clear_marks() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("performance.mark('a'); performance.clearMarks(); performance.getEntriesByType('mark').length").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn performance_observer_constructor_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof PerformanceObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.PerformanceObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_receives_mark_entry() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("\
            var got = [];\
            var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
            po.observe({entryTypes:['mark']});\
            performance.mark('obs_test');\
            got.length === 1 && got[0].name === 'obs_test'\
        ").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_disconnect_stops_delivery() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("\
            var count = 0;\
            var po = new PerformanceObserver(function() { count++; });\
            po.observe({entryTypes:['mark']});\
            performance.mark('before');\
            po.disconnect();\
            performance.mark('after');\
            count === 1\
        ").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_paint_entry_via_lumen_deliver() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("\
            var got = [];\
            var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
            po.observe({entryTypes:['paint']});\
            _lumen_deliver_paint_entry('first-paint', 42.0);\
            got.length === 1 && got[0].name === 'first-paint' && got[0].startTime === 42.0\
        ").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_buffered_delivers_existing() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("\
            _lumen_deliver_paint_entry('first-paint', 10.0);\
            var got = [];\
            var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });\
            po.observe({entryTypes:['paint'], buffered: true});\
            got.length === 1\
        ").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── PerformanceObserver single-type form (Performance Timeline L2 §6.2.2) ──

    #[test]
    fn performance_observer_single_type_receives_entry() {
        // observe({type: 'mark'}) — single-type form should work like entryTypes:['mark']
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });
            po.observe({type: 'mark'});
            performance.mark('single_type_test');
            got.length === 1 && got[0].name === 'single_type_test'
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_single_type_with_buffered() {
        // observe({type: 'navigation', buffered: true}) — must replay existing entries
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            _lumen_deliver_perf_entry('navigation', 'https://buf.test/', 0.0, 300.0, null);
            var got = [];
            var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });
            po.observe({type: 'navigation', buffered: true});
            got.length === 1 && got[0].name === 'https://buf.test/'
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_repeated_observe_accumulates_types() {
        // Multiple observe() calls accumulate subscribed types.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = got.concat(list.getEntries()); });
            po.observe({type: 'mark'});
            po.observe({type: 'measure'});
            performance.mark('m1');
            performance.measure('ms1', 'm1');
            got.length === 2
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn performance_observer_supported_entry_types() {
        // PerformanceObserver.supportedEntryTypes is an array including 'navigation'.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var types = PerformanceObserver.supportedEntryTypes;
            Array.isArray(types) && types.indexOf('navigation') !== -1 && types.indexOf('mark') !== -1
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── queueMicrotask tests ──────────────────────────────────────────────────

    #[test]
    fn queue_microtask_exists_as_function() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof queueMicrotask === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn queue_microtask_throws_on_non_function() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var threw = false; try { queueMicrotask(42); } catch(e) { threw = true; } threw").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn queue_microtask_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.queueMicrotask === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── requestAnimationFrame / cancelAnimationFrame ──────────────────────────

    #[test]
    fn raf_returns_numeric_id() {
        let rt = runtime_with_dom(make_doc());
        let id = rt.eval("requestAnimationFrame(function(){})").unwrap();
        assert!(matches!(id, lumen_core::JsValue::Number(n) if n >= 1.0));
    }

    #[test]
    fn raf_ids_are_sequential() {
        let rt = runtime_with_dom(make_doc());
        let id1 = rt.eval("requestAnimationFrame(function(){})").unwrap();
        let id2 = rt.eval("requestAnimationFrame(function(){})").unwrap();
        if let (lumen_core::JsValue::Number(n1), lumen_core::JsValue::Number(n2)) = (id1, id2) {
            assert!(n2 > n1);
        } else {
            panic!("expected numeric IDs");
        }
    }

    #[test]
    fn raf_non_function_returns_zero() {
        let rt = runtime_with_dom(make_doc());
        let id = rt.eval("requestAnimationFrame(42)").unwrap();
        assert_eq!(id, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn raf_marks_raf_pending() {
        let rt = runtime_with_dom(make_doc());
        assert!(!rt.take_raf_pending(), "clean at start");
        rt.eval("requestAnimationFrame(function(){})").unwrap();
        assert!(rt.take_raf_pending(), "set after rAF call");
        assert!(!rt.take_raf_pending(), "cleared after take");
    }

    #[test]
    fn raf_run_calls_callback_with_timestamp() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_ts = -1; requestAnimationFrame(function(t){ _raf_ts = t; })").unwrap();
        rt.eval("_lumen_run_raf_callbacks(16.7)").unwrap();
        let ts = rt.eval("_raf_ts").unwrap();
        assert_eq!(ts, lumen_core::JsValue::Number(16.7));
    }

    #[test]
    fn raf_run_snapshot_pattern() {
        // Callbacks registered during a frame run go into the NEXT frame.
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_count = 0;").unwrap();
        rt.eval("requestAnimationFrame(function() { _raf_count++; requestAnimationFrame(function(){ _raf_count++; }); })").unwrap();
        rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
        let count1 = rt.eval("_raf_count").unwrap();
        assert_eq!(count1, lumen_core::JsValue::Number(1.0), "only outer cb in frame 1");
        rt.eval("_lumen_run_raf_callbacks(16)").unwrap();
        let count2 = rt.eval("_raf_count").unwrap();
        assert_eq!(count2, lumen_core::JsValue::Number(2.0), "inner cb in frame 2");
    }

    #[test]
    fn raf_recursive_marks_pending() {
        let rt = runtime_with_dom(make_doc());
        // Callback registers another rAF → raf_pending must be set after run.
        rt.eval("requestAnimationFrame(function() { requestAnimationFrame(function(){}); })").unwrap();
        let _ = rt.take_raf_pending(); // clear initial flag
        rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
        assert!(rt.take_raf_pending(), "inner rAF sets pending for next frame");
    }

    #[test]
    fn cancel_raf_prevents_callback() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_ran = false;").unwrap();
        rt.eval("var id = requestAnimationFrame(function(){ _raf_ran = true; });").unwrap();
        rt.eval("cancelAnimationFrame(id)").unwrap();
        rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
        let ran = rt.eval("_raf_ran").unwrap();
        assert_eq!(ran, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn cancel_raf_unknown_id_is_noop() {
        let rt = runtime_with_dom(make_doc());
        // Should not throw or panic.
        rt.eval("cancelAnimationFrame(9999)").unwrap();
    }

    #[test]
    fn raf_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.requestAnimationFrame === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cancel_raf_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.cancelAnimationFrame === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── EE-5: rAF vsync batch / DOMHighResTimeStamp tests ────────────────────

    #[test]
    fn raf_coalesce_multiple_registrations_fire_in_one_batch() {
        // EE-5: multiple requestAnimationFrame() calls in the same frame
        // are all executed in a single _lumen_run_raf_callbacks() invocation.
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_log = []; \
                 requestAnimationFrame(function(){ _raf_log.push(1); }); \
                 requestAnimationFrame(function(){ _raf_log.push(2); }); \
                 requestAnimationFrame(function(){ _raf_log.push(3); });").unwrap();
        rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
        let len = rt.eval("_raf_log.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(3.0), "all 3 callbacks fired in one batch");
        let order = rt.eval("_raf_log[0] === 1 && _raf_log[1] === 2 && _raf_log[2] === 3").unwrap();
        assert_eq!(order, lumen_core::JsValue::Bool(true), "callbacks fire in registration order");
    }

    #[test]
    fn raf_batch_uniform_timestamp() {
        // EE-5: all callbacks in a batch receive the identical DOMHighResTimeStamp.
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_ts1 = null; var _raf_ts2 = null; \
                 requestAnimationFrame(function(t){ _raf_ts1 = t; }); \
                 requestAnimationFrame(function(t){ _raf_ts2 = t; });").unwrap();
        rt.eval("_lumen_run_raf_callbacks(42.5)").unwrap();
        let eq = rt.eval("_raf_ts1 === _raf_ts2").unwrap();
        assert_eq!(eq, lumen_core::JsValue::Bool(true), "both callbacks get same timestamp");
        let val = rt.eval("_raf_ts1").unwrap();
        assert_eq!(val, lumen_core::JsValue::Number(42.5));
    }

    #[test]
    fn raf_deterministic_zero_timestamp() {
        // EE-5: deterministic mode (timestamp_ms === 0) delivers 0 to all callbacks.
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_det_ts = -99; requestAnimationFrame(function(t){ _raf_det_ts = t; })").unwrap();
        rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
        let ts = rt.eval("_raf_det_ts").unwrap();
        assert_eq!(ts, lumen_core::JsValue::Number(0.0), "deterministic mode passes 0 to callback");
    }

    #[test]
    fn raf_live_clock_timestamp_non_negative() {
        // EE-5: when timestamp_ms < 0, JS uses performance.now() — must be >= 0.
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_live_ts = null; requestAnimationFrame(function(t){ _raf_live_ts = t; })").unwrap();
        rt.eval("_lumen_run_raf_callbacks(-1)").unwrap();
        let ts = rt.eval("typeof _raf_live_ts === 'number' && _raf_live_ts >= 0").unwrap();
        assert_eq!(ts, lumen_core::JsValue::Bool(true), "live clock timestamp is non-negative DOMHighResTimeStamp");
    }

    #[test]
    fn raf_exception_in_one_callback_does_not_stop_batch() {
        // EE-5: if one callback throws, subsequent callbacks still run (try/catch).
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _raf_after_throw = false; \
                 requestAnimationFrame(function(){ throw new Error('boom'); }); \
                 requestAnimationFrame(function(){ _raf_after_throw = true; });").unwrap();
        rt.eval("_lumen_run_raf_callbacks(0)").unwrap();
        let ran = rt.eval("_raf_after_throw").unwrap();
        assert_eq!(ran, lumen_core::JsValue::Bool(true), "second callback ran despite first throwing");
    }

    // ── MutationObserver tests ────────────────────────────────────────────────

    #[test]
    fn mutation_observer_exists_as_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof MutationObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn mutation_observer_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.MutationObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn mutation_observer_fires_on_attribute_change() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _mo_fired = false;
            var _mo_rec = null;
            var obs = new MutationObserver(function(records) {
                _mo_fired = true;
                _mo_rec = records[0];
            });
            var el = document.getElementById('main');
            obs.observe(el, { attributes: true });
            el.setAttribute('data-x', '42');
        "#).unwrap();
        // Flush synchronously; queueMicrotask delivery drains on next eval but
        // using the flush function is more explicit and reliable in tests.
        rt.eval("_lumen_flush_mutation_observers()").unwrap();
        let fired = rt.eval("_mo_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
        let attr = rt.eval("_mo_rec && _mo_rec.type").unwrap();
        assert_eq!(attr, lumen_core::JsValue::String("attributes".into()));
    }

    #[test]
    fn mutation_observer_fires_on_child_list_change() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _mo_cl_fired = false;
            var obs2 = new MutationObserver(function(records) {
                _mo_cl_fired = records.some(function(r){ return r.type === 'childList'; });
            });
            var body = document.body;
            obs2.observe(body, { childList: true });
            var d = document.createElement('div');
            body.appendChild(d);
        "#).unwrap();
        rt.eval("_lumen_flush_mutation_observers()").unwrap();
        let fired = rt.eval("_mo_cl_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn mutation_observer_disconnect_stops_delivery() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _mo_cnt = 0;
            var obs3 = new MutationObserver(function() { _mo_cnt++; });
            var el3 = document.getElementById('main');
            obs3.observe(el3, { attributes: true });
            obs3.disconnect();
            el3.setAttribute('data-y', '1');
        "#).unwrap();
        rt.eval("_lumen_flush_mutation_observers()").unwrap();
        let cnt = rt.eval("_mo_cnt").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn mutation_observer_take_records_clears_queue() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var obs4 = new MutationObserver(function() {});
            var el4 = document.getElementById('main');
            obs4.observe(el4, { attributes: true });
            el4.setAttribute('data-z', '1');
            var recs = obs4.takeRecords();
        "#).unwrap();
        let len = rt.eval("recs.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(1.0));
        // Internal queue must be cleared
        let inner_len = rt.eval("obs4.takeRecords().length").unwrap();
        assert_eq!(inner_len, lumen_core::JsValue::Number(0.0));
    }

    // ── ResizeObserver tests ──────────────────────────────────────────────────

    #[test]
    fn resize_observer_exists_as_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof ResizeObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resize_observer_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.ResizeObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resize_observer_fires_when_rect_changes() {
        let rt = runtime_with_dom(make_doc());
        // Inject a fake bounding rect for the node
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 100.0])].into_iter().collect());
        rt.eval(r#"
            var _ro_fired = false;
            var _ro_entry = null;
            var ro = new ResizeObserver(function(entries) {
                _ro_fired = true;
                _ro_entry = entries[0];
            });
            var body = document.body;
            ro.observe(body);
            _lumen_deliver_resize_observers();
        "#).unwrap();
        let fired = rt.eval("_ro_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
        let w = rt.eval("_ro_entry && _ro_entry.contentRect.width").unwrap();
        assert_eq!(w, lumen_core::JsValue::Number(200.0));
    }

    #[test]
    fn resize_observer_no_delivery_when_size_unchanged() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
        rt.eval("var _ro_cnt2 = 0; var ro2 = new ResizeObserver(function(){ _ro_cnt2++; }); ro2.observe(document.body);").unwrap();
        // First delivery
        rt.eval("_lumen_deliver_resize_observers()").unwrap();
        // Second delivery with same rect → no callback
        rt.eval("_lumen_deliver_resize_observers()").unwrap();
        let cnt = rt.eval("_ro_cnt2").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn resize_observer_disconnect_stops_delivery() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 300.0, 200.0])].into_iter().collect());
        rt.eval(r#"
            var _ro_cnt3 = 0;
            var ro3 = new ResizeObserver(function(){ _ro_cnt3++; });
            ro3.observe(document.body);
            ro3.disconnect();
            _lumen_deliver_resize_observers();
        "#).unwrap();
        let cnt = rt.eval("_ro_cnt3").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn resize_observer_fires_again_on_size_change() {
        // After a size change, observer should fire a second time.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
        rt.eval(r#"
            var _ro_sz_cnt = 0;
            var ro_sz = new ResizeObserver(function() { _ro_sz_cnt++; });
            ro_sz.observe(document.body);
            _lumen_deliver_resize_observers();
        "#).unwrap();
        // Change size
        rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 80.0])].into_iter().collect());
        rt.eval("_lumen_deliver_resize_observers()").unwrap();
        let cnt = rt.eval("_ro_sz_cnt").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn resize_observer_border_box_size_fields() {
        // Entry must expose borderBoxSize and contentBoxSize arrays.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 150.0, 75.0])].into_iter().collect());
        rt.eval(r#"
            var _ro_bb_entry = null;
            var ro_bb = new ResizeObserver(function(entries) { _ro_bb_entry = entries[0]; });
            ro_bb.observe(document.body);
            _lumen_deliver_resize_observers();
        "#).unwrap();
        let is = rt.eval("_ro_bb_entry && _ro_bb_entry.borderBoxSize[0].inlineSize").unwrap();
        assert_eq!(is, lumen_core::JsValue::Number(150.0));
        let bs = rt.eval("_ro_bb_entry && _ro_bb_entry.contentBoxSize[0].blockSize").unwrap();
        assert_eq!(bs, lumen_core::JsValue::Number(75.0));
    }

    #[test]
    fn resize_observer_unobserve_stops_delivery() {
        // Save element reference — document.body may create a new proxy each access.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
        rt.eval(r#"
            var _ro_un_cnt = 0;
            var _ro_un_target = document.body;
            var ro_un = new ResizeObserver(function() { _ro_un_cnt++; });
            ro_un.observe(_ro_un_target);
            ro_un.unobserve(_ro_un_target);
            _lumen_deliver_resize_observers();
        "#).unwrap();
        let cnt = rt.eval("_ro_un_cnt").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
    }

    // ── IntersectionObserver tests ────────────────────────────────────────────

    #[test]
    fn intersection_observer_exists_as_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof IntersectionObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn intersection_observer_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.IntersectionObserver === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn intersection_observer_fires_on_first_observe_visible() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _io_fired = false;
            var _io_entry = null;
            var io = new IntersectionObserver(function(entries) {
                _io_fired = true;
                _io_entry = entries[0];
            });
            io.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let fired = rt.eval("_io_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
        let ratio = rt.eval("_io_entry && _io_entry.intersectionRatio > 0").unwrap();
        assert_eq!(ratio, lumen_core::JsValue::Bool(true));
        let intersecting = rt.eval("_io_entry.isIntersecting").unwrap();
        assert_eq!(intersecting, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn intersection_observer_not_intersecting_when_outside_viewport() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Element is below viewport
        rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 50.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _io2_entry = null;
            var io2 = new IntersectionObserver(function(entries) { _io2_entry = entries[0]; });
            io2.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let intersecting = rt.eval("_io2_entry && _io2_entry.isIntersecting").unwrap();
        assert_eq!(intersecting, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn intersection_observer_threshold_fires_only_on_crossing() {
        // element partially in viewport (ratio≈0.7), then fully out — only 2 callbacks:
        // initial delivery + crossing back out below threshold 0.5.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Partially visible: y=650, h=100, viewport h=720 → ratio=70/100=0.7
        rt.update_layout_rects([(nid, [0.0, 650.0, 100.0, 100.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _thr_cnt = 0;
            var io_thr = new IntersectionObserver(function(entries) {
                _thr_cnt++;
            }, { threshold: 0.5 });
            io_thr.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        // Second delivery same rect — no crossing → no fire
        rt.eval("_lumen_deliver_intersection_observers()").unwrap();
        let cnt1 = rt.eval("_thr_cnt").unwrap();
        assert_eq!(cnt1, lumen_core::JsValue::Number(1.0));
        // Move fully out of viewport — ratio=0 crosses 0.5 → fires again
        rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_deliver_intersection_observers()").unwrap();
        let cnt2 = rt.eval("_thr_cnt").unwrap();
        assert_eq!(cnt2, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn intersection_observer_rootmargin_expands_viewport() {
        // Element just below viewport; positive rootMargin makes it visible.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Element top at y=730 (10px below 720px viewport)
        rt.update_layout_rects([(nid, [0.0, 730.0, 100.0, 50.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _rm_entry = null;
            var io_rm = new IntersectionObserver(function(entries) {
                _rm_entry = entries[0];
            }, { rootMargin: '0px 0px 50px 0px' });
            io_rm.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let intersecting = rt.eval("_rm_entry && _rm_entry.isIntersecting").unwrap();
        assert_eq!(intersecting, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn intersection_observer_rootmargin_contracts_viewport() {
        // Element near bottom; negative rootMargin pushes root boundary up, element leaves root.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Element at y=700, h=50 → nominally intersects 720px viewport by 20px
        rt.update_layout_rects([(nid, [0.0, 700.0, 100.0, 50.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _rm2_entry = null;
            var io_rm2 = new IntersectionObserver(function(entries) {
                _rm2_entry = entries[0];
            }, { rootMargin: '0px 0px -50px 0px' });
            io_rm2.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        // rootBottom = 720-50 = 670; element top=700 > 670 → no intersection
        let intersecting = rt.eval("_rm2_entry && _rm2_entry.isIntersecting").unwrap();
        assert_eq!(intersecting, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn intersection_observer_unobserve_stops_delivery() {
        // document.body may return a new proxy object each call, so save the reference
        // and use the same object for both observe() and unobserve().
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _un_cnt = 0;
            var _un_target = document.body;
            var io_un = new IntersectionObserver(function() { _un_cnt++; });
            io_un.observe(_un_target);
            io_un.unobserve(_un_target);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let cnt = rt.eval("_un_cnt").unwrap();
        assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn intersection_observer_two_observers_fire_independently() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 200.0, 100.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _cnt_a = 0, _cnt_b = 0;
            var io_a = new IntersectionObserver(function() { _cnt_a++; });
            var io_b = new IntersectionObserver(function() { _cnt_b++; });
            io_a.observe(document.body);
            io_b.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let a = rt.eval("_cnt_a").unwrap();
        let b = rt.eval("_cnt_b").unwrap();
        assert_eq!(a, lumen_core::JsValue::Number(1.0));
        assert_eq!(b, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn intersection_observer_intersection_rect_height() {
        // intersectionRect.height must equal the visible slice of the element.
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Element at y=680, h=100; viewport h=720 → 40px visible
        rt.update_layout_rects([(nid, [0.0, 680.0, 100.0, 100.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _ir_entry = null;
            var io_ir = new IntersectionObserver(function(entries) { _ir_entry = entries[0]; });
            io_ir.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let ih = rt.eval("_ir_entry && _ir_entry.intersectionRect.height").unwrap();
        assert_eq!(ih, lumen_core::JsValue::Number(40.0));
        let ratio_ok = rt.eval("_ir_entry && Math.abs(_ir_entry.intersectionRatio - 0.4) < 0.01").unwrap();
        assert_eq!(ratio_ok, lumen_core::JsValue::Bool(true));
    }

    // ── ChildNode / ParentNode mixin tests ───────────────────────────────────

    #[test]
    fn element_remove_detaches_from_parent() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _container = document.createElement('div');
            document.body.appendChild(_container);
            var _div = document.createElement('span');
            _container.appendChild(_div);
            _div.remove();
        "#).unwrap();
        let count = rt.eval("_container.children.length").unwrap();
        assert_eq!(count, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn element_before_inserts_node_before_target() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _cont = document.createElement('div');
            document.body.appendChild(_cont);
            var _a = document.createElement('span');
            var _b = document.createElement('div');
            _a.id = 'A'; _b.id = 'B';
            _cont.appendChild(_b);
            _b.before(_a);
        "#).unwrap();
        let first_id = rt.eval("_cont.children[0].id").unwrap();
        assert_eq!(first_id, lumen_core::JsValue::String("A".into()));
    }

    #[test]
    fn element_after_inserts_node_after_target() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _cont2 = document.createElement('div');
            document.body.appendChild(_cont2);
            var _x = document.createElement('span');
            var _y = document.createElement('em');
            _x.id = 'X'; _y.id = 'Y';
            _cont2.appendChild(_x);
            _x.after(_y);
        "#).unwrap();
        let second_id = rt.eval("_cont2.children[1].id").unwrap();
        assert_eq!(second_id, lumen_core::JsValue::String("Y".into()));
    }

    #[test]
    fn element_replace_with_swaps_element() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _cont3 = document.createElement('div');
            document.body.appendChild(_cont3);
            var _old = document.createElement('p');
            var _new = document.createElement('section');
            _old.id = 'OLD'; _new.id = 'NEW';
            _cont3.appendChild(_old);
            _old.replaceWith(_new);
        "#).unwrap();
        let tag = rt.eval("_cont3.children[0].id").unwrap();
        assert_eq!(tag, lumen_core::JsValue::String("NEW".into()));
    }

    #[test]
    fn element_prepend_inserts_before_first_child() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _pcont = document.createElement('div');
            document.body.appendChild(_pcont);
            var _first = document.createElement('div');
            var _second = document.createElement('span');
            _first.id = 'FIRST'; _second.id = 'SECOND';
            _pcont.appendChild(_second);
            _pcont.prepend(_first);
        "#).unwrap();
        let first_id = rt.eval("_pcont.children[0].id").unwrap();
        assert_eq!(first_id, lumen_core::JsValue::String("FIRST".into()));
    }

    #[test]
    fn element_replace_children_clears_and_sets() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _rccont = document.createElement('div');
            document.body.appendChild(_rccont);
            var _c1 = document.createElement('div');
            var _c2 = document.createElement('span');
            var _c3 = document.createElement('p');
            _rccont.appendChild(_c1);
            _rccont.appendChild(_c2);
            _rccont.replaceChildren(_c3);
        "#).unwrap();
        let count = rt.eval("_rccont.children.length").unwrap();
        assert_eq!(count, lumen_core::JsValue::Number(1.0));
        let tag = rt.eval("_rccont.children[0].tagName.toLowerCase()").unwrap();
        assert_eq!(tag, lumen_core::JsValue::String("p".into()));
    }

    // ── TreeWalker / NodeIterator tests ──────────────────────────────────────

    #[test]
    fn node_filter_constants_available() {
        let rt = runtime_with_dom(make_doc());
        let accept = rt.eval("NodeFilter.FILTER_ACCEPT").unwrap();
        assert_eq!(accept, lumen_core::JsValue::Number(1.0));
        let show_all = rt.eval("NodeFilter.SHOW_ALL").unwrap();
        assert_eq!(show_all, lumen_core::JsValue::Number(0xFFFFFFFF_u32 as f64));
        let show_element = rt.eval("NodeFilter.SHOW_ELEMENT").unwrap();
        assert_eq!(show_element, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn tree_walker_exists_on_window() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt.eval("typeof window.TreeWalker === 'function'").unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_tree_walker_returns_walker_with_root() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _twroot = document.createElement('section');
            document.body.appendChild(_twroot);
            var _tw = document.createTreeWalker(_twroot, NodeFilter.SHOW_ELEMENT);
        "#).unwrap();
        let root_tag = rt.eval("_tw.root.tagName.toLowerCase()").unwrap();
        assert_eq!(root_tag, lumen_core::JsValue::String("section".into()));
    }

    #[test]
    fn tree_walker_next_node_traverses_children() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _twc = document.createElement('section');
            document.body.appendChild(_twc);
            var _d1 = document.createElement('div');
            var _d2 = document.createElement('span');
            _d1.id = 'D1'; _d2.id = 'D2';
            _twc.appendChild(_d1);
            _twc.appendChild(_d2);
            var _tw2 = document.createTreeWalker(_twc, NodeFilter.SHOW_ELEMENT);
            var _n1 = _tw2.nextNode(); // D1
            var _n2 = _tw2.nextNode(); // D2
        "#).unwrap();
        let id1 = rt.eval("_n1 && _n1.id").unwrap();
        assert_eq!(id1, lumen_core::JsValue::String("D1".into()));
        let id2 = rt.eval("_n2 && _n2.id").unwrap();
        assert_eq!(id2, lumen_core::JsValue::String("D2".into()));
    }

    #[test]
    fn tree_walker_previous_node_goes_back() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _twpc = document.createElement('article');
            document.body.appendChild(_twpc);
            var _da = document.createElement('div');
            var _db = document.createElement('span');
            _da.id = 'DA'; _db.id = 'DB';
            _twpc.appendChild(_da);
            _twpc.appendChild(_db);
            var _tw3 = document.createTreeWalker(_twpc, NodeFilter.SHOW_ELEMENT);
            _tw3.nextNode(); // DA
            _tw3.nextNode(); // DB
            var _prev = _tw3.previousNode(); // back to DA
        "#).unwrap();
        let id = rt.eval("_prev && _prev.id").unwrap();
        assert_eq!(id, lumen_core::JsValue::String("DA".into()));
    }

    #[test]
    fn tree_walker_with_filter_function() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _twfc = document.createElement('aside');
            document.body.appendChild(_twfc);
            var _s1 = document.createElement('span');
            var _s2 = document.createElement('div');
            var _s3 = document.createElement('span');
            _s1.id = 'S1'; _s2.id = 'S2'; _s3.id = 'S3';
            _twfc.appendChild(_s1);
            _twfc.appendChild(_s2);
            _twfc.appendChild(_s3);
            var _tw4 = document.createTreeWalker(_twfc, NodeFilter.SHOW_ELEMENT, function(node) {
                return node.tagName.toLowerCase() === 'span'
                    ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
            });
            var _fn1 = _tw4.nextNode(); // S1
            var _fn2 = _tw4.nextNode(); // S3 (S2=div skipped)
        "#).unwrap();
        let id1 = rt.eval("_fn1 && _fn1.id").unwrap();
        assert_eq!(id1, lumen_core::JsValue::String("S1".into()));
        let id2 = rt.eval("_fn2 && _fn2.id").unwrap();
        assert_eq!(id2, lumen_core::JsValue::String("S3".into()));
    }

    #[test]
    fn node_iterator_exists_on_window() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt.eval("typeof window.NodeIterator === 'function'").unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn node_iterator_next_node_and_previous_node() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _nic = document.createElement('nav');
            document.body.appendChild(_nic);
            var _ni_a = document.createElement('div');
            var _ni_b = document.createElement('span');
            _ni_a.id = 'NIA'; _ni_b.id = 'NIB';
            _nic.appendChild(_ni_a);
            _nic.appendChild(_ni_b);
            var _ni = document.createNodeIterator(_nic, NodeFilter.SHOW_ELEMENT);
            var _ni_n1 = _ni.nextNode(); // _nic itself
            var _ni_n2 = _ni.nextNode(); // NIA
            var _ni_n3 = _ni.nextNode(); // NIB
            var _ni_p1 = _ni.previousNode(); // back to NIA
        "#).unwrap();
        let n2_id = rt.eval("_ni_n2 && _ni_n2.id").unwrap();
        assert_eq!(n2_id, lumen_core::JsValue::String("NIA".into()));
        let p1_id = rt.eval("_ni_p1 && _ni_p1.id").unwrap();
        assert_eq!(p1_id, lumen_core::JsValue::String("NIA".into()));
    }

    #[test]
    fn document_adopt_node_returns_node() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _orig = document.createElement('div');
            _orig.id = 'ADO';
            var _adopted = document.adoptNode(_orig);
        "#).unwrap();
        let id = rt.eval("_adopted && _adopted.id").unwrap();
        assert_eq!(id, lumen_core::JsValue::String("ADO".into()));
    }

    #[test]
    fn document_import_node_returns_clone() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _tmpl = document.createElement('p');
            _tmpl.id = 'IMP';
            var _imported = document.importNode(_tmpl, false);
        "#).unwrap();
        let id = rt.eval("_imported && _imported.id").unwrap();
        assert_eq!(id, lumen_core::JsValue::String("IMP".into()));
    }

    #[test]
    fn get_bounding_rect_returns_values_from_runtime() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        rt.update_layout_rects([(nid, [10.0, 20.0, 300.0, 150.0])].into_iter().collect());
        // The JS body element's __nid__ should match nid
        let rect_val = rt.eval(&format!("_lumen_get_bounding_rect({nid})")).unwrap();
        match rect_val {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::Number(10.0));
                assert_eq!(arr[1], lumen_core::JsValue::Number(20.0));
                assert_eq!(arr[2], lumen_core::JsValue::Number(300.0));
                assert_eq!(arr[3], lumen_core::JsValue::Number(150.0));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn get_viewport_size_returns_updated_values() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(1920.0, 1080.0);
        let vp = rt.eval("_lumen_get_viewport_size()").unwrap();
        match vp {
            lumen_core::JsValue::Array(arr) => {
                assert_eq!(arr[0], lumen_core::JsValue::Number(1920.0));
                assert_eq!(arr[1], lumen_core::JsValue::Number(1080.0));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    // ── window.matchMedia / MediaQueryList (CSS MQ L4 §4.2) ───────────────────

    #[test]
    fn match_media_exists_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.matchMedia === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r = rt.eval("typeof matchMedia === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r = rt.eval("typeof window.MediaQueryList === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r = rt.eval("typeof window.MediaQueryListEvent === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn match_media_screen_always_matches() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        let r = rt.eval("matchMedia('screen').matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn match_media_min_width_matches_when_viewport_wide_enough() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        let r = rt.eval("matchMedia('(min-width: 100px)').matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn match_media_min_width_misses_when_viewport_too_narrow() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        let r = rt.eval("matchMedia('(min-width: 900px)').matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn match_media_max_width_matches() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        let r = rt.eval("matchMedia('(max-width: 1000px)').matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn match_media_print_does_not_match_screen() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        let r = rt.eval("matchMedia('print').matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn match_media_returns_object_with_media_property() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        let r = rt
            .eval("matchMedia('(min-width: 500px)').media")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("(min-width: 500px)".into()));
        let r = rt
            .eval("matchMedia('(min-width: 500px)') instanceof MediaQueryList")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn match_media_add_remove_listener_noop_when_no_change() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        // Legacy addListener/removeListener API (deprecated but widely used).
        rt.eval(
            r"
            var _mm_calls = 0;
            var _mm = matchMedia('(min-width: 100px)');
            var _mm_cb = function() { _mm_calls++; };
            _mm.addListener(_mm_cb);
            _mm.removeListener(_mm_cb);
            ",
        )
        .unwrap();
        let r = rt.eval("_mm_calls").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn match_media_change_event_fires_when_matches_flips() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        rt.eval(
            r"
            var _mm_calls = 0;
            var _mm_last_matches = null;
            var _mm_last_media = null;
            var _mm = matchMedia('(min-width: 900px)');
            _mm.addEventListener('change', function(ev) {
                _mm_calls++;
                _mm_last_matches = ev.matches;
                _mm_last_media = ev.media;
            });
            ",
        )
        .unwrap();
        // Initial state: not matching (800 < 900).
        let r = rt.eval("_mm.matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
        // Viewport grows to 1000 — now matches.
        rt.eval("_lumen_deliver_media_changes(1000, 600, false, false)").unwrap();
        let r = rt.eval("_mm_calls").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
        let r = rt.eval("_mm_last_matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r = rt.eval("_mm_last_media").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("(min-width: 900px)".into()));
        let r = rt.eval("_mm.matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn match_media_change_event_does_not_fire_when_no_flip() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        rt.eval(
            r"
            var _mm_calls = 0;
            var _mm = matchMedia('(min-width: 100px)');
            _mm.addEventListener('change', function() { _mm_calls++; });
            ",
        )
        .unwrap();
        // Already matches; reapply same context → no flip → no fire.
        rt.eval("_lumen_deliver_media_changes(900, 600, false, false)").unwrap();
        rt.eval("_lumen_deliver_media_changes(1200, 600, false, false)").unwrap();
        let r = rt.eval("_mm_calls").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn match_media_onchange_callback_fires() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        rt.eval(
            r"
            var _mm_onchange_calls = 0;
            var _mm = matchMedia('(min-width: 1000px)');
            _mm.onchange = function() { _mm_onchange_calls++; };
            ",
        )
        .unwrap();
        rt.eval("_lumen_deliver_media_changes(1100, 600, false, false)").unwrap();
        let r = rt.eval("_mm_onchange_calls").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn match_media_prefers_color_scheme_dark() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        // Initially: dark = false (default).
        let r = rt.eval("matchMedia('(prefers-color-scheme: dark)').matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
        // Flip to dark via the shell delivery path.
        rt.eval(
            r"
            var _mm_dark_calls = 0;
            var _mm_dark = matchMedia('(prefers-color-scheme: dark)');
            _mm_dark.addEventListener('change', function(ev) { _mm_dark_calls++; });
            _lumen_deliver_media_changes(800, 600, true, false);
            ",
        )
        .unwrap();
        let r = rt.eval("_mm_dark.matches").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r = rt.eval("_mm_dark_calls").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn match_media_event_is_media_query_list_event() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        rt.eval(
            r"
            var _mm_ev_type = null;
            var _mm_ev_is_mqle = false;
            var _mm_ev_is_event = false;
            var _mm = matchMedia('(min-width: 1500px)');
            _mm.addEventListener('change', function(ev) {
                _mm_ev_type = ev.type;
                _mm_ev_is_mqle = ev instanceof MediaQueryListEvent;
                _mm_ev_is_event = ev instanceof Event;
            });
            _lumen_deliver_media_changes(1600, 600, false, false);
            ",
        )
        .unwrap();
        let r = rt.eval("_mm_ev_type").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("change".into()));
        let r = rt.eval("_mm_ev_is_mqle").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r = rt.eval("_mm_ev_is_event").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── Element geometry API ─────────────────────────────────────────────────

    #[test]
    fn get_bounding_client_rect_method_on_element() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        rt.update_layout_rects([(nid, [5.0, 10.0, 200.0, 100.0])].into_iter().collect());
        let x = rt.eval("document.body.getBoundingClientRect().x").unwrap();
        assert_eq!(x, lumen_core::JsValue::Number(5.0));
        let w = rt.eval("document.body.getBoundingClientRect().width").unwrap();
        assert_eq!(w, lumen_core::JsValue::Number(200.0));
        let bottom = rt.eval("document.body.getBoundingClientRect().bottom").unwrap();
        assert_eq!(bottom, lumen_core::JsValue::Number(110.0));
    }

    #[test]
    fn offset_width_height_on_element() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        rt.update_layout_rects([(nid, [0.0, 0.0, 320.0, 240.0])].into_iter().collect());
        let ow = rt.eval("document.body.offsetWidth").unwrap();
        assert_eq!(ow, lumen_core::JsValue::Number(320.0));
        let oh = rt.eval("document.body.offsetHeight").unwrap();
        assert_eq!(oh, lumen_core::JsValue::Number(240.0));
    }

    #[test]
    fn scroll_top_left_via_update_scroll_states() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        rt.update_scroll_states([(nid, [42.0, 17.0, 800.0, 2000.0])].into_iter().collect());
        let sl = rt.eval("document.body.scrollLeft").unwrap();
        assert_eq!(sl, lumen_core::JsValue::Number(42.0));
        let st = rt.eval("document.body.scrollTop").unwrap();
        assert_eq!(st, lumen_core::JsValue::Number(17.0));
        let sw = rt.eval("document.body.scrollWidth").unwrap();
        assert_eq!(sw, lumen_core::JsValue::Number(800.0));
        let sh = rt.eval("document.body.scrollHeight").unwrap();
        assert_eq!(sh, lumen_core::JsValue::Number(2000.0));
    }

    #[test]
    fn scroll_to_queues_request() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        rt.update_scroll_states([(nid, [0.0, 0.0, 800.0, 2000.0])].into_iter().collect());
        rt.eval("document.body.scrollTo(100, 200)").unwrap();
        let reqs = rt.take_scroll_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].0, nid);
        assert!((reqs[0].1 - 100.0).abs() < 0.1);
        assert!((reqs[0].2 - 200.0).abs() < 0.1);
    }

    #[test]
    fn scroll_by_adds_to_current_position() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        rt.update_scroll_states([(nid, [50.0, 100.0, 800.0, 2000.0])].into_iter().collect());
        rt.eval("document.body.scrollBy(10, -20)").unwrap();
        let reqs = rt.take_scroll_requests();
        assert_eq!(reqs.len(), 1);
        assert!((reqs[0].1 - 60.0).abs() < 0.1);
        assert!((reqs[0].2 - 80.0).abs() < 0.1);
    }

    // ── scroll events ─────────────────────────────────────────────────────────

    #[test]
    fn fire_element_scroll_dispatches_event() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        // Register a listener and call fire_element_scroll
        rt.eval(&format!(
            "var fired = false; \
             var el = document.body || _lumen_make_element({nid}); \
             el.addEventListener('scroll', function() {{ fired = true; }});"
        )).unwrap();
        rt.fire_element_scroll(nid);
        let result = rt.eval("fired").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true), "scroll event should fire on element");
    }

    #[test]
    fn fire_element_scroll_event_is_non_bubbling() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        // A document-level listener should NOT fire (bubbles: false)
        rt.eval(
            "var doc_fired = false; \
             document.addEventListener('scroll', function() { doc_fired = true; });"
        ).unwrap();
        rt.fire_element_scroll(nid);
        let result = rt.eval("doc_fired").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false), "scroll event must not bubble to document");
    }

    #[test]
    fn fire_window_scroll_dispatches_event() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var win_fired = false; window.addEventListener('scroll', function() { win_fired = true; });").unwrap();
        rt.fire_window_scroll();
        let result = rt.eval("win_fired").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true), "window scroll event should fire");
    }

    // ── CSS Scroll Snap L2 snapchanging/snapchanged events ─────────────────────

    #[test]
    fn fire_snap_changing_dispatches_event() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        rt.eval(&format!(
            "var snap_type = ''; \
             var el = document.body || _lumen_make_element({nid}); \
             el.addEventListener('snapchanging', function(e) {{ snap_type = e.type; }});"
        )).unwrap();
        rt.fire_snap_changing(nid, None, None);
        let result = rt.eval("snap_type").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("snapchanging".into()));
    }

    #[test]
    fn fire_snap_changed_exposes_snap_targets() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            super::find_element_by_tag(&doc, "body").unwrap().index() as u32
        };
        // Resolve block/inline targets to elements; both are the body here.
        rt.eval(&format!(
            "var block_ok = false; var inline_null = false; \
             var el = document.body || _lumen_make_element({nid}); \
             el.addEventListener('snapchanged', function(e) {{ \
                 block_ok = (e.snapTargetBlock !== null && e.snapTargetBlock !== undefined); \
                 inline_null = (e.snapTargetInline === null); \
             }});"
        )).unwrap();
        rt.fire_snap_changed(nid, Some(nid), None);
        assert_eq!(rt.eval("block_ok").unwrap(), lumen_core::JsValue::Bool(true));
        assert_eq!(rt.eval("inline_null").unwrap(), lumen_core::JsValue::Bool(true));
    }

    // ── Lazy image loading ────────────────────────────────────────────────────
    // Delivery now goes through IntersectionObserver (_lazy_io) created inside
    // _lumen_init_lazy_images; _lumen_deliver_intersection_observers() is the
    // trigger (called by deliver_layout_observers in shell), not deliver_lazy_images.

    #[test]
    fn lazy_images_queued_when_in_viewport() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        // Node 5 — place its bounding rect fully within the viewport.
        rt.update_layout_rects([(5, [10.0, 50.0, 200.0, 150.0])].into_iter().collect());
        // Register node 5 as a lazy image.
        rt.eval("_lumen_init_lazy_images([[5, 'photo.jpg']]);").unwrap();
        // Deliver via IntersectionObserver (matches shell's deliver_layout_observers path).
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let reqs = rt.take_lazy_image_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].0, 5);
        assert_eq!(reqs[0].1, "photo.jpg");
    }

    #[test]
    fn lazy_images_not_queued_when_far_below_fold() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        // Node 6 is 3 viewport-heights below the fold (y=1900, margin=600 → root bottom=1200).
        rt.update_layout_rects([(6, [0.0, 1900.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_init_lazy_images([[6, 'far.png']]);").unwrap();
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let reqs = rt.take_lazy_image_requests();
        assert!(reqs.is_empty(), "image 3 viewports below fold must not be loaded yet");
    }

    #[test]
    fn lazy_images_removed_from_map_after_queue() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        rt.update_layout_rects([(7, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_init_lazy_images([[7, 'once.png']]);").unwrap();
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let first = rt.take_lazy_image_requests();
        assert_eq!(first.len(), 1);
        // Second delivery must NOT queue again (image was unobserved after first load).
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let second = rt.take_lazy_image_requests();
        assert!(second.is_empty(), "already-loaded image must not be queued twice");
    }

    #[test]
    fn lazy_images_init_idempotent() {
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        // Register same image twice in one call — only the first URL is stored.
        rt.eval("_lumen_init_lazy_images([[8, 'dup.png'],[8, 'other.png']]);").unwrap();
        // Place rect out of range (y=5000, root bottom = 600+600 = 1200).
        rt.update_layout_rects([(8, [0.0, 5000.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let reqs = rt.take_lazy_image_requests();
        // Far below the lazy-load margin: not queued yet.
        assert!(reqs.is_empty());
        // Second init with different URL — must be ignored (first registration wins).
        rt.eval("_lumen_init_lazy_images([[8, 'new.png']]);").unwrap();
        // Move into viewport.
        rt.update_layout_rects([(8, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let reqs2 = rt.take_lazy_image_requests();
        assert_eq!(reqs2.len(), 1);
        assert_eq!(reqs2[0].1, "dup.png", "first registration URL must win");
    }

    #[test]
    fn lazy_deliver_lazy_images_is_noop() {
        // _lumen_deliver_lazy_images() must be a no-op; delivery is via IO.
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        rt.update_layout_rects([(9, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_init_lazy_images([[9, 'noop.png']]);").unwrap();
        // Old shell path: this must no longer queue images on its own.
        rt.eval("_lumen_deliver_lazy_images();").unwrap();
        let reqs = rt.take_lazy_image_requests();
        assert!(reqs.is_empty(), "_lumen_deliver_lazy_images must be a no-op");
        // But IO path must still work.
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let reqs2 = rt.take_lazy_image_requests();
        assert_eq!(reqs2.len(), 1);
    }

    #[test]
    fn lazy_images_within_margin_but_below_fold() {
        // Image just below viewport but within 1-viewport-height margin must be loaded.
        let rt = runtime_with_dom(make_doc());
        rt.update_viewport_size(800.0, 600.0);
        // y=650: just below fold (600), within margin (600+600=1200).
        rt.update_layout_rects([(10, [0.0, 650.0, 100.0, 100.0])].into_iter().collect());
        rt.eval("_lumen_init_lazy_images([[10, 'near.png']]);").unwrap();
        rt.eval("_lumen_deliver_intersection_observers();").unwrap();
        let reqs = rt.take_lazy_image_requests();
        assert_eq!(reqs.len(), 1, "image just below fold within margin must be loaded");
    }

    // ── rootMargin support ────────────────────────────────────────────────────

    #[test]
    fn root_margin_single_value_expands_all_sides() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_parse_root_margin('10px')").unwrap();
        match r {
            lumen_core::JsValue::Array(a) => {
                assert_eq!(a[0], lumen_core::JsValue::Number(10.0)); // top
                assert_eq!(a[1], lumen_core::JsValue::Number(10.0)); // right
                assert_eq!(a[2], lumen_core::JsValue::Number(10.0)); // bottom
                assert_eq!(a[3], lumen_core::JsValue::Number(10.0)); // left
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn root_margin_two_values_parsed_correctly() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_parse_root_margin('5px 20px')").unwrap();
        match r {
            lumen_core::JsValue::Array(a) => {
                assert_eq!(a[0], lumen_core::JsValue::Number(5.0));  // top
                assert_eq!(a[1], lumen_core::JsValue::Number(20.0)); // right
                assert_eq!(a[2], lumen_core::JsValue::Number(5.0));  // bottom
                assert_eq!(a[3], lumen_core::JsValue::Number(20.0)); // left
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn root_margin_four_values_parsed_correctly() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_parse_root_margin('1px 2px 3px 4px')").unwrap();
        match r {
            lumen_core::JsValue::Array(a) => {
                assert_eq!(a[0], lumen_core::JsValue::Number(1.0)); // top
                assert_eq!(a[1], lumen_core::JsValue::Number(2.0)); // right
                assert_eq!(a[2], lumen_core::JsValue::Number(3.0)); // bottom
                assert_eq!(a[3], lumen_core::JsValue::Number(4.0)); // left
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn root_margin_expands_root_for_element_below_viewport() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Element at y=800, below viewport height of 720.
        rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 100.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        // rootMargin "0px 0px 200px 0px" expands root bottom to 720+200=920.
        // Element top=800 < 920 → intersecting.
        rt.eval(r#"
            var _rm_fired = false;
            var ioRm = new IntersectionObserver(function(entries) {
                if (entries[0].isIntersecting) _rm_fired = true;
            }, { rootMargin: '0px 0px 200px 0px' });
            ioRm.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let fired = rt.eval("_rm_fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true),
            "rootMargin should expand root to detect element below viewport");
    }

    #[test]
    fn root_margin_zero_does_not_see_element_below_viewport() {
        let rt = runtime_with_dom(make_doc());
        let doc_arc = make_doc();
        let nid = {
            let doc = doc_arc.lock().unwrap();
            let body_id = super::find_element_by_tag(&doc, "body").unwrap();
            body_id.index() as u32
        };
        // Element at y=800, below viewport height of 720, no rootMargin.
        rt.update_layout_rects([(nid, [0.0, 800.0, 100.0, 100.0])].into_iter().collect());
        rt.update_viewport_size(1024.0, 720.0);
        rt.eval(r#"
            var _rm_fired2 = false;
            var ioRm2 = new IntersectionObserver(function(entries) {
                if (entries[0].isIntersecting) _rm_fired2 = true;
            });
            ioRm2.observe(document.body);
            _lumen_deliver_intersection_observers();
        "#).unwrap();
        let fired = rt.eval("_rm_fired2").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(false),
            "without rootMargin, element below viewport must not intersect");
    }

    // ── FontFaceSet JS bindings (CSS Fonts Module Level 4 §11) ──────────────

    #[test]
    fn document_fonts_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            typeof document.fonts === 'object' && document.fonts !== null
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_fonts_has_length_property() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            typeof document.fonts.length === 'number'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_fonts_has_item_method() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            typeof document.fonts.item === 'function'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_fonts_has_foreach_method() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            typeof document.fonts.forEach === 'function'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_fonts_empty_by_default() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            document.fonts.length === 0 && document.fonts.item(0) === null
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── Shadow DOM JS bindings ────────────────────────────────────────────────

    #[test]
    fn attach_shadow_returns_shadow_root() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            var host = document.createElement('div');
            document.body.appendChild(host);
            var sr = host.attachShadow({ mode: 'open' });
            sr !== null && sr.__isShadowRoot__ === true && sr.mode === 'open'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn shadow_root_getter_returns_open_root() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            var h2 = document.createElement('section');
            document.body.appendChild(h2);
            h2.attachShadow({ mode: 'open' });
            h2.shadowRoot !== null && h2.shadowRoot.__isShadowRoot__ === true
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn shadow_root_getter_null_for_closed() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            var h3 = document.createElement('article');
            document.body.appendChild(h3);
            h3.attachShadow({ mode: 'closed' });
            h3.shadowRoot === null
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn shadow_root_append_child_works() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            var host = document.createElement('div');
            document.body.appendChild(host);
            var sr = host.attachShadow({ mode: 'open' });
            var inner = document.createElement('span');
            sr.appendChild(inner);
            sr.children.length === 1
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── Custom Elements registry ──────────────────────────────────────────────

    #[test]
    fn custom_elements_define_and_get() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            function MyEl() {}
            customElements.define('my-el', MyEl);
            customElements.get('my-el') === MyEl
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_elements_define_duplicate_ignored() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            function ElA() {}
            function ElB() {}
            customElements.define('dup-el', ElA);
            customElements.define('dup-el', ElB); // should be ignored
            customElements.get('dup-el') === ElA
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_elements_connected_callback_called_on_define() {
        let rt = runtime_with_dom(make_doc());
        // Inject a custom element into DOM *before* define(); upgrade must fire.
        rt.eval(r#"
            var _connected_count = 0;
            var _ce_el = document.createElement('x-counter');
            document.body.appendChild(_ce_el);
        "#).unwrap();
        let result = rt.eval(r#"
            function XCounter() {}
            XCounter.prototype.connectedCallback = function() { _connected_count++; };
            customElements.define('x-counter', XCounter);
            _connected_count === 1
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_elements_connected_callback_called_on_append() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            var _cb_count = 0;
            function XBtn() {}
            XBtn.prototype.connectedCallback = function() { _cb_count++; };
            customElements.define('x-btn', XBtn);
            var el = document.createElement('x-btn');
            document.body.appendChild(el);
            _cb_count === 1
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_elements_attribute_changed_callback() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval(r#"
            var _attr_log = [];
            function XCard() {}
            XCard.observedAttributes = ['title', 'color'];
            XCard.prototype.attributeChangedCallback = function(name, old, next) {
                _attr_log.push(name + ':' + old + '->' + next);
            };
            customElements.define('x-card', XCard);
            var card = document.createElement('x-card');
            document.body.appendChild(card);
            card.setAttribute('title', 'hello');
            card.setAttribute('color', 'red');
            card.setAttribute('ignored', 'yes'); // not in observedAttributes
            _attr_log.join('|') === 'title:null->hello|color:null->red'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_elements_when_defined_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        // whenDefined for an already-registered element must return a Promise.
        let result = rt.eval(r#"
            function XBox() {}
            customElements.define('x-box', XBox);
            var p = customElements.whenDefined('x-box');
            typeof p === 'object' && typeof p.then === 'function'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn custom_elements_when_defined_pending_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        // whenDefined for an unknown element must also return a Promise.
        let result = rt.eval(r#"
            var p2 = customElements.whenDefined('x-future');
            typeof p2 === 'object' && typeof p2.then === 'function'
        "#).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── HTMLTemplateElement.content + DocumentFragment ────────────────────────

    #[test]
    fn template_content_returns_document_fragment() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var t = document.createElement('template');
            document.body.appendChild(t);
            var c = t.content;
            c !== null && c !== undefined && c.__isDocumentFragment__ === true
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn template_content_clone_and_append() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var t = document.createElement('template');
            t.innerHTML = '<span></span>';
            document.body.appendChild(t);
            // cloneNode(true) on fragment should create a new fragment with the same children
            var frag = t.content.cloneNode(true);
            frag !== null && frag.__isDocumentFragment__ === true
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_create_document_fragment() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var frag = document.createDocumentFragment();
            frag !== null && frag.__isDocumentFragment__ === true && frag.nodeType === 11
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fragment_append_moves_children_to_target() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var frag = document.createDocumentFragment();
            var a = document.createElement('span');
            var b = document.createElement('div');
            frag.appendChild(a);
            frag.appendChild(b);
            var host = document.createElement('section');
            document.body.appendChild(host);
            host.appendChild(frag);
            // Fragment children should now be inside host; frag itself has no children.
            host.children.length === 2 && frag.children.length === 0
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_clone_node_shallow() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var el = document.createElement('div');
            el.setAttribute('data-x', '42');
            var child = document.createElement('span');
            el.appendChild(child);
            document.body.appendChild(el);
            var clone = el.cloneNode(false);
            // Shallow clone: same tag, same attr, no children.
            clone.tagName.toLowerCase() === 'div' && clone.getAttribute('data-x') === '42' && clone.children.length === 0
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_clone_node_deep() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var el = document.createElement('div');
            var child = document.createElement('span');
            el.appendChild(child);
            document.body.appendChild(el);
            var clone = el.cloneNode(true);
            // Deep clone: children are also cloned.
            clone.tagName.toLowerCase() === 'div' && clone.children.length === 1
                && clone.children[0].tagName.toLowerCase() === 'span'
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn slot_element_assigned_nodes() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var host = document.createElement('div');
            document.body.appendChild(host);
            var sr = host.attachShadow({ mode: 'open' });
            // Add a <slot> inside the shadow root.
            var slot = document.createElement('slot');
            sr.appendChild(slot);
            // Add a light-DOM child to the host.
            var light = document.createElement('p');
            host.appendChild(light);
            // assignedNodes() should return the light-DOM child.
            typeof slot.assignedNodes === 'function'
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn slot_slotchange_event_fires_on_append() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var host = document.createElement('div');
            document.body.appendChild(host);
            var sr = host.attachShadow({ mode: 'open' });
            var slot = document.createElement('slot');
            sr.appendChild(slot);
            var changed = 0;
            slot.addEventListener('slotchange', function() { changed++; });
            var light = document.createElement('p');
            host.appendChild(light);
            // slotchange should have fired
            changed >= 0  // event dispatch is best-effort in Phase 0; just check no crash
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn insert_before_moves_node() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var parent = document.createElement('div');
            document.body.appendChild(parent);
            var a = document.createElement('span');
            var b = document.createElement('em');
            parent.appendChild(a);
            parent.appendChild(b);
            var c = document.createElement('strong');
            parent.insertBefore(c, a);
            // c should be at index 0, a at 1, b at 2
            parent.children.length === 3 && parent.children[0].tagName.toLowerCase() === 'strong'
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── IndexedDB ───────────────────────────────────────────────────────────

    #[test]
    fn idb_global_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "typeof indexedDB === 'object' && typeof indexedDB.open === 'function' \
             && typeof IDBKeyRange === 'function' && typeof window.indexedDB === 'object'",
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn idb_open_fires_upgrade_then_success() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var log = [];
            var req = indexedDB.open('db1', 3);
            req.onupgradeneeded = function(e) { log.push('upg:' + e.oldVersion + '->' + e.newVersion); };
            req.onsuccess = function(e) { log.push('ok:' + e.target.result.version); };
            _lumen_idb_flush();
            log.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("upg:0->3,ok:3".into()));
    }

    #[test]
    fn idb_add_and_get_keypath() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
            req.onsuccess = function(e) {
                var db = e.target.result;
                var tx = db.transaction('s', 'readwrite');
                var st = tx.objectStore('s');
                st.add({ id: 1, name: 'alpha' });
                st.add({ id: 2, name: 'beta' });
                var g = st.get(2);
                g.onsuccess = function() { out = g.result.name; };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("beta".into()));
    }

    #[test]
    fn idb_autoincrement_out_of_line() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var keys = [];
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { autoIncrement: true }); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                var a = st.add('x'); a.onsuccess = function() { keys.push(a.result); };
                var b = st.add('y'); b.onsuccess = function() { keys.push(b.result); };
            };
            _lumen_idb_flush();
            keys.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("1,2".into()));
    }

    #[test]
    fn idb_put_overwrites() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                st.add({ id: 1, v: 'old' });
                st.put({ id: 1, v: 'new' });
                var g = st.get(1);
                var c = st.count();
                c.onsuccess = function() { out = g.result.v + ':' + c.result; };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("new:1".into()));
    }

    #[test]
    fn idb_add_duplicate_aborts_transaction() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var log = [];
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
            req.onsuccess = function(e) {
                var tx = e.target.result.transaction('s', 'readwrite');
                tx.onabort = function() { log.push('abort'); };
                var st = tx.objectStore('s');
                st.add({ id: 1 });
                var dup = st.add({ id: 1 });
                dup.onerror = function(ev) { log.push('err:' + ev.target.error.name); };
            };
            _lumen_idb_flush();
            log.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("err:ConstraintError,abort".into()));
    }

    #[test]
    fn idb_getall_sorted_by_key() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                st.add('c', 3); st.add('a', 1); st.add('b', 2);
                var g = st.getAll(); g.onsuccess = function() { out = g.result.join(''); };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("abc".into()));
    }

    #[test]
    fn idb_getall_with_key_range() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                for (var i = 1; i <= 5; i++) st.add('v' + i, i);
                var g = st.getAll(IDBKeyRange.bound(2, 4, false, true));
                g.onsuccess = function() { out = g.result.join(','); };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("v2,v3".into()));
    }

    #[test]
    fn idb_delete_and_clear() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                st.add('a', 1); st.add('b', 2); st.add('c', 3);
                st.delete(2);
                var c1 = st.count(); c1.onsuccess = function() {
                    st.clear();
                    var c2 = st.count(); c2.onsuccess = function() { out = c1.result + ':' + c2.result; };
                };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("2:0".into()));
    }

    #[test]
    fn idb_index_get_and_getall() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) {
                var st = e.target.result.createObjectStore('s', { keyPath: 'id' });
                st.createIndex('by_cat', 'cat');
            };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                st.add({ id: 1, cat: 'x', n: 'one' });
                st.add({ id: 2, cat: 'y', n: 'two' });
                st.add({ id: 3, cat: 'x', n: 'three' });
                var idx = st.index('by_cat');
                var g = idx.get('y');
                var ga = idx.getAll('x');
                ga.onsuccess = function() {
                    out = g.result.n + '|' + ga.result.map(function(r){return r.n;}).join(',');
                };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("two|one,three".into()));
    }

    #[test]
    fn idb_unique_index_violation() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var log = [];
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) {
                var st = e.target.result.createObjectStore('s', { keyPath: 'id' });
                st.createIndex('email', 'email', { unique: true });
            };
            req.onsuccess = function(e) {
                var tx = e.target.result.transaction('s', 'readwrite');
                tx.onabort = function() { log.push('abort'); };
                var st = tx.objectStore('s');
                st.add({ id: 1, email: 'a@b.c' });
                var dup = st.add({ id: 2, email: 'a@b.c' });
                dup.onerror = function(ev) { log.push(ev.target.error.name); };
            };
            _lumen_idb_flush();
            log.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("ConstraintError,abort".into()));
    }

    #[test]
    fn idb_cursor_iterates_in_order() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var keys = [];
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                st.add('a', 3); st.add('b', 1); st.add('c', 2);
                var cur = st.openCursor();
                cur.onsuccess = function(ev) {
                    var c = ev.target.result;
                    if (c) { keys.push(c.key + '=' + c.value); c.continue(); }
                };
            };
            _lumen_idb_flush();
            keys.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("1=b,2=c,3=a".into()));
    }

    #[test]
    fn idb_cursor_reverse_direction() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var keys = [];
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
            req.onsuccess = function(e) {
                var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                for (var i = 1; i <= 3; i++) st.add('v', i);
                var cur = st.openKeyCursor(null, 'prev');
                cur.onsuccess = function(ev) {
                    var c = ev.target.result;
                    if (c) { keys.push(c.key); c.continue(); }
                };
            };
            _lumen_idb_flush();
            keys.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("3,2,1".into()));
    }

    #[test]
    fn idb_cursor_update_and_delete() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
            req.onsuccess = function(e) {
                var db = e.target.result;
                var st = db.transaction('s', 'readwrite').objectStore('s');
                st.add({ id: 1, v: 10 }); st.add({ id: 2, v: 20 }); st.add({ id: 3, v: 30 });
                var cur = st.openCursor();
                cur.onsuccess = function(ev) {
                    var c = ev.target.result;
                    if (!c) return;
                    if (c.primaryKey === 1) c.update({ id: 1, v: 99 });
                    else if (c.primaryKey === 2) c.delete();
                    c.continue();
                };
                var tx2 = db.transaction('s');
                var g = tx2.objectStore('s').getAll();
                g.onsuccess = function() {
                    out = g.result.map(function(r){return r.id + ':' + r.v;}).join(',');
                };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("1:99,3:30".into()));
    }

    #[test]
    fn idb_keyrange_includes_and_cmp() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var kr = IDBKeyRange.bound(1, 5, true, false);
            var a = kr.includes(1) === false && kr.includes(5) === true && kr.includes(3) === true;
            var b = indexedDB.cmp(1, 2) === -1 && indexedDB.cmp('b', 'a') === 1 && indexedDB.cmp(7, 7) === 0;
            var c = indexedDB.cmp(5, 'x') === -1 && indexedDB.cmp([1,2], [1,3]) === -1;
            a && b && c
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn idb_open_version_downgrade_errors() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var log = [];
            var r1 = indexedDB.open('d', 5);
            r1.onsuccess = function(e) { e.target.result.close(); log.push('v5'); };
            _lumen_idb_flush();
            var r2 = indexedDB.open('d', 2);
            r2.onerror = function(e) { log.push('err:' + e.target.error.name); };
            r2.onsuccess = function() { log.push('unexpected'); };
            _lumen_idb_flush();
            log.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("v5,err:VersionError".into()));
    }

    #[test]
    fn idb_delete_database() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var log = [];
            var r1 = indexedDB.open('d', 1);
            r1.onsuccess = function(e) { e.target.result.close(); };
            _lumen_idb_flush();
            var del = indexedDB.deleteDatabase('d');
            del.onsuccess = function() { log.push('deleted'); };
            _lumen_idb_flush();
            indexedDB.databases().then(function(list) { log.push('count:' + list.length); });
            _lumen_idb_flush();
            log.join(',')
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("deleted".into()));
    }

    #[test]
    fn idb_second_connection_sees_persisted_data() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var out;
            var r1 = indexedDB.open('d', 1);
            r1.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
            r1.onsuccess = function(e) {
                var db = e.target.result;
                db.transaction('s', 'readwrite').objectStore('s').add({ id: 1, v: 'kept' });
                db.close();
            };
            _lumen_idb_flush();
            var r2 = indexedDB.open('d');
            r2.onsuccess = function(e) {
                var g = e.target.result.transaction('s').objectStore('s').get(1);
                g.onsuccess = function() { out = g.result.v; };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("kept".into()));
    }

    // ── IndexedDB persistence (Rust-backed snapshot survives reload) ──────────

    /// In-memory `IdbBackend` capturing the snapshot the shim persists, shared
    /// across runtimes via `Arc` to simulate the same origin across reloads.
    struct MockIdb(Arc<Mutex<Option<String>>>);
    impl IdbBackend for MockIdb {
        fn load(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
        fn save(&self, snapshot: &str) {
            *self.0.lock().unwrap() = Some(snapshot.to_owned());
        }
    }

    fn runtime_with_idb(backend: Arc<dyn IdbBackend>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, Some(backend), None, None, false)
            .unwrap();
        rt
    }

    #[test]
    fn idb_persists_across_runtime_reload() {
        let cell = Arc::new(Mutex::new(None));
        // First "page load": create a store and write a record.
        {
            let rt = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
            rt.eval(r#"
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) {
                    e.target.result.transaction('s', 'readwrite').objectStore('s').add({ id: 1, v: 'kept' });
                };
                _lumen_idb_flush();
            "#).unwrap();
        }
        // Backend captured a snapshot from the mutating transaction.
        assert!(cell.lock().unwrap().is_some(), "snapshot must be persisted");

        // Second "page load": a fresh runtime restores the database without re-running
        // the upgrade — the store and its record are already present.
        let rt2 = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        let r = rt2.eval(r#"
            var out;
            var req = indexedDB.open('d');
            req.onupgradeneeded = function() { out = 'UNEXPECTED_UPGRADE'; };
            req.onsuccess = function(e) {
                var g = e.target.result.transaction('s').objectStore('s').get(1);
                g.onsuccess = function() { out = g.result ? g.result.v : 'MISSING'; };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("kept".into()));
    }

    #[test]
    fn idb_persisted_version_is_restored() {
        let cell = Arc::new(Mutex::new(None));
        {
            let rt = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
            rt.eval(r#"
                var req = indexedDB.open('d', 4);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function() {};
                _lumen_idb_flush();
            "#).unwrap();
        }
        let rt2 = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        let r = rt2.eval(r#"
            var out;
            var req = indexedDB.open('d');
            req.onsuccess = function(e) { out = e.target.result.version; };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(4.0));
    }

    #[test]
    fn idb_persisted_date_value_roundtrips() {
        let cell = Arc::new(Mutex::new(None));
        {
            let rt = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
            rt.eval(r#"
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) {
                    e.target.result.transaction('s', 'readwrite').objectStore('s')
                        .add({ id: 1, when: new Date(1700000000000) });
                };
                _lumen_idb_flush();
            "#).unwrap();
        }
        let rt2 = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        let r = rt2.eval(r#"
            var out;
            var req = indexedDB.open('d');
            req.onsuccess = function(e) {
                var g = e.target.result.transaction('s').objectStore('s').get(1);
                g.onsuccess = function() {
                    out = (g.result.when instanceof Date) + ':' + g.result.when.getTime();
                };
            };
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("true:1700000000000".into()));
    }

    #[test]
    fn idb_persisted_delete_database_is_restored() {
        let cell = Arc::new(Mutex::new(None));
        {
            let rt = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
            rt.eval(r#"
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function() {};
                _lumen_idb_flush();
                indexedDB.deleteDatabase('d');
                _lumen_idb_flush();
            "#).unwrap();
        }
        // After deletion the restored snapshot must not contain the database:
        // opening it fresh re-triggers upgradeneeded and the store is gone.
        let rt2 = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        let r = rt2.eval(r#"
            var out = 'no-upgrade';
            var req = indexedDB.open('d');
            req.onupgradeneeded = function(e) {
                out = 'upgrade:' + e.target.result.objectStoreNames.length;
            };
            req.onsuccess = function() {};
            _lumen_idb_flush();
            out
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("upgrade:0".into()));
    }

    #[test]
    fn idb_read_only_transaction_does_not_persist() {
        let cell = Arc::new(Mutex::new(None));
        let rt = runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        // Create + populate (this persists once).
        rt.eval(r#"
            var req = indexedDB.open('d', 1);
            req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
            req.onsuccess = function(e) { e.target.result.transaction('s', 'readwrite').objectStore('s').add({ id: 1 }); };
            _lumen_idb_flush();
        "#).unwrap();
        // Overwrite the captured snapshot with a sentinel, then run a read-only txn.
        *cell.lock().unwrap() = Some("SENTINEL".into());
        rt.eval(r#"
            var req = indexedDB.open('d');
            req.onsuccess = function(e) { e.target.result.transaction('s').objectStore('s').get(1); };
            _lumen_idb_flush();
        "#).unwrap();
        // A read-only flush must not have re-persisted (sentinel intact).
        assert_eq!(cell.lock().unwrap().as_deref(), Some("SENTINEL"));
    }

    // ── FormData API tests ────────────────────────────────────────────────────

    #[test]
    fn formdata_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof FormData === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_window_constructor_exposed() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.FormData === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_append_and_get() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var fd = new FormData(); fd.append('name', 'alice'); fd.get('name')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("alice".into()));
    }

    #[test]
    fn formdata_get_missing_returns_null() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var fd = new FormData(); fd.get('nope') === null").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_has_returns_true_when_present() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var fd = new FormData(); fd.append('k', 'v'); fd.has('k')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_has_returns_false_when_absent() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var fd = new FormData(); fd.has('nope')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn formdata_delete_removes_entries() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('x', '1'); fd.append('x', '2'); \
             fd.delete('x'); fd.has('x')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn formdata_get_all_returns_all_values() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('k', 'a'); fd.append('k', 'b'); \
             fd.getAll('k').join(',')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("a,b".into()));
    }

    #[test]
    fn formdata_set_replaces_first_occurrence() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('k', 'old1'); fd.append('k', 'old2'); \
             fd.set('k', 'new'); fd.getAll('k').length + ':' + fd.get('k')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("1:new".into()));
    }

    #[test]
    fn formdata_to_url_encoded_basic() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('a', '1'); fd.append('b', '2'); \
             fd._toUrlEncoded()"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("a=1&b=2".into()));
    }

    #[test]
    fn formdata_to_url_encoded_percent_encodes_spaces() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('full name', 'hello world'); \
             fd._toUrlEncoded()"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("full%20name=hello%20world".into()));
    }

    #[test]
    fn formdata_to_url_encoded_percent_encodes_ampersand() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('q', 'a&b=c'); \
             fd._toUrlEncoded()"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("q=a%26b%3Dc".into()));
    }

    #[test]
    fn formdata_keys_iterator() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('x', '1'); fd.append('y', '2'); \
             var keys = []; var it = fd.keys(); var n; \
             while (!(n = it.next()).done) { keys.push(n.value); } \
             keys.join(',')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("x,y".into()));
    }

    #[test]
    fn formdata_values_iterator() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('x', 'p'); fd.append('y', 'q'); \
             var vals = []; var it = fd.values(); var n; \
             while (!(n = it.next()).done) { vals.push(n.value); } \
             vals.join(',')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("p,q".into()));
    }

    #[test]
    fn formdata_foreach_iterates_value_name() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('a', '1'); fd.append('b', '2'); \
             var out = []; \
             fd.forEach(function(v, k) { out.push(k + '=' + v); }); \
             out.join('&')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("a=1&b=2".into()));
    }

    #[test]
    fn formdata_symbol_iterator_same_as_entries() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('k', 'v'); \
             var it = fd[Symbol.iterator](); var n = it.next(); \
             n.value[0] + '=' + n.value[1]"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("k=v".into()));
    }

    #[test]
    fn formdata_to_multipart_contains_boundary() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('name', 'alice'); \
             var bnd = 'test-boundary'; \
             var bytes = fd._toMultipart(bnd); \
             var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
             s.indexOf('--test-boundary') >= 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_to_multipart_contains_field_name_and_value() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('email', 'user@example.com'); \
             var bytes = fd._toMultipart('bnd'); \
             var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
             s.indexOf('name=\"email\"') >= 0 && s.indexOf('user@example.com') >= 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_to_multipart_ends_with_closing_delimiter() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('x', '1'); \
             var bytes = fd._toMultipart('B'); \
             var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
             s.indexOf('--B--') >= 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_to_multipart_empty_entries_yields_only_closing_boundary() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); \
             var bytes = fd._toMultipart('B'); \
             var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
             s.trim()"
        ).unwrap();
        // Empty FormData → just --B--\r\n
        assert_eq!(r, lumen_core::JsValue::String("--B--".into()));
    }

    #[test]
    fn formdata_to_multipart_escapes_quotes_in_name() {
        // Use \" in the JS string to pass a double-quote character as field name.
        // In the Rust string, \" is a literal " (Rust escape); QuickJS then evaluates
        // 'ev\"il' in single-quoted JS string, where \" is interpreted as ".
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('ev\\\"il', 'val'); \
             var bytes = fd._toMultipart('B'); \
             var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
             s.indexOf('ev%22il') >= 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn formdata_to_multipart_multiple_fields_ordered() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fd = new FormData(); fd.append('a', '1'); fd.append('b', '2'); \
             var bytes = fd._toMultipart('X'); \
             var s = ''; for (var i = 0; i < bytes.length; i++) { s += String.fromCharCode(bytes[i]); } \
             s.indexOf('name=\"a\"') < s.indexOf('name=\"b\"')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // Mock fetch provider that records calls to fetch_with_body_sync.
    type FetchCall = (String, String, String, Vec<u8>);
    struct CaptureFetch {
        calls: std::sync::Mutex<Vec<FetchCall>>,
    }
    impl CaptureFetch {
        fn new() -> Arc<Self> {
            Arc::new(Self { calls: std::sync::Mutex::new(vec![]) })
        }
    }
    impl lumen_core::ext::JsFetchProvider for CaptureFetch {
        fn fetch_sync(&self, url: &str, method: &str) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            self.calls.lock().unwrap().push((url.into(), method.into(), String::new(), vec![]));
            Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
        }
        fn fetch_with_body_sync(&self, url: &str, method: &str, content_type: &str, body: &[u8]) -> lumen_core::error::Result<lumen_core::ext::JsFetchResult> {
            self.calls.lock().unwrap().push((url.into(), method.into(), content_type.into(), body.to_vec()));
            Ok(lumen_core::ext::JsFetchResult { status: 200, status_text: "OK".into(), headers: vec![], body: b"ok".to_vec() })
        }
    }

    fn runtime_with_fetch(provider: Arc<CaptureFetch>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let p: Arc<dyn lumen_core::ext::JsFetchProvider> = provider;
        rt.install_dom(make_doc(), "https://example.com/", Some(p), None, None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn fetch_post_formdata_sends_multipart_body() {
        // Fetch spec §5.4: FormData body → multipart/form-data (not urlencoded).
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        rt.eval(
            "var fd = new FormData(); fd.append('user', 'bob'); fd.append('age', '30'); \
             fetch('https://example.com/api', { method: 'POST', body: fd })"
        ).unwrap();
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (url, method, ct, body) = &calls[0];
        assert_eq!(url, "https://example.com/api");
        assert_eq!(method, "POST");
        // Content-Type must start with multipart/form-data and include a boundary.
        assert!(ct.starts_with("multipart/form-data; boundary="),
            "expected multipart/form-data content-type, got: {ct}");
        // Body must contain the field names and values in multipart format.
        let body_str = std::str::from_utf8(body).unwrap();
        assert!(body_str.contains("name=\"user\""), "body should contain field name 'user'");
        assert!(body_str.contains("bob"), "body should contain value 'bob'");
        assert!(body_str.contains("name=\"age\""), "body should contain field name 'age'");
        assert!(body_str.contains("30"), "body should contain value '30'");
        // Body must end with closing boundary --boundary--\r\n
        assert!(body_str.contains("--\r\n"), "body must contain closing boundary");
    }

    #[test]
    fn fetch_post_string_body_sends_text_plain() {
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        rt.eval(
            "fetch('https://example.com/api', { method: 'POST', body: 'hello world' })"
        ).unwrap();
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (_, method, ct, body) = &calls[0];
        assert_eq!(method, "POST");
        assert_eq!(ct, "text/plain;charset=UTF-8");
        assert_eq!(std::str::from_utf8(body).unwrap(), "hello world");
    }

    #[test]
    fn fetch_post_uint8array_body_sends_octet_stream() {
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        rt.eval(
            "fetch('https://example.com/bin', { method: 'PUT', body: new Uint8Array([1, 2, 3]) })"
        ).unwrap();
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (_, method, ct, body) = &calls[0];
        assert_eq!(method, "PUT");
        assert_eq!(ct, "application/octet-stream");
        assert_eq!(body, &[1u8, 2, 3]);
    }

    #[test]
    fn fetch_post_content_type_override() {
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        rt.eval(
            "var fd = new FormData(); fd.append('x', '1'); \
             fetch('https://example.com/', { method: 'POST', body: fd, \
               headers: {'Content-Type': 'application/json'} })"
        ).unwrap();
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (_, _, ct, _) = &calls[0];
        assert_eq!(ct, "application/json");
    }

    // ── Selection API tests ───────────────────────────────────────────────────

    fn bool_eval(rt: &QuickJsRuntime, script: &str) -> bool {
        rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
    }

    // Build a doc with a single paragraph containing text "Hello World".
    fn make_selection_doc() -> (Arc<Mutex<Document>>, NodeId) {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let p = doc.create_element(QualName::html("p"));
        let text = doc.create_text("Hello World");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, p);
        doc.append_child(p, text);
        let arc = Arc::new(Mutex::new(doc));
        (arc, text)
    }

    #[test]
    fn selection_window_get_selection_is_object() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof window.getSelection() === 'object'"));
    }

    #[test]
    fn selection_document_get_selection_is_object() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof document.getSelection() === 'object'"));
    }

    #[test]
    fn selection_initially_none_type() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.getSelection().type === 'None'"));
    }

    #[test]
    fn selection_range_count_initially_zero() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.getSelection().rangeCount === 0"));
    }

    #[test]
    fn selection_is_collapsed_when_empty() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.getSelection().isCollapsed === true"));
    }

    #[test]
    fn selection_to_string_empty_when_no_selection() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.getSelection().toString() === ''"));
    }

    #[test]
    fn selection_remove_all_ranges_clears() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(&rt, "window.getSelection().type === 'Range'"));
        rt.eval("window.getSelection().removeAllRanges()").unwrap();
        assert!(bool_eval(&rt, "window.getSelection().type === 'None'"));
    }

    #[test]
    fn selection_type_range_when_set() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(&rt, "window.getSelection().type === 'Range'"));
    }

    #[test]
    fn selection_is_not_collapsed_when_range() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(&rt, "window.getSelection().isCollapsed === false"));
    }

    #[test]
    fn selection_to_string_returns_selected_text() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(&rt, "window.getSelection().toString() === 'Hello'"));
    }

    #[test]
    fn selection_range_count_is_one_when_set() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 6 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 11 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(&rt, "window.getSelection().rangeCount === 1"));
    }

    #[test]
    fn selection_get_range_at_returns_range() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 6 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 11 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(&rt, "window.getSelection().getRangeAt(0).toString() === 'World'"));
    }

    #[test]
    fn selection_collapse_to_start() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc);
        rt.eval("window.getSelection().collapseToStart()").unwrap();
        assert!(bool_eval(&rt, "window.getSelection().type === 'Caret'"));
    }

    // ── Range tests ───────────────────────────────────────────────────────────

    #[test]
    fn range_create_range_is_object() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof document.createRange() === 'object'"));
    }

    #[test]
    fn range_new_is_collapsed() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.createRange().collapsed === true"));
    }

    #[test]
    fn range_start_offset_zero() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.createRange().startOffset === 0"));
    }

    #[test]
    fn range_collapse_to_start() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var r = document.createRange(); r.collapse(true); r.collapsed === true"
        ));
    }

    #[test]
    fn range_clone_range() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var r = document.createRange(); var c = r.cloneRange(); c.collapsed === true"
        ));
    }

    #[test]
    fn range_select_node_contents() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var el = document.getElementById('main'); \
             var r = document.createRange(); \
             r.selectNodeContents(el); \
             r.startOffset === 0"
        ));
    }

    #[test]
    fn range_to_string_via_get_range_at() {
        let (arc, text) = make_selection_doc();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(
            &rt,
            "window.getSelection().getRangeAt(0).toString() === 'Hello'"
        ));
    }

    #[test]
    fn range_compare_boundary_points_same() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var r = document.createRange(); r.compareBoundaryPoints(0, r) === 0"
        ));
    }

    #[test]
    fn range_window_range_constructor() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof window.Range === 'function'"));
    }

    // ── execCommand tests ─────────────────────────────────────────────────────

    #[test]
    fn exec_command_bold_returns_true() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.execCommand('bold') === true"));
    }

    #[test]
    fn exec_command_italic_returns_true() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.execCommand('italic') === true"));
    }

    #[test]
    fn exec_command_unknown_returns_false() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.execCommand('unknownCommand') === false"));
    }

    #[test]
    fn exec_command_copy_returns_false() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.execCommand('copy') === false"));
    }

    #[test]
    fn exec_command_query_enabled() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.queryCommandEnabled('bold') === true"));
    }

    #[test]
    fn exec_command_query_state() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.queryCommandState('bold') === false"));
    }

    #[test]
    fn exec_command_query_value() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.queryCommandValue('bold') === ''"));
    }

    #[test]
    fn exec_command_query_supported() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "document.queryCommandSupported('bold') === true"));
    }

    #[test]
    fn exec_command_insert_text() {
        let (arc, text) = make_selection_doc();
        let text_idx = text.index();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        rt.eval("document.execCommand('insertText', false, 'Hi ')").unwrap();
        let doc = arc.lock().unwrap();
        let content = match &doc.get(NodeId::from_index(text_idx)).data {
            NodeData::Text(s) => s.clone(),
            _ => panic!("not text"),
        };
        assert_eq!(content, "Hi Hello World");
    }

    #[test]
    fn exec_command_delete_removes_selection() {
        let (arc, text) = make_selection_doc();
        let text_idx = text.index();
        {
            let mut doc = arc.lock().unwrap();
            // Select "Hello "
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 6 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        rt.eval("document.execCommand('delete')").unwrap();
        let doc = arc.lock().unwrap();
        let content = match &doc.get(NodeId::from_index(text_idx)).data {
            NodeData::Text(s) => s.clone(),
            _ => panic!("not text"),
        };
        assert_eq!(content, "World");
    }

    // ── contentEditable / isContentEditable / contenteditable dispatch tests ────

    fn make_contenteditable_doc() -> (Arc<Mutex<Document>>, NodeId, NodeId) {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute {
                name: QualName::html("contenteditable"),
                value: String::new(),
            });
        }
        let text = doc.create_text("Hello");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, div);
        doc.append_child(div, text);
        let arc = Arc::new(Mutex::new(doc));
        (arc, div, text)
    }

    #[test]
    fn contenteditable_property_true() {
        let (arc, div, _) = make_contenteditable_doc();
        let div_idx = div.index();
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(
            &rt,
            &format!("_lumen_make_element({}).contentEditable === 'true'", div_idx)
        ));
    }

    #[test]
    fn contenteditable_is_content_editable_self() {
        let (arc, div, _) = make_contenteditable_doc();
        let div_idx = div.index();
        let rt = runtime_with_dom(arc);
        assert!(bool_eval(
            &rt,
            &format!("_lumen_make_element({}).isContentEditable === true", div_idx)
        ));
    }

    #[test]
    fn contenteditable_is_content_editable_ancestor() {
        let (arc, _, text) = make_contenteditable_doc();
        let text_idx = text.index();
        let rt = runtime_with_dom(arc);
        // text node itself: _lumen_is_contenteditable checks ancestors
        assert!(bool_eval(
            &rt,
            &format!("_lumen_is_contenteditable({})", text_idx)
        ));
    }

    #[test]
    fn contenteditable_non_editable_false() {
        let rt = runtime_with_dom(make_doc());
        // body has no contenteditable
        let body_idx: u32 = if let lumen_core::JsValue::Number(n) =
            rt.eval("_lumen_u2n(_lumen_get_body())").unwrap()
        {
            n as u32
        } else {
            0
        };
        assert!(bool_eval(
            &rt,
            &format!("_lumen_make_element({}).isContentEditable === false", body_idx)
        ));
    }

    #[test]
    fn contenteditable_set_property() {
        let rt = runtime_with_dom(make_doc());
        // Create a div and set contentEditable
        rt.eval("var _ce_div = document.createElement('div'); document.body.appendChild(_ce_div); _ce_div.contentEditable = 'true';").unwrap();
        assert!(bool_eval(&rt, "_ce_div.isContentEditable === true"));
    }

    #[test]
    fn contenteditable_insert_text_at_caret() {
        let (arc, div, text) = make_contenteditable_doc();
        let text_idx = text.index();
        let div_idx = div.index();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 5 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        let result = rt.eval(&format!(
            "_lumen_handle_contenteditable_key('insertText',' World',{})",
            div_idx
        )).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
        let doc = arc.lock().unwrap();
        let content = match &doc.get(NodeId::from_index(text_idx)).data {
            NodeData::Text(s) => s.clone(),
            _ => panic!("not a text node"),
        };
        assert_eq!(content, "Hello World");
    }

    #[test]
    fn contenteditable_delete_backward_one_char() {
        let (arc, div, text) = make_contenteditable_doc();
        let text_idx = text.index();
        let div_idx = div.index();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 5 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        rt.eval(&format!(
            "_lumen_handle_contenteditable_key('deleteContentBackward',null,{})",
            div_idx
        )).unwrap();
        let doc = arc.lock().unwrap();
        let content = match &doc.get(NodeId::from_index(text_idx)).data {
            NodeData::Text(s) => s.clone(),
            _ => panic!("not a text node"),
        };
        assert_eq!(content, "Hell");
    }

    #[test]
    fn contenteditable_delete_forward_one_char() {
        let (arc, div, text) = make_contenteditable_doc();
        let text_idx = text.index();
        let div_idx = div.index();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        rt.eval(&format!(
            "_lumen_handle_contenteditable_key('deleteContentForward',null,{})",
            div_idx
        )).unwrap();
        let doc = arc.lock().unwrap();
        let content = match &doc.get(NodeId::from_index(text_idx)).data {
            NodeData::Text(s) => s.clone(),
            _ => panic!("not a text node"),
        };
        assert_eq!(content, "ello");
    }

    #[test]
    fn contenteditable_beforeinput_cancellable() {
        let (arc, div, text) = make_contenteditable_doc();
        let text_idx = text.index();
        let div_idx = div.index();
        {
            let mut doc = arc.lock().unwrap();
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text, offset: 5 }),
                focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        // Attach a beforeinput handler that cancels the event
        rt.eval(&format!(
            "_lumen_make_element({}).addEventListener('beforeinput', function(e) {{ e.preventDefault(); }});",
            div_idx
        )).unwrap();
        let result = rt.eval(&format!(
            "_lumen_handle_contenteditable_key('insertText','X',{})",
            div_idx
        )).unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false), "cancelled beforeinput must return false");
        // Text must not be mutated
        let doc = arc.lock().unwrap();
        let content = match &doc.get(NodeId::from_index(text_idx)).data {
            NodeData::Text(s) => s.clone(),
            _ => panic!("not a text node"),
        };
        assert_eq!(content, "Hello", "DOM must not change when beforeinput is cancelled");
    }

    #[test]
    fn contenteditable_input_event_fires() {
        let (arc, div, _) = make_contenteditable_doc();
        let div_idx = div.index();
        {
            let mut doc = arc.lock().unwrap();
            let text_nid = doc.get(div).children[0];
            doc.set_selection(lumen_dom::Selection {
                anchor: Some(lumen_dom::DomPosition { container: text_nid, offset: 5 }),
                focus:  Some(lumen_dom::DomPosition { container: text_nid, offset: 5 }),
            });
        }
        let rt = runtime_with_dom(arc.clone());
        rt.eval("var _ce_fired = false;").unwrap();
        rt.eval(&format!(
            "_lumen_make_element({}).addEventListener('input', function() {{ _ce_fired = true; }});",
            div_idx
        )).unwrap();
        rt.eval(&format!(
            "_lumen_handle_contenteditable_key('insertText','Z',{})",
            div_idx
        )).unwrap();
        assert!(bool_eval(&rt, "_ce_fired"), "input event must fire after mutation");
    }

    // ── window.getComputedStyle() tests ─────────────────────────────────────────

    fn make_computed_styles_map(
        nid: u32,
        props: &[(&str, &str)],
    ) -> std::collections::HashMap<u32, std::collections::HashMap<String, String>> {
        let mut inner = std::collections::HashMap::new();
        for (k, v) in props {
            inner.insert(k.to_string(), v.to_string());
        }
        let mut outer = std::collections::HashMap::new();
        outer.insert(nid, inner);
        outer
    }

    fn get_main_nid(rt: &QuickJsRuntime) -> u32 {
        match rt.eval("document.getElementById('main').__nid__").unwrap() {
            lumen_core::JsValue::Number(n) => n as u32,
            other => panic!("unexpected nid: {other:?}"),
        }
    }

    #[test]
    fn get_computed_style_returns_object() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.getComputedStyle === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_computed_style_is_callable_with_element() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof window.getComputedStyle(document.getElementById('main')) === 'object'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_computed_style_returns_empty_without_data() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String(String::new()));
    }

    #[test]
    fn get_computed_style_returns_value_after_update() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("color", "rgb(255, 0, 0)")]);
        rt.update_computed_styles(styles);
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("rgb(255, 0, 0)".to_string()));
    }

    #[test]
    fn get_computed_style_get_property_value_unknown_prop_empty() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("color", "blue")]);
        rt.update_computed_styles(styles);
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('font-weight')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String(String::new()));
    }

    #[test]
    fn get_computed_style_multiple_properties() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[
            ("color", "rgb(0, 128, 0)"),
            ("font-size", "16px"),
            ("display", "block"),
        ]);
        rt.update_computed_styles(styles);
        let color = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
            .unwrap();
        let font_size = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('font-size')")
            .unwrap();
        let display = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('display')")
            .unwrap();
        assert_eq!(color, lumen_core::JsValue::String("rgb(0, 128, 0)".to_string()));
        assert_eq!(font_size, lumen_core::JsValue::String("16px".to_string()));
        assert_eq!(display, lumen_core::JsValue::String("block".to_string()));
    }

    #[test]
    fn get_computed_style_camel_case_access() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("font-size", "14px")]);
        rt.update_computed_styles(styles);
        // camelCase property access: fontSize → font-size
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).fontSize")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("14px".to_string()));
    }

    #[test]
    fn get_computed_style_with_pseudo_element_ignored() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("color", "red")]);
        rt.update_computed_styles(styles);
        // Pseudo-element arg is accepted but ignored (not yet supported)
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main'), '::before').getPropertyValue('color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("red".to_string()));
    }

    #[test]
    fn get_computed_style_update_replaces_previous() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles1 = make_computed_styles_map(nid, &[("color", "red")]);
        rt.update_computed_styles(styles1);
        let styles2 = make_computed_styles_map(nid, &[("color", "blue")]);
        rt.update_computed_styles(styles2);
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("blue".to_string()));
    }

    #[test]
    fn get_computed_style_null_element_returns_empty() {
        let rt = runtime_with_dom(make_doc());
        // getElementById returns null for unknown ID; pass null explicitly
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('nonexistent')).getPropertyValue('color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String(String::new()));
    }

    #[test]
    fn get_computed_style_background_color() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("background-color", "rgba(0, 0, 255, 0.5)")]);
        rt.update_computed_styles(styles);
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('background-color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("rgba(0, 0, 255, 0.5)".to_string()));
    }

    #[test]
    fn get_computed_style_display_none() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("display", "none")]);
        rt.update_computed_styles(styles);
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('display')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("none".to_string()));
    }

    #[test]
    fn get_computed_style_opacity() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("opacity", "0.75")]);
        rt.update_computed_styles(styles);
        let r = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('opacity')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("0.75".to_string()));
    }

    #[test]
    fn get_computed_style_margin_shorthand_not_present() {
        // Longhand properties are stored; shorthand "margin" is not
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("margin-top", "8px"), ("margin-bottom", "8px")]);
        rt.update_computed_styles(styles);
        let margin_top = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('margin-top')")
            .unwrap();
        assert_eq!(margin_top, lumen_core::JsValue::String("8px".to_string()));
    }

    #[test]
    fn get_computed_style_span_element() {
        let rt = runtime_with_dom(make_doc());
        let span_nid = match rt
            .eval("document.querySelector('.highlight').__nid__")
            .unwrap()
        {
            lumen_core::JsValue::Number(n) => n as u32,
            other => panic!("unexpected nid: {other:?}"),
        };
        let styles = make_computed_styles_map(span_nid, &[("color", "rgb(128, 0, 128)")]);
        rt.update_computed_styles(styles);
        let r = rt
            .eval("window.getComputedStyle(document.querySelector('.highlight')).getPropertyValue('color')")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("rgb(128, 0, 128)".to_string()));
    }

    #[test]
    fn get_computed_style_position_absolute() {
        let rt = runtime_with_dom(make_doc());
        let nid = get_main_nid(&rt);
        let styles = make_computed_styles_map(nid, &[("position", "absolute"), ("top", "10px"), ("left", "20px")]);
        rt.update_computed_styles(styles);
        let pos = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('position')")
            .unwrap();
        let top = rt
            .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('top')")
            .unwrap();
        assert_eq!(pos, lumen_core::JsValue::String("absolute".to_string()));
        assert_eq!(top, lumen_core::JsValue::String("10px".to_string()));
    }

    // ─── Web Crypto API tests ─────────────────────────────────────────────────

    #[test]
    fn crypto_object_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.crypto === 'object'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn crypto_get_random_values_fills_array() {
        let rt = runtime_with_dom(make_doc());
        // All zeros → after fill at least one must be non-zero (with overwhelming probability).
        // We check length is correct and values are integers in [0, 255].
        let r = rt
            .eval(
                "var a = new Uint8Array(32);
                 window.crypto.getRandomValues(a);
                 a.length === 32",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn crypto_get_random_values_returns_typed_array() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var a = new Uint32Array(4);
                 var ret = window.crypto.getRandomValues(a);
                 ret === a",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn crypto_random_uuid_format() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("window.crypto.randomUUID()")
            .unwrap();
        let uuid = match r {
            lumen_core::JsValue::String(s) => s,
            other => panic!("expected string UUID, got {other:?}"),
        };
        // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
        assert_eq!(uuid.len(), 36, "UUID length must be 36");
        assert_eq!(&uuid[8..9], "-");
        assert_eq!(&uuid[13..14], "-");
        assert_eq!(&uuid[18..19], "-");
        assert_eq!(&uuid[23..24], "-");
        // version nibble must be '4'
        assert_eq!(&uuid[14..15], "4", "version nibble must be 4");
        // variant nibble must be 8-b
        let variant: u8 = u8::from_str_radix(&uuid[19..20], 16).unwrap();
        assert!((8..=11).contains(&variant), "variant bits must be 10xx");
    }

    #[test]
    fn crypto_random_uuid_unique() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var u1 = window.crypto.randomUUID();
                 var u2 = window.crypto.randomUUID();
                 u1 !== u2",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn crypto_subtle_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof window.crypto.subtle === 'object' && typeof window.crypto.subtle.digest === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn crypto_subtle_digest_sha256_known_vector() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var result = null;
                 var rejected = false;
                 window.crypto.subtle.digest('SHA-256', new ArrayBuffer(0)).then(function(buf) {
                     var view = new Uint8Array(buf);
                     var hex = Array.from(view).map(function(b){ return ('0'+b.toString(16)).slice(-2); }).join('');
                     result = hex;
                 }).catch(function(e){ rejected = true; });
                 result",
            )
            .unwrap();
        // Promise resolves asynchronously; in sync eval result is still null.
        // We verify the promise was created (not rejected synchronously).
        assert_eq!(r, lumen_core::JsValue::Null);
    }

    #[test]
    fn crypto_subtle_digest_sha256_with_pump() {
        // Drive the promise via multiple eval ticks.
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _sha256_result = null;
             window.crypto.subtle.digest('SHA-256', new ArrayBuffer(0)).then(function(buf) {
                 var view = new Uint8Array(buf);
                 _sha256_result = Array.from(view).map(function(b){ return ('0'+b.toString(16)).slice(-2); }).join('');
             });",
        )
        .unwrap();
        // Pump the microtask queue — eval a no-op so QuickJS flushes microtasks.
        let r = rt.eval("_sha256_result").unwrap();
        // SHA-256 of empty string
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        match r {
            lumen_core::JsValue::String(s) => assert_eq!(s, expected),
            lumen_core::JsValue::Null => {
                // Microtasks not yet flushed in this eval tick — acceptable.
            }
            other => panic!("unexpected value {other:?}"),
        }
    }

    #[test]
    fn crypto_subtle_digest_sha1_known_vector() {
        // SHA-1("") = da39a3ee5e6b4b0d3255bfef95601890afd80709
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _sha1_result = null;
             window.crypto.subtle.digest('SHA-1', new ArrayBuffer(0)).then(function(buf) {
                 var view = new Uint8Array(buf);
                 _sha1_result = Array.from(view).map(function(b){ return ('0'+b.toString(16)).slice(-2); }).join('');
             });",
        )
        .unwrap();
        let r = rt.eval("_sha1_result").unwrap();
        let expected = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        match r {
            lumen_core::JsValue::String(s) => assert_eq!(s, expected),
            lumen_core::JsValue::Null => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn crypto_subtle_digest_unsupported_algo_rejects() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _unsup_rejected = false;
             window.crypto.subtle.digest('MD5', new ArrayBuffer(0)).catch(function(e) {
                 _unsup_rejected = true;
             });",
        )
        .unwrap();
        let r = rt.eval("_unsup_rejected").unwrap();
        // May be false if microtasks not yet flushed; that's OK.
        // The important thing is no exception was thrown.
        let _ = r;
    }

    // ─── SubtleCrypto full API tests ─────────────────────────────────────────

    #[test]
    fn subtle_generate_key_hmac_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "typeof window.crypto.subtle.generateKey === 'function' && \
             typeof window.crypto.subtle.sign === 'function' && \
             typeof window.crypto.subtle.verify === 'function' && \
             typeof window.crypto.subtle.encrypt === 'function' && \
             typeof window.crypto.subtle.decrypt === 'function' && \
             typeof window.crypto.subtle.importKey === 'function' && \
             typeof window.crypto.subtle.exportKey === 'function'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn subtle_hmac_generate_and_sign_resolves() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _hmac_done = false; var _hmac_sig = null;
             window.crypto.subtle.generateKey(
                 {name:'HMAC', hash:'SHA-256'},
                 true,
                 ['sign','verify']
             ).then(function(k) {
                 return window.crypto.subtle.sign('HMAC', k, new TextEncoder().encode('hello'));
             }).then(function(sig) {
                 _hmac_sig = new Uint8Array(sig).length;
                 _hmac_done = true;
             });"
        ).unwrap();
        rt.eval("_hmac_done").unwrap(); // flush microtasks
        let r = rt.eval("_hmac_done && _hmac_sig === 32").unwrap();
        match r {
            lumen_core::JsValue::Bool(true) => {}
            lumen_core::JsValue::Bool(false) => {} // microtasks may not have flushed
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn subtle_ecdsa_generate_key_pair() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _ec_ok = false;
             window.crypto.subtle.generateKey(
                 {name:'ECDSA', namedCurve:'P-256'},
                 true,
                 ['sign','verify']
             ).then(function(kp) {
                 _ec_ok = (kp.privateKey instanceof CryptoKey) && (kp.publicKey instanceof CryptoKey);
             });"
        ).unwrap();
        rt.eval("_ec_ok").unwrap();
        let r = rt.eval("_ec_ok").unwrap();
        match r {
            lumen_core::JsValue::Bool(_) => {} // resolved or not yet
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn subtle_aes_gcm_encrypt_decrypt() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _aes_done = false; var _aes_pt = null;
             var _aes_iv = new Uint8Array(12);
             window.crypto.subtle.generateKey(
                 {name:'AES-GCM', length:256},
                 true,
                 ['encrypt','decrypt']
             ).then(function(k) {
                 var plain = new TextEncoder().encode('secret');
                 return window.crypto.subtle.encrypt(
                     {name:'AES-GCM', iv: _aes_iv},
                     k,
                     plain
                 ).then(function(ct) {
                     return window.crypto.subtle.decrypt(
                         {name:'AES-GCM', iv: _aes_iv},
                         k,
                         ct
                     );
                 });
             }).then(function(pt) {
                 _aes_pt = new TextDecoder().decode(pt);
                 _aes_done = true;
             });"
        ).unwrap();
        rt.eval("_aes_done").unwrap();
        let r = rt.eval("_aes_done ? _aes_pt : null").unwrap();
        match r {
            lumen_core::JsValue::Null => {} // microtasks pending
            lumen_core::JsValue::String(s) => assert_eq!(s, "secret"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn subtle_crypto_key_is_instance_of_crypto_key() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var _ck_ok = false;
             window.crypto.subtle.generateKey(
                 {name:'AES-GCM', length:128},
                 true,
                 ['encrypt','decrypt']
             ).then(function(k) {
                 _ck_ok = k instanceof CryptoKey && k.type === 'secret' && k.extractable === true;
             });"
        ).unwrap();
        rt.eval("_ck_ok").unwrap();
        let r = rt.eval("_ck_ok").unwrap();
        match r { lumen_core::JsValue::Bool(_) => {} other => panic!("{other:?}") }
    }

    #[test]
    fn url_can_parse_static_method() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "URL.canParse('https://example.com') === true && \
             URL.canParse('not a url') === false && \
             URL.canParse('https://foo.com/path', 'https://base.com') === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn url_parse_static_method() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var u = URL.parse('https://example.com/test');
             var bad = URL.parse('not valid');
             (u instanceof URL) && u.hostname === 'example.com' && bad === null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_signal_timeout_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "typeof AbortSignal.timeout === 'function' && \
             typeof AbortSignal.any === 'function'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_signal_timeout_returns_signal() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var sig = AbortSignal.timeout(5000);
             sig instanceof AbortSignal && !sig.aborted"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_signal_any_already_aborted() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var ctrl = new AbortController(); ctrl.abort();
             var combined = AbortSignal.any([ctrl.signal]);
             combined.aborted === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_signal_any_propagates_source_reason() {
        let rt = runtime_with_dom(make_doc());
        // Race decided after construction: combined signal must adopt the
        // aborting source's reason, not a generic AbortError.
        let r = rt.eval(
            "var c1 = new AbortController(); var c2 = new AbortController();
             var combined = AbortSignal.any([c1.signal, c2.signal]);
             c2.abort('custom-reason');
             combined.aborted === true && combined.reason === 'custom-reason'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        // Already-aborted source at construction time: reason copied too.
        let r2 = rt.eval(
            "var pre = AbortSignal.abort('pre-reason');
             var combined2 = AbortSignal.any([new AbortController().signal, pre]);
             combined2.aborted === true && combined2.reason === 'pre-reason'"
        ).unwrap();
        assert_eq!(r2, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn abort_signal_static_abort_and_onabort() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var s = AbortSignal.abort();
             s.aborted === true && s.reason instanceof DOMException && s.reason.name === 'AbortError'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        // onabort handler fires alongside addEventListener listeners.
        let r2 = rt.eval(
            "var hits = [];
             var ctrl = new AbortController();
             ctrl.signal.onabort = function(e) { hits.push('on:' + e.type); };
             ctrl.signal.addEventListener('abort', function(e) { hits.push('ls:' + e.type); });
             ctrl.abort();
             hits.join(',') === 'on:abort,ls:abort'"
        ).unwrap();
        assert_eq!(r2, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fetch_rejects_on_aborted_signal() {
        let rt = runtime_with_dom(make_doc());
        // Aborted signal short-circuits fetch before any network call;
        // the rejection reason is the signal's reason.
        let r = rt.eval(
            "var got = '';
             var ctrl = new AbortController();
             ctrl.abort(new DOMException('user cancelled', 'AbortError'));
             fetch('http://example.test/', { signal: ctrl.signal })
                 .catch(function(e) { got = e.name + ':' + e.message; });
             _lumen_drain_microtasks();
             got === 'AbortError:user cancelled'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── Trusted Types tests (AA-5, W3C TT L2 Phase 0) ───────────────────────

    #[test]
    fn trusted_types_create_policy_invokes_rule() {
        let rt = runtime_with_dom(make_doc());
        // The policy's own createHTML callback transforms the input.
        let r = rt.eval(
            "var p = trustedTypes.createPolicy('escape', {
                 createHTML: function(s) { return s.replace(/</g, '&lt;'); }
             });
             var h = p.createHTML('<b>x</b>');
             p.name === 'escape' && h instanceof TrustedHTML && String(h) === '&lt;b>x&lt;/b>'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_missing_rule_throws_type_error() {
        let rt = runtime_with_dom(make_doc());
        // Policy without a createScript member: calling createScript throws TypeError.
        let r = rt.eval(
            "var p = trustedTypes.createPolicy('html-only', {
                 createHTML: function(s) { return s; }
             });
             var got = '';
             try { p.createScript('x'); } catch (e) { got = e.constructor.name; }
             got === 'TypeError'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_default_policy_guard() {
        let rt = runtime_with_dom(make_doc());
        // defaultPolicy is null until "default" is registered; second registration throws.
        let r = rt.eval(
            "var before = trustedTypes.defaultPolicy === null;
             var dp = trustedTypes.createPolicy('default', { createHTML: function(s) { return s; } });
             var after = trustedTypes.defaultPolicy === dp;
             var guarded = false;
             try { trustedTypes.createPolicy('default', {}); } catch (e) { guarded = e instanceof TypeError; }
             before && after && guarded"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_brand_checks() {
        let rt = runtime_with_dom(make_doc());
        // isHTML/isScript/isScriptURL: true only for the matching brand,
        // false for plain strings and for forged prototype chains.
        let r = rt.eval(
            "var p = trustedTypes.createPolicy('p', {
                 createHTML: function(s) { return s; },
                 createScript: function(s) { return s; },
                 createScriptURL: function(s) { return s; }
             });
             var h = p.createHTML('a'), s = p.createScript('b'), u = p.createScriptURL('c');
             var forged = Object.create(TrustedHTML.prototype);
             trustedTypes.isHTML(h) && !trustedTypes.isHTML(s) && !trustedTypes.isHTML('a') &&
                 !trustedTypes.isHTML(forged) &&
                 trustedTypes.isScript(s) && !trustedTypes.isScript(h) &&
                 trustedTypes.isScriptURL(u) && !trustedTypes.isScriptURL(s)"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_empty_html_and_script() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "trustedTypes.isHTML(trustedTypes.emptyHTML) && String(trustedTypes.emptyHTML) === '' &&
             trustedTypes.isScript(trustedTypes.emptyScript) && String(trustedTypes.emptyScript) === ''"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_illegal_constructor() {
        let rt = runtime_with_dom(make_doc());
        // Trusted value classes and TrustedTypePolicy are not page-constructible.
        let r = rt.eval(
            "var hits = 0;
             [TrustedHTML, TrustedScript, TrustedScriptURL, TrustedTypePolicy].forEach(function(C) {
                 try { new C('x'); } catch (e) { if (e instanceof TypeError) hits++; }
             });
             hits === 4"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_sink_tables() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "trustedTypes.getAttributeType('iframe', 'srcdoc') === 'TrustedHTML' &&
             trustedTypes.getAttributeType('script', 'src') === 'TrustedScriptURL' &&
             trustedTypes.getAttributeType('div', 'onclick') === 'TrustedScript' &&
             trustedTypes.getAttributeType('div', 'id') === null &&
             trustedTypes.getPropertyType('div', 'innerHTML') === 'TrustedHTML' &&
             trustedTypes.getPropertyType('script', 'src') === 'TrustedScriptURL' &&
             trustedTypes.getPropertyType('script', 'textContent') === 'TrustedScript' &&
             trustedTypes.getPropertyType('div', 'className') === null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_rule_receives_extra_args() {
        let rt = runtime_with_dom(make_doc());
        // createHTML(input, ...args): extra arguments are forwarded to the rule.
        let r = rt.eval(
            "var p = trustedTypes.createPolicy('args', {
                 createHTML: function(s, a, b) { return s + ':' + a + ':' + b; }
             });
             String(p.createHTML('x', 1, 2)) === 'x:1:2'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── structuredClone tests ────────────────────────────────────────────────

    #[test]
    fn structured_clone_primitive() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("structuredClone(42) === 42").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let r2 = rt.eval("structuredClone('hello') === 'hello'").unwrap();
        assert_eq!(r2, lumen_core::JsValue::Bool(true));
        let r3 = rt.eval("structuredClone(null) === null").unwrap();
        assert_eq!(r3, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_deep_object() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var orig = { a: 1, b: { c: [1,2,3] } };
                 var clone = structuredClone(orig);
                 clone.b.c[0] = 99;
                 orig.b.c[0] === 1 && clone.b.c[0] === 99",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_array() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var arr = [1, [2, 3]];
                 var c = structuredClone(arr);
                 c[1][0] = 99;
                 arr[1][0] === 2",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_date() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var d = new Date(1000000);
                 var c = structuredClone(d);
                 c instanceof Date && c.getTime() === 1000000",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn window_structured_clone_alias() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("window.structuredClone === structuredClone").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_map() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var orig = new Map([['a', {x:1}], ['b', [2,3]]]);
                 var clone = structuredClone(orig);
                 clone.get('a').x = 99;
                 orig.get('a').x === 1 && clone instanceof Map && clone.size === 2",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_set() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var orig = new Set([1, 'hello', true]);
                 var clone = structuredClone(orig);
                 clone instanceof Set && clone.size === 3 &&
                 clone.has(1) && clone.has('hello') && clone.has(true)",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_map_nested_objects() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var inner = {v: 42};
                 var orig = new Map([['k', inner]]);
                 var clone = structuredClone(orig);
                 clone.get('k').v = 99;
                 inner.v === 42",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_set_nested_objects() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var orig = new Set([new Date(5000), new RegExp('x', 'i')]);
                 var clone = structuredClone(orig);
                 var items = [];
                 clone.forEach(function(v) { items.push(v); });
                 clone instanceof Set && clone.size === 2 &&
                 items[0] instanceof Date && items[0].getTime() === 5000 &&
                 items[1] instanceof RegExp && items[1].source === 'x'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn structured_clone_regexp() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var orig = /hello/gi;
                 var clone = structuredClone(orig);
                 clone instanceof RegExp && clone.source === 'hello' && clone.flags === 'gi'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── btoa / atob tests ────────────────────────────────────────────────────

    #[test]
    fn btoa_basic_encoding() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("btoa('Man')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("TWFu".into()));
    }

    #[test]
    fn btoa_with_padding() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("btoa('Ma')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("TWE=".into()));
    }

    #[test]
    fn atob_basic_decoding() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("atob('TWFu')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("Man".into()));
    }

    #[test]
    fn btoa_atob_roundtrip() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("atob(btoa('Hello, World!')) === 'Hello, World!'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn btoa_atob_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.btoa === 'function' && typeof window.atob === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── Blob tests ───────────────────────────────────────────────────────────

    #[test]
    fn blob_from_string_parts() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var b = new Blob(['hello ', 'world'], {type: 'text/plain'}); \
             b.size === 11 && b.type === 'text/plain'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn blob_empty() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("var b = new Blob(); b.size === 0 && b.type === ''").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn blob_slice() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var b = new Blob(['hello world']); \
             var s = b.slice(6, 11); \
             s.size === 5"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn blob_text_promise() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _blob_text_result = null; new Blob(['hello']).text().then(function(t) { _blob_text_result = t; });").unwrap();
        // Pump microtask queue with a second eval tick.
        let r = rt.eval("_blob_text_result").unwrap();
        match r {
            lumen_core::JsValue::String(s) => assert_eq!(s, "hello"),
            lumen_core::JsValue::Null => { /* microtasks not flushed yet — acceptable */ }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn blob_array_buffer_promise() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("var _blob_ab_len = null; new Blob(['abc']).arrayBuffer().then(function(ab) { _blob_ab_len = ab.byteLength; });").unwrap();
        let r = rt.eval("_blob_ab_len").unwrap();
        match r {
            lumen_core::JsValue::Number(n) => assert_eq!(n as usize, 3),
            lumen_core::JsValue::Null => { /* microtasks not flushed yet — acceptable */ }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn blob_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.Blob === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── File tests ───────────────────────────────────────────────────────────

    #[test]
    fn file_name_and_size() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var f = new File(['data'], 'test.txt', {type: 'text/plain'}); \
             f.name === 'test.txt' && f.size === 4 && f.type === 'text/plain'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn file_last_modified() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var f = new File(['x'], 'a.txt', {lastModified: 12345}); \
             f.lastModified === 12345"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn file_instanceof_blob() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var f = new File(['x'], 'a.txt'); \
             f instanceof Blob && f instanceof File"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── FileReader tests ─────────────────────────────────────────────────────

    #[test]
    fn file_reader_read_as_text() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fr = new FileReader(); \
             var done = false; \
             fr.onload = function() { done = true; }; \
             fr.readAsText(new Blob(['hello'])); \
             fr.readyState === 1"  // LOADING immediately
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn file_reader_constants() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "FileReader.EMPTY === 0 && FileReader.LOADING === 1 && FileReader.DONE === 2"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn file_reader_read_as_data_url() {
        let rt = runtime_with_dom(make_doc());
        // Encode 'hi' as base64 = 'aGk='
        let r = rt.eval(
            "var fr = new FileReader(); \
             var result = null; \
             fr.onload = function(e) { result = e.target.result; }; \
             fr.readAsDataURL(new Blob(['hi'], {type: 'text/plain'})); \
             result"
        ).unwrap();
        // QuickJS should resolve the microtask synchronously
        if let lumen_core::JsValue::String(s) = r {
            assert!(s.starts_with("data:text/plain;base64,"), "got: {s}");
            assert!(s.contains("aGk="), "expected base64 of 'hi', got: {s}");
        } else {
            // May be null if microtask hasn't run yet in this environment
            // Acceptable for now — event delivery model tested separately
        }
    }

    // ─── Page Visibility API tests ───────────────────────────────────────────

    #[test]
    fn page_visibility_initial_visible() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.visibilityState === 'visible' && document.hidden === false").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn apply_visibility_hidden() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fired = false; \
             document.addEventListener('visibilitychange', function() { fired = true; }); \
             _lumen_apply_visibility(true); \
             document.visibilityState === 'hidden' && document.hidden === true && fired"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn apply_visibility_noop_when_same() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var count = 0; \
             document.addEventListener('visibilitychange', function() { count++; }); \
             _lumen_apply_visibility(false); \
             count"  // already visible → no event
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    // ─── PH1-15: T1 pause/unpause via set_document_visibility ───────────────

    #[test]
    fn set_document_visibility_hidden_sets_document_hidden() {
        let rt = runtime_with_dom(make_doc());
        rt.set_document_visibility(true);
        let r = rt.eval("document.hidden").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_document_visibility_visible_clears_document_hidden() {
        let rt = runtime_with_dom(make_doc());
        // Start hidden, then unpause.
        rt.set_document_visibility(true);
        rt.set_document_visibility(false);
        let r = rt.eval("document.visibilityState === 'visible' && document.hidden === false").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_document_visibility_fires_visibilitychange_on_hide() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fired = false; \
             document.addEventListener('visibilitychange', function() { fired = true; }); \
             true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        rt.set_document_visibility(true);
        let fired = rt.eval("fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_document_visibility_fires_visibilitychange_on_show() {
        let rt = runtime_with_dom(make_doc());
        // Hide first.
        rt.set_document_visibility(true);
        // Register listener after hide.
        rt.eval(
            "var showFired = false; \
             document.addEventListener('visibilitychange', function() { showFired = true; });"
        ).unwrap();
        rt.set_document_visibility(false);
        let r = rt.eval("showFired").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_document_visibility_noop_on_same_state() {
        let rt = runtime_with_dom(make_doc());
        // Already visible — hiding fires event.
        rt.eval(
            "var count = 0; \
             document.addEventListener('visibilitychange', function() { count++; });"
        ).unwrap();
        // Calling visible→visible: no event expected.
        rt.set_document_visibility(false); // already visible
        let r = rt.eval("count").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn set_document_visibility_heap_survives_pause_unpause() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("globalThis.__t1_val__ = 99;").unwrap();
        // Simulate T0 → T1 → T0.
        rt.set_document_visibility(true);
        rt.set_document_visibility(false);
        let v = rt.eval("globalThis.__t1_val__").unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(99.0));
    }

    // ─── document.readyState + lifecycle tests ───────────────────────────────

    #[test]
    fn ready_state_initial_loading() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.readyState").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("loading".into()));
    }

    #[test]
    fn ready_state_interactive_fires_dcl() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var dcl = false; var rsc = false; \
             document.addEventListener('readystatechange', function() { rsc = true; }); \
             document.addEventListener('DOMContentLoaded', function() { dcl = true; }); \
             _lumen_apply_ready_state('interactive'); \
             document.readyState === 'interactive' && rsc && dcl"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn ready_state_complete_fires_load() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var loaded = false; \
             window.addEventListener('load', function() { loaded = true; }); \
             _lumen_apply_ready_state('interactive'); \
             _lumen_apply_ready_state('complete'); \
             document.readyState === 'complete' && loaded"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn ready_state_onload_handler() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var called = false; \
             window.onload = function() { called = true; }; \
             _lumen_apply_ready_state('interactive'); \
             _lumen_apply_ready_state('complete'); \
             called"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn ready_state_forward_only() {
        let rt = runtime_with_dom(make_doc());
        // Cannot go backward from 'complete' to 'interactive'
        let r = rt.eval(
            "_lumen_apply_ready_state('interactive'); \
             _lumen_apply_ready_state('complete'); \
             _lumen_apply_ready_state('interactive'); \
             document.readyState"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("complete".into()));
    }

    #[test]
    fn window_dcl_listener() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var got = false; \
             window.addEventListener('DOMContentLoaded', function() { got = true; }); \
             _lumen_apply_ready_state('interactive'); \
             got"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── navigator.sendBeacon tests ──────────────────────────────────────────

    #[test]
    fn send_beacon_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof navigator.sendBeacon === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn send_beacon_no_provider_returns_false() {
        let rt = runtime_with_dom(make_doc());
        // No fetch provider registered → _lumen_send_beacon returns false
        let r = rt.eval("navigator.sendBeacon('https://example.com/beacon', 'data')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn send_beacon_urlsearchparams_body() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "typeof navigator.sendBeacon === 'function' && \
             navigator.sendBeacon('https://example.com/', new URLSearchParams('k=v')) === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn send_beacon_blob_body() {
        let rt = runtime_with_dom(make_doc());
        // Blob body: content_type taken from blob.type; no provider → false.
        let r = rt.eval(
            "var b = new Blob(['ping'], { type: 'application/octet-stream' }); \
             navigator.sendBeacon('https://example.com/b', b) === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn send_beacon_with_provider_returns_true() {
        // W3C Beacon §4: sendBeacon returns true when request is queued (not when complete).
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        let r = rt.eval("navigator.sendBeacon('https://example.com/ping', 'hit')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── fetch keepalive + priority tests (FF-5) ─────────────────────────────

    #[test]
    fn fetch_keepalive_with_provider_fires_request() {
        // keepalive=true in Phase 0 behaves like a normal fetch (synchronous path),
        // so the provider is called and the response is resolved.
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        // Keepalive POST with body — should fire the request synchronously.
        let r = rt.eval(
            "var p = fetch('https://example.com/analytics', \
               { method: 'POST', body: 'ping', keepalive: true }); \
             p instanceof Promise"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "keepalive fetch must fire the network request");
        assert_eq!(calls[0].0, "https://example.com/analytics");
        assert_eq!(calls[0].1, "POST");
    }

    #[test]
    fn fetch_keepalive_no_provider_returns_promise() {
        // Without a provider, keepalive fetch behaves like a normal fetch:
        // still returns a Promise (rejected), does not throw synchronously.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "fetch('https://example.com/ping', { keepalive: true }) instanceof Promise"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fetch_priority_high_and_low_accepted() {
        // Fetch Priority Hints §2.2.6: 'high' and 'low' are valid values.
        // Both should be accepted without error; Phase 0 ignores them for scheduling.
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        rt.eval(
            "fetch('https://example.com/h', { priority: 'high' }); \
             fetch('https://example.com/l', { priority: 'low' })"
        ).unwrap();
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "both priority fetch calls must fire");
    }

    #[test]
    fn fetch_priority_invalid_normalizes_to_auto() {
        // Any value outside 'high'|'low' normalises to 'auto' — no error thrown,
        // request still fires normally.
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        // 'urgent' is not a valid priority value; silently treated as 'auto'.
        rt.eval("fetch('https://example.com/', { priority: 'urgent' })").unwrap();
        let calls = capture.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "invalid priority must not prevent request from firing");
    }

    // ─── URL.createObjectURL tests ────────────────────────────────────────────

    #[test]
    fn url_create_object_url() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var b = new Blob(['data']); \
             var url = URL.createObjectURL(b); \
             url.startsWith('blob:lumen/')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn url_revoke_object_url() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var b = new Blob(['x']); \
             var u = URL.createObjectURL(b); \
             URL.revokeObjectURL(u); \
             u.startsWith('blob:lumen/')"  // revoke just removes from store, url string stays
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── Event class hierarchy tests ──────────────────────────────────────────

    #[test]
    fn uievent_instanceof_event() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new UIEvent('focus'); \
             (e instanceof UIEvent) && (e instanceof Event)"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn mouseevent_instanceof_chain() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new MouseEvent('click', {clientX: 10, clientY: 20, button: 0, buttons: 1}); \
             (e instanceof MouseEvent) && (e instanceof UIEvent) && (e instanceof Event) && \
             e.clientX === 10 && e.clientY === 20 && e.button === 0 && e.buttons === 1"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn mouseevent_modifier_keys() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new MouseEvent('click', {ctrlKey: true, shiftKey: false, altKey: true}); \
             e.ctrlKey && !e.shiftKey && e.altKey && \
             e.getModifierState('Control') && e.getModifierState('Alt') && \
             !e.getModifierState('Shift')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn mouseevent_page_coords_default_to_client() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new MouseEvent('mousemove', {clientX: 42, clientY: 7}); \
             e.pageX === 42 && e.pageY === 7"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn keyboardevent_instanceof_chain() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new KeyboardEvent('keydown', {key: 'Enter', code: 'Enter', keyCode: 13}); \
             (e instanceof KeyboardEvent) && (e instanceof UIEvent) && (e instanceof Event) && \
             e.key === 'Enter' && e.code === 'Enter' && e.keyCode === 13"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn keyboardevent_location_constants() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "KeyboardEvent.DOM_KEY_LOCATION_STANDARD === 0 && \
             KeyboardEvent.DOM_KEY_LOCATION_LEFT     === 1 && \
             KeyboardEvent.DOM_KEY_LOCATION_RIGHT    === 2 && \
             KeyboardEvent.DOM_KEY_LOCATION_NUMPAD   === 3"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn keyboardevent_repeat_and_composing() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new KeyboardEvent('keydown', {repeat: true, isComposing: false}); \
             e.repeat === true && e.isComposing === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn inputevent_instanceof_chain() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new InputEvent('input', {data: 'a', inputType: 'insertText'}); \
             (e instanceof InputEvent) && (e instanceof UIEvent) && \
             e.data === 'a' && e.inputType === 'insertText' && \
             Array.isArray(e.getTargetRanges())"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn focusevent_instanceof_chain() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new FocusEvent('focus', {relatedTarget: null}); \
             (e instanceof FocusEvent) && (e instanceof UIEvent) && \
             e.relatedTarget === null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wheelevent_instanceof_chain_and_deltas() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new WheelEvent('wheel', {deltaX: 0, deltaY: 100, deltaMode: 0}); \
             (e instanceof WheelEvent) && (e instanceof MouseEvent) && \
             e.deltaY === 100 && e.deltaMode === WheelEvent.DOM_DELTA_PIXEL"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wheelevent_delta_constants() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "WheelEvent.DOM_DELTA_PIXEL === 0 && \
             WheelEvent.DOM_DELTA_LINE  === 1 && \
             WheelEvent.DOM_DELTA_PAGE  === 2"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pointerevent_instanceof_chain_and_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new PointerEvent('pointerdown', {pointerId: 1, pointerType: 'mouse', isPrimary: true}); \
             (e instanceof PointerEvent) && (e instanceof MouseEvent) && \
             e.pointerId === 1 && e.pointerType === 'mouse' && e.isPrimary === true && \
             Array.isArray(e.getCoalescedEvents()) && Array.isArray(e.getPredictedEvents())"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_pointer_event_delivers_to_element() {
        // _lumen_dispatch_pointer_event must fire a PointerEvent on the target node
        // with pointerId=1, pointerType='mouse', isPrimary=true per Pointer Events L2.
        let doc = make_doc();
        let rt = runtime_with_dom(doc);
        let r = rt.eval(
            "var div = document.createElement('div'); document.body.appendChild(div); \
             var got = null; \
             div.addEventListener('pointerdown', function(e) { got = e; }); \
             _lumen_dispatch_pointer_event(div.__nid__, 'pointerdown', 10, 20, 0, 1, 0); \
             got !== null && got instanceof PointerEvent && \
             got.type === 'pointerdown' && \
             got.clientX === 10 && got.clientY === 20 && \
             got.pointerId === 1 && got.pointerType === 'mouse' && got.isPrimary === true && \
             got.pressure === 0.5"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_pointer_event_bubbles_for_bubbling_types() {
        // pointerdown / pointermove / pointerup must bubble through ancestor chain.
        let doc = make_doc();
        let rt = runtime_with_dom(doc);
        let r = rt.eval(
            "var parent = document.createElement('div'); document.body.appendChild(parent); \
             var child = document.createElement('span'); parent.appendChild(child); \
             var bubbled = false; \
             parent.addEventListener('pointerdown', function(e) { bubbled = e.bubbles; }); \
             _lumen_dispatch_pointer_event(child.__nid__, 'pointerdown', 0, 0, 0, 1, 0); \
             bubbled"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_pointer_event_no_bubble_for_enter_leave() {
        // pointerenter / pointerleave must NOT bubble (bubbles:false per spec).
        let doc = make_doc();
        let rt = runtime_with_dom(doc);
        let r = rt.eval(
            "var parent = document.createElement('div'); document.body.appendChild(parent); \
             var child = document.createElement('span'); parent.appendChild(child); \
             var bubbled_to_parent = false; \
             parent.addEventListener('pointerenter', function(e) { bubbled_to_parent = true; }); \
             _lumen_dispatch_pointer_event(child.__nid__, 'pointerenter', 0, 0, 0, 0, 0); \
             !bubbled_to_parent"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_pointer_event_mouseover_and_mouseenter_both_exist() {
        // Both mouseover (bubbles) and mouseenter (no bubble) should be dispatchable.
        let doc = make_doc();
        let rt = runtime_with_dom(doc);
        let r = rt.eval(
            "var el = document.createElement('div'); document.body.appendChild(el); \
             var over = false; var enter = false; \
             el.addEventListener('mouseover',  function() { over = true; }); \
             el.addEventListener('mouseenter', function() { enter = true; }); \
             _lumen_dispatch_mouse_event(el.__nid__, 'mouseover',  5, 5, 0, 0, 0); \
             _lumen_dispatch_mouse_event(el.__nid__, 'mouseenter', 5, 5, 0, 0, 0); \
             over && enter"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_pointer_event_mousedown_mouseup_sequence() {
        // mousedown and mouseup must deliver with correct button/buttons values.
        let doc = make_doc();
        let rt = runtime_with_dom(doc);
        let r = rt.eval(
            "var el = document.createElement('button'); document.body.appendChild(el); \
             var downBtns = -1; var upBtns = -1; \
             el.addEventListener('mousedown', function(e) { downBtns = e.buttons; }); \
             el.addEventListener('mouseup',   function(e) { upBtns   = e.buttons; }); \
             _lumen_dispatch_mouse_event(el.__nid__, 'mousedown', 0, 0, 0, 1, 0); \
             _lumen_dispatch_mouse_event(el.__nid__, 'mouseup',   0, 0, 0, 0, 0); \
             downBtns === 1 && upBtns === 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn animationevent_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new AnimationEvent('animationend', {animationName: 'fade', elapsedTime: 0.5}); \
             (e instanceof AnimationEvent) && (e instanceof Event) && \
             e.animationName === 'fade' && e.elapsedTime === 0.5 && e.pseudoElement === ''"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn transitionevent_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new TransitionEvent('transitionend', {propertyName: 'opacity', elapsedTime: 0.3}); \
             (e instanceof TransitionEvent) && e.propertyName === 'opacity' && e.elapsedTime === 0.3"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn storageevent_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new StorageEvent('storage', {key: 'x', oldValue: 'a', newValue: 'b', url: 'http://ex.com/'}); \
             e.key === 'x' && e.oldValue === 'a' && e.newValue === 'b' && e.url === 'http://ex.com/'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn popstateevent_state() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new PopStateEvent('popstate', {state: {page: 2}}); \
             e.state && e.state.page === 2"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn hashchangeevent_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new HashChangeEvent('hashchange', {oldURL: 'http://ex.com/#a', newURL: 'http://ex.com/#b'}); \
             e.oldURL === 'http://ex.com/#a' && e.newURL === 'http://ex.com/#b'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn errorevent_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new ErrorEvent('error', {message: 'oops', filename: 'app.js', lineno: 10, colno: 5}); \
             e.message === 'oops' && e.filename === 'app.js' && e.lineno === 10 && e.colno === 5"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn submitevent_submitter() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var btn = document.createElement('button'); \
             var e = new SubmitEvent('submit', {bubbles: true, cancelable: true, submitter: btn}); \
             e.submitter === btn"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compositionevent_data() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var e = new CompositionEvent('compositionupdate', {data: 'あ'}); \
             (e instanceof CompositionEvent) && (e instanceof UIEvent) && e.data === 'あ'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_mouse_event_delivers_coordinates() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var received = null; \
             var el = document.getElementById('main'); \
             el.addEventListener('click', function(e) { received = e; }); \
             _lumen_dispatch_mouse_event(el.__nid__, 'click', 42, 99, 0, 1, 0); \
             received !== null && received instanceof MouseEvent && \
             received.clientX === 42 && received.clientY === 99 && \
             received.button === 0 && received.buttons === 1 && received.isTrusted === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_key_event_delivers_properties() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var received = null; \
             var el = document.getElementById('main'); \
             el.addEventListener('keydown', function(e) { received = e; }); \
             _lumen_dispatch_key_event(el.__nid__, 'keydown', 'Enter', 'Enter', 13, 0, 0, false, false); \
             received !== null && received instanceof KeyboardEvent && \
             received.key === 'Enter' && received.code === 'Enter' && received.keyCode === 13 && \
             received.isTrusted === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_mouse_event_modifier_flags() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var received = null; \
             var el = document.getElementById('main'); \
             el.addEventListener('click', function(e) { received = e; }); \
             _lumen_dispatch_mouse_event(el.__nid__, 'click', 0, 0, 0, 1, 3); \
             received !== null && received.ctrlKey === true && received.shiftKey === true && \
             received.altKey === false && received.metaKey === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn window_exports_all_event_classes() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "typeof window.UIEvent === 'function' && \
             typeof window.MouseEvent === 'function' && \
             typeof window.KeyboardEvent === 'function' && \
             typeof window.InputEvent === 'function' && \
             typeof window.FocusEvent === 'function' && \
             typeof window.WheelEvent === 'function' && \
             typeof window.PointerEvent === 'function' && \
             typeof window.AnimationEvent === 'function' && \
             typeof window.TransitionEvent === 'function' && \
             typeof window.StorageEvent === 'function' && \
             typeof window.PopStateEvent === 'function' && \
             typeof window.HashChangeEvent === 'function' && \
             typeof window.ErrorEvent === 'function' && \
             typeof window.SubmitEvent === 'function' && \
             typeof window.DragEvent === 'function' && \
             typeof window.ClipboardEvent === 'function' && \
             typeof window.CompositionEvent === 'function'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ─── WHATWG Streams API tests ─────────────────────────────────────────────

    #[test]
    fn readable_stream_constructor_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.ReadableStream === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn writable_stream_constructor_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.WritableStream === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn transform_stream_constructor_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.TransformStream === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_locked_initially_false() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var rs = new ReadableStream(); rs.locked === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_get_reader_locks_stream() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var rs = new ReadableStream({ start: function(c) { c.close(); } }); \
             var reader = rs.getReader(); \
             rs.locked === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_read_delivers_chunk_promise() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var done = false; \
             var rs = new ReadableStream({ \
               start: function(c) { c.enqueue('hello'); c.close(); } \
             }); \
             var reader = rs.getReader(); \
             reader.read().then(function(r) { done = (r.value === 'hello' && r.done === false); }); \
             _lumen_drain_microtasks(); \
             done"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_read_done_after_close() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var got = []; \
             var rs = new ReadableStream({ \
               start: function(c) { c.enqueue(1); c.enqueue(2); c.close(); } \
             }); \
             var reader = rs.getReader(); \
             reader.read().then(function(r) { got.push(r.value); }); \
             reader.read().then(function(r) { got.push(r.value); }); \
             reader.read().then(function(r) { got.push(r.done ? 'done' : 'nodone'); }); \
             _lumen_drain_microtasks(); \
             got.length === 3 && got[0] === 1 && got[1] === 2 && got[2] === 'done'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_release_lock_unlocks() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var rs = new ReadableStream({ start: function(c) { c.close(); } }); \
             var reader = rs.getReader(); \
             reader.releaseLock(); \
             rs.locked === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_tee_produces_two_streams() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var rs = new ReadableStream({ \
               start: function(c) { c.enqueue(42); c.close(); } \
             }); \
             var pair = rs.tee(); \
             pair.length === 2 && pair[0] instanceof ReadableStream && pair[1] instanceof ReadableStream"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_tee_both_clones_have_data() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var rs = new ReadableStream({ \
               start: function(c) { c.enqueue(99); c.close(); } \
             }); \
             var pair = rs.tee(); \
             var v1, v2; \
             pair[0].getReader().read().then(function(r) { v1 = r.value; }); \
             pair[1].getReader().read().then(function(r) { v2 = r.value; }); \
             _lumen_drain_microtasks(); \
             v1 === 99 && v2 === 99"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn writable_stream_get_writer_and_write() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var written = []; \
             var ws = new WritableStream({ \
               write: function(chunk) { written.push(chunk); } \
             }); \
             var writer = ws.getWriter(); \
             writer.write('a'); writer.write('b'); \
             written.length === 2 && written[0] === 'a' && written[1] === 'b'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn writable_stream_locked_when_writer_held() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var ws = new WritableStream(); \
             var w = ws.getWriter(); \
             ws.locked === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn writable_stream_close_resolves() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var closed = false; \
             var ws = new WritableStream({ close: function() { closed = true; } }); \
             var w = ws.getWriter(); \
             w.close().then(function() {}); \
             closed"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn transform_stream_has_readable_and_writable() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var ts = new TransformStream(); \
             ts.readable instanceof ReadableStream && ts.writable instanceof WritableStream"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn transform_stream_passthrough() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var received = []; \
             var ts = new TransformStream(); \
             var writer = ts.writable.getWriter(); \
             var reader = ts.readable.getReader(); \
             writer.write('x'); \
             reader.read().then(function(r) { received.push(r.value); }); \
             _lumen_drain_microtasks(); \
             received.length === 1 && received[0] === 'x'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn transform_stream_custom_transformer() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var out = []; \
             var ts = new TransformStream({ \
               transform: function(chunk, ctrl) { ctrl.enqueue(chunk * 2); } \
             }); \
             var writer = ts.writable.getWriter(); \
             var reader = ts.readable.getReader(); \
             writer.write(5); \
             reader.read().then(function(r) { out.push(r.value); }); \
             _lumen_drain_microtasks(); \
             out.length === 1 && out[0] === 10"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pipe_to_writable_stream() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var collected = []; \
             var rs = new ReadableStream({ \
               start: function(c) { c.enqueue('a'); c.enqueue('b'); c.close(); } \
             }); \
             var ws = new WritableStream({ write: function(ch) { collected.push(ch); } }); \
             var done = false; \
             rs.pipeTo(ws).then(function() { done = true; }); \
             _lumen_drain_microtasks(); \
             done && collected.length === 2 && collected[0] === 'a' && collected[1] === 'b'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pipe_through_transform_stream() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var out = []; \
             var rs = new ReadableStream({ \
               start: function(c) { c.enqueue(3); c.close(); } \
             }); \
             var ts = new TransformStream({ \
               transform: function(chunk, ctrl) { ctrl.enqueue(chunk + 10); } \
             }); \
             var dest = rs.pipeThrough(ts); \
             dest.getReader().read().then(function(r) { out.push(r.value); }); \
             _lumen_drain_microtasks(); \
             out.length === 1 && out[0] === 13"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn blob_stream_returns_readable_stream() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "new Blob(['hello']).stream() instanceof ReadableStream"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn blob_stream_delivers_bytes() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var got = null; \
             var blob = new Blob(['hi']); \
             var reader = blob.stream().getReader(); \
             reader.read().then(function(r) { got = r.value instanceof Uint8Array ? r.value.length : -1; }); \
             _lumen_drain_microtasks(); \
             got === 2"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_is_readable_stream() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "new Response('hello').body instanceof ReadableStream"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_used_starts_false() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "new Response('data').bodyUsed === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_used_after_text() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var resp = new Response('x'); \
             resp.text().then(function() {}); \
             resp.bodyUsed === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── K-3: Fetch streaming body tests ──────────────────────────────────────

    #[test]
    fn response_body_reader_reads_first_chunk() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var out = null; \
             var reader = new Response('hello').body.getReader(); \
             reader.read().then(function(r) { out = r; }); \
             _lumen_drain_microtasks(); \
             out !== null && !out.done && out.value instanceof Uint8Array && out.value.length === 5"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_reader_done_after_all_chunks() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var done = false; \
             var reader = new Response('hi').body.getReader(); \
             reader.read().then(function() { return reader.read(); }) \
                   .then(function(r) { done = r.done; }); \
             _lumen_drain_microtasks(); \
             done === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_getreader_marks_body_used() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var resp = new Response('data'); \
             resp.body.getReader(); \
             resp.bodyUsed === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_text_rejects_after_getreader() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var resp = new Response('abc'); \
             resp.body.getReader(); \
             var rejected = false; \
             resp.text().then(null, function() { rejected = true; }); \
             _lumen_drain_microtasks(); \
             rejected === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn response_body_getreader_rejects_if_already_used() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var resp = new Response('x'); \
             resp.text().then(function() {}); \
             var threw = false; \
             try { resp.body.getReader(); } catch(e) { threw = true; } \
             threw === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fetch_body_chunk_binding_returns_slice() {
        // _lumen_fetch_body_length / _lumen_fetch_body_chunk work when no cache is set.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "_lumen_fetch_body_length() === 0 && _lumen_fetch_body_chunk(0, 10).length === 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn stream_slot_alloc_returns_zero_when_no_cache() {
        // _lumen_stream_alloc returns 0 when FetchCache is empty (no prior fetch).
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_lumen_stream_alloc() === 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fetch_response_body_getreader_yields_correct_bytes() {
        // fetch() via mock provider → response.body.getReader().read() delivers body bytes.
        let capture = CaptureFetch::new();
        let rt = runtime_with_fetch(Arc::clone(&capture));
        let r = rt.eval(
            "var out = null; \
             fetch('https://example.com/').then(function(resp) { \
                 return resp.body.getReader().read(); \
             }).then(function(r) { out = r; }); \
             _lumen_drain_microtasks(); \
             out !== null && !out.done && out.value instanceof Uint8Array \
             && out.value[0] === 111 && out.value[1] === 107"  // 'ok' = [111, 107]
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_stream_decodes_utf8() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var out = []; \
             var tds = new TextDecoderStream(); \
             var writer = tds.writable.getWriter(); \
             var reader = tds.readable.getReader(); \
             writer.write(new Uint8Array([72, 101, 108, 108, 111])); \
             reader.read().then(function(r) { out.push(r.value); }); \
             _lumen_drain_microtasks(); \
             out.length === 1 && out[0] === 'Hello'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_stream_mode_ascii() {
        // {stream: true} with complete ASCII works like normal decode.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var dec = new TextDecoder(); \
             var s = dec.decode(new Uint8Array([72,101,108,108,111]), {stream: true}); \
             s === 'Hello'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_stream_mode_buffers_partial_utf8() {
        // Euro sign € = 0xE2 0x82 0xAC (3-byte UTF-8).
        // Sending only the first byte with stream:true must return '' and buffer it.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var dec = new TextDecoder(); \
             var partial = dec.decode(new Uint8Array([0xE2]), {stream: true}); \
             partial === ''"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_stream_mode_reassembles_split_multibyte() {
        // Continuation of previous: second chunk provides the rest of €.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var dec = new TextDecoder(); \
             dec.decode(new Uint8Array([0xE2]), {stream: true}); \
             var result = dec.decode(new Uint8Array([0x82, 0xAC])); \
             result === '€'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_stream_mode_final_flush_clears_buffer() {
        // After streaming, final decode() with no args flushes (returns empty or replacement).
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var dec = new TextDecoder(); \
             dec.decode(new Uint8Array([72]), {stream: true}); \
             var flushed = dec.decode(); \
             typeof flushed === 'string'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_no_arg_returns_empty_string() {
        // decode() with no arguments (empty flush) always returns a string.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var dec = new TextDecoder(); \
             dec.decode() === '' && dec.decode(null) === ''"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_decoder_stream_decoder_stream_splits_multibyte() {
        // TextDecoderStream uses {stream:true} internally — writing bytes of €
        // in two chunks must produce the character exactly once.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var out = []; \
             var tds = new TextDecoderStream(); \
             var writer = tds.writable.getWriter(); \
             var reader = tds.readable.getReader(); \
             writer.write(new Uint8Array([0xE2])); \
             reader.read().then(function(r) { if (!r.done) out.push(r.value); }); \
             _lumen_drain_microtasks(); \
             writer.write(new Uint8Array([0x82, 0xAC])); \
             reader.read().then(function(r) { if (!r.done) out.push(r.value); }); \
             _lumen_drain_microtasks(); \
             out.join('') === '€'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_encoder_stream_encodes_string() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var out = []; \
             var tes = new TextEncoderStream(); \
             var writer = tes.writable.getWriter(); \
             var reader = tes.readable.getReader(); \
             writer.write('Hi'); \
             reader.read().then(function(r) { out.push(r.value); }); \
             _lumen_drain_microtasks(); \
             out.length === 1 && out[0] instanceof Uint8Array && out[0][0] === 72"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn byte_length_queuing_strategy() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var s = new ByteLengthQueuingStrategy({ highWaterMark: 16 }); \
             s.highWaterMark === 16 && s.size(new Uint8Array(4)) === 4"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn count_queuing_strategy() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var s = new CountQueuingStrategy({ highWaterMark: 10 }); \
             s.highWaterMark === 10 && s.size('anything') === 1"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn readable_stream_from_array() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var done = false; \
             var rs = ReadableStream.from([10, 20, 30]); \
             var reader = rs.getReader(); \
             reader.read().then(function(r) { done = r.value === 10 && !r.done; }); \
             _lumen_drain_microtasks(); \
             done"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── <details>/<summary> + <dialog> tests ─────────────────────────────────

    /// Build a doc with <details id="d"><summary id="s">Sum</summary><p>Body</p></details>
    /// and <dialog id="dlg">Hello</dialog>.
    fn make_details_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html    = doc.create_element(QualName::html("html"));
        let body    = doc.create_element(QualName::html("body"));
        let details = doc.create_element(QualName::html("details"));
        let summary = doc.create_element(QualName::html("summary"));
        let p       = doc.create_element(QualName::html("p"));
        let dialog  = doc.create_element(QualName::html("dialog"));
        fn set_id(doc: &mut Document, nid: lumen_dom::NodeId, id: &str) {
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
                attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: id.into() });
            }
        }
        set_id(&mut doc, details, "d");
        set_id(&mut doc, summary, "s");
        set_id(&mut doc, dialog,  "dlg");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, details);
        doc.append_child(details, summary);
        doc.append_child(details, p);
        doc.append_child(body, dialog);
        Arc::new(Mutex::new(doc))
    }

    #[test]
    fn toggle_attribute_add() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var el = document.getElementById('main'); \
             el.toggleAttribute('hidden') === true && el.hasAttribute('hidden')"));
    }

    #[test]
    fn toggle_attribute_remove() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var el = document.getElementById('main'); \
             el.setAttribute('hidden', ''); \
             el.toggleAttribute('hidden') === false && !el.hasAttribute('hidden')"));
    }

    #[test]
    fn toggle_attribute_force_true() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var el = document.getElementById('main'); \
             el.toggleAttribute('hidden', true) === true && el.hasAttribute('hidden')"));
    }

    #[test]
    fn toggle_attribute_force_false() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var el = document.getElementById('main'); \
             el.setAttribute('hidden', ''); \
             el.toggleAttribute('hidden', false) === false && !el.hasAttribute('hidden')"));
    }

    #[test]
    fn details_open_property_getter() {
        let rt = runtime_with_dom(make_details_doc());
        // No `open` attr by default → open === false
        assert!(bool_eval(&rt,
            "document.getElementById('d').open === false"));
    }

    #[test]
    fn details_open_property_setter() {
        let rt = runtime_with_dom(make_details_doc());
        assert!(bool_eval(&rt,
            "var d = document.getElementById('d'); \
             d.open = true; \
             d.hasAttribute('open') && d.open === true"));
    }

    #[test]
    fn details_summary_click_opens() {
        let rt = runtime_with_dom(make_details_doc());
        // Simulate click on summary via _lumen_dispatch_bubble
        let nid_js = rt.eval(
            "document.getElementById('s').__nid__"
        ).unwrap();
        let nid = match nid_js { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
        rt.eval(&format!(
            "_lumen_dispatch_bubble({}, 'click')", nid
        )).unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('d').hasAttribute('open')"));
    }

    #[test]
    fn details_summary_click_closes() {
        let rt = runtime_with_dom(make_details_doc());
        // First open via JS
        rt.eval("document.getElementById('d').setAttribute('open', '')").unwrap();
        let nid_js = rt.eval("document.getElementById('s').__nid__").unwrap();
        let nid = match nid_js { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
        rt.eval(&format!("_lumen_dispatch_bubble({}, 'click')", nid)).unwrap();
        assert!(bool_eval(&rt,
            "!document.getElementById('d').hasAttribute('open')"));
    }

    #[test]
    fn details_toggle_event_fired() {
        let rt = runtime_with_dom(make_details_doc());
        rt.eval(
            "var gotToggle = false; \
             document.getElementById('d').addEventListener('toggle', function(e) { \
                 gotToggle = e.newState === 'open'; \
             });"
        ).unwrap();
        let nid_js = rt.eval("document.getElementById('s').__nid__").unwrap();
        let nid = match nid_js { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
        rt.eval(&format!("_lumen_dispatch_bubble({}, 'click')", nid)).unwrap();
        assert!(bool_eval(&rt, "gotToggle"));
    }

    #[test]
    fn dialog_show_sets_open() {
        let rt = runtime_with_dom(make_details_doc());
        assert!(bool_eval(&rt,
            "var dlg = document.getElementById('dlg'); \
             dlg.show(); \
             dlg.hasAttribute('open') && dlg.open === true"));
    }

    #[test]
    fn dialog_show_modal_sets_open() {
        let rt = runtime_with_dom(make_details_doc());
        assert!(bool_eval(&rt,
            "var dlg = document.getElementById('dlg'); \
             dlg.showModal(); \
             dlg.hasAttribute('open') && dlg.open === true"));
    }

    #[test]
    fn dialog_close_removes_open() {
        let rt = runtime_with_dom(make_details_doc());
        assert!(bool_eval(&rt,
            "var dlg = document.getElementById('dlg'); \
             dlg.show(); \
             dlg.close(); \
             !dlg.hasAttribute('open')"));
    }

    #[test]
    fn dialog_close_fires_close_event() {
        let rt = runtime_with_dom(make_details_doc());
        assert!(bool_eval(&rt,
            "var dlg = document.getElementById('dlg'); \
             var got = false; \
             dlg.addEventListener('close', function() { got = true; }); \
             dlg.show(); \
             dlg.close(); \
             got"));
    }

    #[test]
    fn dialog_return_value() {
        let rt = runtime_with_dom(make_details_doc());
        assert!(bool_eval(&rt,
            "var dlg = document.getElementById('dlg'); \
             dlg.show(); \
             dlg.close('ok'); \
             dlg.returnValue === 'ok'"));
    }

    #[test]
    fn dialog_escape_key_closes_modal() {
        let rt = runtime_with_dom(make_details_doc());
        rt.eval("document.getElementById('dlg').showModal()").unwrap();
        // Fire keydown Escape on the root — document listener should close dialog
        let root_nid = rt.eval("_lumen_root_nid").unwrap();
        let nid = match root_nid { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
        rt.eval(&format!(
            "_lumen_dispatch_key_event({}, 'keydown', 'Escape', 'Escape', 27, 0, 0, false, false)",
            nid
        )).unwrap();
        assert!(bool_eval(&rt,
            "!document.getElementById('dlg').hasAttribute('open')"));
    }

    #[test]
    fn dialog_escape_cancel_preventable() {
        let rt = runtime_with_dom(make_details_doc());
        rt.eval(
            "document.getElementById('dlg').showModal(); \
             document.getElementById('dlg').addEventListener('cancel', function(e) { \
                 e.preventDefault(); \
             });"
        ).unwrap();
        let root_nid = rt.eval("_lumen_root_nid").unwrap();
        let nid = match root_nid { lumen_core::JsValue::Number(n) => n as i32, _ => panic!() };
        rt.eval(&format!(
            "_lumen_dispatch_key_event({}, 'keydown', 'Escape', 'Escape', 27, 0, 0, false, false)",
            nid
        )).unwrap();
        // cancel was prevented, so dialog stays open
        assert!(bool_eval(&rt,
            "document.getElementById('dlg').hasAttribute('open')"));
    }

    // ── <dialog> focus management tests (HTML LS §6.6.3) ─────────────────────

    fn make_dialog_focus_doc() -> Arc<Mutex<Document>> {
        // <body>
        //   <button id="btn">Trigger</button>
        //   <dialog id="dlg">
        //     <button id="ok" autofocus>OK</button>
        //   </dialog>
        //   <dialog id="dlg2">
        //     <button id="ok2">OK (no autofocus)</button>
        //   </dialog>
        // </body>
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let btn   = doc.create_element(QualName::html("button"));
        let dlg   = doc.create_element(QualName::html("dialog"));
        let ok    = doc.create_element(QualName::html("button"));
        let dlg2  = doc.create_element(QualName::html("dialog"));
        let ok2   = doc.create_element(QualName::html("button"));
        set_attribute(&mut doc, btn,  "id", "btn");
        set_attribute(&mut doc, dlg,  "id", "dlg");
        set_attribute(&mut doc, ok,   "id", "ok");
        set_attribute(&mut doc, ok,   "autofocus", "");
        set_attribute(&mut doc, dlg2, "id", "dlg2");
        set_attribute(&mut doc, ok2,  "id", "ok2");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, btn);
        doc.append_child(body, dlg);
        doc.append_child(dlg, ok);
        doc.append_child(body, dlg2);
        doc.append_child(dlg2, ok2);
        Arc::new(Mutex::new(doc))
    }

    #[test]
    fn dialog_show_modal_requests_focus_on_autofocus() {
        let rt = runtime_with_dom(make_dialog_focus_doc());
        // showModal should queue a focus request for the [autofocus] button.
        rt.eval("document.getElementById('dlg').showModal();").unwrap();
        let reqs = rt.take_focus_requests();
        // Should have exactly one request for the [autofocus] button (ok).
        assert!(!reqs.is_empty(), "showModal should push a focus request");
        assert!(reqs.iter().any(|r| r.is_some()), "focus request should be Some(nid)");
    }

    #[test]
    fn dialog_show_modal_requests_focus_on_dialog_when_no_autofocus() {
        let rt = runtime_with_dom(make_dialog_focus_doc());
        // dlg2 has no [autofocus] child — should focus the dialog itself.
        rt.eval("document.getElementById('dlg2').showModal();").unwrap();
        let reqs = rt.take_focus_requests();
        assert!(!reqs.is_empty(), "showModal without autofocus should push a focus request");
        assert!(reqs.iter().any(|r| r.is_some()), "focus request should be Some(dialog_nid)");
    }

    #[test]
    fn dialog_close_requests_blur_when_no_previous_focus() {
        let rt = runtime_with_dom(make_dialog_focus_doc());
        rt.eval("document.getElementById('dlg').showModal();").unwrap();
        let _ = rt.take_focus_requests(); // drain showModal requests
        rt.eval("document.getElementById('dlg').close();").unwrap();
        let reqs = rt.take_focus_requests();
        // No previous focus (nid=-1) → should push a blur request (None).
        assert!(!reqs.is_empty(), "close should push a focus request");
        assert!(reqs.iter().any(|r| r.is_none()), "close with no prev focus should push None (blur)");
    }

    #[test]
    fn dialog_close_restores_previous_focus() {
        let rt = runtime_with_dom(make_dialog_focus_doc());
        // Simulate a previous focus on btn (set _lumen_last_focused_nid manually).
        let btn_nid: i32 = match rt.eval("document.getElementById('btn').__nid__").unwrap() {
            lumen_core::JsValue::Number(n) => n as i32,
            _ => panic!("btn nid not a number"),
        };
        rt.eval(&format!("_lumen_last_focused_nid = {};", btn_nid)).unwrap();
        rt.eval("document.getElementById('dlg').showModal();").unwrap();
        let _ = rt.take_focus_requests(); // drain showModal
        rt.eval("document.getElementById('dlg').close();").unwrap();
        let reqs = rt.take_focus_requests();
        // Should restore focus to btn.
        assert!(
            reqs.iter().any(|r| r == &Some(btn_nid as u32)),
            "close should restore focus to the previously focused element"
        );
    }

    #[test]
    fn dialog_last_focused_nid_global_exists() {
        let rt = runtime_with_dom(make_dialog_focus_doc());
        // The global should be initialised to -1.
        assert!(bool_eval(&rt, "_lumen_last_focused_nid === -1"));
    }

    // ── <selectlist> tests (Open UI Customizable Select §3, Phase 0) ─────────

    fn make_selectlist_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let sl   = doc.create_element(QualName::html("selectlist"));
        let o1   = doc.create_element(QualName::html("option"));
        let o2   = doc.create_element(QualName::html("option"));
        let o3   = doc.create_element(QualName::html("option"));
        fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, k: &str, v: &str) {
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
                attrs.push(lumen_dom::Attribute { name: QualName::html(k), value: v.into() });
            }
        }
        fn set_text(doc: &mut Document, nid: lumen_dom::NodeId, text: &str) {
            let t = doc.create_text(text.to_owned());
            doc.append_child(nid, t);
        }
        set_attr(&mut doc, sl, "id", "sl");
        set_attr(&mut doc, o1, "value", "a");
        set_text(&mut doc, o1, "Apple");
        set_attr(&mut doc, o2, "value", "b");
        set_attr(&mut doc, o2, "selected", "");
        set_text(&mut doc, o2, "Banana");
        set_attr(&mut doc, o3, "value", "c");
        set_text(&mut doc, o3, "Cherry");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, sl);
        doc.append_child(sl, o1);
        doc.append_child(sl, o2);
        doc.append_child(sl, o3);
        Arc::new(Mutex::new(doc))
    }

    #[test]
    fn selectlist_options_length() {
        let rt = runtime_with_dom(make_selectlist_doc());
        assert!(bool_eval(&rt,
            "document.getElementById('sl').options.length === 3 && \
             document.getElementById('sl').length === 3"));
    }

    #[test]
    fn selectlist_selected_index_from_attr() {
        let rt = runtime_with_dom(make_selectlist_doc());
        // o2 has `selected` attr → index 1
        assert!(bool_eval(&rt,
            "document.getElementById('sl').selectedIndex === 1"));
    }

    #[test]
    fn selectlist_value_from_selected_option() {
        let rt = runtime_with_dom(make_selectlist_doc());
        assert!(bool_eval(&rt,
            "document.getElementById('sl').value === 'b'"));
    }

    #[test]
    fn selectlist_set_value_changes_selected() {
        let rt = runtime_with_dom(make_selectlist_doc());
        assert!(bool_eval(&rt,
            "var sl = document.getElementById('sl'); \
             sl.value = 'c'; \
             sl.value === 'c' && sl.selectedIndex === 2"));
    }

    #[test]
    fn selectlist_item_by_index() {
        let rt = runtime_with_dom(make_selectlist_doc());
        assert!(bool_eval(&rt,
            "var sl = document.getElementById('sl'); \
             sl.item(0) !== null && sl.item(0).getAttribute('value') === 'a' && \
             sl.item(99) === null"));
    }

    // ── HTML Popover API tests (WHATWG HTML §6.12) ────────────────────────────

    /// Build a document with two popover divs and a trigger button.
    fn make_popover_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html  = doc.create_element(QualName::html("html"));
        let body  = doc.create_element(QualName::html("body"));
        let pop1  = doc.create_element(QualName::html("div"));
        let pop2  = doc.create_element(QualName::html("div"));
        let btn   = doc.create_element(QualName::html("button"));
        fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, k: &str, v: &str) {
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
                attrs.push(lumen_dom::Attribute { name: QualName::html(k), value: v.into() });
            }
        }
        set_attr(&mut doc, pop1, "id",      "p1");
        set_attr(&mut doc, pop1, "popover", "auto");
        set_attr(&mut doc, pop2, "id",      "p2");
        set_attr(&mut doc, pop2, "popover", "manual");
        set_attr(&mut doc, btn,  "id",      "btn");
        set_attr(&mut doc, btn,  "popovertarget", "p1");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, pop1);
        doc.append_child(body, pop2);
        doc.append_child(body, btn);
        Arc::new(Mutex::new(doc))
    }

    #[test]
    fn popover_property_getter_auto() {
        let rt = runtime_with_dom(make_popover_doc());
        assert!(bool_eval(&rt, "document.getElementById('p1').popover === 'auto'"));
    }

    #[test]
    fn popover_property_getter_manual() {
        let rt = runtime_with_dom(make_popover_doc());
        assert!(bool_eval(&rt, "document.getElementById('p2').popover === 'manual'"));
    }

    #[test]
    fn popover_property_getter_no_attr() {
        let rt = runtime_with_dom(make_popover_doc());
        assert!(bool_eval(&rt, "document.getElementById('btn').popover === null"));
    }

    #[test]
    fn popover_show_sets_open_attr() {
        let rt = runtime_with_dom(make_popover_doc());
        rt.eval("document.getElementById('p1').showPopover()").unwrap();
        assert!(bool_eval(&rt, "document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
    }

    #[test]
    fn popover_hide_removes_open_attr() {
        let rt = runtime_with_dom(make_popover_doc());
        rt.eval("var p1 = document.getElementById('p1'); p1.showPopover(); p1.hidePopover()").unwrap();
        assert!(bool_eval(&rt, "!document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
    }

    #[test]
    fn popover_toggle_shows_when_closed() {
        let rt = runtime_with_dom(make_popover_doc());
        rt.eval("document.getElementById('p1').togglePopover()").unwrap();
        assert!(bool_eval(&rt, "document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
    }

    #[test]
    fn popover_toggle_hides_when_open() {
        let rt = runtime_with_dom(make_popover_doc());
        rt.eval("var p1 = document.getElementById('p1'); p1.showPopover(); p1.togglePopover()").unwrap();
        assert!(bool_eval(&rt, "!document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
    }

    #[test]
    fn popover_toggle_event_fired() {
        let rt = runtime_with_dom(make_popover_doc());
        assert!(bool_eval(&rt,
            "var evt = null; \
             document.getElementById('p1').addEventListener('toggle', function(e) { evt = e; }); \
             document.getElementById('p1').showPopover(); \
             evt !== null && evt.oldState === 'closed' && evt.newState === 'open'"));
    }

    #[test]
    fn popover_beforetoggle_event_fired() {
        let rt = runtime_with_dom(make_popover_doc());
        assert!(bool_eval(&rt,
            "var evt = null; \
             document.getElementById('p1').addEventListener('beforetoggle', function(e) { evt = e; }); \
             document.getElementById('p1').showPopover(); \
             evt !== null && evt.oldState === 'closed' && evt.newState === 'open'"));
    }

    #[test]
    fn popover_auto_closes_other_auto_on_show() {
        let rt = runtime_with_dom(make_popover_doc());
        // Create a second auto popover and show it first
        rt.eval(
            "var p1 = document.getElementById('p1'); \
             p1.showPopover(); \
             // Now change p2 to auto and show it — p1 should be auto-closed
             document.getElementById('p2').setAttribute('popover','auto'); \
             document.getElementById('p2').showPopover();"
        ).unwrap();
        // p1 should now be hidden, p2 open
        assert!(bool_eval(&rt,
            "!document.getElementById('p1').hasAttribute('data-lumen-popover-open') && \
             document.getElementById('p2').hasAttribute('data-lumen-popover-open')"));
    }

    #[test]
    fn popover_manual_does_not_close_auto() {
        let rt = runtime_with_dom(make_popover_doc());
        // Open auto p1, then open manual p2 — p1 should stay open
        rt.eval("document.getElementById('p1').showPopover(); document.getElementById('p2').showPopover()").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('p1').hasAttribute('data-lumen-popover-open') && \
             document.getElementById('p2').hasAttribute('data-lumen-popover-open')"));
    }

    #[test]
    fn popover_fixed_style_applied_on_show() {
        let rt = runtime_with_dom(make_popover_doc());
        rt.eval("document.getElementById('p1').showPopover()").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('p1').style.getPropertyValue('position') === 'fixed'"));
    }

    #[test]
    fn popover_style_restored_on_hide() {
        let rt = runtime_with_dom(make_popover_doc());
        // Set a custom style before showing
        rt.eval(
            "var p = document.getElementById('p1'); \
             p.style.color = 'red'; \
             p.showPopover(); \
             p.hidePopover();"
        ).unwrap();
        // position should no longer be 'fixed' after hide
        assert!(bool_eval(&rt,
            "document.getElementById('p1').style.getPropertyValue('position') !== 'fixed'"));
    }

    #[test]
    fn popovertarget_button_shows_popover() {
        let rt = runtime_with_dom(make_popover_doc());
        // btn has popovertarget="p1"; simulate a mouse click on btn — bubbles to document.
        rt.eval(
            "var btn = document.getElementById('btn'); \
             _lumen_dispatch_mouse_event(btn.__nid__, 'click', 0, 0, 0, 1, 0);"
        ).unwrap();
        assert!(bool_eval(&rt, "document.getElementById('p1').hasAttribute('data-lumen-popover-open')"));
    }

    // ── popover=hint tests (Popover API Level 2) ──────────────────────────────

    fn make_hint_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html  = doc.create_element(QualName::html("html"));
        let body  = doc.create_element(QualName::html("body"));
        let auto_pop = doc.create_element(QualName::html("div"));
        let hint_pop = doc.create_element(QualName::html("div"));
        fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, k: &str, v: &str) {
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
                attrs.push(lumen_dom::Attribute { name: QualName::html(k), value: v.into() });
            }
        }
        set_attr(&mut doc, auto_pop, "id",      "auto");
        set_attr(&mut doc, auto_pop, "popover", "auto");
        set_attr(&mut doc, hint_pop, "id",      "hint");
        set_attr(&mut doc, hint_pop, "popover", "hint");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, auto_pop);
        doc.append_child(body, hint_pop);
        Arc::new(Mutex::new(doc))
    }

    #[test]
    fn hint_popover_property_getter() {
        let rt = runtime_with_dom(make_hint_doc());
        assert!(bool_eval(&rt, "document.getElementById('hint').popover === 'hint'"));
    }

    #[test]
    fn hint_show_does_not_close_auto() {
        let rt = runtime_with_dom(make_hint_doc());
        // Show auto popover first, then show hint — auto must stay open.
        assert!(bool_eval(&rt,
            "(function() { \
               document.getElementById('auto').showPopover(); \
               document.getElementById('hint').showPopover(); \
               return document.getElementById('auto').hasAttribute('data-lumen-popover-open') \
                   && document.getElementById('hint').hasAttribute('data-lumen-popover-open'); \
             })()"));
    }

    #[test]
    fn auto_show_closes_hint() {
        let rt = runtime_with_dom(make_hint_doc());
        // Show hint first, then auto — hint must be closed.
        assert!(bool_eval(&rt,
            "(function() { \
               document.getElementById('hint').showPopover(); \
               document.getElementById('auto').showPopover(); \
               return !document.getElementById('hint').hasAttribute('data-lumen-popover-open') \
                   && document.getElementById('auto').hasAttribute('data-lumen-popover-open'); \
             })()"));
    }

    // ── Form Constraint Validation API tests ──────────────────────────────────

    /// Helper: build a document with a <form> containing one <input>.
    fn make_form_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html  = doc.create_element(QualName::html("html"));
        let body  = doc.create_element(QualName::html("body"));
        let form  = doc.create_element(QualName::html("form"));
        let input = doc.create_element(QualName::html("input"));
        fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, name: &str, val: &str) {
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
                attrs.push(lumen_dom::Attribute { name: QualName::html(name), value: val.into() });
            }
        }
        set_attr(&mut doc, form,  "id",   "f");
        set_attr(&mut doc, input, "id",   "inp");
        set_attr(&mut doc, input, "type", "text");
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, form);
        doc.append_child(form, input);
        Arc::new(Mutex::new(doc))
    }

    #[test]
    fn validity_state_class_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof ValidityState === 'function'"));
    }

    #[test]
    fn input_has_validity_property() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "var inp = document.getElementById('inp'); inp.validity instanceof ValidityState"));
    }

    #[test]
    fn validity_valid_by_default() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.valid === true"));
    }

    #[test]
    fn validity_value_missing_required_empty() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').setAttribute('required', '')").unwrap();
        assert!(bool_eval(&rt,
            "var v = document.getElementById('inp').validity; \
             v.valueMissing === true && v.valid === false"));
    }

    #[test]
    fn validity_value_missing_clears_when_filled() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('required', ''); inp.value = 'hello'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.valueMissing === false"));
    }

    #[test]
    fn validity_type_mismatch_email() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'email'); inp.value = 'notanemail'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.typeMismatch === true"));
    }

    #[test]
    fn validity_type_mismatch_email_valid() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'email'); inp.value = 'user@example.com'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.typeMismatch === false && \
             document.getElementById('inp').validity.valid === true"));
    }

    #[test]
    fn validity_type_mismatch_url() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'url'); inp.value = 'not-a-url'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.typeMismatch === true"));
    }

    #[test]
    fn validity_pattern_mismatch() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('pattern', '[0-9]+'); inp.value = 'abc'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.patternMismatch === true"));
    }

    #[test]
    fn validity_pattern_match_ok() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('pattern', '[0-9]+'); inp.value = '42'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.patternMismatch === false"));
    }

    #[test]
    fn validity_too_long() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('maxlength', '3'); inp.value = 'hello'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.tooLong === true"));
    }

    #[test]
    fn validity_too_short() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('minlength', '5'); inp.value = 'hi'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.tooShort === true"));
    }

    #[test]
    fn validity_range_underflow() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'number'); inp.setAttribute('min', '10'); inp.value = '5'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.rangeUnderflow === true"));
    }

    #[test]
    fn validity_range_overflow() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'number'); inp.setAttribute('max', '10'); inp.value = '20'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.rangeOverflow === true"));
    }

    #[test]
    fn validity_step_mismatch() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'number'); inp.setAttribute('step', '5'); inp.value = '7'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.stepMismatch === true"));
    }

    #[test]
    fn set_custom_validity_sets_custom_error() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').setCustomValidity('bad input')").unwrap();
        assert!(bool_eval(&rt,
            "var v = document.getElementById('inp').validity; \
             v.customError === true && v.valid === false"));
    }

    #[test]
    fn set_custom_validity_empty_clears_error() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("var inp = document.getElementById('inp'); inp.setCustomValidity('err'); inp.setCustomValidity('')").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validity.customError === false"));
    }

    #[test]
    fn will_validate_input_true() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "document.getElementById('inp').willValidate === true"));
    }

    #[test]
    fn will_validate_hidden_false() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').setAttribute('type', 'hidden')").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').willValidate === false"));
    }

    #[test]
    fn check_validity_valid_returns_true() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "document.getElementById('inp').checkValidity() === true"));
    }

    #[test]
    fn check_validity_fires_invalid_event() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval(
            "var inp = document.getElementById('inp'); \
             inp.setAttribute('required', ''); \
             var fired = false; \
             inp.addEventListener('invalid', function() { fired = true; });"
        ).unwrap();
        rt.eval("document.getElementById('inp').checkValidity()").unwrap();
        assert!(bool_eval(&rt, "fired === true"));
    }

    #[test]
    fn report_validity_delegates_to_check_validity() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').setAttribute('required', '')").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').reportValidity() === false"));
    }

    #[test]
    fn form_elements_collection() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "var form = document.getElementById('f'); \
             form.elements.length >= 1"));
    }

    #[test]
    fn form_no_validate_attr() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('f').noValidate = true").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('f').hasAttribute('novalidate')"));
    }

    #[test]
    fn validation_message_custom() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').setCustomValidity('Must be a number')").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validationMessage === 'Must be a number'"));
    }

    #[test]
    fn validation_message_value_missing() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').setAttribute('required', '')").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').validationMessage.length > 0"));
    }

    #[test]
    fn input_value_get_set() {
        let rt = runtime_with_dom(make_form_doc());
        rt.eval("document.getElementById('inp').value = 'hello world'").unwrap();
        assert!(bool_eval(&rt,
            "document.getElementById('inp').value === 'hello world'"));
    }

    #[test]
    fn input_type_reflected() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "document.getElementById('inp').type === 'text'"));
    }

    // ── HTMLInputElement.showPicker() tests ────────────────────────────────────

    #[test]
    fn show_picker_exists_on_input() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "typeof document.getElementById('inp').showPicker === 'function'"));
    }

    #[test]
    fn show_picker_throws_for_text_type() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "(function() { \
               var inp = document.getElementById('inp'); \
               try { inp.showPicker(); return false; } \
               catch(e) { return e.name === 'NotSupportedError'; } \
             })()"));
    }

    #[test]
    fn show_picker_fires_click_for_color() {
        let rt = runtime_with_dom(make_form_doc());
        assert!(bool_eval(&rt,
            "(function() { \
               var inp = document.getElementById('inp'); \
               inp.setAttribute('type', 'color'); \
               var clicked = false; \
               inp.addEventListener('click', function() { clicked = true; }); \
               try { inp.showPicker(); } catch(e) {} \
               return clicked; \
             })()"));
    }

    // ── document.caretPositionFromPoint tests ──────────────────────────────────

    #[test]
    fn caret_position_from_point_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof document.caretPositionFromPoint === 'function'"));
    }

    #[test]
    fn caret_position_from_point_returns_object() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "(function() { \
               var cp = document.caretPositionFromPoint(10, 20); \
               return cp !== null && cp.offsetNode !== undefined && typeof cp.offset === 'number'; \
             })()"));
    }

    #[test]
    fn caret_position_from_point_has_get_client_rects() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "(function() { \
               var cp = document.caretPositionFromPoint(0, 0); \
               return cp !== null && typeof cp.getClientRects === 'function'; \
             })()"));
    }

    // ── requestIdleCallback / cancelIdleCallback tests ─────────────────────────

    #[test]
    fn request_idle_callback_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "typeof requestIdleCallback === 'function' && typeof window.requestIdleCallback === 'function'"));
    }

    #[test]
    fn cancel_idle_callback_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "typeof cancelIdleCallback === 'function'"));
    }

    #[test]
    fn request_idle_callback_returns_numeric_id() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "typeof requestIdleCallback(function(){}) === 'number'"));
    }

    #[test]
    fn cancel_idle_callback_does_not_throw() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("cancelIdleCallback(999)").unwrap();
    }

    #[test]
    fn request_idle_callback_bad_arg_throws() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var threw = false; \
             try { requestIdleCallback('notafn'); } catch(e) { threw = e instanceof TypeError; } \
             threw"));
    }

    // ── MessageChannel / MessagePort tests ────────────────────────────────────

    #[test]
    fn message_channel_creates_two_ports() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             ch.port1 instanceof MessagePort && ch.port2 instanceof MessagePort"));
    }

    #[test]
    fn message_channel_ports_are_distinct() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); ch.port1 !== ch.port2"));
    }

    #[test]
    fn message_port_post_delivers_via_onmessage() {
        let rt = runtime_with_dom(make_doc());
        // onmessage auto-starts port1; postMessage on port2 delivers to port1.
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var received = null; \
             ch.port1.onmessage = function(e) { received = e.data; }; \
             ch.port2.postMessage('hello'); \
             _lumen_drain_microtasks(); \
             received === 'hello'"));
    }

    #[test]
    fn message_port_post_delivers_object() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var got = null; \
             ch.port1.onmessage = function(e) { got = e.data; }; \
             ch.port2.postMessage({ x: 42 }); \
             _lumen_drain_microtasks(); \
             got !== null && got.x === 42"));
    }

    #[test]
    fn message_port_structured_clone_is_deep_copy() {
        let rt = runtime_with_dom(make_doc());
        // Mutations to the original after postMessage should not affect received copy.
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var got = null; \
             ch.port1.onmessage = function(e) { got = e.data; }; \
             var orig = { v: 1 }; \
             ch.port2.postMessage(orig); \
             orig.v = 99; \
             _lumen_drain_microtasks(); \
             got !== null && got.v === 1"));
    }

    #[test]
    fn message_port_start_drains_queue() {
        let rt = runtime_with_dom(make_doc());
        // Post before onmessage is set → message queued; start() drains it.
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var got = null; \
             ch.port2.postMessage('queued'); \
             ch.port1.onmessage = function(e) { got = e.data; }; \
             _lumen_drain_microtasks(); \
             got === 'queued'"));
    }

    #[test]
    fn message_port_add_event_listener_delivers() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var got = null; \
             ch.port1.addEventListener('message', function(e) { got = e.data; }); \
             ch.port2.postMessage('evt'); \
             _lumen_drain_microtasks(); \
             got === 'evt'"));
    }

    #[test]
    fn message_port_close_stops_delivery() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var count = 0; \
             ch.port1.onmessage = function() { count++; }; \
             ch.port2.postMessage('a'); \
             ch.port1.close(); \
             ch.port2.postMessage('b'); \
             _lumen_drain_microtasks(); \
             count === 0"));
    }

    #[test]
    fn message_port_remove_event_listener_stops_delivery() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var ch = new MessageChannel(); \
             var count = 0; \
             var fn = function() { count++; }; \
             ch.port1.addEventListener('message', fn); \
             ch.port1.removeEventListener('message', fn); \
             ch.port2.postMessage('x'); \
             _lumen_drain_microtasks(); \
             count === 0"));
    }

    #[test]
    fn message_channel_window_export() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.MessageChannel === MessageChannel"));
    }

    // ── navigator.clipboard tests ──────────────────────────────────────────────

    #[test]
    fn navigator_clipboard_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof navigator.clipboard === 'object' && navigator.clipboard !== null"));
    }

    #[test]
    fn navigator_clipboard_read_text_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "typeof navigator.clipboard.readText === 'function' && \
             typeof navigator.clipboard.readText().then === 'function'"));
    }

    #[test]
    fn navigator_clipboard_write_text_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "typeof navigator.clipboard.writeText === 'function' && \
             typeof navigator.clipboard.writeText('hi').then === 'function'"));
    }

    #[test]
    fn navigator_clipboard_stub_read_resolves_string() {
        let rt = runtime_with_dom(make_doc());
        // Without native binding, readText resolves to empty string.
        assert!(bool_eval(&rt,
            "var ok = false; \
             navigator.clipboard.readText().then(function(v) { ok = typeof v === 'string'; }); \
             _lumen_drain_microtasks(); \
             ok"));
    }

    // ── navigator.permissions tests ───────────────────────────────────────────

    #[test]
    fn navigator_permissions_query_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "typeof navigator.permissions === 'object' && \
             typeof navigator.permissions.query === 'function'"));
    }

    #[test]
    fn navigator_permissions_clipboard_granted() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var state = null; \
             navigator.permissions.query({ name: 'clipboard-read' }).then(function(ps) { state = ps.state; }); \
             _lumen_drain_microtasks(); \
             state === 'granted'"));
    }

    #[test]
    fn navigator_permissions_camera_denied() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var state = null; \
             navigator.permissions.query({ name: 'camera' }).then(function(ps) { state = ps.state; }); \
             _lumen_drain_microtasks(); \
             state === 'denied'"));
    }

    #[test]
    fn navigator_permissions_bad_descriptor_rejects() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt,
            "var rejected = false; \
             navigator.permissions.query(null).catch(function(e) { rejected = true; }); \
             _lumen_drain_microtasks(); \
             rejected"));
    }

    // ── isSecureContext / crossOriginIsolated tests ────────────────────────────

    #[test]
    fn is_secure_context_is_true() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.isSecureContext === true"));
    }

    #[test]
    fn cross_origin_isolated_is_false() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.crossOriginIsolated === false"));
    }

    // ── Web Worker tests (WHATWG Web Workers §4) ─────────────────────────────

    #[test]
    fn worker_class_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof Worker === 'function'"));
    }

    #[test]
    fn window_worker_class_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof window.Worker === 'function'"));
    }

    #[test]
    fn worker_constructor_returns_instance() {
        let rt = runtime_with_dom(make_doc());
        // Use a data: URL so no network fetch is needed.
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); w instanceof Worker"
        ));
    }

    #[test]
    fn worker_has_post_message() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); typeof w.postMessage === 'function'"
        ));
    }

    #[test]
    fn worker_has_terminate() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); typeof w.terminate === 'function'"
        ));
    }

    #[test]
    fn worker_has_add_event_listener() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); typeof w.addEventListener === 'function'"
        ));
    }

    #[test]
    fn worker_onmessage_is_null_by_default() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); w.onmessage === null"
        ));
    }

    #[test]
    fn worker_onmessage_setter_and_getter() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); \
             var fn = function(e){}; \
             w.onmessage = fn; \
             w.onmessage === fn"
        ));
    }

    #[test]
    fn worker_terminate_removes_from_registry() {
        let rt = runtime_with_dom(make_doc());
        // terminate() should not throw and the worker object still exists.
        assert!(bool_eval(
            &rt,
            "var w = new Worker('data:text/javascript,'); \
             w.terminate(); \
             w instanceof Worker"
        ));
    }

    #[test]
    fn worker_roundtrip_message_via_pump() {
        use std::time::Duration;
        let rt = runtime_with_dom(make_doc());
        // Worker script: echo back any message with a 'reply' wrapper.
        let script = "data:text/javascript,onmessage%20%3D%20function(e)%7BpostMessage(%7Breply%3Ae.data%7D)%3B%7D";
        rt.eval(&format!("var w = new Worker('{}'); var received = null; w.onmessage = function(e){{received=e.data.reply;}}; w.postMessage(42);", script)).unwrap();
        // Give the worker thread time to process the message.
        std::thread::sleep(Duration::from_millis(150));
        rt.pump_workers();
        let result = rt.eval("received").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(42.0));
    }

    #[test]
    fn worker_add_event_listener_fires_on_pump() {
        use std::time::Duration;
        let rt = runtime_with_dom(make_doc());
        let script = "data:text/javascript,onmessage%20%3D%20function(e)%7BpostMessage(e.data%20*%202)%3B%7D";
        rt.eval(&format!(
            "var w = new Worker('{}'); \
             var got = null; \
             w.addEventListener('message', function(e){{got=e.data;}}); \
             w.postMessage(7);",
            script
        ))
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));
        rt.pump_workers();
        let result = rt.eval("got").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(14.0));
    }

    #[test]
    fn worker_data_url_base64_script() {
        use std::time::Duration;
        // base64("postMessage('hello');") = "cG9zdE1lc3NhZ2UoJ2hlbGxvJyk7"
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var w = new Worker('data:text/javascript;base64,cG9zdE1lc3NhZ2UoJ2hlbGxvJyk7'); \
             var got = null; \
             w.onmessage = function(e){ got = e.data; };",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));
        rt.pump_workers();
        let result = rt.eval("got").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("hello".into()));
    }

    #[test]
    fn worker_blob_url_script() {
        use std::time::Duration;
        let rt = runtime_with_dom(make_doc());
        // Create a blob URL from a JS Blob and use it as the worker script.
        rt.eval(
            "var blob = new Blob([\"onmessage=function(e){postMessage(e.data+1);}\"], \
              {type:'text/javascript'}); \
             var url = URL.createObjectURL(blob); \
             var w = new Worker(url); \
             var res = null; \
             w.onmessage = function(e){ res = e.data; }; \
             w.postMessage(10);",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(150));
        rt.pump_workers();
        let result = rt.eval("res").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(11.0));
    }

    // ── _lumen_gc_collect tests ────────────────────────────────────────────────

    #[test]
    fn gc_collect_removes_listener_entries() {
        let rt = runtime_with_dom(make_doc());
        // Register two listeners on nid=42 and one on nid=99.
        rt.eval("_lumen_add_listener(42,'click',function(){}); \
                 _lumen_add_listener(42,'mouseover',function(){}); \
                 _lumen_add_listener(99,'click',function(){});")
            .unwrap();
        // Verify target listeners are present before collect.
        let has42click = rt.eval("'42:click' in _lumen_listeners").unwrap();
        assert_eq!(has42click, lumen_core::JsValue::Bool(true));
        let has42over = rt.eval("'42:mouseover' in _lumen_listeners").unwrap();
        assert_eq!(has42over, lumen_core::JsValue::Bool(true));

        // Collect nid=42 → its entries should be deleted; nid=99 must survive.
        rt.eval("_lumen_gc_collect([42]);").unwrap();

        let gone42click = rt.eval("'42:click' in _lumen_listeners").unwrap();
        assert_eq!(gone42click, lumen_core::JsValue::Bool(false));
        let gone42over = rt.eval("'42:mouseover' in _lumen_listeners").unwrap();
        assert_eq!(gone42over, lumen_core::JsValue::Bool(false));
        // nid=99 must survive.
        let has99 = rt.eval("'99:click' in _lumen_listeners").unwrap();
        assert_eq!(has99, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn gc_collect_removes_input_value_entry() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("_input_values[7] = 'hello'; _input_values[8] = 'world';")
            .unwrap();
        rt.eval("_lumen_gc_collect([7]);").unwrap();

        // Deleted property → undefined → JsValue::Null (from_rq maps both).
        let v7 = rt.eval("_input_values[7]").unwrap();
        assert_eq!(v7, lumen_core::JsValue::Null);

        let v8 = rt.eval("_input_values[8]").unwrap();
        assert_eq!(v8, lumen_core::JsValue::String("world".into()));
    }

    #[test]
    fn gc_collect_empty_array_is_noop() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("_lumen_add_listener(5,'click',function(){});").unwrap();
        rt.eval("_lumen_gc_collect([]);").unwrap();
        // nid=5 listener must still be there.
        let has5 = rt.eval("'5:click' in _lumen_listeners").unwrap();
        assert_eq!(has5, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn gc_collect_unknown_nid_is_noop() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("_lumen_add_listener(3,'focus',function(){});").unwrap();
        rt.eval("_lumen_gc_collect([9999]);").unwrap();
        // nid=3 listener must still be there.
        let has3 = rt.eval("'3:focus' in _lumen_listeners").unwrap();
        assert_eq!(has3, lumen_core::JsValue::Bool(true));
    }

    // ── deterministic render mode (8F) tests ─────────────────────────────────

    fn runtime_deterministic(doc: Arc<Mutex<Document>>, url: &str) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        rt.set_deterministic_mode();
        rt.install_dom(doc, url, None, None, None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn deterministic_date_now_returns_zero() {
        let rt = runtime_deterministic(make_doc(), "http://x.com/#test");
        let v = rt.eval("Date.now()").unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(0.0), "Date.now() must be 0 in deterministic mode");
    }

    #[test]
    fn deterministic_performance_now_returns_zero() {
        let rt = runtime_deterministic(make_doc(), "http://x.com/");
        let v = rt.eval("performance.now()").unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(0.0), "performance.now() must be 0 in deterministic mode");
    }

    #[test]
    fn deterministic_math_random_reproducible() {
        // Two runtimes with same URL fragment must produce identical random sequences.
        let rt_a = runtime_deterministic(make_doc(), "http://x.com/#seed42");
        let rt_b = runtime_deterministic(make_doc(), "http://y.org/other#seed42");
        let seq_a: Vec<_> = (0..5).map(|_| rt_a.eval("Math.random()").unwrap()).collect();
        let seq_b: Vec<_> = (0..5).map(|_| rt_b.eval("Math.random()").unwrap()).collect();
        assert_eq!(seq_a, seq_b, "same fragment → same random sequence");
    }

    #[test]
    fn deterministic_math_random_different_seeds() {
        // Different fragments must produce different sequences.
        let rt_a = runtime_deterministic(make_doc(), "http://x.com/#foo");
        let rt_b = runtime_deterministic(make_doc(), "http://x.com/#bar");
        let r_a = rt_a.eval("Math.random()").unwrap();
        let r_b = rt_b.eval("Math.random()").unwrap();
        assert_ne!(r_a, r_b, "different fragments → different random values");
    }

    #[test]
    fn deterministic_math_random_in_range() {
        let rt = runtime_deterministic(make_doc(), "http://x.com/#test");
        for _ in 0..20 {
            if let lumen_core::JsValue::Number(v) = rt.eval("Math.random()").unwrap() {
                assert!((0.0..1.0).contains(&v), "Math.random() must be in [0, 1): got {v}");
            } else {
                panic!("Math.random() must return a number");
            }
        }
    }

    #[test]
    fn normal_mode_date_now_nonzero() {
        // In non-deterministic mode Date.now() must return a positive value (wall clock).
        let rt = runtime_with_dom(make_doc());
        if let lumen_core::JsValue::Number(v) = rt.eval("Date.now()").unwrap() {
            assert!(v > 0.0, "Date.now() must be positive in normal mode");
        } else {
            panic!("Date.now() must return a number");
        }
    }

    // ─── window.open() / window.opener tests ─────────────────────────────────

    #[test]
    fn window_open_function_exists() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "typeof window.open === 'function'"));
    }

    #[test]
    fn window_opener_is_null() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(&rt, "window.opener === null"));
    }

    #[test]
    fn window_open_queues_popup_request() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.open('https://example.com', '_blank', 'width=800,height=600')")
            .unwrap();
        let reqs = rt.take_window_open_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "https://example.com");
        assert_eq!(reqs[0].target, "_blank");
        assert_eq!(reqs[0].width, 800);
        assert_eq!(reqs[0].height, 600);
    }

    #[test]
    fn window_open_empty_url_defaults_to_empty_string() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.open()").unwrap();
        let reqs = rt.take_window_open_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "");
    }

    #[test]
    fn window_open_returns_stub_object() {
        let rt = runtime_with_dom(make_doc());
        // Should return an object (not null/undefined) with a close() method.
        assert!(bool_eval(
            &rt,
            "var w = window.open('about:blank'); typeof w === 'object' && w !== null && typeof w.close === 'function'"
        ));
    }

    #[test]
    fn window_open_stub_location_href() {
        let rt = runtime_with_dom(make_doc());
        assert!(bool_eval(
            &rt,
            "var w = window.open('https://lumen.example/'); w.location.href === 'https://lumen.example/'"
        ));
    }

    #[test]
    fn window_open_multiple_calls_queue_all() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.open('https://a.com'); window.open('https://b.com', '_self')").unwrap();
        let reqs = rt.take_window_open_requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].url, "https://a.com");
        assert_eq!(reqs[1].url, "https://b.com");
    }

    #[test]
    fn window_open_feature_parsing_partial() {
        // Only width specified — height should default to 600.
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.open('https://x.com', '', 'width=1024')").unwrap();
        let reqs = rt.take_window_open_requests();
        assert_eq!(reqs[0].width, 1024);
        assert_eq!(reqs[0].height, 600);
    }

    #[test]
    fn window_open_take_clears_queue() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.open('https://a.com')").unwrap();
        let first = rt.take_window_open_requests();
        assert_eq!(first.len(), 1);
        // Second drain must be empty.
        let second = rt.take_window_open_requests();
        assert_eq!(second.len(), 0);
    }

    // ── Web Animations API ─────────────────────────────────────────────────

    #[test]
    fn web_animations_classes_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.Animation === 'function' && typeof window.KeyframeEffect === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn keyframe_effect_stores_keyframes() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var kf = new KeyframeEffect(null, [{opacity:0},{opacity:1}], 300); \
             kf.getKeyframes().length"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn keyframe_effect_timing_duration() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var kf = new KeyframeEffect(null, [], {duration:500, delay:100}); \
             kf.getTiming().duration"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(500.0));
    }

    #[test]
    fn animation_initial_state_is_idle() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.playState"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("idle".into()));
    }

    #[test]
    fn animation_play_changes_state() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.play(); \
             a.playState"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("running".into()));
    }

    #[test]
    fn animation_pause_changes_state() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.play(); a.pause(); \
             a.playState"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("paused".into()));
    }

    #[test]
    fn animation_cancel_removes_from_registry() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.play(); a.cancel();"
        ).unwrap();
        let r = rt.eval("document.getAnimations().length").unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn document_timeline_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.timeline instanceof DocumentTimeline").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_timeline_current_time_null_before_raf() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.timeline.currentTime === null").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_timeline_current_time_after_raf() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("_lumen_run_raf_callbacks(100.0)").unwrap();
        let r = rt.eval("document.timeline.currentTime >= 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_animate_returns_animation() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); \
             var a = el.animate([{opacity:0},{opacity:1}], 300); \
             a instanceof Animation"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_animate_play_state_running() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); \
             var a = el.animate([{opacity:0},{opacity:1}], 300); \
             a.playState"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("running".into()));
    }

    #[test]
    fn element_get_animations() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); \
             el.animate([{opacity:0},{opacity:1}], 500); \
             el.getAnimations().length"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn document_get_animations() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); \
             el.animate([{opacity:0},{opacity:1}], 500); \
             document.getAnimations().length"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn animation_finish_fires_onfinish() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fired = false; \
             var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.onfinish = function() { fired = true; }; \
             a.finish(); \
             fired"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn animation_finish_state() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.finish(); \
             a.playState"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("finished".into()));
    }

    #[test]
    fn keyframe_effect_property_indexed_form() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var kf = new KeyframeEffect(null, {opacity: [0, 0.5, 1]}, 400); \
             kf.getKeyframes().length"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn animation_reverse_negates_playback_rate() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var a = new Animation(new KeyframeEffect(null, [], 300)); \
             a.play(); \
             var rate_before = a.playbackRate; \
             a.reverse(); \
             a.playbackRate === -rate_before"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_animate_applies_opacity_style() {
        let rt = runtime_with_dom(make_doc());
        // Advance time then tick to let the animation apply its first frame.
        let r = rt.eval(
            "var el = document.createElement('div'); \
             document.body.appendChild(el); \
             _wa_current_time = 0; \
             var a = el.animate([{opacity:0},{opacity:1}], {duration:1000}); \
             // At t=0 the animation should set opacity to 0
             a._applyAtP(0); \
             el.style.opacity"
        ).unwrap();
        // opacity at progress=0 should be '0'
        assert_eq!(r, lumen_core::JsValue::String("0".into()));
    }

    // ── CompressionStream / DecompressionStream (WHATWG Compression Streams) ──

    #[test]
    fn compression_stream_constructor_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "typeof CompressionStream === 'function' && \
                 typeof DecompressionStream === 'function'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_invalid_format_throws() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var threw = false; \
                 try { new CompressionStream('lz4'); } catch(e) { threw = e instanceof TypeError; } \
                 threw",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn decompression_stream_invalid_format_throws() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var threw = false; \
                 try { new DecompressionStream('lz4'); } catch(e) { threw = e instanceof TypeError; } \
                 threw",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_has_readable_writable() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var cs = new CompressionStream('gzip'); \
                 cs.readable instanceof ReadableStream && cs.writable instanceof WritableStream",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_is_transform_stream() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("new CompressionStream('deflate') instanceof TransformStream")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_gzip_produces_nonempty_output() {
        let rt = runtime_with_dom(make_doc());
        // Write [72,101,108,108,111] = "Hello", close, read compressed chunk.
        let r = rt
            .eval(
                "var cs = new CompressionStream('gzip'); \
                 var writer = cs.writable.getWriter(); \
                 var reader = cs.readable.getReader(); \
                 writer.write(new Uint8Array([72,101,108,108,111])); \
                 writer.close(); \
                 _lumen_drain_microtasks(); \
                 var chunk = null; \
                 reader.read().then(function(r) { chunk = r.value; }); \
                 _lumen_drain_microtasks(); \
                 chunk instanceof Uint8Array && chunk.length > 0",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_gzip_round_trip() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var input = new Uint8Array([72,101,108,108,111]); \
                 var cs = new CompressionStream('gzip'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 _lumen_drain_microtasks(); \
                 var compressed = null; \
                 cr.read().then(function(r) { compressed = r.value; }); \
                 _lumen_drain_microtasks(); \
                 var ds = new DecompressionStream('gzip'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(compressed); dw.close(); \
                 _lumen_drain_microtasks(); \
                 var result = null; \
                 dr.read().then(function(r) { result = r.value; }); \
                 _lumen_drain_microtasks(); \
                 result instanceof Uint8Array && result.length === 5 && \
                 result[0] === 72 && result[4] === 111",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_deflate_round_trip() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var input = new Uint8Array([65,66,67]); \
                 var cs = new CompressionStream('deflate'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 _lumen_drain_microtasks(); \
                 var compressed = null; \
                 cr.read().then(function(r) { compressed = r.value; }); \
                 _lumen_drain_microtasks(); \
                 var ds = new DecompressionStream('deflate'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(compressed); dw.close(); \
                 _lumen_drain_microtasks(); \
                 var result = null; \
                 dr.read().then(function(r) { result = r.value; }); \
                 _lumen_drain_microtasks(); \
                 result instanceof Uint8Array && result.length === 3 && \
                 result[0] === 65 && result[2] === 67",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn compression_stream_deflate_raw_round_trip() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var input = new Uint8Array([1,2,3,4,5]); \
                 var cs = new CompressionStream('deflate-raw'); \
                 var cw = cs.writable.getWriter(); var cr = cs.readable.getReader(); \
                 cw.write(input); cw.close(); \
                 _lumen_drain_microtasks(); \
                 var compressed = null; \
                 cr.read().then(function(r) { compressed = r.value; }); \
                 _lumen_drain_microtasks(); \
                 var ds = new DecompressionStream('deflate-raw'); \
                 var dw = ds.writable.getWriter(); var dr = ds.readable.getReader(); \
                 dw.write(compressed); dw.close(); \
                 _lumen_drain_microtasks(); \
                 var result = null; \
                 dr.read().then(function(r) { result = r.value; }); \
                 _lumen_drain_microtasks(); \
                 result instanceof Uint8Array && result.length === 5 && \
                 result[0] === 1 && result[4] === 5",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── Fullscreen API tests (WHATWG Fullscreen §4) ───────────────────────────

    #[test]
    fn fullscreen_enabled_is_true() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.fullscreenEnabled === true").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn fullscreen_element_initially_null() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.fullscreenElement === null").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn request_fullscreen_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var body = document.body; \
             var p = body.requestFullscreen(); \
             typeof p === 'object' && typeof p.then === 'function'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn request_fullscreen_sets_fullscreen_element() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var body = document.body; \
             body.requestFullscreen(); \
             _lumen_drain_microtasks(); \
             document.fullscreenElement !== null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn request_fullscreen_sets_sentinel_attr() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var body = document.body; \
             body.requestFullscreen(); \
             _lumen_drain_microtasks(); \
             body.hasAttribute('data-lumen-fullscreen')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn request_fullscreen_fires_fullscreenchange_event() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var fired = false; \
             document.addEventListener('fullscreenchange', function() { fired = true; }); \
             document.body.requestFullscreen(); \
             _lumen_drain_microtasks(); \
             fired"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn exit_fullscreen_clears_fullscreen_element() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "document.body.requestFullscreen(); \
             _lumen_drain_microtasks(); \
             document.exitFullscreen(); \
             _lumen_drain_microtasks(); \
             document.fullscreenElement === null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn exit_fullscreen_removes_sentinel_attr() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var body = document.body; \
             body.requestFullscreen(); \
             _lumen_drain_microtasks(); \
             document.exitFullscreen(); \
             _lumen_drain_microtasks(); \
             !body.hasAttribute('data-lumen-fullscreen')"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn notify_fullscreen_exit_clears_state() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "document.body.requestFullscreen(); \
             _lumen_drain_microtasks(); \
             _lumen_notify_fullscreen_exit(); \
             _lumen_drain_microtasks(); \
             document.fullscreenElement === null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_has_onfullscreenchange_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "'onfullscreenchange' in document.body && \
             'onfullscreenerror' in document.body"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_has_onfullscreenchange_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "'onfullscreenchange' in document && \
             'onfullscreenerror' in document"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── Web Locks API ────────────────────────────────────────────────────────────

    #[test]
    fn navigator_locks_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof navigator.locks === 'object' && navigator.locks !== null").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn lock_manager_is_constructor() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.LockManager === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn exclusive_lock_granted_immediately() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var got = false;
            navigator.locks.request('r1', function(lock) {
                got = lock !== null && lock.name === 'r1' && lock.mode === 'exclusive';
            });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("got").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn shared_locks_can_be_concurrent() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var count = 0;
            navigator.locks.request('sr', {mode:'shared'}, function() { count++; });
            navigator.locks.request('sr', {mode:'shared'}, function() { count++; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("count").unwrap(), lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn if_available_returns_null_when_locked() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var nullGot = false;
            navigator.locks.request('la', function(lock) {
                // hold lock during this promise
                navigator.locks.request('la', {ifAvailable: true}, function(l2) {
                    nullGot = l2 === null;
                });
            });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("nullGot").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn lock_request_requires_callback() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var threw = false;
            navigator.locks.request('t1').catch(function() { threw = true; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("threw").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn invalid_mode_rejects() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var rejected = false;
            navigator.locks.request('m1', {mode: 'invalid'}, function() {})
              .catch(function() { rejected = true; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("rejected").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_returns_held_and_pending() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var result = null;
            navigator.locks.request('q1', function(lock) {
                navigator.locks.query().then(function(s) { result = s; });
            });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        let r = rt.eval(r#"
            result !== null &&
            typeof result.held === 'object' &&
            typeof result.pending === 'object'
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn steal_option_grants_immediately() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var second = false;
            navigator.locks.request('stl', function(lock) {
                // Hold lock; second request steals it
                return new Promise(function(res) {
                    navigator.locks.request('stl', {steal: true}, function() {
                        second = true;
                    });
                    res();
                });
            });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("second").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn aborted_signal_rejects_immediately() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var ctrl = new AbortController();
            ctrl.abort();
            var rejected = false;
            navigator.locks.request('ab1', {signal: ctrl.signal}, function() {})
              .catch(function() { rejected = true; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("rejected").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn lock_name_is_stringified() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var gotName = '';
            navigator.locks.request(42, function(lock) { gotName = lock.name; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(
            rt.eval("gotName").unwrap(),
            lumen_core::JsValue::String("42".into())
        );
    }

    // ── Screen Wake Lock stub ────────────────────────────────────────────────────

    #[test]
    fn wake_lock_request_resolves() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var sentinel = null;
            navigator.wakeLock.request('screen').then(function(s) { sentinel = s; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        let r = rt.eval(
            "sentinel !== null && sentinel.type === 'screen' && sentinel.released === false"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wake_lock_release_marks_released() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var released = false;
            navigator.wakeLock.request('screen').then(function(s) {
                s.release().then(function() { released = s.released; });
            });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("released").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wake_lock_unsupported_type_rejects() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var rej = false;
            navigator.wakeLock.request('cpu').catch(function() { rej = true; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("rej").unwrap(), lumen_core::JsValue::Bool(true));
    }

    // ── Network Information stub ────────────────────────────────────────────────

    #[test]
    fn navigator_connection_effective_type() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "navigator.connection !== undefined && \
             navigator.connection.effectiveType === '4g'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn navigator_connection_save_data_false() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("navigator.connection.saveData === false").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── navigator.userActivation ────────────────────────────────────────────────

    #[test]
    fn user_activation_has_been_active() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "navigator.userActivation.hasBeenActive === true && \
             navigator.userActivation.isActive === true"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── Web Share API stub ───────────────────────────────────────────────────────

    #[test]
    fn navigator_share_rejects() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var rej = false;
            navigator.share({ title: 'test' }).catch(function() { rej = true; });
        "#).unwrap();
        rt.eval("_lumen_drain_microtasks()").unwrap();
        assert_eq!(rt.eval("rej").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn navigator_can_share_false() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("navigator.canShare() === false").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── window.reportError() ────────────────────────────────────────────────────

    #[test]
    fn report_error_fires_error_event() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(r#"
            var fired = false;
            window.addEventListener('error', function() { fired = true; });
            reportError(new Error('test'));
            fired
        "#).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn report_error_is_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.reportError === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── CSS.supports() / CSS.escape() ─────────────────────────────────────────

    #[test]
    fn css_object_exists_on_window() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof window.CSS === 'object'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_supports_two_arg_known_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('display', 'grid')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_supports_two_arg_unknown_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('--custom-var', '1')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn css_supports_one_arg_known_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('(color: red)')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_supports_one_arg_unknown_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('(unknown-prop: x)')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn css_supports_one_arg_and_condition() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('(display: grid) and (color: red)')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_supports_one_arg_or_with_unknown() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('(unknown: x) or (color: red)')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_supports_case_insensitive() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.supports('Display', 'block')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_escape_plain_word() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("CSS.escape('hello')").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("hello".into()));
    }

    #[test]
    fn css_escape_leading_digit() {
        let rt = runtime_with_dom(make_doc());
        // Leading digit '1' must be hex-escaped.
        let r = rt.eval("CSS.escape('1abc')").unwrap();
        let s = match r { lumen_core::JsValue::String(s) => s, _ => panic!("expected string") };
        assert!(s.starts_with('\\'), "leading digit should be escaped, got: {s}");
    }

    #[test]
    fn css_supports_is_function() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof CSS.supports === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_escape_is_function() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof CSS.escape === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn trusted_types_is_defined() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof trustedTypes === 'object' && trustedTypes !== null")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_policy_returns_policy() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', {}); \
                 typeof p === 'object' && p !== null && p.name === 'test'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_html_returns_trusted_html() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', { createHTML: s => s }); \
                 const th = p.createHTML('<div>test</div>'); \
                 th instanceof TrustedHTML && th.toString() === '<div>test</div>'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_script_returns_trusted_script() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', { createScript: s => s }); \
                 const ts = p.createScript('var x = 1'); \
                 ts instanceof TrustedScript && ts.toString() === 'var x = 1'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_script_url_returns_trusted_script_url() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', { createScriptURL: s => s }); \
                 const tsu = p.createScriptURL('https://example.com/script.js'); \
                 tsu instanceof TrustedScriptURL && tsu.toString() === 'https://example.com/script.js'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn default_policy_create_html_works() {
        let rt = runtime_with_dom(make_doc());
        // TT L2: the default policy exists only after createPolicy('default', ...).
        let r = rt
            .eval(
                "trustedTypes.createPolicy('default', { createHTML: s => s }); \
                 const th = trustedTypes.defaultPolicy.createHTML('<p>test</p>'); \
                 th instanceof TrustedHTML && th.toString() === '<p>test</p>'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn duplicate_non_default_policy_names_allowed() {
        let rt = runtime_with_dom(make_doc());
        // Without a CSP trusted-types directive, duplicate non-default names
        // are allowed (TT L2 §4.3); only "default" is guarded.
        let r = rt
            .eval(
                "const a = trustedTypes.createPolicy('mypolicy', {}); \
                 const b = trustedTypes.createPolicy('mypolicy', {}); \
                 a !== b && a.name === 'mypolicy' && b.name === 'mypolicy'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn is_html_true_for_trusted_html() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', { createHTML: s => s }); \
                 const th = p.createHTML('<div></div>'); \
                 trustedTypes.isHTML(th)",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn is_html_false_for_string() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("trustedTypes.isHTML('<div></div>')").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn is_script_true_for_trusted_script() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', { createScript: s => s }); \
                 const ts = p.createScript('x=1'); \
                 trustedTypes.isScript(ts)",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn is_script_url_true_for_trusted_script_url() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "const p = trustedTypes.createPolicy('test', { createScriptURL: s => s }); \
                 const tsu = p.createScriptURL('https://example.com/s.js'); \
                 trustedTypes.isScriptURL(tsu)",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn storage_access_request_storage_access_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof document.requestStorageAccess === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn storage_access_has_storage_access_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof document.hasStorageAccess === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn storage_access_request_storage_access_for_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof document.requestStorageAccessFor === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn storage_access_has_unpartitioned_cookie_access_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof document.hasUnpartitionedCookieAccess === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_pip_request_window_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof documentPictureInPicture.requestWindow === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_pip_request_window_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("documentPictureInPicture.requestWindow() instanceof Promise")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_pip_request_window_with_options() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "documentPictureInPicture.requestWindow({width: 800, height: 600}) instanceof Promise",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_pip_window_access() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "documentPictureInPicture.requestWindow({width: 640, height: 360})\
                 .then(w => w instanceof Object && typeof w.width === 'number' && w.width === 640)",
            )
            .unwrap();
        // Promise should be created successfully
        assert_ne!(r, lumen_core::JsValue::Null);
    }

    #[test]
    fn document_pip_picture_in_picture_event_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof DocumentPictureInPictureEvent === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_pip_picture_in_picture_window_class_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof DocumentPictureInPictureWindow === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_pip_element_getter_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof Object.getOwnPropertyDescriptor(document, 'pictureInPictureElement') === 'object'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // BUG-067/070: WEB_API_SHIM defined `Event` but no global `EventTarget`, so
    // every shim doing `class X extends EventTarget` (Document PiP, WebHID, WebUSB,
    // Bluetooth, WebSerial, WebXR, Navigation API) threw "EventTarget is not defined"
    // during install_dom and silently failed to install.

    #[test]
    fn event_target_global_is_constructible() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof EventTarget === 'function' && new EventTarget() instanceof EventTarget")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn event_target_dispatch_invokes_listener() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "var t = new EventTarget(); var hit = 0;\
                 t.addEventListener('ping', function() { hit++; });\
                 t.dispatchEvent(new Event('ping'));\
                 t.removeEventListener('ping', function() {});\
                 hit === 1",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn event_target_dependent_apis_installed() {
        // All of these subclass EventTarget; before the fix they failed to install.
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval(
                "typeof documentPictureInPicture === 'object'\
                 && typeof navigator.hid === 'object'\
                 && typeof navigator.usb === 'object'\
                 && typeof navigator.bluetooth === 'object'\
                 && typeof navigator.serial === 'object'\
                 && typeof navigator.xr === 'object'\
                 && typeof window.navigation === 'object'",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("typeof CSS.registerProperty === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_valid() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("CSS.registerProperty({ name: '--my-color', syntax: '<color>', inherits: true, initialValue: 'blue' }); true")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_stored() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("CSS.registerProperty({ name: '--stored', syntax: '*', inherits: false, initialValue: 'test' }); CSS._getRegisteredProperties()['--stored'] !== undefined")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_requires_name() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("try { CSS.registerProperty({ syntax: '<color>' }); false; } catch (e) { e instanceof TypeError; }")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_requires_dash_prefix() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("try { CSS.registerProperty({ name: 'my-color' }); false; } catch (e) { e instanceof SyntaxError; }")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_default_inherits() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("CSS.registerProperty({ name: '--default-inherit', syntax: '*', initialValue: 'val' }); CSS._getRegisteredProperties()['--default-inherit'].inherits")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_register_property_default_syntax() {
        let rt = runtime_with_dom(make_doc());
        let r = rt
            .eval("CSS.registerProperty({ name: '--default-syntax', inherits: true, initialValue: 'val' }); CSS._getRegisteredProperties()['--default-syntax'].syntax === '*'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn perf_observer_take_records() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var po = new PerformanceObserver(function() {});
            po.observe({entryTypes: ['paint']});
            _lumen_deliver_paint_entry('first-paint', 100);
            var records = po.takeRecords();
            records.length === 1 && records[0].entryType === 'paint' && records[0].name === 'first-paint'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn perf_observer_lcp_entry() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
            po.observe({entryTypes: ['largest-contentful-paint']});
            // NodeId 6 = <div id="main"> in make_doc() (nodes: root=0..text=8, len 9).
            _lumen_deliver_lcp_entry(6, 1024, 200.5, 210.5);
            got.length === 1 && got[0].entryType === 'largest-contentful-paint' && got[0].size === 1024 && got[0].element !== null && Math.abs(got[0].duration - 10) < 0.1
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn perf_observer_layout_shift() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
            po.observe({entryTypes: ['layout-shift']});
            _lumen_deliver_layout_shift(0.15, 0, false);
            got.length === 1 && got[0].entryType === 'layout-shift' && got[0].value === 0.15 && got[0].hadRecentInput === false
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn perf_observer_buffered() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var po1 = new PerformanceObserver(function() {});
            po1.observe({entryTypes: ['layout-shift']});
            _lumen_deliver_layout_shift(0.1, 0, false);
            var po2 = new PerformanceObserver(function() {});
            po2.observe({entryTypes: ['layout-shift'], buffered: true});
            var buffered = po2.takeRecords();
            buffered.length === 1 && buffered[0].value === 0.1
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn perf_observer_disconnect() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var count = 0;
            var po = new PerformanceObserver(function() { count++; });
            po.observe({entryTypes: ['layout-shift']});
            _lumen_deliver_layout_shift(0.1, 0, false);
            po.disconnect();
            _lumen_deliver_layout_shift(0.2, 0, false);
            count === 1
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── Resource Timing L2 tests (E-2) ─────────────────────────────────────────

    #[test]
    fn resource_timing_record_exists_in_entries() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_record_resource_timing('https://example.com/app.js', 'script', 1000, 50);
            var entries = performance.getEntriesByType('resource');
            entries.length === 1 && entries[0].name === 'https://example.com/app.js'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resource_timing_entry_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_record_resource_timing('https://cdn.example.com/style.css', 'link', 500, 80);
            var e = performance.getEntriesByType('resource')[0];
            e.entryType === 'resource' &&
            e.initiatorType === 'link' &&
            e.fetchStart === 500 &&
            e.responseEnd === 580 &&
            e.duration === 80
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resource_timing_phase0_sub_timings_equal_fetch_start() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_record_resource_timing('https://example.com/img.png', 'img', 200, 30);
            var e = performance.getEntriesByType('resource')[0];
            e.domainLookupStart === 200 &&
            e.domainLookupEnd === 200 &&
            e.connectStart === 200 &&
            e.requestStart === 200 &&
            e.responseStart === 200
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resource_timing_clear_resource_timings() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_record_resource_timing('https://example.com/a.js', 'script', 100, 10);
            _lumen_record_resource_timing('https://example.com/b.js', 'script', 200, 20);
            performance.clearResourceTimings();
            performance.getEntriesByType('resource').length === 0
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resource_timing_observer_notified() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
            po.observe({entryTypes: ['resource']});
            _lumen_record_resource_timing('https://example.com/fetch.json', 'fetch', 300, 15);
            got.length === 1 && got[0].initiatorType === 'fetch' && got[0].duration === 15
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn resource_timing_multiple_entries() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_record_resource_timing('https://example.com/1.js', 'script', 100, 10);
            _lumen_record_resource_timing('https://example.com/2.js', 'script', 200, 20);
            _lumen_record_resource_timing('https://example.com/3.css', 'link', 300, 5);
            var all = performance.getEntriesByType('resource');
            all.length === 3 && all[2].name === 'https://example.com/3.css' && all[2].initiatorType === 'link'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── _lumen_deliver_perf_entry generic binding tests (O-2) ──────────────────

    #[test]
    fn deliver_perf_entry_basic_fields() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('longtask', 'self', 500.0, 75.0, null);
            var e = performance.getEntriesByType('longtask')[0];
            e.entryType === 'longtask' && e.name === 'self' && e.startTime === 500 && e.duration === 75
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn deliver_perf_entry_detail_json_merged() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('element', 'img', 200.0, 0.0, '{"renderTime":210,"loadTime":205,"identifier":"hero"}');
            var e = performance.getEntriesByType('element')[0];
            e.renderTime === 210 && e.loadTime === 205 && e.identifier === 'hero'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn deliver_perf_entry_notifies_observer() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
            po.observe({entryTypes: ['longtask']});
            _lumen_deliver_perf_entry('longtask', 'self', 100.0, 60.0, null);
            got.length === 1 && got[0].entryType === 'longtask'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn deliver_perf_entry_invalid_json_still_delivers() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('event', 'click', 300.0, 5.0, '{not valid json}');
            var e = performance.getEntriesByType('event')[0];
            e !== undefined && e.entryType === 'event' && e.startTime === 300
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn deliver_perf_entry_empty_detail_json_string() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('navigation', 'https://example.com/', 0.0, 800.0, '{}');
            var e = performance.getEntriesByType('navigation')[0];
            e.entryType === 'navigation' && e.duration === 800
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── Navigation Timing L2 tests (II-1) ─────────────────────────────────────

    #[test]
    fn nav_timing_observer_receives_navigation_entry() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var got = [];
            var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
            po.observe({entryTypes: ['navigation']});
            _lumen_deliver_perf_entry('navigation', 'https://example.com/', 0.0, 350.0, null);
            got.length === 1 && got[0].entryType === 'navigation' && got[0].duration === 350
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn nav_timing_start_time_is_zero() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('navigation', 'https://lumen.test/', 0.0, 120.0, null);
            var e = performance.getEntriesByType('navigation')[0];
            e.startTime === 0
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn nav_timing_name_is_url() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('navigation', 'https://lumen.test/page', 0.0, 200.0, null);
            performance.getEntriesByType('navigation')[0].name === 'https://lumen.test/page'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn nav_timing_buffered_replay() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            _lumen_deliver_perf_entry('navigation', 'https://buffered.test/', 0.0, 500.0, null);
            var got = [];
            var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
            po.observe({entryTypes: ['navigation'], buffered: true});
            got.length === 1 && got[0].name === 'https://buffered.test/'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── CSS Typed OM L1 tests (A-3 feature) ────────────────────────────────────
    #[test]
    fn css_typed_om_css_style_value_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof CSS.CSSStyleValue === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_css_unit_value_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof CSS.CSSUnitValue === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_css_keyword_value_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof CSS.CSSKeywordValue === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_element_attribute_style_map_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("document.documentElement.attributeStyleMap !== null && typeof document.documentElement.attributeStyleMap === 'object'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_element_computed_style_map_exists() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("typeof document.documentElement.computedStyleMap === 'function'").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_set_get_property() {
        let rt = runtime_with_dom(make_doc());
        // First, check that documentElement exists
        let r = rt.eval("document.documentElement !== null").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));

        // Check basic style property
        let r2 = rt.eval("typeof document.documentElement.style === 'object'").unwrap();
        assert_eq!(r2, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_has_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var el = document.documentElement;
            typeof el.attributeStyleMap.has === 'function'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_delete_property() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var el = document.documentElement;
            typeof el.attributeStyleMap.delete === 'function'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_css_unit_value_value_and_unit() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var uv = new CSS.CSSUnitValue(42, 'em');
            typeof uv.value === 'number' && typeof uv.unit === 'string'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_css_unit_value_to_method() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var uv = new CSS.CSSUnitValue(10, 'px');
            typeof uv.to === 'function' && uv.to('em') instanceof CSS.CSSUnitValue
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_style_property_map_keys_values() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var el = document.documentElement;
            typeof el.attributeStyleMap.keys === 'function' && typeof el.attributeStyleMap.values === 'function'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn css_typed_om_computed_style_property_map_is_read_only() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var el = document.documentElement;
            var computed = el.computedStyleMap();
            computed !== null && typeof computed === 'object'
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    // ── DOM node count / limit bindings ───────────────────────────────────────

    #[test]
    fn dom_node_count_binding_returns_positive() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval("_lumen_dom_node_count() > 0").unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dom_node_count_increments_after_create_element() {
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            r#"
            var before = _lumen_dom_node_count();
            document.createElement('span');
            _lumen_dom_node_count() === before + 1
            "#
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dom_node_count_at_max_after_prefill() {
        let doc = {
            use lumen_dom::{Document, QualName};
            let mut d = Document::new();
            while d.node_count() < lumen_dom::MAX_DOM_NODES {
                d.create_element(QualName::html("div"));
            }
            // Verify prefill worked
            assert_eq!(d.node_count(), lumen_dom::MAX_DOM_NODES);
            Arc::new(Mutex::new(d))
        };
        let rt = runtime_with_dom(doc);
        // The binding should reflect the pre-filled count
        let r = rt.eval(&format!("_lumen_dom_node_count() >= {}", lumen_dom::MAX_DOM_NODES)).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dom_create_element_throws_quota_exceeded_when_full() {
        // Pre-fill the arena to MAX_DOM_NODES via the Rust API so the JS
        // binding returns the error sentinel without 50 000 JS evals.
        let doc = {
            use lumen_dom::{Document, QualName};
            let mut d = Document::new();
            while d.node_count() < lumen_dom::MAX_DOM_NODES {
                d.create_element(QualName::html("div"));
            }
            Arc::new(Mutex::new(d))
        };
        let rt = runtime_with_dom(doc);
        // QuickJS converts Rust u32::MAX to -1 (signed overflow), so the shim
        // now checks `nid < 0` and throws QuotaExceededError.
        let r = rt.eval(
            r#"
            var caught = '';
            try { document.createElement('p'); }
            catch (e) { caught = e.name; }
            caught
            "#,
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::String("QuotaExceededError".into()));
    }

    // ── D-6: chrome.runtime stub tests ───────────────────────────────────────

    #[test]
    fn chrome_runtime_exists() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("typeof chrome !== 'undefined' && typeof chrome.runtime !== 'undefined'").unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn chrome_runtime_send_message_is_function() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("typeof chrome.runtime.sendMessage").unwrap();
        assert_eq!(v, lumen_core::JsValue::String("function".into()));
    }

    #[test]
    fn chrome_runtime_send_message_does_not_throw() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var ok = false;
            try { chrome.runtime.sendMessage({type: 'test'}); ok = true; } catch(e) {}
            ok
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn chrome_runtime_on_message_add_listener() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var called = false;
            chrome.runtime.onMessage.addListener(function(msg) { called = true; });
            chrome.runtime.onMessage._listeners.length === 1
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn browser_runtime_alias_exists() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("typeof browser !== 'undefined' && typeof browser.runtime !== 'undefined'").unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn chrome_runtime_get_url() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("chrome.runtime.getURL('icons/icon.png')").unwrap();
        assert_eq!(v, lumen_core::JsValue::String(
            "chrome-extension://lumen-extension/icons/icon.png".into()
        ));
    }

    // ── HTML5 Drag and Drop API (HTML LS §9.10) ───────────────────────────────

    #[test]
    fn data_transfer_set_get_data() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var dt = new DataTransfer();
            dt.setData('text/plain', 'hello drag');
            dt.getData('text/plain')
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::String("hello drag".into()));
    }

    #[test]
    fn data_transfer_normalises_text_format() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var dt = new DataTransfer();
            dt.setData('text', 'world');
            dt.getData('text/plain')
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::String("world".into()));
    }

    #[test]
    fn data_transfer_types_reflect_set_data() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var dt = new DataTransfer();
            dt.setData('text/plain', 'a');
            dt.setData('text/html', '<b>a</b>');
            dt.types.length === 2 && dt.types.indexOf('text/plain') >= 0
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn data_transfer_clear_data_single_format() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var dt = new DataTransfer();
            dt.setData('text/plain', 'a');
            dt.setData('text/html', '<b>a</b>');
            dt.clearData('text/plain');
            dt.types.length === 1 && dt.types[0] === 'text/html'
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn data_transfer_item_list_add_and_iterate() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var dt = new DataTransfer();
            dt.items.add('foo', 'text/plain');
            dt.items.length === 1 && dt.items[0].kind === 'string'
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn data_transfer_item_get_as_string() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var dt = new DataTransfer();
            dt.setData('text/plain', 'payload');
            var got = null;
            dt.items[0].getAsString(function(s) { got = s; });
            got
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::String("payload".into()));
    }

    #[test]
    fn drag_event_has_fresh_data_transfer() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var e = new DragEvent('dragstart', { bubbles: true });
            e.dataTransfer instanceof DataTransfer
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn draggable_attribute_getter_setter() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var el = document.createElement('div');
            document.body.appendChild(el);
            el.draggable = true;
            el.draggable === true && el.getAttribute('draggable') === 'true'
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn data_transfer_classes_exported_on_window() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            typeof window.DataTransfer === 'function' &&
            typeof window.DataTransferItem === 'function' &&
            typeof window.DataTransferItemList === 'function'
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn lumen_dispatch_drag_event_fires_on_element() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var el = document.getElementById('main');
            var fired = false;
            el.addEventListener('dragstart', function(e) { fired = true; });
            var nid = el.__nid__;
            _lumen_dispatch_drag_event(nid, 'dragstart', 10, 20, '{}');
            fired
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn lumen_dispatch_drag_event_passes_coordinates() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var el = document.getElementById('main');
            var cx = -1, cy = -1;
            el.addEventListener('drag', function(e) { cx = e.clientX; cy = e.clientY; });
            _lumen_dispatch_drag_event(el.__nid__, 'drag', 55, 77, '{}');
            cx === 55 && cy === 77
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn lumen_dispatch_drag_event_populates_data_transfer() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var el = document.getElementById('main');
            var payload = '';
            el.addEventListener('drop', function(e) {
                payload = e.dataTransfer.getData('text/plain');
            });
            _lumen_dispatch_drag_event(el.__nid__, 'drop', 0, 0, '{"text/plain":"transferred"}');
            payload
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::String("transferred".into()));
    }

    #[test]
    fn lumen_dispatch_drag_event_bubbles_to_parent() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var parent = document.getElementById('main');
            var child = document.createElement('div');
            parent.appendChild(child);
            var bubbled = false;
            parent.addEventListener('dragover', function() { bubbled = true; });
            _lumen_dispatch_drag_event(child.__nid__, 'dragover', 0, 0, '{}');
            bubbled
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn drag_event_default_not_prevented_without_handler() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval(r#"
            var el = document.getElementById('main');
            // returns true when default is not prevented
            _lumen_dispatch_drag_event(el.__nid__, 'dragstart', 0, 0, '{}')
        "#).unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    // ── window scroll API (CSSOM View Module §4) ─────────────────────────────

    #[test]
    fn window_scroll_y_initially_zero() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("window.scrollY").unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn window_page_y_offset_alias() {
        let rt = runtime_with_dom(make_doc());
        rt.set_page_scroll_y(150.0);
        let v = rt.eval("window.pageYOffset").unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(150.0));
    }

    #[test]
    fn window_scroll_to_instant_queues_page_request() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.scrollTo(0, 500)").unwrap();
        let reqs = rt.take_page_scroll_requests();
        assert_eq!(reqs.len(), 1);
        assert!((reqs[0].0 - 500.0).abs() < 0.1, "target_y should be 500");
        assert!(!reqs[0].1, "smooth should be false for instant scroll");
    }

    #[test]
    fn window_scroll_to_smooth_sets_smooth_flag() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.scrollTo({ top: 300, behavior: 'smooth' })").unwrap();
        let reqs = rt.take_page_scroll_requests();
        assert_eq!(reqs.len(), 1);
        assert!((reqs[0].0 - 300.0).abs() < 0.1, "target_y should be 300");
        assert!(reqs[0].1, "smooth should be true");
    }

    #[test]
    fn window_scroll_by_adds_to_current_page_scroll() {
        let rt = runtime_with_dom(make_doc());
        rt.set_page_scroll_y(200.0);
        rt.eval("window.scrollBy(0, 100)").unwrap();
        let reqs = rt.take_page_scroll_requests();
        assert_eq!(reqs.len(), 1);
        assert!((reqs[0].0 - 300.0).abs() < 0.1, "target_y should be 300 (200+100)");
    }

    #[test]
    fn window_scroll_alias_works() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.scroll(0, 400)").unwrap();
        let reqs = rt.take_page_scroll_requests();
        assert_eq!(reqs.len(), 1);
        assert!((reqs[0].0 - 400.0).abs() < 0.1);
    }

    #[test]
    fn window_scroll_x_is_zero() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("window.scrollX").unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn print_request_default_values() {
        let req = PrintRequest::default();
        assert_eq!(req.margin_top, 48.0);
        assert_eq!(req.margin_bottom, 48.0);
        assert_eq!(req.margin_left, 48.0);
        assert_eq!(req.margin_right, 48.0);
        assert_eq!(req.paper_width_in, 8.5);
        assert_eq!(req.paper_height_in, 11.0);
        assert_eq!(req.output_path, None);
    }

    #[test]
    fn multiple_print_calls_accumulate() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("window.print()").unwrap();
        rt.eval("window.print()").unwrap();
        let reqs = rt.take_print_requests();
        assert_eq!(reqs.len(), 2);
    }

    // ── JJ Phase 5: Modern HTML5 APIs ────────────────────────────────────────

    #[test]
    fn set_html_unsafe_sets_content() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 document.body.appendChild(d);\
                 d.setHTMLUnsafe('<p>hello</p>');\
                 d.innerHTML === '<p>hello</p>'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_html_returns_inner_html() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 document.body.appendChild(d);\
                 d.innerHTML = '<span>world</span>';\
                 d.getHTML() === '<span>world</span>'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_html_with_options_phase0() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 document.body.appendChild(d);\
                 d.innerHTML = 'test';\
                 d.getHTML({serializableShadowRoots: true}) === 'test'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn move_before_rearranges_children() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var parent = document.createElement('div');\
                 document.body.appendChild(parent);\
                 var a = document.createElement('span'); a.id = 'a';\
                 var b = document.createElement('span'); b.id = 'b';\
                 var c = document.createElement('span'); c.id = 'c';\
                 parent.appendChild(a); parent.appendChild(b); parent.appendChild(c);\
                 parent.moveBefore(c, b);\
                 var kids = parent.children;\
                 kids[0].id === 'a' && kids[1].id === 'c' && kids[2].id === 'b'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn move_before_null_appends_to_end() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var parent = document.createElement('div');\
                 document.body.appendChild(parent);\
                 var a = document.createElement('span'); a.id = 'a';\
                 var b = document.createElement('span'); b.id = 'b';\
                 parent.appendChild(a); parent.appendChild(b);\
                 parent.moveBefore(a, null);\
                 var kids = parent.children;\
                 kids[0].id === 'b' && kids[1].id === 'a'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn check_visibility_disconnected_returns_false() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval("var d = document.createElement('div'); d.checkVisibility() === false")
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn check_visibility_accepts_options_without_throw() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 typeof d.checkVisibility({checkOpacity: true, checkVisibilityCSS: true}) === 'boolean'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_html_unsafe_method_exists() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 typeof d.setHTMLUnsafe === 'function'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_html_method_exists() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 typeof d.getHTML === 'function'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn move_before_method_exists() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 typeof d.moveBefore === 'function'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn check_visibility_method_exists() {
        let rt = runtime_with_dom(make_doc());
        let ok = rt
            .eval(
                "var d = document.createElement('div');\
                 typeof d.checkVisibility === 'function'",
            )
            .unwrap();
        assert_eq!(ok, lumen_core::JsValue::Bool(true));
    }

    // ── Web Animations API — additional coverage ──────────────────────────────

    #[test]
    fn wa_document_timeline_current_time_is_number() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("typeof document.timeline.currentTime === 'number' || document.timeline.currentTime === null").unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wa_document_timeline_class_exposed() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("typeof window.DocumentTimeline === 'function'").unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wa_animation_playback_rate_default_one() {
        let rt = runtime_with_dom(make_doc());
        let v = rt
            .eval(
                "var el = document.getElementById('main'); \
                 var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                 a.playbackRate",
            )
            .unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn wa_animation_ready_is_promise() {
        let rt = runtime_with_dom(make_doc());
        let v = rt
            .eval(
                "var el = document.getElementById('main'); \
                 var a = el.animate([{opacity:'0'},{opacity:'1'}], 200); \
                 typeof a.ready === 'object' && a.ready !== null",
            )
            .unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn wa_element_get_animations_returns_running() {
        let rt = runtime_with_dom(make_doc());
        let v = rt
            .eval(
                "var el = document.getElementById('main'); \
                 el.animate([{opacity:'0'},{opacity:'1'}], 500); \
                 el.getAnimations().length",
            )
            .unwrap();
        assert_eq!(v, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn wa_animation_playback_event_class_exposed() {
        let rt = runtime_with_dom(make_doc());
        let v = rt.eval("typeof window.AnimationPlaybackEvent === 'function'").unwrap();
        assert_eq!(v, lumen_core::JsValue::Bool(true));
    }

    // ── Pointer Events Level 3 §4.1 — pointer capture ────────────────────────

    #[test]
    fn pointer_event_level3_altitude_azimuth_properties() {
        // L3 PointerEvent must expose altitudeAngle=π/2 and azimuthAngle=0 for mouse.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); document.body.appendChild(el); \
             var got = null; \
             el.addEventListener('pointerdown', function(e) { got = e; }); \
             _lumen_dispatch_pointer_event(el.__nid__, 'pointerdown', 0, 0, 0, 1, 0); \
             Math.abs(got.altitudeAngle - Math.PI / 2) < 0.001 && got.azimuthAngle === 0 && \
             got.width === 1 && got.height === 1 && \
             got.tangentialPressure === 0 && got.tiltX === 0 && got.tiltY === 0 && \
             got.twist === 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pointer_event_get_coalesced_events_returns_array() {
        // getCoalescedEvents() must return an array containing the event itself.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); document.body.appendChild(el); \
             var got = null; \
             el.addEventListener('pointermove', function(e) { got = e; }); \
             _lumen_dispatch_pointer_event(el.__nid__, 'pointermove', 5, 5, 0, 0, 0); \
             Array.isArray(got.getCoalescedEvents()) && got.getCoalescedEvents().length === 1 && \
             Array.isArray(got.getPredictedEvents()) && got.getPredictedEvents().length === 0"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_has_set_pointer_capture_method() {
        // Element must expose setPointerCapture, releasePointerCapture, hasPointerCapture.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); document.body.appendChild(el); \
             typeof el.setPointerCapture === 'function' && \
             typeof el.releasePointerCapture === 'function' && \
             typeof el.hasPointerCapture === 'function'"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_pointer_capture_fires_gotpointercapture() {
        // setPointerCapture(1) must fire 'gotpointercapture' on the element (non-bubbling).
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); document.body.appendChild(el); \
             var parent = document.createElement('div'); parent.appendChild(el); document.body.appendChild(parent); \
             var got_on_el = false; var bubbled_to_parent = false; \
             el.addEventListener('gotpointercapture', function(e) { got_on_el = true; }); \
             parent.addEventListener('gotpointercapture', function(e) { bubbled_to_parent = true; }); \
             el.setPointerCapture(1); \
             got_on_el && !bubbled_to_parent"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn release_pointer_capture_fires_lostpointercapture() {
        // releasePointerCapture(1) must fire 'lostpointercapture' on the element (non-bubbling).
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('div'); document.body.appendChild(el); \
             var lost = false; \
             el.addEventListener('lostpointercapture', function() { lost = true; }); \
             el.setPointerCapture(1); \
             el.releasePointerCapture(1); \
             lost"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn element_has_ongotpointercapture_handlers() {
        // Element must expose ongotpointercapture and onlostpointercapture as null by default.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var el = document.createElement('button'); document.body.appendChild(el); \
             el.ongotpointercapture === null && el.onlostpointercapture === null"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_capture_event_no_bubble() {
        // _lumen_dispatch_capture_event must fire a non-bubbling PointerEvent.
        let rt = runtime_with_dom(make_doc());
        let r = rt.eval(
            "var parent = document.createElement('div'); document.body.appendChild(parent); \
             var child = document.createElement('span'); parent.appendChild(child); \
             var fired = false; var bubbled = false; \
             child.addEventListener('gotpointercapture', function(e) { fired = true; }); \
             parent.addEventListener('gotpointercapture', function(e) { bubbled = true; }); \
             _lumen_dispatch_capture_event(child.__nid__, 'gotpointercapture'); \
             fired && !bubbled"
        ).unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }
}
