//! JS↔DOM bridge for lumen-js.
//!
//! Hosts shared DOM/JS data types, helper functions used by the V8 native
//! install path (`v8_runtime.rs::install_dom`), and the engine-agnostic
//! `WEB_API_SHIM` JavaScript that builds standard `document`, `window`,
//! `console` globals on top of the `_lumen_*` natives that path registers.
// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

// Only exercised by the test suite below (`mod tests`), which itself only
// glob-imports this module's scope under `v8-backend` — the production
// `_lumen_*` natives live in the V8 install path (`v8_runtime.rs`), which
// carries its own copies of these imports.
#[cfg(all(test, feature = "v8-backend"))]
use std::sync::{Arc, Mutex};

#[cfg(all(test, feature = "v8-backend"))]
use lumen_core::ext::IdbBackend;
#[cfg(all(test, feature = "v8-backend"))]
use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

// ─── navigation request ───────────────────────────────────────────────────────

/// Navigation request emitted by JS (`location.href =`, `location.assign()`,
/// `location.replace()`, `location.reload()`).  Captured in `nav_out` during
/// script execution and read by the shell after `v8_runtime.rs::install_dom` returns.
#[derive(Debug, Clone)]
pub enum NavigateRequest {
    /// Navigate to URL and push a new entry onto the history stack.
    Push(String),
    /// Navigate to URL and replace the current history entry.
    Replace(String),
    /// Reload the current page.
    Reload,
    /// Run the form-submission algorithm for a `<form>` the page submitted from
    /// script (`form.submit()` / `form.requestSubmit()`, BUG-383).
    ///
    /// Encoding, enctype and the navigation itself live in the shell, so the JS
    /// side only names the nodes: `form` is the `<form>`'s node index and
    /// `submitter` the activated control's, or `-1` when there is none.
    /// Travels on the same single-slot channel as the other navigations because
    /// a form submission *is* one — a second request in the same JS pump
    /// supersedes the first, exactly as two `location.href =` do.
    SubmitForm {
        /// Node index of the `<form>` element.
        form: u32,
        /// Node index of the submitter control, or `-1` for none.
        submitter: i32,
    },
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

// ─── Navigation API action tag ────────────────────────────────────────────────

/// Discriminant embedded in `pending_navigation_updates` to tell the shell
/// which Navigation API method produced this request.
///
/// The shell matches on the integer discriminant and reads the remaining fields
/// as `(url, key, data)`.  Only `url` is populated for `Push`/`Replace`; only
/// `key` for `TraverseTo`; only `data` for `TraverseBy` (data = `delta` string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NavAction {
    Push       = 0,
    Replace    = 1,
    Back       = 2,
    Forward    = 3,
    TraverseTo = 4,
    Reload     = 5,
    InterceptedSuccess = 6,
    InterceptedError   = 7,
}

// ─── Navigation API update record ─────────────────────────────────────────────

/// Tuple stored in `pending_navigation_updates`:
/// `(action, url, key, data)`.
pub type NavUpdate = (NavAction, String, String, String);

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

// ─── primitive registrations ──────────────────────────────────────────────────

/// Extract `"method"` field from a cache meta JSON string.
///
/// Fast path without serde — scans for `"method":"<VALUE>"` literally.
/// Falls back to `"GET"` on any parse failure.
///
/// Only exercised by `MockCacheBackend` in the test suite — the V8 install
/// path (`v8_runtime.rs`) has its own copy for the production `_lumen_*` natives.
#[cfg(all(test, feature = "v8-backend"))]
fn cache_meta_method(meta_json: &str) -> String {
    if let Some(start) = meta_json.find("\"method\":\"") {
        let rest = &meta_json[start + 10..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    "GET".to_string()
}

// ─── DOM helpers ──────────────────────────────────────────────────────────────
//
// Test-only below this point: the production `_lumen_*` natives (V8 install
// path, `v8_runtime.rs`) carry their own copies of these helpers; the ones
// here now back only the `dom.rs`-local test suite's document fixtures.

#[cfg(all(test, feature = "v8-backend"))]
fn find_element_by_tag(doc: &Document, tag: &str) -> Option<NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

#[cfg(all(test, feature = "v8-backend"))]
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

// Mirrors `v8_runtime::collect_text_content` — see there for the CharacterData
// rationale (a Comment's own data isn't recursed into via `collect_text_inner`,
// which intentionally only concatenates Text descendants for element
// `.textContent`).
#[cfg(all(test, feature = "v8-backend"))]
fn collect_text_content(doc: &Document, id: NodeId) -> String {
    if let NodeData::Comment(s) = &doc.get(id).data {
        return s.clone();
    }
    let mut out = String::new();
    collect_text_inner(doc, id, &mut out);
    out
}

#[cfg(all(test, feature = "v8-backend"))]
fn collect_text_inner(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children.clone() {
        collect_text_inner(doc, child, out);
    }
}

#[cfg(all(test, feature = "v8-backend"))]
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

// ─── JavaScript Web API shim ──────────────────────────────────────────────────

/// First of the five parts of the page shim — see [`web_api_shim`], which
/// concatenates them back into the single program that is evaluated once after
/// the `_lumen_*` natives are registered (V8 install path,
/// `v8_runtime.rs::install_dom`) and builds the standard Web API globals.
/// Everything below in this doc comment applies to all five parts.
///
/// Uses top-level `var` so declarations land on the global object under plain
/// script eval. No IIFE — avoids strict-mode `this`-is-undefined edge cases.
///
/// A missing/`None` value from a native comes back as `undefined`, not `null`
/// — all places where the Web API spec requires `null` use `_lumen_u2n`
/// (undefined-to-null).
///
/// `parentElement` and `children` are defined as non-enumerable via
/// `Object.defineProperty` to avoid parent↔child infinite recursion when the
/// V8 compat layer serializes the returned object.
///
/// The text itself lives in `src/shim/*.js`, one file per const, pulled in by
/// `include_str!` (SPLIT-JS3, 2026-08-28). Those files are read verbatim, so
/// **nothing in them is escaped**: a `"` is a `"` and a `\` is a `\`. Until
/// that batch these were plain `"..."` Rust strings — every quote written as
/// `\"`, a stray one closing the literal early and producing a wall of
/// unrelated-looking "character literal"/"unknown prefix" errors anchored deep
/// inside the JS (BUG-360 postmortem, 2026-08-09). That trap is gone; the one
/// that replaces it is that an editor's "insert escape" habit now *corrupts*
/// the JS instead of saving it.
///
/// The split into files changes nothing about the program: [`web_api_shim`]
/// concatenates the consts in source order, so V8 still compiles one script
/// with one hoisting scope. Do not subdivide a `.js` file further — one file
/// per const is what keeps that correspondence checkable by eye.
#[cfg(feature = "v8-backend")]
const WEB_API_SHIM_HEAD: &str = include_str!("shim/web_api_shim_head.js");

/// `EventTarget` — the first of the two shim blocks shared verbatim between the
/// page global scope and every `WorkerGlobalScope` (BUG-401).
///
/// WHATWG DOM declares `EventTarget` `[Exposed=*]`, so a worker needs the very
/// same class the page has — and [`PERFORMANCE_SHIM`] below cannot build a real
/// `Performance : EventTarget` prototype chain without it. Kept as its own
/// const purely so `worker.rs` can evaluate it; [`web_api_shim`] splices it back
/// into the page shim at its original position, so nothing about the page
/// program changes.
#[cfg(feature = "v8-backend")]
pub(crate) const EVENT_TARGET_SHIM: &str = include_str!("shim/event_target_shim.js");

/// Page shim source between [`EVENT_TARGET_SHIM`] and [`PERFORMANCE_SHIM`].
/// Not shared with workers — see [`web_api_shim`].
#[cfg(feature = "v8-backend")]
const WEB_API_SHIM_MID: &str = include_str!("shim/web_api_shim_mid.js");

/// `_lumen_parse_url` — разбор URL, общий для страницы и воркера.
///
/// Вынесен из [`WEB_API_SHIM_MID`] отдельным куском, потому что `URL`/
/// `URLSearchParams` (`[Exposed=(Window,Worker)]`) опираются на него, а в
/// воркере остального шима страницы нет. Кусок — дословный срез, не копия:
/// склейка в [`web_api_shim`] обязана давать прежний текст.
pub(crate) const URL_PARSE_SHIM: &str = include_str!("shim/url_parse_shim.js");

const WEB_API_SHIM_MID_B: &str = include_str!("shim/web_api_shim_mid_b.js");

/// `URLSearchParams` + `URL` (WHATWG URL §5/§6.1) — `[Exposed=(Window,Worker)]`.
///
/// Второй кусок, общий с воркером: сервис-воркеры разбирают запросы именно
/// этими классами (живой пример — `sw.js` t-банка падал на `URLSearchParams
/// is not defined`, и вместе с ним не вставал весь SW).
pub(crate) const URL_SHIM: &str = include_str!("shim/url_shim.js");

const WEB_API_SHIM_MID_C: &str = include_str!("shim/web_api_shim_mid_c.js");

/// `Performance` — the second shim block shared between the page global scope
/// and every `WorkerGlobalScope` (BUG-401).
///
/// HR Time L3 marks the interface `[Exposed=(Window,Worker)]`, so a worker must
/// get the identical object, not a second hand-written one that drifts from
/// this one the next time the page copy is fixed (BUG-400 had just rebuilt it
/// as a real `EventTarget` subclass). Depends on [`EVENT_TARGET_SHIM`] and on
/// the `_lumen_now_ms` native; `_perf_observer_notify` lives further down the
/// page shim and is therefore called through a `typeof` guard, which is a
/// no-op for the page (same script, hoisted) and the reason a worker without
/// `PerformanceObserver` can still call `mark()`/`measure()`.
#[cfg(feature = "v8-backend")]
pub(crate) const PERFORMANCE_SHIM: &str = include_str!("shim/performance_shim.js");

/// Page shim source after [`PERFORMANCE_SHIM`]. Not shared with workers — see
/// [`web_api_shim`].
#[cfg(feature = "v8-backend")]
const WEB_API_SHIM_TAIL: &str = include_str!("shim/web_api_shim_tail.js");

/// `MessageChannel`/`MessagePort` — ещё одна часть шима с
/// `[Exposed=(Window,Worker)]`, вырезанная 2026-08-17 ради области
/// сервис-воркера: `sw.js` t-банка строит канал на верхнем уровне, и без
/// класса воркер не доживал до регистрации обработчиков.
///
/// Блок ни на что страничное не опирается — только на `setTimeout`, который
/// есть в обеих областях (см. его собственный комментарий про BUG-702: доставка
/// обязана быть задачей, а не микрозадачей).
#[cfg(feature = "v8-backend")]
pub(crate) const MESSAGE_CHANNEL_SHIM: &str = include_str!("shim/message_channel_shim.js");

/// Продолжение хвоста шима после [`MESSAGE_CHANNEL_SHIM`]. Существует только
/// ради этого разреза; порядок склейки закреплён тестом
/// `web_api_shim_splices_its_parts_in_source_order`.
const WEB_API_SHIM_TAIL_MC: &str = include_str!("shim/web_api_shim_tail_mc.js");

/// IndexedDB (W3C Indexed Database API 3.0) — вырезан из хвоста страничного
/// шима отдельной частью по той же причине, что `EVENT_TARGET_SHIM` и
/// `URL_SHIM`: интерфейс `[Exposed=(Window,Worker)]`, и области сервис-воркера
/// он нужен ровно тот же, а не второй копией.
///
/// Внутри блок опирается только на `_lumen_console_error` и на нативы
/// `_lumen_idb_*` (каждый под охраной `typeof … === 'function'`, поэтому без
/// них база живёт в куче и просто не переживает перезагрузку). Публикация
/// классов идёт через `globalThis`, а не `window`: в области воркера окна нет,
/// а на странице это тот же объект (`window` — настоящий глобал с BUG-280).
#[cfg(feature = "v8-backend")]
pub(crate) const IDB_SHIM: &str = include_str!("shim/idb_shim.js");

/// Хвост страничного шима после [`IDB_SHIM`] — от `window.getSelection` до
/// конца. Отдельная часть существует только ради разреза IndexedDB; порядок
/// склейки закреплён тестом `web_api_shim_splices_its_parts_in_source_order`.
const WEB_API_SHIM_TAIL_B: &str = include_str!("shim/web_api_shim_tail_b.js");

/// `WorkerLocation` (HTML LS §10.2.4) and `WorkerNavigator` (§10.2.5) — the two
/// `[Exposed=Worker]` interfaces every `WorkerGlobalScope` has and the page does
/// not, so unlike its neighbours this piece is spliced **only** into
/// [`worker_exposed_shim`] and is not a slice of the page program (BUG-776:
/// before it, `location`/`navigator` were plain `ReferenceError`s in a dedicated
/// and a shared worker, and `navigator` in a service worker too).
///
/// Only the `navigator` singleton is created here, over the `_lumen_navigator_id`
/// values built in Rust by
/// [`crate::navigator_bindings::worker_navigator_id_shim`] and evaluated just
/// before this piece. `location` is not: its URL is the worker's own script URL,
/// which each flavour learns differently (dedicated/shared from the Rust-set
/// `_lumen_worker_location_url` global, service from its scope), so each calls
/// `_lumen_make_worker_location(url)` itself.
///
/// The location members are own, non-configurable accessors *without* a setter,
/// which is what `[LegacyUnforgeable] readonly attribute` means in a
/// non-strict script: `location.href = 1` must silently do nothing rather than
/// throw (`interfaces/WorkerGlobalScope/location/setting-members.html` asserts
/// exactly zero exceptions from eight such assignments). The navigator members
/// are ordinary prototype accessors — those are not unforgeable.
pub(crate) const WORKER_LOCATION_NAVIGATOR_SHIM: &str = include_str!("shim/worker_location_navigator_shim.js");

/// The page-scope Web API shim, re-assembled from its five parts in source
/// order: head, [`EVENT_TARGET_SHIM`], mid, [`PERFORMANCE_SHIM`], tail.
///
/// The split exists only so `worker.rs` can evaluate the two `[Exposed=*]` /
/// `[Exposed=(Window,Worker)]` blocks in a `WorkerGlobalScope` without a second
/// copy of them (BUG-401). Because the pieces are concatenated in their
/// original order, V8 still compiles one program with one hoisting scope — the
/// split is invisible to the shim's own code.
#[cfg(feature = "v8-backend")]
pub(crate) fn web_api_shim() -> String {
    format!("{WEB_API_SHIM_HEAD}{EVENT_TARGET_SHIM}{WEB_API_SHIM_MID}{URL_PARSE_SHIM}{WEB_API_SHIM_MID_B}{URL_SHIM}{WEB_API_SHIM_MID_C}{PERFORMANCE_SHIM}{WEB_API_SHIM_TAIL}{MESSAGE_CHANNEL_SHIM}{WEB_API_SHIM_TAIL_MC}{IDB_SHIM}{WEB_API_SHIM_TAIL_B}")
}

/// The subset of the page shim that WHATWG also exposes in a
/// `WorkerGlobalScope`: [`EVENT_TARGET_SHIM`] followed by [`PERFORMANCE_SHIM`].
///
/// Evaluated as one script (like in the page) so `Performance`'s prototype
/// chain finds `EventTarget`. The trailing `undefined` keeps the completion
/// value convertible for [`crate::v8_runtime::V8JsRuntime::eval`], whose return
/// path has no representation for the function object the last statement of
/// [`EVENT_TARGET_SHIM`] would otherwise yield.
///
/// [`WORKER_LOCATION_NAVIGATOR_SHIM`] is the one part that is *not* a slice of
/// the page program — the page has no `WorkerLocation`/`WorkerNavigator`. It
/// comes last because it reads `URL_PARSE_SHIM`'s parser and the
/// `_lumen_navigator_id` object its caller evaluates first.
#[cfg(feature = "v8-backend")]
pub(crate) fn worker_exposed_shim() -> String {
    format!(
        "{EVENT_TARGET_SHIM}{PERFORMANCE_SHIM}{URL_PARSE_SHIM}{URL_SHIM}\
         {WORKER_LOCATION_NAVIGATOR_SHIM}\nundefined;\n"
    )
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
