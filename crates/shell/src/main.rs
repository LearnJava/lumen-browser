//! Lumen shell — точка входа браузера.
//!
//! Режимы запуска:
//! - `lumen` — открыть пустое окно.
//! - `lumen <path.html>` — распарсить файл, layout, paint, нарисовать в окне.
//! - `lumen <http(s)://...>` — загрузить страницу по сети, layout, paint.
//! - `lumen --dump-source <path-or-url>` — печать декодированного HTML в stdout.
//! - `lumen --dump-layout <path-or-url>` — печать layout-дерева в stdout.
//! - `lumen --dump-display-list <path-or-url>` — печать display list в stdout.
//! - `lumen --devtools-port <N>` — запустить DevTools WebSocket сервер на порту N.
//!
//! Dump-режимы не создают окна и не инициализируют wgpu — pipeline прогоняется
//! до нужной фазы, результат сериализуется и пишется в stdout. Полезно для CI
//! (без GPU), отладки сложных страниц и сравнения вывода между версиями.
//!
//! Внешние CSS: `<link rel="stylesheet" href="...">` загружается с диска или
//! по сети — в зависимости от того, каким способом загружена страница.

mod address_bar;
mod animation_scheduler;
mod find;
mod forms;
mod hints;
mod links;
mod momentum_anim;
mod runtime;
mod scroll_anim;
mod scrollbar;

use std::cell::Cell;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use lumen_core::event::{Event, FetchPriority, SubresourceKind};
use lumen_core::ext::EventSink;
use lumen_core::geom::{Point, Rect, Size};
use lumen_devtools::DevToolsServer;
use lumen_knowledge::HistoryFts;
use lumen_storage::session_export::{self, ExportedTab, SessionFile};
use lumen_storage::{BfCache, BfCacheEntry, SearchHistory};
use lumen_dom::{
    Document, NodeData, NodeId, check_form_gate, check_navigation_gate,
    collect_iframes, check_popup_gate,
};
use std::collections::HashMap;
use lumen_layout::{LayoutBox, PaintOrder, StackingTree, TransitionScheduler};
use lumen_layout::style::ComputedStyle;
use lumen_paint::{build_display_list_ordered, build_display_list_ordered_with_anim, hit_test, DisplayList, Renderer};
use lumen_layout::Cursor as CssCursor;
use winit::application::ApplicationHandler;

/// Событие от background-потока загрузки страницы в event loop.
///
/// Загрузка разбита на четыре фазы: (0) `EarlyPreloadHints` — хинты из первых
/// байт HTML для раннего старта subresource fetch-ов; (1) chunks сырых байт для
/// инкрементального парсинга и промежуточных кадров через
/// `IncrementalTreeBuilder::feed_bytes`; (2) `LoadDone` — все байты доступны,
/// запускаем полный pipeline (CSS + изображения); (3) `LoadError` — ошибка fetch.
enum LoadEvent {
    /// Subresource-хинты из первого chunk HTML (HTML LS §13.2.6.4.7
    /// «Speculative HTML parsing»). Отправляются ДО первого `HtmlChunk`,
    /// чтобы sink мог начать загружать CSS/шрифты ещё в процессе парсинга.
    /// Дедупликация с финальными хинтами из `LoadDone` — через
    /// `preload_dispatched` в `Lumen`.
    EarlyPreloadHints(Vec<lumen_html_parser::PreloadHint>, ResourceBase),
    /// Очередной chunk сырых байт HTML. UTF-8 границы не выравниваются —
    /// `IncrementalTreeBuilder::feed_bytes` буферизует незавершённые
    /// code-point-ы внутри.
    HtmlChunk(Vec<u8>),
    /// Все байты получены — для финального полного pipeline.
    LoadDone(RawPage),
    /// Ошибка при загрузке страницы.
    LoadError(String),
}

/// Размер одного HTML-chunk при разбивке для инкрементального парсинга.
const STREAM_CHUNK_BYTES: usize = 8 * 1024;
/// Минимальный интервал между промежуточными кадрами при streaming (мс).
const STREAM_PAINT_INTERVAL_MS: u128 = 150;

/// EventSink, который печатает сетевые события в stdout — это и есть
/// «network log» Phase 0, реализующий принцип №4 «каждый исходящий байт
/// виден». Позже заменится на структурированный UI-логгер.
struct StdoutEventSink;

impl EventSink for StdoutEventSink {
    fn emit(&self, event: &Event) {
        // Сетевой лог идёт в stderr, чтобы stdout dump-режимов оставался чистым
        // (на нём — только сериализованный результат pipeline-а). В оконном
        // режиме разница невидима: оба потока попадают в терминал.
        match event {
            Event::RequestStarted { url, .. } => eprintln!("→ GET {url}"),
            Event::RequestCompleted { url, status, .. } => eprintln!("← {status} {url}"),
            Event::RequestBlocked { url, reason, .. } => eprintln!("✗ {url} ({reason})"),
            Event::SubresourceHintFound { url, kind, priority } => {
                let label = match kind {
                    SubresourceKind::Stylesheet => "css",
                    SubresourceKind::Script => "js",
                    SubresourceKind::Image => "img",
                    SubresourceKind::Font => "font",
                    SubresourceKind::Preconnect { dns_only: true } => "dns-prefetch",
                    SubresourceKind::Preconnect { dns_only: false } => "preconnect",
                    SubresourceKind::Other { .. } => "preload",
                };
                let prio = match priority {
                    FetchPriority::High => "high",
                    FetchPriority::Medium => "medium",
                    FetchPriority::Low => "low",
                };
                eprintln!("⤷ preload {label} [{prio}] {url}");
            }
            Event::FormSubmit { method, action, body, .. } => {
                if body.is_empty() {
                    eprintln!("⊢ form {method} {action}");
                } else {
                    eprintln!("⊢ form {method} {action} body={body}");
                }
            }
            _ => {}
        }
    }
}

/// Bundled-шрифт: статический Inter v4.1 Regular (~411 КБ),
/// SIL OFL 1.1, см. assets/fonts/OFL.txt.
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{CursorIcon, Window, WindowId};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (devtools_port, rest_args) = match extract_devtools_port(&args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (import_session, rest_args) = match extract_import_session(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка --import-session: {err}");
            return ExitCode::FAILURE;
        }
    };
    let (no_scrollbar, rest_args) = extract_no_scrollbar(&rest_args);
    let cli = match parse_cli(&rest_args) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    if let Some(port) = devtools_port
        && let Err(e) = DevToolsServer::spawn(port)
    {
        eprintln!("Ошибка запуска DevTools на порту {port}: {e}");
        return ExitCode::FAILURE;
    }

    let event_sink: Arc<dyn EventSink> = Arc::new(StdoutEventSink);

    // --import-session переопределяет источник страницы и начальный scroll.
    let (cli, initial_scroll) = match import_session {
        Some((session_source, scroll)) => (CliMode::OpenWindow(session_source), scroll),
        None => (cli, (0.0_f32, 0.0_f32)),
    };

    match cli {
        CliMode::Dump { source, kind } => run_dump_mode(&source, kind, event_sink),
        CliMode::OpenWindow(source) => run_window_mode(source, event_sink, initial_scroll, no_scrollbar),
    }
}

fn run_window_mode(
    source: PageSource,
    event_sink: Arc<dyn EventSink>,
    initial_scroll: (f32, f32),
    no_scrollbar: bool,
) -> ExitCode {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    // Streaming pipeline: окно создаётся немедленно, загрузка стартует
    // после `resumed` в background-потоке. До прихода данных рисуем пустую страницу.
    let event_loop = match EventLoop::<LoadEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(err) => {
            eprintln!("Не удалось создать event loop: {err}");
            return ExitCode::FAILURE;
        }
    };
    let load_proxy = event_loop.create_proxy();
    let mut app = Lumen {
        display_list: Vec::new(),
        title: None,
        pending_images: Vec::new(),
        source,
        event_sink,
        modifiers: ModifiersState::empty(),
        window: None,
        renderer: None,
        runtime: runtime::EventLoop::new(),
        animation_scheduler: animation_scheduler::AnimationScheduler::new(),
        transition_scheduler: TransitionScheduler::new(),
        prev_styles: HashMap::new(),
        anim_frame: None,
        layout_box: None,
        epoch: std::time::Instant::now(),
        find: find::FindState::default(),
        address_bar: address_bar::AddressBarState::default(),
        hint: hints::HintState::default(),
        scroll_y: initial_scroll.1,
        scroll_x: initial_scroll.0,
        content_height: 0.0,
        content_width: 0.0,
        cursor_position: None,
        scroll_drag: None,
        scroll_anim: None,
        momentum_anim: None,
        touchpad_vel: (0.0, 0.0),
        touchpad_vel_time_ms: 0.0,
        last_cursor_icon: None,
        layout_source: None,
        pending_reload: Rc::new(Cell::new(false)),
        pending_js_navigate: None,
        load_proxy,
        stream_builder: None,
        stream_last_paint: std::time::Instant::now(),
        preload_dispatched: std::collections::HashSet::new(),
        ime_composing: None,
        bfcache: BfCache::new(16),
        nav_back: Vec::new(),
        nav_fwd: Vec::new(),
        form_state: HashMap::new(),
        validation_tooltip: None,
        color_picker_node: None,
        ls_storage: HashMap::new(),
        js_ctx: None,
        no_scrollbar,
        first_paint_delivered: false,
        first_contentful_paint_delivered: false,
        history_fts: HistoryFts::open_in_memory()
            .unwrap_or_else(|_| HistoryFts::open_in_memory().expect("history_fts init")),
        search_history: SearchHistory::open_in_memory()
            .unwrap_or_else(|_| SearchHistory::open_in_memory().expect("search_history init")),
    };
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("Ошибка event loop: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_dump_mode(source: &PageSource, kind: DumpKind, event_sink: Arc<dyn EventSink>) -> ExitCode {
    match run_dump(source, kind, event_sink) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Ошибка dump {}: {err}", source.describe());
            ExitCode::FAILURE
        }
    }
}

fn run_dump(
    source: &PageSource,
    kind: DumpKind,
    event_sink: Arc<dyn EventSink>,
) -> Result<(), Box<dyn Error>> {
    let raw = source.load_bytes(event_sink.clone())?;
    match kind {
        DumpKind::Source => {
            let encoding = lumen_encoding::detect(&raw.bytes, raw.content_type);
            let decoded = lumen_encoding::decode(encoding, &raw.bytes);
            eprintln!("Кодировка: {}", encoding.name());
            print!("{decoded}");
            Ok(())
        }
        DumpKind::Layout => {
            let vp = Size::new(1024.0, 720.0);
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp, &mut std::collections::HashSet::new(), None)?;
            print!("{}", lumen_layout::serialize_layout_tree(&parsed.layout));
            Ok(())
        }
        DumpKind::DisplayList => {
            let vp = Size::new(1024.0, 720.0);
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp, &mut std::collections::HashSet::new(), None)?;
            let dl = paint_ordered(&parsed.layout);
            print!("{}", lumen_paint::serialize_display_list(&dl));
            Ok(())
        }
    }
}

fn print_usage() {
    eprintln!("Использование:");
    eprintln!("  lumen                                    — пустое окно");
    eprintln!("  lumen <path-or-url>                      — открыть страницу в окне");
    eprintln!("  lumen --dump-source <path-or-url>        — декодированный HTML в stdout");
    eprintln!("  lumen --dump-layout <path-or-url>        — layout-дерево в stdout");
    eprintln!("  lumen --dump-display-list <path-or-url>  — display list в stdout");
    eprintln!("  [--devtools-port <N>]                    — DevTools WS сервер (любой режим)");
    eprintln!("  --import-session <file.lsession>         — восстановить сессию из файла");
}

/// Результат разбора `--import-session`: (source, (scroll_x, scroll_y)).
type ImportedSession = (PageSource, (f32, f32));

/// Извлечь `--import-session <file>` из аргументов.
///
/// Возвращает (Some((source, (scroll_x, scroll_y))), остальные аргументы)
/// или (None, аргументы) если флаг не указан.
fn extract_import_session(
    args: &[String],
) -> Result<(Option<ImportedSession>, Vec<String>), String> {
    let mut session: Option<(PageSource, (f32, f32))> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--import-session" {
            i += 1;
            let path = args.get(i).ok_or("--import-session требует путь к файлу")?;
            let json = std::fs::read_to_string(path)
                .map_err(|e| format!("не удалось прочитать {path}: {e}"))?;
            let file = session_export::from_json(&json)
                .map_err(|e| format!("ошибка разбора сессии {path}: {e}"))?;
            let tab = session_export::active_tab(&file)
                .ok_or_else(|| format!("сессия {path} не содержит вкладок"))?;
            let source = PageSource::from_arg(Some(&tab.url));
            session = Some((source, (tab.scroll_x, tab.scroll_y)));
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((session, rest))
}

/// Извлечь `--no-scrollbar` из аргументов, вернуть (flag, остальные аргументы).
fn extract_no_scrollbar(args: &[String]) -> (bool, Vec<String>) {
    let mut found = false;
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--no-scrollbar" {
            found = true;
        } else {
            rest.push(arg.clone());
        }
    }
    (found, rest)
}

/// Извлечь `--devtools-port N` из аргументов, вернуть (port, остальные аргументы).
fn extract_devtools_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--devtools-port" {
            i += 1;
            let s = args.get(i).ok_or("--devtools-port требует номер порта")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("неверный порт: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// Источник страницы. Запоминается в `Lumen`, чтобы reload (F5/Ctrl+R) мог
/// заново выполнить fetch/parse/layout/paint без аргументов командной строки.
#[derive(Debug, Clone)]
enum PageSource {
    /// Без аргументов — рисуем пустое окно. Reload no-op (грузить нечего).
    Empty,
    File(PathBuf),
    Url(String),
    /// Страница восстанавливается из bfcache: HTML уже есть в памяти,
    /// сетевой запрос не нужен. `base_url` — оригинальный URL страницы
    /// (для разрешения относительных ссылок внутри HTML).
    Snapshot { html: String, base_url: String },
}

/// Запись в стеке истории навигации браузера.
struct NavEntry {
    source: PageSource,
    scroll_x: f32,
    scroll_y: f32,
}

/// Навигационный запрос от JS (location.href=, assign, replace, reload).
/// Хранится в `Lumen::pending_js_navigate` и выполняется в `about_to_wait`.
#[cfg_attr(not(feature = "quickjs"), allow(dead_code))]
enum JsNavigateRequest {
    /// Перейти на URL, добавить запись в историю.
    Push(String),
    /// Перейти на URL, заменить текущую запись истории (без push).
    Replace(String),
    /// Перезагрузить текущую страницу.
    Reload,
}

/// Shell-local abstraction over a persistent JS context that survives between
/// renders. The JS DOM closures hold a reference to the same
/// `Arc<Mutex<Document>>` as `LayoutSource::document`, so event-driven DOM
/// mutations are visible to the next relayout without a full page reload.
trait PersistentJs {
    /// Evaluate a JS script (event handler dispatch, rAF tick, etc.).
    fn eval_js(&self, script: &str);
    /// Consume any navigation request placed by JS during the last `eval_js`.
    fn take_navigate_request(&self) -> Option<JsNavigateRequest>;
    /// Drain all expired JS timers (setTimeout/setInterval).
    ///
    /// Called each `about_to_wait`. Timer callbacks run synchronously inside
    /// the JS context and may themselves schedule further timers or navigation.
    fn tick_timers(&self);
    /// Take the next timer wakeup deadline as Unix epoch ms, clearing the stored
    /// value.  Returns `None` if no timers are pending after the last tick.
    fn take_timer_wakeup(&self) -> Option<f64>;
    /// Returns `true` if JS mutated the DOM since the last call, clearing the flag.
    ///
    /// Called after each rAF pass in `RedrawRequested`; when `true`, a relayout
    /// must happen before the next paint to reflect DOM changes.
    fn take_dom_dirty(&self) -> bool;
    /// Run all pending `requestAnimationFrame` callbacks with `timestamp_ms`.
    ///
    /// Called in `RedrawRequested` before paint. Callbacks may register new rAF
    /// callbacks (animation loop); use `take_raf_pending` to detect this.
    fn run_animation_frame(&self, timestamp_ms: f64);
    /// Returns `true` if `requestAnimationFrame` was called since the last
    /// `take_raf_pending`, clearing the flag.
    ///
    /// Shell requests another redraw when this returns `true` so animation loops
    /// continue without busy-polling.
    fn take_raf_pending(&self) -> bool;
    /// Push a fresh snapshot of layout bounding rects into the JS runtime.
    ///
    /// Called after every `relayout_page`. The JS side uses this for
    /// `getBoundingClientRect`, `ResizeObserver`, and `IntersectionObserver`.
    #[allow(dead_code)] // called only from #[cfg(feature = "quickjs")] blocks
    fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>);
    /// Update the current viewport dimensions in the JS runtime.
    ///
    /// Called after every resize and on initial load.
    #[allow(dead_code)] // called only from #[cfg(feature = "quickjs")] blocks
    fn update_viewport_size(&self, width: f32, height: f32);
    /// Call `_lumen_deliver_resize_observers()` and
    /// `_lumen_deliver_intersection_observers()` in JS.
    ///
    /// Must be called after `update_layout_rects` so that observers read fresh
    /// geometry. Called by the shell after every `relayout_page`.
    #[allow(dead_code)] // called only from #[cfg(feature = "quickjs")] blocks
    fn deliver_layout_observers(&self);
    /// Register lazy images for deferred IntersectionObserver-style proximity loading.
    ///
    /// Called once after the initial page load with `(node_id, url)` pairs for every
    /// `<img loading="lazy">` element.  Subsequent proximity checks happen via
    /// `deliver_lazy_images()` after each relayout.
    #[allow(dead_code)]
    fn register_lazy_images(&self, pairs: &[(u32, &str)]);
    /// Check registered lazy images against the current viewport and enqueue load
    /// requests for those within the lazy-load margin (1 viewport ahead of the fold).
    ///
    /// Must be called after `deliver_layout_observers` (fresh rects in JS).
    #[allow(dead_code)]
    fn deliver_lazy_images(&self);
    /// Drain lazy image load requests queued by JS since the last call.
    ///
    /// Returns `(node_id, url)` pairs for images that entered the lazy-load margin.
    #[allow(dead_code)]
    fn take_lazy_image_requests(&self) -> Vec<(u32, String)>;
    /// Deliver a PerformancePaintTiming entry to JS PerformanceObservers.
    ///
    /// `name` is `"first-paint"` or `"first-contentful-paint"`;
    /// `start_ms` is the DOMHighResTimeStamp relative to performance.timeOrigin.
    /// Calls `_lumen_deliver_paint_entry(name, start_ms)` in QuickJS.
    #[allow(dead_code)]
    fn deliver_paint_timing(&self, name: &str, start_ms: f64);
}

#[cfg(feature = "quickjs")]
struct QuickPersistentJs {
    rt: lumen_js::QuickJsRuntime,
}

#[cfg(feature = "quickjs")]
impl PersistentJs for QuickPersistentJs {
    fn eval_js(&self, script: &str) {
        use lumen_core::ext::JsRuntime as _;
        if let Err(e) = self.rt.eval(script)
            && !matches!(e, lumen_core::JsError::NotImplemented)
        {
            eprintln!("JS event error: {e}");
        }
    }
    fn take_navigate_request(&self) -> Option<JsNavigateRequest> {
        self.rt.take_navigate_request().map(|r| match r {
            lumen_js::NavigateRequest::Push(u)    => JsNavigateRequest::Push(u),
            lumen_js::NavigateRequest::Replace(u) => JsNavigateRequest::Replace(u),
            lumen_js::NavigateRequest::Reload     => JsNavigateRequest::Reload,
        })
    }
    fn tick_timers(&self) {
        self.eval_js("_lumen_tick_timers()");
    }
    fn take_timer_wakeup(&self) -> Option<f64> {
        self.rt.take_timer_wakeup()
    }
    fn take_dom_dirty(&self) -> bool {
        self.rt.take_dom_dirty()
    }
    fn run_animation_frame(&self, timestamp_ms: f64) {
        self.eval_js(&format!("_lumen_run_raf_callbacks({timestamp_ms})"));
    }
    fn take_raf_pending(&self) -> bool {
        self.rt.take_raf_pending()
    }
    fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>) {
        self.rt.update_layout_rects(rects);
    }
    fn update_viewport_size(&self, width: f32, height: f32) {
        self.rt.update_viewport_size(width, height);
    }
    fn deliver_layout_observers(&self) {
        self.eval_js("_lumen_deliver_resize_observers();_lumen_deliver_intersection_observers();");
    }
    fn register_lazy_images(&self, pairs: &[(u32, &str)]) {
        if pairs.is_empty() {
            return;
        }
        let args = pairs
            .iter()
            .map(|(nid, url)| format!("[{nid},{}]", js_string_literal(url)))
            .collect::<Vec<_>>()
            .join(",");
        self.eval_js(&format!("_lumen_init_lazy_images([{args}]);"));
    }
    fn deliver_lazy_images(&self) {
        self.eval_js("_lumen_deliver_lazy_images();");
    }
    fn take_lazy_image_requests(&self) -> Vec<(u32, String)> {
        self.rt.take_lazy_image_requests()
    }
    fn deliver_paint_timing(&self, name: &str, start_ms: f64) {
        self.eval_js(&format!(
            "_lumen_deliver_paint_entry({}, {start_ms})",
            js_string_literal(name),
        ));
    }
}

impl PageSource {
    fn from_arg(arg: Option<&str>) -> Self {
        match arg {
            Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
                PageSource::Url(s.to_owned())
            }
            Some(s) => PageSource::File(PathBuf::from(s)),
            None => PageSource::Empty,
        }
    }

    fn describe(&self) -> String {
        match self {
            PageSource::Empty => "(пустая вкладка)".to_owned(),
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url(u) => u.clone(),
            PageSource::Snapshot { base_url, .. } => format!("[bfcache] {base_url}"),
        }
    }

    /// Origin string (scheme+host+port) for localStorage partitioning.
    /// Returns `None` for file: and empty sources (no cross-origin storage needed).
    fn origin_str(&self) -> Option<String> {
        let url_s = match self {
            PageSource::Url(u) => u.as_str(),
            PageSource::Snapshot { base_url, .. } => base_url.as_str(),
            _ => return None,
        };
        lumen_core::url::Url::parse(url_s).ok().map(|u| {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", u.scheme(), u.host(), port)
        })
    }

    /// URL-строка страницы для bfcache-ключа. `None` если нет URL (пустая вкладка, файл).
    fn url_str(&self) -> Option<&str> {
        match self {
            PageSource::Url(u) => Some(u.as_str()),
            PageSource::Snapshot { base_url, .. } => Some(base_url.as_str()),
            _ => None,
        }
    }

    /// Resolve a relative or absolute `href` against this page's base URL/path.
    /// Returns the resolved string (absolute URL or absolute file path string).
    /// Falls back to the raw `href` when the base is `Empty` or resolution fails.
    fn resolve_href(&self, href: &str) -> String {
        let base = match self {
            PageSource::File(p) => ResourceBase::File(p.clone()),
            PageSource::Url(u) => ResourceBase::Url(u.clone()),
            PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
            PageSource::Empty => return href.to_owned(),
        };
        base.resolve_str(href)
    }

    /// Прочитать байты страницы с диска или из сети, плюс вернуть базу для
    /// относительных URL и подсказку о content-type. Используется и обычным
    /// `load`, и dump-режимами.
    fn load_bytes(&self, sink: Arc<dyn EventSink>) -> Result<RawPage, Box<dyn Error>> {
        match self {
            PageSource::Empty => Err("источник пуст — нечего загружать".into()),
            PageSource::File(path) => {
                let bytes = std::fs::read(path)?;
                Ok(RawPage {
                    bytes,
                    base: ResourceBase::File(path.clone()),
                    content_type: None,
                })
            }
            PageSource::Url(url) => {
                use lumen_core::ext::NetworkTransport;
                use lumen_core::url::Url;
                use lumen_network::{BrotliContentDecoder, HttpClient};

                let lumen_url = Url::parse(url)?;
                let client = HttpClient::new()
                    .with_sink(sink)
                    .with_content_decoder(std::sync::Arc::new(BrotliContentDecoder::new()));
                let bytes = client.fetch(&lumen_url)?;
                eprintln!("Получено {} байт", bytes.len());
                Ok(RawPage {
                    bytes,
                    base: ResourceBase::Url(url.clone()),
                    content_type: Some("text/html"),
                })
            }
            PageSource::Snapshot { html, base_url } => {
                // bfcache restoration: HTML already in memory, no network request.
                Ok(RawPage {
                    bytes: html.as_bytes().to_vec(),
                    base: ResourceBase::Url(base_url.clone()),
                    content_type: Some("text/html"),
                })
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn load(
        &self,
        sink: Arc<dyn EventSink>,
        viewport: Size,
        ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    ) -> Result<(LoadedPage, Option<LayoutSource>, Option<Box<dyn PersistentJs>>), Box<dyn Error>> {
        if matches!(self, PageSource::Empty) {
            return Ok((LoadedPage::empty(), None, None));
        }
        let raw = self.load_bytes(sink.clone())?;
        let (page, layout_source, js_ctx) =
            render_bytes(&raw.bytes, raw.content_type, &raw.base, sink, viewport, &mut std::collections::HashSet::new(), ls_store)?;
        Ok((page, Some(layout_source), js_ctx))
    }
}

/// Сырые байты страницы + контекст, необходимый для последующего парсинга и
/// разрешения относительных ссылок. Возвращается `PageSource::load_bytes`.
struct RawPage {
    bytes: Vec<u8>,
    base: ResourceBase,
    content_type: Option<&'static str>,
}

/// Режим запуска shell. Решается на основе CLI-аргументов в `parse_cli`.
#[derive(Debug, Clone)]
enum CliMode {
    /// Обычное окно — текущий source открывается в winit-окне.
    OpenWindow(PageSource),
    /// Headless: pipeline прогоняется до нужной фазы, результат идёт в stdout.
    Dump { source: PageSource, kind: DumpKind },
}

/// Что именно печатать в dump-режиме.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum DumpKind {
    /// Декодированный HTML после `lumen_encoding::decode`.
    Source,
    /// `serialize_layout_tree` — детерминированный текстовый формат layout-дерева.
    Layout,
    /// `serialize_display_list` — текстовый формат paint-команд.
    DisplayList,
}

impl DumpKind {
    fn from_flag(s: &str) -> Option<Self> {
        match s {
            "--dump-source" => Some(DumpKind::Source),
            "--dump-layout" => Some(DumpKind::Layout),
            "--dump-display-list" => Some(DumpKind::DisplayList),
            _ => None,
        }
    }
}

/// Разобрать аргументы (без `argv[0]`) в режим запуска.
///
/// Грамматика:
/// - `[]`           → OpenWindow(Empty)
/// - `[arg]`        → OpenWindow(from_arg(arg)) если arg не dump-флаг; иначе ошибка
/// - `[flag, tgt]`  → Dump если flag — dump-флаг; иначе ошибка
/// - `[…]` (>2)     → ошибка
///
/// Dump-флаги принимаются только в первой позиции — иначе пришлось бы парсить
/// аргументы произвольным порядком, что не нужно для текущего скоупа.
fn parse_cli(args: &[String]) -> Result<CliMode, String> {
    match args {
        [] => Ok(CliMode::OpenWindow(PageSource::Empty)),
        [arg] => {
            if DumpKind::from_flag(arg).is_some() {
                Err(format!("флаг {arg} требует путь или URL"))
            } else if arg.starts_with("--") {
                Err(format!("неизвестный флаг: {arg}"))
            } else {
                Ok(CliMode::OpenWindow(PageSource::from_arg(Some(arg))))
            }
        }
        [flag, target] => {
            let kind = DumpKind::from_flag(flag)
                .ok_or_else(|| format!("неизвестный флаг: {flag}"))?;
            if target.starts_with("--") {
                return Err(format!(
                    "ожидался путь или URL после {flag}, получен флаг {target}"
                ));
            }
            Ok(CliMode::Dump {
                source: PageSource::from_arg(Some(target)),
                kind,
            })
        }
        _ => Err(format!("слишком много аргументов: {}", args.len())),
    }
}

/// Результат загрузки страницы: что рисовать и как назвать окно.
/// Расширяется: favicon, current URL, scroll state — позже.
struct LoadedPage {
    display_list: DisplayList,
    title: Option<String>,
    /// Декодированные `<img src="…">` для GPU upload через
    /// `Renderer::register_image`. Ключ — raw src attribute value (тот же,
    /// что попадает в `DisplayCommand::DrawImage.src`), чтобы render-side
    /// мог сделать lookup без отдельной нормализации URL.
    images: Vec<(String, lumen_image::Image)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` — registered with JS
    /// after page load via `_lumen_init_lazy_images` for proximity-based loading.
    #[allow(dead_code)] // read only inside #[cfg(feature = "quickjs")] blocks
    lazy_pairs: Vec<(u32, String)>,
    /// Layout-дерево страницы — используется animation scheduler-ом.
    layout_box: lumen_layout::LayoutBox,
    /// Провайдер шрифтов с @font-face URL-источниками страницы.
    /// Передаётся рендеру через `set_font_provider` при apply_loaded_page.
    font_registry: Arc<dyn lumen_core::FontProvider>,
    /// Навигационный запрос от JS (location.href= и т.п.), выполненный
    /// в процессе загрузки. Обрабатывается в `about_to_wait`.
    js_navigate: Option<JsNavigateRequest>,
}

impl LoadedPage {
    fn empty() -> Self {
        Self {
            display_list: DisplayList::new(),
            title: None,
            images: Vec::new(),
            lazy_pairs: Vec::new(),
            layout_box: lumen_layout::LayoutBox {
                node: NodeId::from_index(0),
                rect: Rect::ZERO,
                style: lumen_layout::style::ComputedStyle::root(),
                kind: lumen_layout::BoxKind::Block,
                children: Vec::new(),
                col_span: 1,
                row_span: 1,
            },
            font_registry: Arc::new(lumen_font::SystemFontIndex::new()),
            js_navigate: None,
        }
    }
}

/// Действия shell-а, на которые мапятся клавиши. Изолированы от winit, чтобы
/// маппер был тестируем без event loop.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum KeyCommand {
    Reload,
    Exit,
    FindOpen,
    /// Открыть адресную строку (Ctrl+L / F6). Позволяет ввести URL или
    /// поисковый запрос прямо в браузере без перезапуска из CLI.
    OpenAddressBar,
    /// Навигация назад (Alt+Left). Восстанавливает из bfcache если возможно.
    HistoryBack,
    /// Навигация вперёд (Alt+Right). Восстанавливает из bfcache если возможно.
    HistoryForward,
    /// Скролл на одну строку вниз (стрелка вниз).
    ScrollLineDown,
    /// Скролл на одну строку вверх (стрелка вниз).
    ScrollLineUp,
    /// Скролл на ~90% viewport-а вниз (PageDown / Space).
    ScrollPageDown,
    /// Скролл на ~90% viewport-а вверх (PageUp / Shift+Space).
    ScrollPageUp,
    /// Прыжок к началу документа (Home).
    ScrollHome,
    /// Прыжок к концу документа (End).
    ScrollEnd,
    /// Горизонтальный скролл на одну колонку вправо (стрелка вправо).
    ScrollLineRight,
    /// Горизонтальный скролл на одну колонку влево (стрелка влево).
    ScrollLineLeft,
    /// Открыть hint-режим: показать kbd-бейджи на всех кликабельных элементах (F).
    HintModeOpen,
}

/// Маппинг физической клавиши + модификаторов на shell-action.
///
/// F5 без модификаторов  → Reload.
/// Ctrl+R                → Reload.
/// Esc без модификаторов → Exit.
/// Ctrl+W                → Exit.
/// Ctrl+F                → FindOpen.
/// F (без модификаторов) → HintModeOpen (kbd-навигация по ссылкам/кнопкам).
/// ↓ / ↑                 → ScrollLineDown / ScrollLineUp (без модификаторов).
/// → / ←                 → ScrollLineRight / ScrollLineLeft (без модификаторов).
/// PageDown / PageUp     → ScrollPageDown / ScrollPageUp.
/// Space / Shift+Space   → ScrollPageDown / ScrollPageUp (привычка пробела в браузерах).
/// Home / End            → ScrollHome / ScrollEnd.
///
/// Прочие комбинации (Ctrl+Shift+R, F5+Ctrl, и т.д.) — пока None: не хотим
/// перехватывать привычные web-shortcuts (force-reload, etc.) до того, как
/// решим, что они должны делать.
fn keybinding_for(code: KeyCode, mods: ModifiersState) -> Option<KeyCommand> {
    let ctrl_only = mods == ModifiersState::CONTROL;
    let shift_only = mods == ModifiersState::SHIFT;
    let alt_only = mods == ModifiersState::ALT;
    let no_mods = mods.is_empty();
    match code {
        KeyCode::F5 if no_mods => Some(KeyCommand::Reload),
        KeyCode::KeyR if ctrl_only => Some(KeyCommand::Reload),
        KeyCode::Escape if no_mods => Some(KeyCommand::Exit),
        KeyCode::KeyW if ctrl_only => Some(KeyCommand::Exit),
        KeyCode::KeyF if ctrl_only => Some(KeyCommand::FindOpen),
        KeyCode::KeyF if no_mods => Some(KeyCommand::HintModeOpen),
        KeyCode::KeyL if ctrl_only => Some(KeyCommand::OpenAddressBar),
        KeyCode::F6 if no_mods => Some(KeyCommand::OpenAddressBar),
        KeyCode::ArrowLeft if alt_only => Some(KeyCommand::HistoryBack),
        KeyCode::ArrowRight if alt_only => Some(KeyCommand::HistoryForward),
        KeyCode::ArrowDown if no_mods => Some(KeyCommand::ScrollLineDown),
        KeyCode::ArrowUp if no_mods => Some(KeyCommand::ScrollLineUp),
        KeyCode::ArrowRight if no_mods => Some(KeyCommand::ScrollLineRight),
        KeyCode::ArrowLeft if no_mods => Some(KeyCommand::ScrollLineLeft),
        KeyCode::PageDown if no_mods => Some(KeyCommand::ScrollPageDown),
        KeyCode::PageUp if no_mods => Some(KeyCommand::ScrollPageUp),
        KeyCode::Space if no_mods => Some(KeyCommand::ScrollPageDown),
        KeyCode::Space if shift_only => Some(KeyCommand::ScrollPageUp),
        KeyCode::Home if no_mods => Some(KeyCommand::ScrollHome),
        KeyCode::End if no_mods => Some(KeyCommand::ScrollEnd),
        _ => None,
    }
}

// ── Разрешение внешних ресурсов ──────────────────────────────────────────────

/// Откуда загружена страница — нужно для разрешения относительных URL в `<link>`.
#[derive(Clone)]
enum ResourceBase {
    /// Страница загружена из файла. `href` разрешается относительно директории файла.
    File(PathBuf),
    /// Страница загружена по URL. `href` разрешается относительно этого URL.
    Url(String),
}

impl ResourceBase {
    fn resolve(&self, href: &str) -> ResolvedResource {
        if href.starts_with("http://") || href.starts_with("https://") {
            return ResolvedResource::Url(href.to_owned());
        }
        match self {
            ResourceBase::File(base_path) => {
                let dir = base_path.parent().unwrap_or(std::path::Path::new("."));
                ResolvedResource::File(dir.join(href))
            }
            ResourceBase::Url(base_url) => {
                // Resolve через структурированный Url из lumen-core; при сбое
                // base (не должно случаться — base сами и положили в загрузке
                // страницы) откатываемся на raw href, чтобы из-за одного
                // битого <link> не валить весь рендер.
                let resolved = lumen_core::url::Url::parse(base_url)
                    .and_then(|u| u.resolve(href))
                    .map(|u| u.as_str().to_owned())
                    .unwrap_or_else(|_| href.to_owned());
                ResolvedResource::Url(resolved)
            }
        }
    }

    /// Резолвить `href` относительно base и вернуть строковое представление.
    /// Для `File` base — абсолютный путь; для `Url` base — абсолютный URL.
    /// Используется в preload-dispatcher, где нужна строка (не `ResolvedResource`).
    fn resolve_str(&self, href: &str) -> String {
        match self.resolve(href) {
            ResolvedResource::File(p) => p.to_string_lossy().into_owned(),
            ResolvedResource::Url(u) => u,
        }
    }

    /// Извлечь Origin страницы, если base — URL (не файл).
    fn origin(&self) -> Option<lumen_network::Origin> {
        if let ResourceBase::Url(base_url) = self
            && let Ok(url) = lumen_core::url::Url::parse(base_url)
        {
            return lumen_network::Origin::from_url(&url).ok();
        }
        None
    }

    /// Построить `HttpClient` для загрузки подресурсов. Если страница загружена
    /// по HTTPS, подключает mixed-content enforcement (SpecDefault по W3C Mixed
    /// Content spec). Caller выбирает `RequestDestination` и вызывает
    /// `fetch_subresource`, а не `fetch`.
    fn http_client_for_subresource(
        &self,
        sink: Arc<dyn EventSink>,
    ) -> lumen_network::HttpClient {
        use lumen_network::{BrotliContentDecoder, HttpClient, MixedContentMode};
        let client = HttpClient::new()
            .with_sink(sink)
            .with_content_decoder(Arc::new(BrotliContentDecoder::new()));
        if let Some(origin) = self.origin()
            && origin.is_potentially_trustworthy()
        {
            return client.with_mixed_content_policy(origin, MixedContentMode::SpecDefault);
        }
        client
    }
}

enum ResolvedResource {
    File(PathBuf),
    Url(String),
}

// ── Загрузка внешних CSS ─────────────────────────────────────────────────────

fn load_linked_stylesheets(doc: &Document, base: &ResourceBase, sink: &Arc<dyn EventSink>) -> String {
    let mut hrefs = Vec::new();
    collect_link_hrefs(doc, doc.root(), &mut hrefs);

    let mut css = String::new();
    for href in hrefs {
        match base.resolve(&href) {
            ResolvedResource::File(path) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    eprintln!("Загружен CSS: {}", path.display());
                    css.push_str(&content);
                    css.push('\n');
                }
                Err(e) => eprintln!("Пропуск CSS {}: {e}", path.display()),
            },
            ResolvedResource::Url(url) => {
                use lumen_core::event::{Event, TabId};
                use lumen_core::url::Url;
                use lumen_network::{Origin, RequestDestination};

                let sub_url = match Url::parse(&url) {
                    Ok(u) => u,
                    Err(e) => { eprintln!("Пропуск CSS {url}: {e}"); continue; }
                };

                // SOP: cross-origin stylesheets blocked without CORS in Phase 0.
                // Same-origin и file-base — пропускают проверку.
                if let Some(page_origin) = base.origin()
                    && let Ok(sub_origin) = Origin::from_url(&sub_url)
                    && !page_origin.same_origin(&sub_origin)
                {
                    sink.emit(&Event::RequestBlocked {
                        tab_id: TabId(0),
                        url: sub_url,
                        reason: "sop: cross-origin stylesheet".to_owned(),
                    });
                    continue;
                }

                let client = base.http_client_for_subresource(sink.clone());
                match client.fetch_subresource(&sub_url, RequestDestination::Style) {
                    Ok(bytes) => {
                        let content = String::from_utf8_lossy(&bytes);
                        css.push_str(&content);
                        css.push('\n');
                    }
                    Err(e) => eprintln!("Пропуск CSS {url}: {e}"),
                }
            }
        }
    }
    css
}

fn collect_link_hrefs(doc: &Document, id: NodeId, out: &mut Vec<String>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "link"
    {
        let rel = attrs
            .iter()
            .find(|a| a.name.local == "rel")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        let href = attrs
            .iter()
            .find(|a| a.name.local == "href")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if rel.split_ascii_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"))
            && !href.is_empty()
        {
            out.push(href.to_owned());
        }
        return;
    }
    for &child in &node.children {
        collect_link_hrefs(doc, child, out);
    }
}

// ── Загрузка <img src> ───────────────────────────────────────────────────────

/// Обходит DOM через `lumen_layout::collect_image_requests` — picker учитывает
/// `<picture>`/`srcset`/`sizes`, поэтому ключ совпадает с тем, что layout
/// эмитит в `DisplayCommand::DrawImage.src`. Для каждого запроса скачивает
/// байты и декодирует через `lumen_image::decode` (PNG/JPEG dispatch).
///
/// Побочный эффект: для `<img>` без явных `width`/`height` проставляет
/// intrinsic dimensions из декодированного изображения (HTML5 §10 mapped
/// attributes). Author CSS затем перекроет при необходимости.
///
/// Возвращает `(images, lazy_pairs)`:
/// - `images` — декодированные картинки для немедленной регистрации в renderer-е;
/// - `lazy_pairs` — `(node_id_u32, url)` для `<img loading="lazy">`, которые
///   не загружаются сейчас и будут зарегистрированы через `_lumen_init_lazy_images`.
#[allow(clippy::type_complexity)]
fn fetch_and_decode_images(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: lumen_core::geom::Size,
) -> (Vec<(String, lumen_image::Image)>, Vec<(u32, String)>) {
    let requests = lumen_layout::collect_image_requests(doc, viewport);

    let mut out: Vec<(String, lumen_image::Image)> = Vec::new();
    let mut lazy_pairs: Vec<(u32, String)> = Vec::new();
    for req in requests {
        if req.is_lazy {
            // loading="lazy": defer until near viewport; register for proximity check.
            lazy_pairs.push((req.node_id.index() as u32, req.url));
            continue;
        }
        let bytes = match fetch_image_bytes(&req.url, base, sink) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Пропуск картинки {}: {e}", req.url);
                continue;
            }
        };
        let image = match lumen_image::decode(&bytes) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Не декодируется {}: {e}", req.url);
                continue;
            }
        };

        if !req.has_explicit_width && !req.has_explicit_height {
            apply_intrinsic_size(doc, req.node_id, image.width, image.height);
        }

        eprintln!(
            "Загружена картинка: {} ({}×{}, {:?})",
            req.url, image.width, image.height, image.format
        );
        out.push((req.url, image));
    }
    (out, lazy_pairs)
}

/// Encode `s` as a JS string literal (double-quoted, with escaping).
/// Used when building JS snippets from Rust strings (e.g., `_lumen_init_lazy_images`).
#[cfg(feature = "quickjs")]
fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fetch_image_bytes(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    match base.resolve(raw_src) {
        ResolvedResource::File(path) => std::fs::read(&path).map_err(|e| {
            format!("file://{} {e}", path.display()).into()
        }),
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;

            // Images are loaded in no-cors mode: cross-origin allowed, but
            // mixed-content enforcement still applies for HTTPS pages.
            let lumen_url = Url::parse(&url)?;
            let client = base.http_client_for_subresource(sink.clone());
            Ok(client.fetch_subresource(&lumen_url, RequestDestination::Image)?)
        }
    }
}

fn apply_intrinsic_size(doc: &mut Document, node_id: NodeId, width: u32, height: u32) {
    use lumen_dom::{Attribute, QualName};
    let NodeData::Element { attrs, .. } = &mut doc.get_mut(node_id).data else {
        return;
    };
    // Защита от race: если другой проход уже добавил атрибут, не дублируем.
    if !attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width")) {
        attrs.push(Attribute {
            name: QualName::html("width"),
            value: width.to_string(),
        });
    }
    if !attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height")) {
        attrs.push(Attribute {
            name: QualName::html("height"),
            value: height.to_string(),
        });
    }
}

// ── Рендер ───────────────────────────────────────────────────────────────────

/// Результат фаз `decode → parse → layout` — общая часть для оконного и
/// dump-режимов. Поля владеют своими данными — нет ссылок наружу.
struct ParsedPage {
    /// Parsed DOM — shared with JS closures via Arc so event handlers can
    /// mutate the document without rebuilding the entire page.
    document: Arc<Mutex<Document>>,
    stylesheet: lumen_css_parser::Stylesheet,
    layout: LayoutBox,
    title: Option<String>,
    rule_count: usize,
    /// Декодированные изображения, найденные при обходе DOM. См. [`LoadedPage::images`].
    images: Vec<(String, lumen_image::Image)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` elements — skipped by
    /// the eager fetch pass; registered with JS `_lumen_init_lazy_images` after load.
    lazy_pairs: Vec<(u32, String)>,
    /// Subresource-хинты, найденные preload-сканером ДО DOM-парсинга.
    /// Source-order: первые хинты важнее (их fetch стартует первым).
    preload_hints: Vec<lumen_html_parser::PreloadHint>,
    /// Decoded UTF-8 HTML source — stored for bfcache snapshot.
    html_source: String,
    /// @font-face URL-шрифты + системные шрифты. Передаётся рендеру.
    font_registry: Arc<dyn lumen_core::FontProvider>,
    /// Навигационный запрос, выставленный JS во время выполнения скриптов.
    js_navigate: Option<JsNavigateRequest>,
    /// Persistent JS context (QuickJS) kept alive after page load so that
    /// event handlers registered via `addEventListener` continue to work.
    /// `None` when the quickjs feature is disabled or script init failed.
    js_ctx: Option<Box<dyn PersistentJs>>,
}

/// Источник для повторного layout без повторной загрузки/парсинга.
/// Хранится в `Lumen`; обновляется только при reload/load новой страницы.
struct LayoutSource {
    /// DOM — shared with the persistent JS runtime via Arc<Mutex> so that
    /// JS event handlers can mutate it between repaints.
    document: Arc<Mutex<Document>>,
    stylesheet: lumen_css_parser::Stylesheet,
    /// Decoded HTML source captured after encoding detection. Used by bfcache
    /// to restore the page without a network round-trip.
    #[allow(dead_code)]
    html_source: Option<String>,
}

fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
) -> Result<ParsedPage, Box<dyn Error>> {
    // Кодировку определяем по BOM -> <meta charset> -> эвристике. Это покрывает
    // и UTF-8 (большинство), и старые cp1251 / koi8-r / cp866 файлы.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    eprintln!("Кодировка: {}", encoding.name());

    // Preload-сканер запускается ДО DOM-парсинга (HTML LS §13.2.6.4.7).
    // `preload_seen` — cross-call dedup: если streaming уже отправил <head>-хинты
    // через EarlyPreloadHints, финальный scan пропустит их и добавит только новые
    // (body-images, lazy-loaded resources и т.п.).
    let preload_hints = lumen_html_parser::scan_preload_hints(&source);
    dispatch_preload_hints(&preload_hints, base, sink, preload_seen);

    let doc = lumen_html_parser::parse(&source);
    let title = extract_title(&doc);

    // Гейт выполнения скриптов: top-level документ не sandboxed.
    // QuickJS + install_dom дают скриптам полный доступ к DOM-дереву.
    // fetch_provider пробрасывается в window.fetch(); ws_provider — в new WebSocket().
    let (fetch_provider, ws_provider) = match base {
        ResourceBase::Url(_) => {
            let client = base.http_client_for_subresource(Arc::clone(sink));
            let arc_client = Arc::new(client);
            let fp: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsFetchProvider>);
            let wp: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> =
                Some(arc_client as Arc<dyn lumen_core::ext::JsWebSocketProvider>);
            (fp, wp)
        }
        ResourceBase::File(_) => (None, None),
    };
    // URL страницы для инициализации window.location в JS.
    let page_url = match base {
        ResourceBase::Url(u) => u.as_str().to_owned(),
        ResourceBase::File(p) => format!("file://{}", p.display()),
    };
    let (doc_arc, js_nav, js_ctx) = run_scripts_with_dom(
        doc,
        lumen_core::SandboxFlags::empty(),
        &page_url,
        fetch_provider,
        ws_provider,
        ls_store,
    );

    // CSS Selectors L4 §9.6 `:target`: set current target from URL fragment so
    // the matcher has the correct target_id before style cascade in layout.
    let page_fragment = if let ResourceBase::Url(u) = base {
        lumen_core::url::Url::parse(u)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned))
    } else {
        None
    };
    {
        let mut d = doc_arc.lock().unwrap();
        d.set_target(page_fragment.as_deref());
        // Гейт отправки форм: Phase 0 — top-level документ не sandboxed.
        check_form_gate(&d, lumen_core::SandboxFlags::empty());
        // Гейт навигации: Phase 0 — top-level документ не sandboxed.
        check_navigation_gate(&d, lumen_core::SandboxFlags::empty());
        // Применяем sandbox-ограничения из <iframe sandbox> элементов.
        // Phase 0: iframe sub-документы не загружаются — применяем гейты
        // к самому iframe-элементу, логируем ограничения для будущего Phase 1.
        apply_iframe_sandbox_gates(&d);
    }

    // Fetch + decode <img src>. Должно идти ДО layout, потому что intrinsic
    // dimensions из декодированного изображения проставляются как HTML
    // presentational hints (width/height attribute) и потом подхватываются
    // style cascade. Errors silently пропускаются — битая картинка не валит
    // всю страницу, layout нарисует серый placeholder.
    // loading="lazy" изображения возвращаются в lazy_pairs и не загружаются сейчас.
    let (images, lazy_pairs) = {
        let mut d = doc_arc.lock().unwrap();
        fetch_and_decode_images(&mut d, base, sink, viewport)
    };

    // Встроенные <style> + внешние <link rel=stylesheet>.
    let css = {
        let d = doc_arc.lock().unwrap();
        let mut css = extract_style_blocks(&d);
        css.push_str(&load_linked_stylesheets(&d, base, sink));
        css
    };

    let sheet = lumen_css_parser::parse(&css);

    // @font-face: загружаем url()-источники до layout.
    // CSS: @font-face multi-font TextMeasurer — P1 нужно поддержать font-family в layout
    let font_registry = load_font_faces(&sheet.font_faces, base, sink);
    let font_provider: Arc<dyn lumen_core::FontProvider> = Arc::new(font_registry);

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("ошибка разбора шрифта: {e}"))?;
    let measurer = lumen_paint::FontMeasurer::new(&font)
        .map_err(|e| format!("ошибка метрик шрифта: {e}"))?;

    let layout = {
        let d = doc_arc.lock().unwrap();
        lumen_layout::layout_measured(&d, &sheet, viewport, &measurer)
    };

    // CSS Backgrounds L3 §3.10 — собираем `background-image: url(...)` уже
    // после layout-а (картинки фона не влияют на расчёт коробок). Декодируем
    // и добавляем к `images` тем же ключом, что эмиттер кладёт в
    // `DisplayCommand::DrawBackgroundImage.src`.
    let mut images = images;
    for (src, image) in fetch_and_decode_background_images(&layout, base, sink) {
        images.push((src, image));
    }

    let rule_count = sheet.rules.len();
    Ok(ParsedPage {
        document: doc_arc,
        stylesheet: sheet,
        layout,
        title,
        rule_count,
        images,
        lazy_pairs,
        preload_hints,
        html_source: source,
        font_registry: font_provider,
        js_navigate: js_nav,
        js_ctx,
    })
}

/// Скачивает и декодирует все `background-image: url(...)` из готового
/// layout-дерева. Дубликаты URL фильтруются на стороне layout
/// (`collect_background_image_requests`). Ошибки скачивания / декодирования
/// логируются в stderr — battle-tested fail-soft: битая bg-картинка не валит
/// страницу, renderer всё равно отобразит background-color поверх.
fn fetch_and_decode_background_images(
    layout: &LayoutBox,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
) -> Vec<(String, lumen_image::Image)> {
    let urls = lumen_layout::collect_background_image_requests(layout);
    let mut out: Vec<(String, lumen_image::Image)> = Vec::new();
    for url in urls {
        let bytes = match fetch_image_bytes(&url, base, sink) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Пропуск bg-картинки {url}: {e}");
                continue;
            }
        };
        let image = match lumen_image::decode(&bytes) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Не декодируется bg-картинка {url}: {e}");
                continue;
            }
        };
        eprintln!(
            "Загружена bg-картинка: {url} ({}×{}, {:?})",
            image.width, image.height, image.format
        );
        out.push((url, image));
    }
    out
}

/// Загружает шрифты из @font-face правил таблицы стилей в `FontRegistry`.
///
/// Для каждого `FontFaceRule` перебирает `src:` источники в порядке (CSS §4.1:
/// первый успешный wins). `local()` пропускается — `SystemFontIndex` уже
/// покрывает системные шрифты. `url()` загружается так же, как изображения.
/// WOFF/WOFF2 прозрачно декодируются в sfnt перед регистрацией.
///
/// Ошибки загрузки/декодирования отдельных источников не фатальны: пишутся в
/// stderr и переходим к следующему источнику.
fn load_font_faces(
    font_faces: &[lumen_css_parser::FontFaceRule],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
) -> lumen_font::FontRegistry {
    use lumen_css_parser::FontFaceSourceKind;
    use lumen_core::FontStyle;

    let registry = lumen_font::FontRegistry::new();

    for rule in font_faces {
        if rule.family.is_empty() || rule.sources.is_empty() {
            continue;
        }

        let weight = parse_font_weight(rule.weight.as_deref());
        let style = rule
            .style
            .as_deref()
            .and_then(FontStyle::parse_keyword)
            .unwrap_or(FontStyle::Normal);

        for src in &rule.sources {
            if src.kind == FontFaceSourceKind::Local {
                continue;
            }

            let raw = match fetch_image_bytes(&src.value, base, sink) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("@font-face «{}»: не загружен {}: {e}", rule.family, src.value);
                    continue;
                }
            };

            let bytes = match lumen_font::maybe_decode_font(&raw) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => raw,
                Err(e) => {
                    eprintln!("@font-face «{}»: не декодирован WOFF: {e}", rule.family);
                    continue;
                }
            };

            if lumen_font::Font::parse(&bytes).is_err() {
                eprintln!("@font-face «{}»: невалидный шрифт {}", rule.family, src.value);
                continue;
            }

            eprintln!(
                "@font-face загружен: «{}» weight={} src={}",
                rule.family, weight, src.value
            );
            registry.register_from_bytes(&rule.family, weight, style, bytes);
            break;
        }
    }

    registry
}

/// Парсит `font-weight` дескриптор @font-face: ключевые слова + числа.
/// Диапазоны (`400 700`) — берём первое значение. Default: 400.
fn parse_font_weight(s: Option<&str>) -> u16 {
    let Some(s) = s else { return 400 };
    match s.trim() {
        "normal" => 400,
        "bold" => 700,
        other => other
            .split_ascii_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(400),
    }
}

/// Рекурсивно собирает `ComputedStyle` всех узлов layout-дерева.
/// Результат используется `transition_scheduler.sync()` для сравнения
/// предыдущего и нового стиля после каждого relayout-а.
fn collect_box_styles(lb: &LayoutBox, map: &mut HashMap<NodeId, ComputedStyle>) {
    map.insert(lb.node, lb.style.clone());
    for child in &lb.children {
        collect_box_styles(child, map);
    }
}

/// Рекурсивно собирает bounding rects всех layout-боксов в плоскую карту
/// `NodeId → [x, y, width, height]` (border-box, viewport-relative CSS px).
/// Используется JS-runtime-ом для `getBoundingClientRect` / `ResizeObserver`
/// / `IntersectionObserver`.
#[cfg(feature = "quickjs")]
fn collect_layout_rects(lb: &LayoutBox) -> HashMap<u32, [f32; 4]> {
    let mut map = HashMap::new();
    collect_layout_rects_rec(lb, &mut map);
    map
}

#[cfg(feature = "quickjs")]
fn collect_layout_rects_rec(lb: &LayoutBox, map: &mut HashMap<u32, [f32; 4]>) {
    let r = &lb.rect;
    map.insert(lb.node.index() as u32, [r.x, r.y, r.width, r.height]);
    for child in &lb.children {
        collect_layout_rects_rec(child, map);
    }
}

/// Строит display list с правильным painting order (CSS 2.1 Appendix E, z-index stacking).
fn paint_ordered(layout: &lumen_layout::LayoutBox) -> DisplayList {
    let tree = StackingTree::build(layout);
    let order = PaintOrder::from_tree(&tree);
    build_display_list_ordered(layout, &tree, &order)
}

/// Повторный layout+paint по сохранённому `LayoutSource` с новым viewport.
/// Возвращает `(DisplayList, LayoutBox)` — LayoutBox нужен для animation scheduler.
fn relayout_page(src: &LayoutSource, viewport: Size) -> (DisplayList, lumen_layout::LayoutBox) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let doc = src.document.lock().unwrap();
    let layout = lumen_layout::layout_measured(&doc, &src.stylesheet, viewport, &measurer);
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout)
}

/// Get-or-create the localStorage partition for the given `ResourceBase` origin.
/// Returns `None` for file: bases (no persistent origin-partitioned storage).
fn ls_store_for_base(
    base: &ResourceBase,
    ls_storage: &mut HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
) -> Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>> {
    let origin = match base {
        ResourceBase::Url(u) => {
            lumen_core::url::Url::parse(u).ok().map(|parsed| {
                let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
                format!("{}://{}{}", parsed.scheme(), parsed.host(), port)
            })?
        }
        ResourceBase::File(_) => return None,
    };
    Some(Arc::clone(ls_storage.entry(origin).or_insert_with(|| {
        Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
    })))
}

#[allow(clippy::type_complexity)]
fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
) -> Result<(LoadedPage, LayoutSource, Option<Box<dyn PersistentJs>>), Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink, viewport, preload_seen, ls_store)?;
    let display_list = paint_ordered(&parsed.layout);
    println!(
        "Распарсено: {} DOM-узлов, {} CSS-правил, {} paint-команд, {} картинок, {} preload-хинтов",
        parsed.document.lock().unwrap().len(),
        parsed.rule_count,
        display_list.len(),
        parsed.images.len(),
        parsed.preload_hints.len(),
    );
    let layout_box = parsed.layout;
    let layout_source = LayoutSource {
        document: Arc::clone(&parsed.document),
        stylesheet: parsed.stylesheet,
        html_source: Some(parsed.html_source),
    };
    Ok((
        LoadedPage {
            display_list,
            title: parsed.title,
            images: parsed.images,
            lazy_pairs: parsed.lazy_pairs,
            layout_box,
            font_registry: parsed.font_registry,
            js_navigate: parsed.js_navigate,
        },
        layout_source,
        parsed.js_ctx,
    ))
}

/// Отправить preload-хинты в EventSink.
///
/// Каждый `PreloadHint` резолвится относительно `base` (4B.3) и
/// преобразуется в `Event::SubresourceHintFound { url, kind, priority }`.
/// Хинты сортируются по убыванию приоритета (High → Medium → Low), чтобы
/// самые критичные ресурсы стартовали первыми (полезно при HTTP/2).
/// `srcset`-строки эмитятся как-есть (multi-URL формат — задача picker-а).
/// `seen` — набор уже отправленных URL (cross-call дедупликация); caller
/// передаёт `&mut HashSet::new()` для одноразового вызова или persistent-сет
/// для дедупа между streaming-сканом и финальным pipeline.
/// В Phase 0 sink логирует в stderr; в будущем запустит fetch через HttpClient.
fn dispatch_preload_hints(
    hints: &[lumen_html_parser::PreloadHint],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    seen: &mut std::collections::HashSet<String>,
) {
    use lumen_html_parser::PreloadHint;

    // Первый проход: резолв URL + вычисление kind.
    let mut resolved: Vec<(String, SubresourceKind)> = Vec::with_capacity(hints.len());
    for hint in hints {
        let pair = match hint {
            PreloadHint::Stylesheet { url } =>
                (base.resolve_str(url), SubresourceKind::Stylesheet),
            PreloadHint::Script { url } =>
                (base.resolve_str(url), SubresourceKind::Script),
            PreloadHint::Image { url: Some(url), .. } =>
                (base.resolve_str(url), SubresourceKind::Image),
            // srcset содержит список URL — резолвинг каждого кандидата
            // откладывается до picker-а; эмитим srcset-строку как-есть.
            PreloadHint::Image { url: None, srcset: Some(s), .. } =>
                (s.clone(), SubresourceKind::Image),
            PreloadHint::SourceSet { srcset, .. } =>
                (srcset.clone(), SubresourceKind::Image),
            PreloadHint::Preload { url, as_kind } => {
                let kind = match as_kind.as_deref() {
                    Some("font") => SubresourceKind::Font,
                    Some("image") => SubresourceKind::Image,
                    Some("script") => SubresourceKind::Script,
                    Some("style") => SubresourceKind::Stylesheet,
                    _ => SubresourceKind::Other { as_kind: as_kind.clone() },
                };
                (base.resolve_str(url), kind)
            }
            // Preconnect URL — origin, не содержит path — резолвинг тривиален.
            PreloadHint::Preconnect { url, dns_only } =>
                (base.resolve_str(url), SubresourceKind::Preconnect { dns_only: *dns_only }),
            PreloadHint::Image { url: None, srcset: None, .. } => continue,
        };
        resolved.push(pair);
    }

    // Stable-sort по приоритету: High первыми. Stable сохраняет source-order
    // внутри одного уровня приоритета (важно для HTTP/2 multiplexing).
    resolved.sort_by_key(|(_, k)| FetchPriority::for_kind(k));

    // Дедупликация + emit: пропускаем URL, уже отправленные в предыдущих вызовах
    // (cross-call dedup для streaming + финального pipeline).
    for (url, kind) in resolved {
        if seen.insert(url.clone()) {
            let priority = FetchPriority::for_kind(&kind);
            sink.emit(&Event::SubresourceHintFound { url, kind, priority });
        }
    }
}

/// Найти первый `<title>` в дереве и склеить его текстовые дети.
///
/// HTML5 разрешает только один `<title>` в `<head>`, но мы lenient-парсер —
/// берём первый встречный. Энтити уже декодированы tokenizer-ом (RCDATA-режим).
fn extract_title(doc: &Document) -> Option<String> {
    let mut buf = String::new();
    if walk_title(doc, doc.root(), &mut buf) {
        let trimmed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

fn walk_title(doc: &Document, id: NodeId, out: &mut String) -> bool {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "title"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
            }
        }
        return true;
    }
    for &child in &node.children {
        if walk_title(doc, child, out) {
            return true;
        }
    }
    false
}

fn extract_style_blocks(doc: &Document) -> String {
    let mut out = String::new();
    walk_style_blocks(doc, doc.root(), &mut out);
    out
}

fn collect_inline_scripts(doc: &Document, id: NodeId, out: &mut Vec<String>) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if !text.trim().is_empty() {
            out.push(text);
        }
        return;
    }
    for &child in &node.children {
        collect_inline_scripts(doc, child, out);
    }
}

/// Применить sandbox-ограничения для всех `<iframe sandbox>` элементов документа.
///
/// Для каждого sandboxed iframe вызывает соответствующие gate-функции:
/// - [`check_form_gate`] с `SandboxFlags::FORMS` если формы запрещены
/// - [`check_navigation_gate`] с `SandboxFlags::NAVIGATION` если навигация запрещена
/// - [`check_popup_gate`] с `SandboxFlags::AUXILIARY_NAVIGATION` если popups запрещены
///
/// Phase 0: iframe sub-документы не загружаются; гейты применяются к самому
/// iframe-элементу через его sandbox-флаги. Логируют ограничения в stderr.
fn apply_iframe_sandbox_gates(doc: &Document) {
    let iframes = collect_iframes(doc);
    for info in &iframes {
        if !info.is_sandboxed {
            continue;
        }
        let sb = info.sandbox;
        let src = info.src.as_deref().unwrap_or("<no src>");
        if sb.contains(lumen_core::SandboxFlags::SCRIPTS) {
            eprintln!("sandbox: iframe '{src}' — скрипты запрещены (sandbox=scripts)");
        }
        if sb.contains(lumen_core::SandboxFlags::FORMS) {
            eprintln!("sandbox: iframe '{src}' — формы запрещены (sandbox=forms)");
            check_form_gate(doc, sb);
        }
        if sb.contains(lumen_core::SandboxFlags::NAVIGATION) {
            check_navigation_gate(doc, sb);
        }
        check_popup_gate(sb);
    }
}

/// Выполнить inline `<script>` блоки с DOM-доступом (QuickJS + install_dom).
///
/// Принимает `doc` по значению, оборачивает в `Arc<Mutex<>>` на время выполнения
/// Выполняет inline `<script>` блоки через QuickJS (если feature включён),
/// возвращает `(Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Box<dyn PersistentJs>>)`.
///
/// Документ оборачивается в `Arc<Mutex>` чтобы JS-замыкания и layout-код
/// могли разделить доступ без лишних клонов. Persistent runtime возвращается
/// как `PersistentJs` для диспатча событий после загрузки страницы.
///
/// `page_url` пробрасывается в `window.location` (инициализация).
/// `fetch_provider` пробрасывается в `window.fetch()`.
/// `ws_provider` пробрасывается в `new WebSocket(url)`.
/// `ls_store` — localStorage partition для текущего origin (persists across reloads).
/// `None` = no network (sandboxed context или отключён quickjs feature).
#[allow(clippy::needless_return)] // `return` inside #[cfg] block is needed for correct control flow
#[allow(unused_variables, clippy::type_complexity)] // ls_store is used only inside #[cfg(feature = "quickjs")]
fn run_scripts_with_dom(
    doc: Document,
    sandbox: lumen_core::SandboxFlags,
    page_url: &str,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
) -> (Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Box<dyn PersistentJs>>) {
    let mut scripts: Vec<String> = Vec::new();
    collect_inline_scripts(&doc, doc.root(), &mut scripts);

    let doc_arc = Arc::new(Mutex::new(doc));

    if scripts.is_empty() {
        return (doc_arc, None, None);
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: заблокировано {} скрипт(ов) (sandbox=scripts)",
            scripts.len()
        );
        return (doc_arc, None, None);
    }

    #[cfg(feature = "quickjs")]
    {
        use lumen_core::ext::JsRuntime as _;
        match lumen_js::QuickJsRuntime::new() {
            Ok(rt) => {
                if let Err(e) = rt.install_dom(Arc::clone(&doc_arc), page_url, fetch_provider, ws_provider, ls_store) {
                    eprintln!("JS DOM init failed: {e}");
                }
                for src in &scripts {
                    match rt.eval(src) {
                        Ok(_) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "script: engine=quickjs, выполнение пропущено ({} байт)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("script error: {e}"),
                    }
                }
                let nav_req = rt.take_navigate_request().map(|r| match r {
                    lumen_js::NavigateRequest::Push(u)    => JsNavigateRequest::Push(u),
                    lumen_js::NavigateRequest::Replace(u) => JsNavigateRequest::Replace(u),
                    lumen_js::NavigateRequest::Reload     => JsNavigateRequest::Reload,
                });
                // Keep rt alive: return as PersistentJs so event handlers work after load.
                let ctx: Box<dyn PersistentJs> = Box::new(QuickPersistentJs { rt });
                return (doc_arc, nav_req, Some(ctx));
            }
            Err(e) => {
                eprintln!("QuickJS init failed: {e}");
                return (doc_arc, None, None);
            }
        }
    }

    #[cfg(not(feature = "quickjs"))]
    {
        let _ = page_url;
        let _ = fetch_provider;
        let _ = ws_provider;
        use lumen_core::ext::JsRuntime as _;
        for src in &scripts {
            match lumen_core::NullJsRuntime.eval(src) {
                Ok(_) => {}
                Err(lumen_core::JsError::NotImplemented) => {
                    eprintln!(
                        "script: engine=null, выполнение пропущено ({} байт)",
                        src.len()
                    );
                }
                Err(e) => eprintln!("script error: {e}"),
            }
        }
        (doc_arc, None, None)
    }
}

/// Выполнить inline `<script>` блоки если sandbox позволяет, иначе заблокировать.
///
/// `SandboxFlags::SCRIPTS` установлен — скрипты запрещены; функция логирует
/// количество заблокированных и возвращает 0. Иначе каждый скрипт передаётся
/// в `runtime.eval()`; без feature `quickjs` это NullJsRuntime → `NotImplemented`.
/// Возвращает число скриптов, переданных в runtime.
#[cfg(test)]
fn run_scripts(
    doc: &Document,
    sandbox: lumen_core::SandboxFlags,
    runtime: &dyn lumen_core::JsRuntime,
) -> usize {
    let mut scripts: Vec<String> = Vec::new();
    collect_inline_scripts(doc, doc.root(), &mut scripts);
    if scripts.is_empty() {
        return 0;
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: заблокировано {} скрипт(ов) (sandbox=scripts)",
            scripts.len()
        );
        return 0;
    }
    for src in &scripts {
        match runtime.eval(src) {
            Ok(_) => {}
            Err(lumen_core::JsError::NotImplemented) => {
                eprintln!(
                    "script: engine={}, выполнение пропущено ({} байт)",
                    runtime.engine_name(),
                    src.len()
                );
            }
            Err(e) => {
                eprintln!("script error: {e}");
            }
        }
    }
    scripts.len()
}

fn walk_style_blocks(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        walk_style_blocks(doc, child, out);
    }
}

/// Формат заголовка окна. С title из страницы — `"<title> — Lumen"`,
/// без — fallback на версию билда.
fn window_title(page_title: Option<&str>) -> String {
    match page_title {
        Some(t) => format!("{t} — Lumen"),
        None => format!("Lumen {}", env!("CARGO_PKG_VERSION")),
    }
}

// ── Window + Renderer ────────────────────────────────────────────────────────

struct Lumen {
    display_list: DisplayList,
    title: Option<String>,
    /// Декодированные `<img>` ресурсы. До создания Renderer-а — хранятся
    /// в Vec и заливаются в GPU в `resumed`; после — register_image идёт
    /// напрямую в `reload`. На переходах между страницами очищается через
    /// `Renderer::clear_images` + переустановка.
    pending_images: Vec<(String, lumen_image::Image)>,
    source: PageSource,
    event_sink: Arc<dyn EventSink>,
    modifiers: ModifiersState,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// HTML event loop runtime. На каждой итерации winit-loop (AboutToWait)
    /// выполняется одна task, на RedrawRequested — run_rendering_step
    /// (вызывает rAF-callback-и), на WindowEvent::Resized —
    /// deliver_observer_records(Resize).
    runtime: runtime::EventLoop,
    /// CSS Animations timeline scheduler — тикается на каждом RedrawRequested.
    /// Хранит start-time для каждой запущенной анимации и вычисляет
    /// интерполированные значения. Очищается при load/reload.
    animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CSS Transitions scheduler — reactive; обнаруживает изменения computed-style
    /// между двумя relayout-ами и интерполирует значения per-frame.
    /// `sync()` вызывается после каждого layout-обновления; `tick()` — на каждом
    /// RedrawRequested вместе с animation_scheduler. Очищается при load/reload.
    transition_scheduler: TransitionScheduler,
    /// Computed styles предыдущего layout-дерева — нужны `transition_scheduler.sync()`
    /// для определения изменившихся свойств. Обновляется после каждого layout.
    prev_styles: HashMap<NodeId, ComputedStyle>,
    /// Последний вычисленный кадр анимаций. `None` — страница не загружена
    /// или нет активных анимаций.
    anim_frame: Option<lumen_layout::AnimationFrame>,
    /// Layout-дерево текущей страницы — нужен scheduler-у для обхода узлов
    /// и извлечения animation-longhands. Обновляется при load/reload/relayout.
    layout_box: Option<lumen_layout::LayoutBox>,
    /// Эпоха для rAF-timestamp-ов в миллисекундах от старта shell-а
    /// (DOMHighResTimeStamp — HTML §8.1.5.1: «timestamp passed to callback
    /// should be the current high resolution time»).
    epoch: std::time::Instant,
    /// Состояние Ctrl+F. Открыт ли bar, текущий query и индекс активного
    /// совпадения. Содержимое поиска не сохраняется между reload-ами
    /// (close() полностью очищает state); это сознательно: после reload
    /// display list другой, и старые позиции совпадений уже невалидны.
    find: find::FindState,
    /// Состояние Ctrl+L адресной строки. Открыт ли бар и текущий ввод.
    /// Закрывается при навигации (commit) и при Esc.
    address_bar: address_bar::AddressBarState,
    /// Click-hint overlay: vimium-style kbd-навигация по кликабельным элементам.
    /// Открывается клавишей F; закрывается Escape, успешной активацией,
    /// открытием find/address bar или переходом на другую страницу.
    hint: hints::HintState,
    /// Текущее вертикальное смещение страницы (CSS px). 0 — верх документа.
    /// Растёт вниз, клампится в `[0, max(0, content_height − viewport_height)]`.
    /// На load/reload сбрасывается в 0.
    scroll_y: f32,
    /// Текущее горизонтальное смещение страницы (CSS px). 0 — левый край.
    /// Растёт вправо, клампится в `[0, max(0, content_width − viewport_width)]`.
    /// На load/reload сбрасывается в 0.
    scroll_x: f32,
    /// Полная высота контента в CSS px — `max(rect.y + rect.height)` по
    /// текущему display list-у. Обновляется после load/reload. 0 — нет контента.
    content_height: f32,
    /// Полная ширина контента в CSS px — `max(rect.x + rect.width)` по
    /// текущему display list-у. Обновляется после load/reload. 0 — нет контента.
    content_width: f32,
    /// Последняя известная позиция курсора в **physical** пикселях (от winit).
    /// `None` пока курсор не вошёл в окно. Конвертируется в CSS px через
    /// `scale_factor()` непосредственно в hit-test / drag callback-ах.
    cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Активный drag scrollbar-thumb-а: `Some` пока зажата левая кнопка после
    /// click-а по thumb-у. `MouseInput Released` или `CursorLeft` сбрасывают
    /// в `None`. Снапшот `(start_scroll_y, start_mouse_y)` фиксирован на момент
    /// начала drag-а — это даёт «закреплённый под пальцем» thumb (стандартный
    /// scrollbar UX).
    scroll_drag: Option<scrollbar::ScrollDrag>,
    /// Активная smooth-scroll анимация для keyboard / wheel / page-jump /
    /// find-scroll-to-match. `None` — `scroll_y` стационарен или меняется
    /// инстантно (drag, reload). При live-анимации `RedrawRequested` тикает
    /// её через `advance_scroll_anim` и просит ещё один redraw до завершения.
    scroll_anim: Option<scroll_anim::ScrollAnim>,
    /// Momentum (kinetic) scroll: запускается при `TouchPhase::Ended` с
    /// ненулевой скоростью от тачпада. Тикается через `advance_momentum`
    /// в `RedrawRequested`. `None` — нет активной инерции.
    momentum_anim: Option<momentum_anim::MomentumAnim>,
    /// Мгновенная скорость тачпада от последних `PixelDelta`-событий
    /// (CSS px / ms). Обновляется EWMA-фильтром. Используется при
    /// `TouchPhase::Ended` для запуска `momentum_anim`.
    touchpad_vel: (f32, f32),
    /// Timestamp последнего `PixelDelta`-события для расчёта dt в EWMA.
    touchpad_vel_time_ms: f64,
    /// Последний выставленный cursor icon — чтобы при каждом CursorMoved (а это
    /// сотни событий в секунду при активном движении мыши) не дёргать
    /// `Window::set_cursor` напрасно. `None` — ещё не выставляли (init).
    last_cursor_icon: Option<CursorIcon>,
    /// DOM + stylesheet для relayout без повторного fetch/parse. Обновляется
    /// при каждом load/reload. `None` — страница не загружена (Empty source).
    layout_source: Option<LayoutSource>,
    /// Флаг «нужно reload после текущего about_to_wait». Устанавливается
    /// closure-ом внутри queue_task — это единственный способ сообщить
    /// Lumen-у из task-closure (которая `+ 'static` и не владеет `&mut self`).
    pending_reload: Rc<Cell<bool>>,
    /// Навигационный запрос от JS (location.href=, assign, replace, reload),
    /// захваченный во время выполнения скриптов страницы. Обрабатывается
    /// в `about_to_wait` после первого рендера загруженной страницы.
    pending_js_navigate: Option<JsNavigateRequest>,
    /// Proxy для отправки LoadEvent из background-потока загрузки в event loop.
    load_proxy: EventLoopProxy<LoadEvent>,
    /// Инкрементальный HTML-парсер — активен во время streaming load.
    /// `None` до первого HtmlChunk или после LoadDone/LoadError.
    stream_builder: Option<lumen_html_parser::IncrementalTreeBuilder>,
    /// Момент последнего промежуточного кадра при streaming — для throttling.
    stream_last_paint: std::time::Instant,
    /// URL subresource-хинтов, уже отправленных в sink во время streaming
    /// (`EarlyPreloadHints`). Финальный `dispatch_preload_hints` в `LoadDone`
    /// пропускает URL из этого набора — без дублей в stderr и без повторных
    /// fetch-триггеров при реальном параллельном prefetch. Очищается в начале
    /// каждого нового страничного load.
    preload_dispatched: std::collections::HashSet<String>,
    /// Текущий IME preedit-текст. `Some` — composition-сессия активна,
    /// `None` — нет активного IME ввода.
    ime_composing: Option<String>,
    /// In-memory bfcache — HTML snapshots keyed by URL for instant back/forward
    /// restoration without a network round-trip (HTML Living Standard §8.6).
    bfcache: BfCache,
    /// Navigation history stack — pages the user navigated away from.
    /// Top = most recent previous page.
    nav_back: Vec<NavEntry>,
    /// Forward history stack — pages the user went back from.
    /// Top = most recently visited "forward" page.
    nav_fwd: Vec<NavEntry>,
    /// Runtime form control state (value, checked) keyed by NodeId.
    /// Persists for the lifetime of the current page; cleared on load/reload.
    form_state: forms::FormState,
    /// Active validation tooltip: (anchor_rect_in_doc_space, message).
    /// Displayed as a viewport-locked overlay. Dismissed on next click.
    validation_tooltip: Option<(Rect, String)>,
    /// NodeId of the `<input type="color">` whose picker is currently open.
    /// The picker overlay is viewport-locked; clicking a swatch closes it.
    color_picker_node: Option<NodeId>,
    /// Persistent `localStorage` partitions keyed by origin (scheme+host+port).
    /// Each entry survives page reloads within the same session.
    /// Partitioned by origin to enforce Same-Origin Policy for storage access.
    ls_storage: HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    /// Live JS context for the current page — keeps event listeners active after
    /// initial script execution. `None` when `quickjs` feature is disabled or
    /// no scripts were registered. Must be dropped before `layout_source` on
    /// navigation to release Arc clones held in JS closures.
    js_ctx: Option<Box<dyn PersistentJs>>,
    /// When true the vertical scrollbar overlay is suppressed entirely.
    /// Set by `--no-scrollbar` CLI flag; used by graphic test pipeline to
    /// avoid scrollbar pixels contaminating the diff against Edge headless.
    no_scrollbar: bool,
    /// Guards for PerformancePaintTiming entries (W3C Paint Timing §2).
    /// `true` once the entry has been delivered to JS so we don't double-fire.
    first_paint_delivered: bool,
    /// `true` once `first-contentful-paint` has been delivered to JS.
    first_contentful_paint_delivered: bool,
    /// FTS5-индекс по тексту посещённых страниц — используется omnibox (@history).
    /// In-memory в Phase 0; в Phase 2 открывается из профильной БД.
    history_fts: HistoryFts,
    /// История поисковых запросов для prefix-match autocomplete в omnibox.
    /// In-memory в Phase 0; в Phase 2 открывается из профильной БД.
    search_history: SearchHistory,
}

impl Lumen {
    /// Повторный layout+paint при изменении размера viewport.
    /// Использует сохранённый `LayoutSource`; парсинг не повторяется.
    fn relayout(&mut self) {
        let Some(src) = self.layout_source.as_ref() else { return };
        let Some(r) = self.renderer.as_ref() else { return };
        let vp_size = r.viewport_size();
        // Guard against degenerate viewport (renderer not yet configured or minimized).
        if vp_size.width <= 0.0 || vp_size.height <= 0.0 {
            return;
        }
        let viewport = Size::new(vp_size.width as f32, vp_size.height as f32);
        let (new_dl, lb) = relayout_page(src, viewport);
        self.content_height = content_height_of(&new_dl);
        self.content_width = content_width_of(&new_dl);
        self.display_list = new_dl;
        // Sync transitions: compare prev styles with new layout before replacing.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = HashMap::new();
        collect_box_styles(&lb, &mut new_styles);
        for (node, new_style) in &new_styles {
            if let Some(old_style) = self.prev_styles.get(node) {
                self.transition_scheduler.sync(*node, old_style, new_style, now_s);
            }
        }
        self.prev_styles = new_styles;
        self.layout_box = Some(lb);
        self.animation_scheduler.clear();
        // Do NOT reset transition_scheduler here: active transitions must survive
        // relayout (viewport resize, DOM mutations) so that in-flight animations
        // continue smoothly. reset happens only on page load (apply_loaded_page).
        self.anim_frame = None;
        self.scroll_y = clamp_scroll(self.scroll_y, self.max_scroll());
        self.scroll_x = clamp_scroll(self.scroll_x, self.max_scroll_x());
        // Notify JS observers about the new layout geometry (ResizeObserver /
        // IntersectionObserver / getBoundingClientRect).
        #[cfg(feature = "quickjs")]
        if let (Some(js), Some(lb_ref)) = (&self.js_ctx, self.layout_box.as_ref()) {
            js.update_layout_rects(collect_layout_rects(lb_ref));
            js.update_viewport_size(viewport.width, viewport.height);
            js.deliver_layout_observers();
            // After fresh rects are in JS: fire lazy-load proximity check.
            // Images that entered the viewport+margin are queued by JS via
            // _lumen_request_lazy_image_load; we drain and fetch them here.
            js.deliver_lazy_images();
            let lazy_reqs = js.take_lazy_image_requests();
            if !lazy_reqs.is_empty() {
                self.fetch_and_register_lazy_images(lazy_reqs);
            }
        }
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Fetch, decode and register lazy images whose node IDs were queued by JS.
    ///
    /// Called from `relayout()` after `_lumen_deliver_lazy_images()` fires load
    /// requests for images that entered the lazy-load proximity margin.
    /// Fetched images are registered in the renderer immediately so the next
    /// repaint (already requested by `relayout`) shows them.
    #[cfg(feature = "quickjs")]
    fn fetch_and_register_lazy_images(&mut self, requests: Vec<(u32, String)>) {
        let base = match &self.source {
            PageSource::File(p) => ResourceBase::File(p.clone()),
            PageSource::Url(u) => ResourceBase::Url(u.clone()),
            PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
            PageSource::Empty => return,
        };
        for (nid, url) in requests {
            let bytes = match fetch_image_bytes(&url, &base, &self.event_sink) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Lazy: пропуск {url}: {e}");
                    continue;
                }
            };
            let image = match lumen_image::decode(&bytes) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Lazy: не декодируется {url}: {e}");
                    continue;
                }
            };
            eprintln!("Lazy загружена: {} ({}×{}, {:?})", url, image.width, image.height, image.format);
            // Apply intrinsic size to DOM so next relayout picks up correct dimensions.
            if let Some(src) = self.layout_source.as_ref() {
                let mut doc = src.document.lock().unwrap();
                let node_id = NodeId::from_index(nid as usize);
                apply_intrinsic_size(&mut doc, node_id, image.width, image.height);
            }
            if let Some(r) = self.renderer.as_mut() {
                if let Err(e) = r.register_image(url.clone(), &image) {
                    eprintln!("Lazy: не зарегистрирована {url}: {e}");
                }
            } else {
                self.pending_images.push((url, image));
            }
        }
    }

    /// Same-page fragment navigation: update `:target` CSS state and scroll to
    /// the target element. `fragment` is the id without the leading `#`; an empty
    /// string scrolls to the top and clears `:target`.
    ///
    /// Triggers a full re-layout so that `:target`-based CSS rules take effect
    /// before the scroll position is calculated.
    fn navigate_fragment(&mut self, fragment: String) {
        if let Some(src) = self.layout_source.as_mut() {
            let mut doc = src.document.lock().unwrap();
            if fragment.is_empty() {
                doc.set_target::<String>(None);
            } else {
                doc.set_target(Some(fragment.clone()));
            }
        }
        // Re-layout so :target cascade is applied.
        self.relayout();
        if fragment.is_empty() {
            self.scroll_to(0.0);
            return;
        }
        let node_id = self
            .layout_source
            .as_ref()
            .and_then(|src| links::find_element_by_id(&src.document.lock().unwrap(), &fragment));
        let target_y = node_id.and_then(|nid| {
            self.layout_box
                .as_ref()
                .and_then(|lb| forms::find_box_rect(lb, nid))
                .map(|r| r.y)
        });
        if let Some(y) = target_y {
            self.scroll_to(y);
        }
    }

    /// Перезагрузить текущий источник: fetch/parse/layout/paint снова. На
    /// `PageSource::Empty` — no-op (грузить нечего). При ошибке — оставляем
    /// предыдущий display_list, печатаем причину в stderr.
    fn reload(&mut self) {
        if matches!(self.source, PageSource::Empty) {
            return;
        }
        println!("Reload: {}", self.source.describe());
        let viewport = self.renderer.as_ref().map_or_else(
            || Size::new(1024.0, 720.0),
            |r| {
                let s = r.viewport_size();
                Size::new(s.width as f32, s.height as f32)
            },
        );
        let ls_store = self.source.origin_str().map(|o| {
            Arc::clone(self.ls_storage.entry(o).or_insert_with(|| {
                Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
            }))
        });
        match self.source.load(self.event_sink.clone(), viewport, ls_store) {
            Ok((page, new_layout_source, new_js_ctx)) => {
                // Drop JS closures before layout_source to release Arc<Mutex<Document>>
                // clones held inside QuickJS closures before LayoutSource's Arc drops.
                self.js_ctx = None;
                self.layout_source = new_layout_source;
                self.js_ctx = new_js_ctx;
                self.content_height = content_height_of(&page.display_list);
                self.content_width = content_width_of(&page.display_list);
                self.display_list = page.display_list;
                self.animation_scheduler.clear();
                self.transition_scheduler = TransitionScheduler::new();
                self.prev_styles.clear();
                collect_box_styles(&page.layout_box, &mut self.prev_styles);
                self.layout_box = Some(page.layout_box);
                // Push initial layout geometry so JS can query bounding rects
                // immediately after page load (before the first relayout).
                #[cfg(feature = "quickjs")]
                if let (Some(js), Some(lb_ref)) = (&self.js_ctx, self.layout_box.as_ref()) {
                    js.update_layout_rects(collect_layout_rects(lb_ref));
                    js.update_viewport_size(viewport.width, viewport.height);
                }
                self.title = page.title;
                self.anim_frame = None;
                // Display list другой → старые match-rect-ы невалидны.
                // Closing полностью сбрасывает query/active — пользователю
                // нужно открыть find заново после reload, что естественно.
                self.find.close();
                self.address_bar.close();
                // Новая страница — показываем сверху-слева.
                self.scroll_y = 0.0;
                self.scroll_x = 0.0;
                // Любой активный drag прерывается (content_height другой,
                // thumb-геометрия пересчитана с нуля).
                self.scroll_drag = None;
                // Активные анимации старой страницы сбрасываем.
                self.scroll_anim = None;
                self.momentum_anim = None;
                self.touchpad_vel = (0.0, 0.0);
                if let Some(r) = self.renderer.as_mut() {
                    // Старая GPU-cache картинок относится к предыдущей странице
                    // (даже если src совпадает, content мог измениться). Чистим
                    // и регистрируем заново.
                    r.clear_images();
                    for (src, image) in &page.images {
                        if let Err(err) = r.register_image(src.clone(), image) {
                            eprintln!("Картинка {src} не зарегистрирована: {err}");
                        }
                    }
                } else {
                    // Renderer ещё не создан — обычно невозможно (reload идёт
                    // по клавише, окно уже есть), но защитимся: складываем в
                    // pending_images, resumed подхватит.
                    self.pending_images = page.images;
                }
                if let Some(w) = self.window.as_ref() {
                    w.set_title(&window_title(self.title.as_deref()));
                    w.request_redraw();
                }
                // JS may have requested navigation via location.href= etc.
                // Store it for processing in about_to_wait (after first render).
                self.pending_js_navigate = page.js_navigate;
            }
            Err(err) => {
                eprintln!("Ошибка reload {}: {err}", self.source.describe());
            }
        }
    }

    /// Запустить background-поток загрузки текущего `source`.
    ///
    /// Поток fetches байты, затем:
    ///
    /// 1. Отправляет `EarlyPreloadHints` из первого STREAM_CHUNK_BYTES байт —
    ///    это даёт sink возможность начать загружать CSS/шрифты ещё до того,
    ///    как main parser дойдёт до `<head>` (HTML LS §13.2.6.4.7).
    /// 2. Разбивает на STREAM_CHUNK_BYTES-кусочки и шлёт `HtmlChunk` через proxy.
    /// 3. По завершении — `LoadDone(raw)` для финального pipeline.
    ///
    /// При ошибке — `LoadError`.
    fn start_streaming_load(&self) {
        if matches!(self.source, PageSource::Empty) {
            return;
        }
        let source = self.source.clone();
        let sink = Arc::clone(&self.event_sink);
        let proxy = self.load_proxy.clone();

        std::thread::spawn(move || {
            let raw = match source.load_bytes(Arc::clone(&sink)) {
                Ok(r) => r,
                Err(e) => {
                    let _ = proxy.send_event(LoadEvent::LoadError(e.to_string()));
                    return;
                }
            };

            // Ранний preload-скан первого chunk-а (обычно содержит весь <head>).
            // Отправляем ДО первого HtmlChunk, чтобы sink начал prefetch
            // сразу, пока парсер ещё не стартовал (real streaming win).
            let scan_end = STREAM_CHUNK_BYTES.min(raw.bytes.len());
            let partial = String::from_utf8_lossy(&raw.bytes[..scan_end]);
            let early = lumen_html_parser::scan_preload_hints(&partial);
            if !early.is_empty() {
                let _ = proxy.send_event(LoadEvent::EarlyPreloadHints(early, raw.base.clone()));
            }

            // Разбить сырые байты на chunk-и. Выравнивание по UTF-8 не нужно:
            // feed_bytes буферизует незавершённые code-point-ы на границах chunk-ов.
            let mut pos = 0;
            while pos < raw.bytes.len() {
                let end = (pos + STREAM_CHUNK_BYTES).min(raw.bytes.len());
                let chunk = raw.bytes[pos..end].to_vec();
                if proxy.send_event(LoadEvent::HtmlChunk(chunk)).is_err() {
                    return; // event loop завершён
                }
                pos = end;
            }
            let _ = proxy.send_event(LoadEvent::LoadDone(raw));
        });
    }

    /// Обновить display list на основе снапшота частичного DOM (без CSS).
    /// Используется для промежуточных кадров во время streaming.
    fn paint_partial_dom(&mut self, doc: &lumen_dom::Document) {
        let Some(renderer) = self.renderer.as_ref() else { return };
        let vp_size = renderer.viewport_size();
        let viewport = Size::new(vp_size.width as f32, vp_size.height as f32);

        let font = match lumen_font::Font::parse(INTER_FONT) {
            Ok(f) => f,
            Err(_) => return,
        };
        let measurer = match lumen_paint::FontMeasurer::new(&font) {
            Ok(m) => m,
            Err(_) => return,
        };

        let empty_sheet = lumen_css_parser::Stylesheet::default();
        let layout = lumen_layout::layout_measured(doc, &empty_sheet, viewport, &measurer);
        let dl = paint_ordered(&layout);

        self.content_height = content_height_of(&dl);
        self.content_width = content_width_of(&dl);
        self.display_list = dl;
        self.layout_box = Some(layout);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Применить результат полного pipeline (fetch + parse + CSS + images).
    /// Используется и при streaming `LoadDone`, и может быть переиспользован
    /// в будущем для других путей загрузки.
    fn apply_loaded_page(&mut self, page: LoadedPage, new_layout_source: Option<LayoutSource>, new_js_ctx: Option<Box<dyn PersistentJs>>) {
        // Drop JS closures before layout_source to release Arc clones in QuickJS.
        self.js_ctx = None;
        self.layout_source = new_layout_source;
        self.js_ctx = new_js_ctx;
        self.content_height = content_height_of(&page.display_list);
        self.content_width = content_width_of(&page.display_list);
        self.display_list = page.display_list;
        self.animation_scheduler.clear();
        self.transition_scheduler = TransitionScheduler::new();
        self.prev_styles.clear();
        collect_box_styles(&page.layout_box, &mut self.prev_styles);
        self.layout_box = Some(page.layout_box);
        self.title = page.title;
        self.anim_frame = None;
        self.find.close();
        self.address_bar.close();
        self.scroll_y = 0.0;
        self.scroll_x = 0.0;
        self.scroll_drag = None;
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.touchpad_vel = (0.0, 0.0);
        self.form_state.clear();
        self.validation_tooltip = None;
        self.color_picker_node = None;
        // Reset paint timing guards so new page fires fresh PerformancePaintTiming entries.
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        if let Some(r) = self.renderer.as_mut() {
            r.set_font_provider(Some(page.font_registry.clone()));
            r.clear_images();
            for (src, image) in &page.images {
                if let Err(err) = r.register_image(src.clone(), image) {
                    eprintln!("Картинка {src} не зарегистрирована: {err}");
                }
            }
        } else {
            self.pending_images = page.images;
        }
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
            w.request_redraw();
        }
        // Register lazy images with JS so _lumen_deliver_lazy_images can check them
        // on subsequent redraws (scroll, resize) via proximity threshold.
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            let pairs: Vec<(u32, &str)> =
                page.lazy_pairs.iter().map(|(n, u)| (*n, u.as_str())).collect();
            js.register_lazy_images(&pairs);
        }
        // JS may have requested navigation via location.href= etc.
        self.pending_js_navigate = page.js_navigate;
    }
}

impl ApplicationHandler<LoadEvent> for Lumen {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(window_title(self.title.as_deref()))
            .with_inner_size(LogicalSize::new(1024.0, 720.0))
            .with_position(LogicalPosition::new(0, 0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Не удалось создать окно: {err}");
                event_loop.exit();
                return;
            }
        };

        let mut renderer = match Renderer::new(window.clone(), INTER_FONT.to_vec()) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Не удалось инициализировать рендер: {err}");
                event_loop.exit();
                return;
            }
        };

        // Заливаем декодированные ранее картинки в GPU. Take, чтобы освободить
        // память Vec (изображение копируется в wgpu Texture внутри register_image).
        for (src, image) in self.pending_images.drain(..) {
            if let Err(err) = renderer.register_image(src.clone(), &image) {
                eprintln!("Картинка {src} не зарегистрирована: {err}");
            }
        }

        self.window = Some(window);
        self.renderer = Some(renderer);

        // Запустить background-загрузку сразу после создания окна —
        // первый кадр (пустой) уже виден, пока идёт fetch/parse.
        // Сбрасываем набор уже отправленных preload-хинтов — новая страница.
        self.preload_dispatched.clear();
        self.start_streaming_load();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: LoadEvent) {
        match event {
            LoadEvent::EarlyPreloadHints(hints, base) => {
                // Ранние хинты из первого chunk — отправить в sink немедленно.
                // `preload_dispatched` запоминает URL, чтобы финальный scan
                // в LoadDone их не дублировал.
                dispatch_preload_hints(&hints, &base, &self.event_sink, &mut self.preload_dispatched);
            }
            LoadEvent::HtmlChunk(chunk) => {
                let builder = self.stream_builder
                    .get_or_insert_with(lumen_html_parser::IncrementalTreeBuilder::new);
                builder.feed_bytes(&chunk);
                if self.stream_last_paint.elapsed().as_millis() >= STREAM_PAINT_INTERVAL_MS {
                    // Клонируем снапшот для layout — builder остаётся живым.
                    let doc_snap = builder.as_doc().clone();
                    self.paint_partial_dom(&doc_snap);
                    self.stream_last_paint = std::time::Instant::now();
                }
            }
            LoadEvent::LoadDone(raw) => {
                eprintln!("Streaming завершён, финальный pipeline");
                self.stream_builder = None;
                let viewport = self.renderer.as_ref().map_or_else(
                    || Size::new(1024.0, 720.0),
                    |r| {
                        let s = r.viewport_size();
                        Size::new(s.width as f32, s.height as f32)
                    },
                );
                let ls_store = ls_store_for_base(&raw.base, &mut self.ls_storage);
                match render_bytes(&raw.bytes, raw.content_type, &raw.base, self.event_sink.clone(), viewport, &mut self.preload_dispatched, ls_store) {
                    Ok((page, new_layout_source, new_js_ctx)) => {
                        self.apply_loaded_page(page, Some(new_layout_source), new_js_ctx);
                    }
                    Err(e) => {
                        eprintln!("Ошибка финального render {}: {e}", self.source.describe());
                    }
                }
            }
            LoadEvent::LoadError(msg) => {
                eprintln!("Ошибка загрузки {}: {msg}", self.source.describe());
                self.stream_builder = None;
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // HTML §8.1.4.2 «Processing model»: между событиями event-loop-а
        // дренируем накопившиеся task-и. Каждый step выполняет одну task +
        // microtask checkpoint. Дренируем все pending tasks за один проход,
        // чтобы UI не отставал. Если task запланирует новую task — она
        // выполнится на следующем about_to_wait (как и `setTimeout(..., 0)`
        // в браузере).
        let mut steps = 0;
        let mut reached_idle = true;
        while self.runtime.step() == runtime::StepResult::Ran {
            steps += 1;
            if steps >= 256 {
                // Защита от runaway: если что-то рекурсивно планирует task в
                // эту же итерацию, не блокируем UI больше чем на 256 task-ов;
                // остаток обработается в следующем about_to_wait.
                reached_idle = false;
                break;
            }
        }

        // W3C `requestIdleCallback` §3: после дренажа очереди task-ов event-loop
        // сообщает «idle window». Phase 0 не знает реального бюджета (нет
        // привязки к vsync), поэтому передаём фиксированные `IDLE_BUDGET_MS`
        // когда дошли до StepResult::Idle. Если упёрлись в cap=256 — есть ещё
        // pending tasks, не idle: передаём 0 ms, чтобы сработали только
        // timeout-callback-и (`request_idle_callback(..., timeout_ms)`).
        // Без этого вызова registered idle-callback-и не получают шанса
        // отработать в принципе.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let remaining_ms = if reached_idle { IDLE_BUDGET_MS } else { 0.0 };
        self.runtime.run_idle_callbacks(remaining_ms, now_ms);

        // JS timers: drain expired setTimeout/setInterval callbacks, then read
        // the next wakeup deadline to schedule ControlFlow::WaitUntil so that
        // winit wakes up exactly when the next timer fires (not only on OS events).
        if let Some(js) = &self.js_ctx {
            js.tick_timers();
            if let Some(nav) = js.take_navigate_request() {
                self.pending_js_navigate = Some(nav);
            }
            if let Some(wakeup_epoch_ms) = js.take_timer_wakeup() {
                let now_epoch_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0);
                let delay_ms = (wakeup_epoch_ms - now_epoch_ms).max(0.0);
                let wakeup = std::time::Instant::now()
                    + std::time::Duration::from_millis(delay_ms as u64 + 1);
                event_loop.set_control_flow(ControlFlow::WaitUntil(wakeup));
            }
        }

        // Пост-дренажный check: reload, запланированный через queue_task
        // (UserInteraction source), исполняется после microtask checkpoint.
        // `take` атомарно сбрасывает флаг, чтобы reload вызвался только раз.
        if self.pending_reload.take() {
            self.reload();
        }

        // JS navigation: location.href=, assign(), replace(), reload().
        // Executed after the initial page render so the user sees something
        // before the redirect completes (matches browser behaviour).
        if let Some(nav) = self.pending_js_navigate.take() {
            match nav {
                JsNavigateRequest::Push(url)    => self.navigate_to(PageSource::Url(url)),
                JsNavigateRequest::Replace(url) => self.navigate_replace(PageSource::Url(url)),
                JsNavigateRequest::Reload       => self.reload(),
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.save_session_on_close();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                // Windows fires Resized(0, 0) when the window is minimized.
                // Skip resize + relayout entirely — the layout stays valid at
                // the last non-zero size and will be refreshed on restore.
                if size.width == 0 || size.height == 0 {
                    return;
                }
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
                self.relayout();
                // HTML §8.1.5.1, шаг 13: ResizeObserver delivery.
                // JS-observers are delivered inside relayout() via deliver_layout_observers().
                // The shell runtime.deliver_observer_records delivers Rust-level observers.
                self.runtime
                    .deliver_observer_records(runtime::ObserverKind::Resize);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Окно перетащили на монитор с другим DPI. Surface не пересоздаём —
                // winit отдаст новый physical inner_size через последующий
                // `WindowEvent::Resized`; здесь только обновляем коэффициент,
                // по которому shader делит координаты, чтобы 1 CSS px остался
                // равен scale_factor device px.
                if let Some(r) = self.renderer.as_mut() {
                    r.set_scale_factor(scale_factor);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = winit_modifiers_state(&new_mods);
            }
            WindowEvent::KeyboardInput { event: ref key_event, .. } => {
                self.handle_key(event_loop, key_event);
            }
            WindowEvent::Ime(ref ime_event) => {
                self.handle_ime(ime_event);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some(position);
                self.update_cursor_icon();
                // Активный drag — пересчитать scroll по новой позиции.
                if let Some(drag) = self.scroll_drag {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let cursor_y_css = (position.y as f32) / dpr;
                    let target = drag.scroll_for(
                        cursor_y_css,
                        self.content_height,
                        self.viewport_height_css(),
                    );
                    self.scroll_to(target);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                // Драг продолжается даже когда курсор вышел из окна — winit
                // продолжит слать CursorMoved-события за пределами client area,
                // пока зажата кнопка. Сбросим drag только на MouseInput Release
                // или если события прекратятся (мы не получим MouseInput, но
                // повторный CursorEntered/CursorMoved оживят drag — допустимо
                // для Phase 0).
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button != MouseButton::Left {
                    // Phase 0: только левая кнопка управляет drag-ом scrollbar-а.
                    // Middle / right / back / forward — пропускаем.
                } else if state == ElementState::Pressed {
                    let Some(cursor) = self.cursor_position else {
                        // Без CursorMoved-snapshot-а до Press — не знаем где
                        // клик; bail out. Реалистично — Press всегда приходит
                        // после CursorMoved, но защитимся.
                        return;
                    };
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let x_css = (cursor.x as f32) / dpr;
                    let y_css = (cursor.y as f32) / dpr;
                    let vh = self.viewport_height_css();
                    match scrollbar::classify_track_click(
                        x_css,
                        y_css,
                        self.scroll_y,
                        self.content_height,
                        self.viewport_width_css(),
                        vh,
                    ) {
                        scrollbar::TrackClick::Thumb => {
                            self.scroll_drag = Some(scrollbar::ScrollDrag::new(
                                self.scroll_y,
                                y_css,
                            ));
                        }
                        scrollbar::TrackClick::Above => {
                            // Клик по track выше thumb-а — прыжок на страницу вверх.
                            self.scroll_by_smooth(-page_step(vh));
                        }
                        scrollbar::TrackClick::Below => {
                            // Клик по track ниже thumb-а — прыжок на страницу вниз.
                            self.scroll_by_smooth(page_step(vh));
                        }
                        scrollbar::TrackClick::None => {
                            // Dismiss validation tooltip on any non-scrollbar click.
                            self.validation_tooltip = None;
                            let scroll_y = self.scroll_y;

                            // ── Color picker swatch hit ──────────────────────
                            // Check if click lands on an open color picker swatch.
                            // Compute swatch result inside a scoped borrow, then act.
                            let picker_swatch_result: Option<(NodeId, [u8; 3])> = {
                                let picker_node = self.color_picker_node;
                                picker_node.and_then(|pn| {
                                    let anchor = forms::find_box_rect(
                                        self.layout_box.as_ref()?,
                                        pn,
                                    )?;
                                    let color = forms::hit_color_swatch(
                                        anchor, scroll_y, x_css, y_css,
                                    )?;
                                    Some((pn, color))
                                })
                            };
                            if let Some((pn, color)) = picker_swatch_result {
                                self.color_picker_node = None;
                                let css_color = forms::swatch_to_css_color(color);
                                if let Some(src) = self.layout_source.as_mut() {
                                    forms::set_value(&mut src.document.lock().unwrap(), pn, &css_color);
                                }
                                self.form_state.entry(pn).or_default().value = css_color;
                                self.relayout();
                                return;
                            }
                            // Any click outside the picker closes it.
                            self.color_picker_node = None;

                            // ── Form control + link click ────────────────────
                            // Single hit test shared by form dispatch and link navigation.
                            let page_x = x_css + self.scroll_x;
                            let page_y = y_css + self.scroll_y;
                            let hit_result = self.layout_box.as_ref().and_then(|lb| {
                                hit_test(Point::new(page_x, page_y), lb)
                            });
                            // Dispatch JS click event (bubbles from hit node to document).
                            if let (Some(result), Some(ctx)) =
                                (hit_result.as_ref(), self.js_ctx.as_ref())
                            {
                                let script = format!(
                                    "_lumen_dispatch_bubble({}, 'click')",
                                    result.node.index()
                                );
                                ctx.eval_js(&script);
                                if let Some(nav) = ctx.take_navigate_request() {
                                    self.pending_js_navigate = Some(nav);
                                }
                            }
                            let form_action: forms::FormClickAction =
                                if let (Some(result), Some(src)) =
                                    (hit_result.as_ref(), self.layout_source.as_ref())
                                {
                                    forms::classify_click(&src.document.lock().unwrap(), result.node)
                                } else {
                                    forms::FormClickAction::Nothing
                                };
                            match form_action {
                                forms::FormClickAction::ToggleCheckbox(id) => {
                                    if let Some(src) = self.layout_source.as_mut() {
                                        forms::toggle_checkbox(&mut src.document.lock().unwrap(), id);
                                    }
                                    self.relayout();
                                }
                                forms::FormClickAction::ToggleRadio {
                                    clicked,
                                    _group_name: _,
                                } => {
                                    if let Some(src) = self.layout_source.as_mut() {
                                        forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                                    }
                                    self.relayout();
                                }
                                forms::FormClickAction::OpenColorPicker(id) => {
                                    self.color_picker_node = Some(id);
                                    if let Some(w) = self.window.as_ref() {
                                        w.request_redraw();
                                    }
                                }
                                forms::FormClickAction::SubmitForm(submit_node) => {
                                    let err = {
                                        match (&self.layout_box, &self.layout_source) {
                                            (Some(lb), Some(src)) => {
                                                forms::find_validation_error(
                                                    lb,
                                                    &src.document.lock().unwrap(),
                                                    &self.form_state,
                                                )
                                            }
                                            _ => None,
                                        }
                                    };
                                    if let Some((_node, rect, msg)) = err {
                                        self.validation_tooltip = Some((rect, msg));
                                        if let Some(w) = self.window.as_ref() {
                                            w.request_redraw();
                                        }
                                    } else if let Some(src) = self.layout_source.as_ref() {
                                        let doc = src.document.lock().unwrap();
                                        if let Some((action, method, body)) =
                                            forms::build_form_submit(&doc, submit_node, &self.form_state)
                                        {
                                            use lumen_core::event::{Event, TabId};
                                            self.event_sink.emit(&Event::FormSubmit {
                                                tab_id: TabId(0),
                                                action: action.clone(),
                                                method: method.clone(),
                                                body: body.clone(),
                                            });
                                            match method.as_str() {
                                                "get" => {
                                                    // HTML LS §form-submission step 23: navigate
                                                    // to action + query-string.
                                                    let get_url = forms::make_get_url(&action, &body);
                                                    let resolved = self.source.resolve_href(&get_url);
                                                    drop(doc);
                                                    self.navigate_to(PageSource::from_arg(Some(&resolved)));
                                                }
                                                _ => {
                                                    // POST: emit event; real network send is P3 task.
                                                    eprintln!(
                                                        "[forms] POST {action} body={body}"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                forms::FormClickAction::Nothing => {
                                    // ── Link click ───────────────────────────
                                    // No form control was activated — check if
                                    // the clicked node is inside an <a href>.
                                    let href = hit_result.as_ref().and_then(|r| {
                                        self.layout_source
                                            .as_ref()
                                            .and_then(|src| links::find_link_href(&src.document.lock().unwrap(), r.node))
                                    });
                                    if let Some(href) = href {
                                        if let Some(frag) = links::fragment_only(&href) {
                                            // Same-page fragment navigation.
                                            self.navigate_fragment(frag.to_owned());
                                        } else if links::is_navigable_href(&href) {
                                            let resolved = self.source.resolve_href(&href);
                                            let target = PageSource::from_arg(Some(&resolved));
                                            self.navigate_to(target);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Released — завершаем drag (если он был).
                    self.scroll_drag = None;
                    // Курсор был «зафиксирован» как Pointer пока тянули
                    // thumb; теперь пересчитаем по hover-точке текущего
                    // положения курсора (CursorMoved-event на release сам
                    // не приходит, поэтому делаем вручную).
                    self.update_cursor_icon();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                // winit отдаёт два типа дельты:
                // - LineDelta(cols, lines): mouse wheel notch, нет momentum.
                // - PixelDelta({x, y}): тачпад, device px, делим на DPR.
                //   Отслеживаем velocity для momentum при TouchPhase::Ended.
                // Y: winit y > 0 — wheel up → scroll_y -= delta.
                // X: winit x > 0 — wheel left → scroll_x -= delta.
                // Shift+вертикальный wheel → горизонтальный скролл.
                let dpr = self
                    .renderer
                    .as_ref()
                    .map_or(1.0_f32, |r| r.scale_factor() as f32);
                let shift = self.modifiers.shift_key();
                match delta {
                    MouseScrollDelta::LineDelta(cols, lines) => {
                        // Mouse wheel: дискретные тики, momentum не нужен.
                        self.momentum_anim = None;
                        self.touchpad_vel = (0.0, 0.0);
                        let dx = -cols * 40.0;
                        let dy = -lines * 40.0;
                        let (dx_css, dy_css) = if shift { (dy, 0.0) } else { (dx, dy) };
                        if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                        self.scroll_by_smooth(dy_css);
                    }
                    MouseScrollDelta::PixelDelta(p) => {
                        let raw_x = -(p.x as f32) / dpr.max(1e-6);
                        let raw_y = -(p.y as f32) / dpr.max(1e-6);
                        let (dx_css, dy_css) = if shift { (raw_y, 0.0) } else { (raw_x, raw_y) };

                        match phase {
                            TouchPhase::Ended | TouchPhase::Cancelled => {
                                // Палец снят: запускаем momentum если есть
                                // скорость (фаза Ended) или сбрасываем (Cancelled).
                                if phase == TouchPhase::Ended {
                                    let (vx, vy) = self.touchpad_vel;
                                    if vx.abs() + vy.abs() >= momentum_anim::MIN_VELOCITY_PX_MS {
                                        let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
                                        self.momentum_anim =
                                            Some(momentum_anim::MomentumAnim::new(vy, vx, now));
                                        self.request_redraw();
                                    }
                                }
                                self.touchpad_vel = (0.0, 0.0);
                            }
                            TouchPhase::Started => {
                                // Новый жест: сбросить momentum и velocity.
                                self.momentum_anim = None;
                                self.touchpad_vel = (0.0, 0.0);
                                let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
                                self.touchpad_vel_time_ms = now;
                                if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                                self.scroll_by_smooth(dy_css);
                            }
                            TouchPhase::Moved => {
                                // Палец движется: обновляем scroll и velocity (EWMA).
                                let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
                                let dt = (now - self.touchpad_vel_time_ms).max(1.0);
                                self.touchpad_vel_time_ms = now;
                                // EWMA alpha = 0.6: быстро следует за движением,
                                // сглаживает дрожание.
                                const ALPHA: f32 = 0.6;
                                let inst_x = dx_css / dt as f32;
                                let inst_y = dy_css / dt as f32;
                                let (vx, vy) = self.touchpad_vel;
                                self.touchpad_vel = (
                                    ALPHA * inst_x + (1.0 - ALPHA) * vx,
                                    ALPHA * inst_y + (1.0 - ALPHA) * vy,
                                );
                                if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                                self.scroll_by_smooth(dy_css);
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // HTML §8.1.5.1 «Update the rendering» — spec-correct order:
                //   1. scroll             ← advance_scroll_anim + advance_momentum
                //   2. CSS Animations + Transitions tick  (spec: update animations before rAF)
                //   3. rAF callbacks      ← runtime.run_rendering_step + JS run_animation_frame
                //   4. layout invalidation ← relayout() if dom_dirty after rAF
                //      → deliver_layout_observers() (ResizeObserver + IntersectionObserver)
                //   5. paint timing       ← PerformanceObserver 'paint' entries
                //   6. paint              ← r.render(...)
                //
                // Scroll before CSS/rAF so callbacks read current scroll position.
                // CSS animations/transitions before rAF: spec §8.1.5.1 step «update
                // animations and send events» precedes «run animation frame callbacks».
                let timestamp_ms =
                    self.epoch.elapsed().as_secs_f64() * 1000.0;

                // Step 1: scroll update.
                if self.advance_scroll_anim() {
                    self.request_redraw();
                }
                if self.advance_momentum(timestamp_ms) {
                    self.request_redraw();
                }

                // Step 2: CSS Animations + Transitions tick (spec order: before rAF).
                // Both schedulers are ticked once per frame and merged into a single
                // AnimationFrame. Transition values override @keyframes when both apply.
                if let (Some(lb), Some(src)) = (&self.layout_box, &self.layout_source) {
                    let mut frame = self.animation_scheduler.tick(
                        timestamp_ms,
                        lb,
                        &src.stylesheet,
                    );
                    let now_s = (timestamp_ms / 1000.0) as f32;
                    let trans_frame = self.transition_scheduler.tick(now_s);
                    frame.merge_from(trans_frame);
                    if frame.has_active {
                        self.request_redraw();
                    }
                    self.anim_frame = if frame.overrides.is_empty() { None } else { Some(frame) };
                }

                // Step 3: rAF callbacks + microtask checkpoint.
                self.runtime.run_rendering_step(timestamp_ms);

                // Step 3.1: JS requestAnimationFrame callbacks.
                // Snapshot-pattern: callbacks registered during this call go into
                // the next frame. If any new rAF was registered (animation loop),
                // request another redraw immediately.
                if let Some(js) = &self.js_ctx {
                    js.run_animation_frame(timestamp_ms);
                    if js.take_raf_pending() {
                        self.request_redraw();
                    }
                }

                // Step 4: layout invalidation — если rAF-callback изменил DOM
                // (setAttribute/textContent/appendChild/etc.), делаем relayout
                // прежде чем красить, чтобы paint отражал актуальный DOM.
                // relayout() also delivers ResizeObserver + IntersectionObserver.
                if self.js_ctx.as_ref().is_some_and(|j| j.take_dom_dirty()) {
                    self.relayout();
                }

                // Step 5: PerformancePaintTiming (W3C Paint Timing §2).
                // Delivered once per page load; subsequent frames skip this block.
                // first-paint = first frame with any painted pixel (non-default bg).
                // first-contentful-paint = first frame with text, image, canvas, etc.
                // Phase 0: both fire on the first non-empty display list since
                // a page load. A page load resets both flags in apply_loaded_page.
                #[cfg(feature = "quickjs")]
                if let Some(js) = &self.js_ctx {
                    let has_content = !self.display_list.is_empty();
                    if has_content && !self.first_paint_delivered {
                        self.first_paint_delivered = true;
                        js.deliver_paint_timing("first-paint", timestamp_ms);
                    }
                    if has_content && !self.first_contentful_paint_delivered {
                        self.first_contentful_paint_delivered = true;
                        js.deliver_paint_timing("first-contentful-paint", timestamp_ms);
                    }
                }

                // Step 6 (paint): build display list buffers and call renderer.
                // Page-полоса: исходный display list + highlight-FillRect-ы
                // перед своими DrawText (когда find открыт). Прокручивается.
                // Overlay-полоса: find-bar + scrollbar — viewport-locked.
                // Без find — page = self.display_list, overlay = только scrollbar.
                let (page_buf, mut overlay_buf): (Option<lumen_paint::DisplayList>, lumen_paint::DisplayList) =
                    if self.find.is_open() {
                        let win_size = self.window.as_ref().map_or((1024, 720), |w| {
                            let s = w.inner_size();
                            (s.width, s.height)
                        });
                        let matches = self.current_matches();
                        let page = find::build_page_with_highlights(
                            &self.display_list,
                            &self.find,
                            &matches,
                        );
                        let bar = find::build_bar_overlay(
                            &self.find,
                            matches.len(),
                            find::BarOverlay { window_size: win_size },
                        );
                        (Some(page), bar)
                    } else {
                        (None, Vec::new())
                    };

                // Scrollbar встаёт перед find-bar в overlay-буфере: рисуется
                // первым = находится под find-bar-ом в painter's order. Они не
                // пересекаются по x (bar занимает левее `ww - 12`, scrollbar
                // справа от `ww - 8`), так что фактического overdraw нет.
                // --no-scrollbar подавляет полосу для screenshot-пайплайна.
                if !self.no_scrollbar {
                    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(
                        self.scroll_y,
                        self.content_height,
                        self.viewport_width_css(),
                        self.viewport_height_css(),
                    );
                    if !scrollbar_cmds.is_empty() {
                        let mut combined = scrollbar_cmds;
                        combined.append(&mut overlay_buf);
                        overlay_buf = combined;
                    }
                }

                // Forms: validation tooltip and color picker overlays.
                let vp_w = self.viewport_width_css();
                if let Some((anchor, msg)) = &self.validation_tooltip {
                    let mut tt = forms::build_validation_tooltip(
                        *anchor, msg, self.scroll_y, vp_w,
                    );
                    tt.append(&mut overlay_buf);
                    overlay_buf = tt;
                }
                if let (Some(picker_node), Some(lb)) =
                    (self.color_picker_node, &self.layout_box)
                    && let Some(anchor) = forms::find_box_rect(lb, picker_node)
                {
                    let mut picker = forms::build_color_picker(anchor, self.scroll_y, vp_w);
                    picker.append(&mut overlay_buf);
                    overlay_buf = picker;
                }

                // Адресная строка (Ctrl+L) — рисуется поверх всего остального.
                if self.address_bar.is_open() {
                    let win_size = self.window.as_ref().map_or((1024, 720), |w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    });
                    let mut bar = address_bar::build_bar_overlay(
                        &self.address_bar,
                        address_bar::BarOverlay { window_size: win_size },
                    );
                    bar.append(&mut overlay_buf);
                    overlay_buf = bar;
                }

                // Compositor offload: если есть активные анимации с opacity/transform —
                // пересобираем display list из layout_box с overrides, минуя relayout.
                // color/background-color остаются в anim_frame на будущее (требуют relayout).
                let anim_dl: Option<lumen_paint::DisplayList> =
                    if let (Some(frame), Some(lb)) = (&self.anim_frame, &self.layout_box) {
                        let comp = frame.to_compositor_frame();
                        if !comp.is_empty() {
                            let tree = StackingTree::build(lb);
                            let order = PaintOrder::from_tree(&tree);
                            Some(build_display_list_ordered_with_anim(lb, &tree, &order, Some(&comp)))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                let scroll_y = self.scroll_y;
                let scroll_x = self.scroll_x;

                // Hint overlay: viewport-locked бейджи kbd-навигации.
                // Добавляются последними → рисуются поверх scrollbar/tooltip.
                if self.hint.is_active() {
                    let mut hint_cmds = hints::build_hints_overlay(&self.hint, scroll_x, scroll_y);
                    overlay_buf.append(&mut hint_cmds);
                }

                if let Some(r) = self.renderer.as_mut() {
                    // Priority: animated DL > find-highlighted DL > base DL.
                    let page: &[lumen_paint::DisplayCommand] = anim_dl
                        .as_deref()
                        .or(page_buf.as_deref())
                        .unwrap_or(&self.display_list);
                    if let Err(err) = r.render(page, &overlay_buf, scroll_y, scroll_x) {
                        eprintln!("Ошибка рендера: {err:?}");
                    }
                }
            }
            _ => {}
        }
    }
}

impl Lumen {
    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = key_event.physical_key else {
            return;
        };

        // Адресная строка (Ctrl+L) перехватывает ввод первой: Esc=close,
        // Enter=navigate, Backspace=удалить символ, иначе — текст URL.
        if self.address_bar.is_open() {
            self.handle_address_bar_key(code, key_event, event_loop);
            return;
        }

        // Когда find bar открыт — все клавиши идут в него: ввод символов,
        // Esc=close, Backspace=стирание, Enter/F3=next (Shift=prev). Это не
        // даёт случайно сработать Esc=Exit или Ctrl+R=Reload в момент поиска.
        if self.find.is_open() {
            self.handle_find_key(code, key_event);
            return;
        }

        // Hint-режим: все клавиши идут в него пока активен.
        // Esc=close, буква=сужение/активация хинта.
        if self.hint.is_active() {
            self.handle_hint_key(code, key_event);
            return;
        }

        let Some(cmd) = keybinding_for(code, self.modifiers) else {
            return;
        };
        // Scroll-команды разрешаем на repeat (auto-repeat при удержании),
        // остальные — только на первое нажатие.
        let is_scroll = matches!(
            cmd,
            KeyCommand::ScrollLineDown
                | KeyCommand::ScrollLineUp
                | KeyCommand::ScrollPageDown
                | KeyCommand::ScrollPageUp
                | KeyCommand::ScrollHome
                | KeyCommand::ScrollEnd
                | KeyCommand::ScrollLineRight
                | KeyCommand::ScrollLineLeft
        );
        if key_event.repeat && !is_scroll {
            return;
        }
        match cmd {
            KeyCommand::Reload => {
                // HTML §8.1.4 «Event loop»: пользовательские действия (reload)
                // планируются через UserInteraction task source, а не вызываются
                // напрямую. `pending_reload` — флаг-мост: closure-задача может
                // быть `+ 'static`, Lumen — нет; Cell позволяет из замыкания
                // установить флаг, который `about_to_wait` проверяет и вызывает
                // `reload()` после дренажа очереди.
                let flag = Rc::clone(&self.pending_reload);
                self.runtime.handle().queue_task(
                    runtime::TaskSource::UserInteraction,
                    move || { flag.set(true); },
                );
            }
            KeyCommand::Exit => event_loop.exit(),
            KeyCommand::FindOpen => {
                self.hint.close();
                self.find.open();
                self.request_redraw();
            }
            KeyCommand::OpenAddressBar => {
                self.hint.close();
                let current = self.source.url_str().unwrap_or("").to_owned();
                self.address_bar.open(&current);
                self.request_redraw();
            }
            KeyCommand::HintModeOpen => {
                if let (Some(lb), Some(src)) =
                    (self.layout_box.as_ref(), self.layout_source.as_ref())
                {
                    let doc = src.document.lock().unwrap();
                    let elements = lumen_layout::collect_clickable_elements(lb, &doc);
                    drop(doc);
                    if !elements.is_empty() {
                        self.hint.open(elements);
                        self.request_redraw();
                    }
                }
            }
            KeyCommand::HistoryBack => self.navigate_back(),
            KeyCommand::HistoryForward => self.navigate_forward(),
            KeyCommand::ScrollLineDown => self.scroll_by_smooth(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineUp => self.scroll_by_smooth(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineRight => self.scroll_x_by(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineLeft => self.scroll_x_by(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollPageDown => {
                let vh = self.viewport_height_css();
                self.scroll_by_smooth(page_step(vh));
            }
            KeyCommand::ScrollPageUp => {
                let vh = self.viewport_height_css();
                self.scroll_by_smooth(-page_step(vh));
            }
            KeyCommand::ScrollHome => self.start_smooth_scroll(0.0),
            KeyCommand::ScrollEnd => self.start_smooth_scroll(f32::INFINITY),
        }
    }

    /// Сохранить текущую страницу в bfcache и стек навигации,
    /// затем загрузить `source` как новую страницу.
    /// Очищает `nav_fwd` (аналог браузера при навигации вперёд из середины истории).
    fn navigate_to(&mut self, source: PageSource) {
        self.hint.close();
        // Snapshot current page into bfcache if it has an HTML source.
        if let Some(ref ls) = self.layout_source
            && let Some(ref html) = ls.html_source
            && let Some(url) = self.source.url_str()
        {
            self.bfcache.store(BfCacheEntry {
                url: url.to_owned(),
                html: html.clone(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                title: self.title.clone(),
            });
        }
        // Push current page to back stack.
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
        });
        // New navigation invalidates forward history.
        self.nav_fwd.clear();
        // Load new page.
        self.source = source;
        self.reload();
    }

    /// Перейти на `source`, заменяя текущую запись истории (без push в back-stack).
    /// Аналог `history.replaceState` / `location.replace()` в браузере.
    fn navigate_replace(&mut self, source: PageSource) {
        // New navigation invalidates forward history but does NOT push to back stack.
        self.nav_fwd.clear();
        self.source = source;
        self.reload();
    }

    /// Перейти на предыдущую страницу в истории (Alt+Left).
    fn navigate_back(&mut self) {
        let Some(prev) = self.nav_back.pop() else { return };
        // Save current page to forward stack.
        self.nav_fwd.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
        });
        // Try bfcache first.
        let restored_scroll = if let Some(url) = prev.source.url_str() {
            if let Some(entry) = self.bfcache.retrieve(url) {
                let html = entry.html.clone();
                let scroll_x = entry.scroll_x;
                let scroll_y = entry.scroll_y;
                let base_url = url.to_owned();
                self.source = PageSource::Snapshot { html, base_url };
                Some((scroll_x, scroll_y))
            } else {
                self.source = prev.source;
                None
            }
        } else {
            self.source = prev.source;
            None
        };
        self.reload();
        // Restore scroll position from bfcache (or from nav entry if no bfcache hit).
        let (sx, sy) = restored_scroll.unwrap_or((prev.scroll_x, prev.scroll_y));
        self.scroll_x = sx;
        self.scroll_y = sy;
        if let Some(w) = self.window.as_ref() { w.request_redraw(); }
    }

    /// Перейти на следующую страницу в истории (Alt+Right).
    fn navigate_forward(&mut self) {
        let Some(next) = self.nav_fwd.pop() else { return };
        // Save current page to back stack.
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
        });
        // Try bfcache first.
        let restored_scroll = if let Some(url) = next.source.url_str() {
            if let Some(entry) = self.bfcache.retrieve(url) {
                let html = entry.html.clone();
                let scroll_x = entry.scroll_x;
                let scroll_y = entry.scroll_y;
                let base_url = url.to_owned();
                self.source = PageSource::Snapshot { html, base_url };
                Some((scroll_x, scroll_y))
            } else {
                self.source = next.source;
                None
            }
        } else {
            self.source = next.source;
            None
        };
        self.reload();
        let (sx, sy) = restored_scroll.unwrap_or((next.scroll_x, next.scroll_y));
        self.scroll_x = sx;
        self.scroll_y = sy;
        if let Some(w) = self.window.as_ref() { w.request_redraw(); }
    }

    fn handle_ime(&mut self, ime: &Ime) {
        use lumen_core::event::{Event, TabId};
        let tab_id = TabId(0);
        match ime {
            Ime::Enabled => {
                // Не диспатчим compositionstart сразу — ждём первый Preedit
                // с текстом (браузеры так же: событие только когда есть данные).
            }
            Ime::Preedit(text, _cursor) if text.is_empty() => {
                // Пустой preedit = конец composition без Commit (отмена).
                if self.ime_composing.take().is_some() {
                    self.event_sink
                        .emit(&Event::ImeCompositionEnded { tab_id, data: String::new() });
                }
            }
            Ime::Preedit(text, _cursor) => {
                if self.ime_composing.is_none() {
                    // Первый непустой preedit — начало composition.
                    self.event_sink
                        .emit(&Event::ImeCompositionStarted { tab_id });
                }
                self.ime_composing = Some(text.clone());
                self.event_sink.emit(&Event::ImeCompositionUpdated {
                    tab_id,
                    data: text.clone(),
                });
            }
            Ime::Commit(text) => {
                // Commit приходит после пустого Preedit (winit гарантирует),
                // но на случай если нет — сбрасываем composing сами.
                self.ime_composing = None;
                self.event_sink.emit(&Event::ImeCompositionEnded {
                    tab_id,
                    data: text.clone(),
                });
            }
            Ime::Disabled => {
                // IME деактивирован. Если composition была открыта — закрываем.
                if self.ime_composing.take().is_some() {
                    self.event_sink
                        .emit(&Event::ImeCompositionEnded { tab_id, data: String::new() });
                }
            }
        }
    }

    fn handle_address_bar_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let _ = event_loop;
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.address_bar.close();
                self.request_redraw();
            }
            KeyCode::Enter if !key_event.repeat => {
                self.address_bar.commit();
                if let Some(value) = self.address_bar.take_commit() {
                    // Записываем в search_history если это не URL.
                    if !value.contains("://") && !value.starts_with('@') {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        let _ = self.search_history.record(&value, now);
                    }
                    self.navigate_to(PageSource::from_arg(Some(&value)));
                }
            }
            KeyCode::ArrowDown if !key_event.repeat => {
                self.address_bar.select_next();
                self.request_redraw();
            }
            KeyCode::ArrowUp if !key_event.repeat => {
                self.address_bar.select_prev();
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.address_bar.backspace();
                let sugg = self.query_omnibox_suggestions();
                self.address_bar.set_suggestions(sugg);
                self.request_redraw();
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                {
                    self.address_bar.append_str(text);
                    let sugg = self.query_omnibox_suggestions();
                    self.address_bar.set_suggestions(sugg);
                    self.request_redraw();
                }
            }
        }
    }

    /// Запрашивает подсказки для текущего ввода в адресной строке.
    ///
    /// `@history <query>` → FTS5-поиск по истории страниц.
    /// Обычный ввод → prefix-match по search_history + FTS5.
    fn query_omnibox_suggestions(&self) -> Vec<address_bar::OmniboxSuggestion> {
        use address_bar::{OmniboxPrefix, OmniboxSuggestion, parse_omnibox_prefix};

        let input = self.address_bar.input();
        if input.is_empty() {
            return Vec::new();
        }

        let (prefix, query) = parse_omnibox_prefix(input);
        let mut suggestions = Vec::new();

        match prefix {
            OmniboxPrefix::History => {
                // @history <query> — только FTS.
                if !query.is_empty() && let Ok(hits) = self.history_fts.search(query, 7) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: hit.snippet,
                        });
                    }
                }
            }
            OmniboxPrefix::Plain => {
                // prefix-match по search_history (до 4 строк).
                if let Ok(queries) = self.search_history.prefix_match(query, 4) {
                    for q in queries {
                        suggestions.push(OmniboxSuggestion::SearchQuery {
                            query: q.query,
                            frequency: q.frequency,
                        });
                    }
                }
                // FTS5 по истории страниц (до 4 строк, итого ≤ 8).
                if let Ok(hits) = self.history_fts.search(query, 4) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: hit.snippet,
                        });
                    }
                }
            }
        }

        suggestions
    }

    fn handle_find_key(&mut self, code: KeyCode, key_event: &KeyEvent) {
        let shift = self.modifiers.shift_key();
        let ctrl_or_super = self.modifiers.control_key() || self.modifiers.super_key();

        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.find.close();
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.find.backspace();
                self.scroll_to_active_match();
                self.request_redraw();
            }
            KeyCode::Enter | KeyCode::F3 => {
                if !key_event.repeat {
                    let total = self.current_matches().len();
                    if shift {
                        self.find.prev(total);
                    } else {
                        self.find.next(total);
                    }
                    self.scroll_to_active_match();
                    self.request_redraw();
                }
            }
            // Ctrl+R — переключить plain-text ↔ regex режим.
            KeyCode::KeyR if ctrl_or_super && !key_event.repeat => {
                self.find.toggle_regex_mode();
                self.scroll_to_active_match();
                self.request_redraw();
            }
            _ => {
                // Текстовый ввод. При модификаторах Ctrl/Cmd не вставляем —
                // это shortcut в адрес find-а (или будущих чего-то ещё), не
                // символ для query. Без них text — это уже layout-aware
                // символ от winit, с учётом IME / dead-keys.
                if ctrl_or_super {
                    return;
                }
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                {
                    self.find.append_str(text);
                    self.scroll_to_active_match();
                    self.request_redraw();
                }
            }
        }
    }

    /// Обрабатывает клавишный ввод пока hint-режим активен.
    ///
    /// `Escape` — закрыть overlay. Любой одиночный символ (строчный ASCII) —
    /// передаётся в `HintState::push_char`; при уникальном совпадении вызывается
    /// `activate_node`. Нераспознанные клавиши игнорируются.
    fn handle_hint_key(&mut self, code: KeyCode, key_event: &KeyEvent) {
        if matches!(code, KeyCode::Escape) && !key_event.repeat {
            self.hint.close();
            self.request_redraw();
            return;
        }
        if let Some(text) = key_event.text.as_ref() {
            for c in text.chars() {
                if c.is_ascii_lowercase() {
                    match self.hint.push_char(c) {
                        hints::HintResult::Activate(node_id) => {
                            self.activate_node(node_id);
                        }
                        hints::HintResult::Partial | hints::HintResult::NoMatch => {}
                    }
                    self.request_redraw();
                    break;
                }
            }
        }
    }

    /// Активировать DOM-узел `node_id` как будто по нему кликнули мышью.
    ///
    /// Диспатчит JS click-событие, обрабатывает form-действие (checkbox/radio),
    /// и навигирует по ссылке если узел внутри `<a href>`. Используется
    /// hint-режимом для активации элемента без участия мыши.
    fn activate_node(&mut self, node_id: NodeId) {
        // JS click dispatch (bubbling от узла до document).
        #[cfg(feature = "quickjs")]
        if let Some(ctx) = self.js_ctx.as_ref() {
            let script = format!("_lumen_dispatch_bubble({}, 'click')", node_id.index());
            ctx.eval_js(&script);
            if let Some(nav) = ctx.take_navigate_request() {
                self.pending_js_navigate = Some(nav);
            }
        }
        // Form action classification.
        let form_action = if let Some(src) = self.layout_source.as_ref() {
            forms::classify_click(&src.document.lock().unwrap(), node_id)
        } else {
            forms::FormClickAction::Nothing
        };
        match form_action {
            forms::FormClickAction::ToggleCheckbox(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), id);
                }
                self.relayout();
            }
            forms::FormClickAction::ToggleRadio { clicked, .. } => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                }
                self.relayout();
            }
            forms::FormClickAction::OpenColorPicker(id) => {
                self.color_picker_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::SubmitForm(_) | forms::FormClickAction::Nothing => {
                // Link navigation.
                let href = self.layout_source.as_ref().and_then(|src| {
                    links::find_link_href(&src.document.lock().unwrap(), node_id)
                });
                if let Some(href) = href {
                    if let Some(frag) = links::fragment_only(&href) {
                        self.navigate_fragment(frag.to_owned());
                    } else if links::is_navigable_href(&href) {
                        let resolved = self.source.resolve_href(&href);
                        self.navigate_to(PageSource::from_arg(Some(&resolved)));
                    }
                }
            }
        }
    }

    /// Если активный match вне видимой части viewport-а — сдвигает scroll так,
    /// чтобы он попал в верхнюю четверть окна. Вызывается после любого
    /// действия, меняющего active match: next/prev, backspace, текстовый ввод.
    /// При закрытом баре / пустом query / отсутствии матчей — no-op.
    fn scroll_to_active_match(&mut self) {
        let matches = self.current_matches();
        if matches.is_empty() {
            return;
        }
        let active = self.find.active_index();
        let Some(m) = matches.get(active) else {
            return;
        };
        let vh = self.viewport_height_css();
        if let Some(target) = find::scroll_to_match(m.rect, vh, self.scroll_y) {
            self.start_smooth_scroll(target);
        }
    }

    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Текущая логическая (CSS px) высота viewport-а. Если окно ещё не создано —
    /// fallback на layout-viewport 720 px, который у нас hardcoded в pipeline.
    fn viewport_height_css(&self) -> f32 {
        match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().height as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 720.0,
        }
    }

    /// CSS px ширина viewport-а — нужна scrollbar-overlay-у для размещения
    /// у правого края. Fallback на layout-viewport 1024 px (тот же hardcoded
    /// размер, что и в pipeline до создания окна).
    fn viewport_width_css(&self) -> f32 {
        match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().width as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 1024.0,
        }
    }

    /// Максимальный валидный scroll_y: ничего не скроллим, если контент
    /// помещается в viewport. Иначе — `content_height − viewport_height`.
    fn max_scroll(&self) -> f32 {
        (self.content_height - self.viewport_height_css()).max(0.0)
    }

    /// Максимальный валидный scroll_x: 0 если контент помещается по ширине.
    fn max_scroll_x(&self) -> f32 {
        (self.content_width - self.viewport_width_css()).max(0.0)
    }

    /// Горизонтальный скролл на delta CSS px (инстантный).
    fn scroll_x_by(&mut self, delta: f32) {
        let clamped = clamp_scroll(self.scroll_x + delta, self.max_scroll_x());
        if (clamped - self.scroll_x).abs() > f32::EPSILON {
            self.scroll_x = clamped;
            self.request_redraw();
        }
    }

    /// Установить scroll_y в абсолютное значение (после clamping-а). `f32::INFINITY`
    /// = «к самому низу», `0.0` = «вверх». Запрашивает redraw только если значение
    /// действительно изменилось — иначе wheel-spam в самом низу не дёргал бы GPU.
    ///
    /// Используется для инстант-путей: drag thumb scrollbar-а. Для
    /// пользовательских scroll-команд (wheel / keys / page-jump / find) —
    /// `start_smooth_scroll` / `scroll_by_smooth`.
    fn scroll_to(&mut self, target: f32) {
        // Инстант-путь cancel-ит активную анимацию — мы только что
        // *приказали* быть в конкретной точке.
        self.scroll_anim = None;
        let clamped = clamp_scroll(target, self.max_scroll());
        if (clamped - self.scroll_y).abs() > f32::EPSILON {
            self.scroll_y = clamped;
            self.request_redraw();
        }
    }

    /// Запустить smooth-scroll к target Y. Cancel-ит активную анимацию.
    /// Target клампится. Если target == текущему scroll_y — анимация не
    /// стартует (и текущая сбрасывается).
    fn start_smooth_scroll(&mut self, target: f32) {
        let max = self.max_scroll();
        let target_clamped = clamp_scroll(target, max);
        if (target_clamped - self.scroll_y).abs() <= f32::EPSILON {
            self.scroll_anim = None;
            return;
        }
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        self.scroll_anim = Some(scroll_anim::ScrollAnim {
            start_y: self.scroll_y,
            target_y: target_clamped,
            start_time_ms: now_ms,
        });
        self.request_redraw();
    }

    /// Smooth-вариант `scroll_by`. Если уже идёт анимация — delta
    /// добавляется к её target-у, а не к текущему scroll_y. Это правильная
    /// семантика для repeat-input (key-repeat, wheel-spam): каждое
    /// нажатие дописывает delta к точке назначения, а не дёргает анимацию
    /// в обратную сторону.
    fn scroll_by_smooth(&mut self, delta: f32) {
        let base = self.scroll_anim.as_ref().map_or(self.scroll_y, |a| a.target());
        self.start_smooth_scroll(base + delta);
    }

    /// Тик анимации перед `Renderer::render`. Если анимация активна —
    /// обновляет `scroll_y` по out-cubic easing и возвращает `true`,
    /// сигнализируя caller-у запросить ещё один redraw. Сбрасывает
    /// `scroll_anim` по завершении.
    fn advance_scroll_anim(&mut self) -> bool {
        let Some(anim) = self.scroll_anim else {
            return false;
        };
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let (y, done) = anim.sample(now_ms);
        self.scroll_y = clamp_scroll(y, self.max_scroll());
        if done {
            self.scroll_anim = None;
            false
        } else {
            true
        }
    }

    /// Тик momentum-анимации. Обновляет `scroll_y` / `scroll_x` напрямую
    /// (без smooth-scroll анимации). Возвращает `true` пока анимация жива.
    fn advance_momentum(&mut self, now_ms: f64) -> bool {
        let Some(ref mut anim) = self.momentum_anim else {
            return false;
        };
        let (dy, dx, done) = anim.advance(now_ms);
        if dy != 0.0 {
            let new_y = clamp_scroll(self.scroll_y + dy, self.max_scroll());
            if (new_y - self.scroll_y).abs() > f32::EPSILON {
                self.scroll_y = new_y;
            }
        }
        if dx != 0.0 {
            let new_x = clamp_scroll(self.scroll_x + dx, self.max_scroll_x());
            if (new_x - self.scroll_x).abs() > f32::EPSILON {
                self.scroll_x = new_x;
            }
        }
        if done {
            self.momentum_anim = None;
            false
        } else {
            true
        }
    }

    /// Пересчитать желаемый `CursorIcon` по текущей позиции курсора и
    /// при изменении вызвать `Window::set_cursor`. CursorMoved может
    /// дёргаться сотни раз в секунду — `last_cursor_icon` кэширует
    /// предыдущее значение, чтобы не делать лишний FFI-вызов в winit.
    fn update_cursor_icon(&mut self) {
        let (Some(window), Some(renderer), Some(pos)) =
            (self.window.as_ref(), self.renderer.as_ref(), self.cursor_position)
        else {
            return;
        };
        let dpr = (renderer.scale_factor() as f32).max(1e-6);
        let x_css = (pos.x as f32) / dpr;
        let y_css = (pos.y as f32) / dpr;

        // Scrollbar takes highest priority.
        let hover = scrollbar::classify_track_click(
            x_css,
            y_css,
            self.scroll_y,
            self.content_height,
            self.viewport_width_css(),
            self.viewport_height_css(),
        );
        let scrollbar_icon = cursor_icon_for_hover(hover, self.scroll_drag.is_some());

        let desired = if scrollbar_icon != CursorIcon::Default {
            scrollbar_icon
        } else if let Some(lb) = &self.layout_box {
            // Hit-test layout tree in page coordinates (viewport + scroll offset).
            let page_x = x_css + self.scroll_x;
            let page_y = y_css + self.scroll_y;
            match hit_test(Point::new(page_x, page_y), lb) {
                Some(result) => css_cursor_to_winit(result.cursor),
                None => CursorIcon::Default,
            }
        } else {
            CursorIcon::Default
        };

        if self.last_cursor_icon != Some(desired) {
            window.set_cursor(desired);
            self.last_cursor_icon = Some(desired);
        }
    }

    /// Пересчитывает текущий список совпадений.
    ///
    /// - Plain-text режим: substring search по DrawText-командам display list.
    /// - Regex режим (Ctrl+R): regex по [`TextFragment`][lumen_layout::TextFragment]
    ///   из [`collect_visible_text`][lumen_layout::collect_visible_text]; позиции
    ///   берутся из `TextFragment.rect`, `dl_index` — lookup по (x, y, text) в DL.
    fn current_matches(&self) -> Vec<find::FindMatch> {
        if !self.find.is_open() || self.find.query().is_empty() {
            return Vec::new();
        }
        let Ok(font) = lumen_font::Font::parse(INTER_FONT) else {
            return Vec::new();
        };
        let Ok(measurer) = lumen_paint::FontMeasurer::new(&font) else {
            return Vec::new();
        };
        if self.find.is_regex_mode() {
            let frags = self.layout_box.as_ref().map_or_else(Vec::new, |lb| {
                lumen_layout::collect_visible_text(lb)
            });
            find::find_matches_regex(&frags, &self.display_list, self.find.query(), &measurer)
        } else {
            find::find_matches(&self.display_list, self.find.query(), &measurer)
        }
    }

    /// Сохранить текущую вкладку в `last_session.lsession` при закрытии окна.
    ///
    /// Silent — ошибки записи не ломают выход. Не сохраняет Empty-страницу.
    fn save_session_on_close(&self) {
        let url = match &self.source {
            PageSource::Empty => return,
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url(u) => u.clone(),
            PageSource::Snapshot { base_url, .. } => base_url.clone(),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let file = SessionFile {
            version: 1,
            name: format!("auto-save {now}"),
            created_at: now,
            tabs: vec![ExportedTab {
                url,
                title: self.title.clone().unwrap_or_default(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                is_active: true,
            }],
        };
        let json = session_export::to_json(&file);
        let _ = std::fs::write("last_session.lsession", json.as_bytes());
    }
}

/// Достать чистый `ModifiersState` из обёртки `Modifiers` (winit 0.30 различает
/// "physical state" — Ctrl как клавиша — и "lock state"; для shortcuts нам
/// нужно физическое состояние).
fn winit_modifiers_state(mods: &Modifiers) -> ModifiersState {
    mods.state()
}

/// Сколько CSS px скроллим за стрелку (line-step). Эмпирическое значение,
/// близкое к Firefox/Chromium без smooth-scroll — около 2.5 строк 16-px текста.
const LINE_STEP_CSS_PX: f32 = 40.0;

/// Бюджет idle-окна для `requestIdleCallback`-ов, передаваемый в
/// `EventLoop::run_idle_callbacks` на каждом `about_to_wait`. Phase 0 не знает
/// реального времени до следующего vsync, поэтому используется фиксированный
/// 10 ms — тот же дефолт, что у Chromium при отсутствии явного measurement-а
/// idle-окна. Idle-callback-и трактуют это как «успей за ~10 ms».
const IDLE_BUDGET_MS: f64 = 10.0;

/// PageDown / PageUp / Space — сколько от viewport-а захватываем за нажатие.
/// Меньше 100% даёт overlap между «страницами»: пользователь не теряет последнюю
/// строку из вида, читать длинные тексты комфортнее.
fn page_step(viewport_height: f32) -> f32 {
    viewport_height * 0.9
}

/// Pure-fn: какой `CursorIcon` показать по результату hit-теста scrollbar-а
/// и флагу активного drag-а. `Pointer` сигналит «здесь интерактив»:
/// - drag активен → `Pointer` независимо от текущей точки (винит шлёт
///   CursorMoved за пределами окна тоже, и cursor должен «прилипнуть»);
/// - hover thumb → `Pointer`;
/// - hover track выше/ниже thumb-а или клик мимо → `Default` (track-click
///   тоже clickable, но cursor-change на пустом track-е был бы шумным —
///   стандарт всех браузеров).
fn cursor_icon_for_hover(hover: scrollbar::TrackClick, drag_active: bool) -> CursorIcon {
    if drag_active {
        return CursorIcon::Pointer;
    }
    match hover {
        scrollbar::TrackClick::Thumb => CursorIcon::Pointer,
        _ => CursorIcon::Default,
    }
}

/// Конвертирует CSS `cursor` keyword в winit `CursorIcon`.
/// `Auto` → `Default` (UA-решение для Phase 0); `None` → `Default` (winit не
/// поддерживает «скрытый курсор» через CursorIcon — нужен отдельный API).
fn css_cursor_to_winit(c: CssCursor) -> CursorIcon {
    match c {
        CssCursor::Auto | CssCursor::Default => CursorIcon::Default,
        CssCursor::None => CursorIcon::Default,
        CssCursor::ContextMenu => CursorIcon::ContextMenu,
        CssCursor::Help => CursorIcon::Help,
        CssCursor::Pointer => CursorIcon::Pointer,
        CssCursor::Progress => CursorIcon::Progress,
        CssCursor::Wait => CursorIcon::Wait,
        CssCursor::Cell => CursorIcon::Cell,
        CssCursor::Crosshair => CursorIcon::Crosshair,
        CssCursor::Text => CursorIcon::Text,
        CssCursor::VerticalText => CursorIcon::VerticalText,
        CssCursor::Alias => CursorIcon::Alias,
        CssCursor::Copy => CursorIcon::Copy,
        CssCursor::Move => CursorIcon::Move,
        CssCursor::NoDrop => CursorIcon::NoDrop,
        CssCursor::NotAllowed => CursorIcon::NotAllowed,
        CssCursor::Grab => CursorIcon::Grab,
        CssCursor::Grabbing => CursorIcon::Grabbing,
        CssCursor::AllScroll => CursorIcon::AllScroll,
        CssCursor::ColResize => CursorIcon::ColResize,
        CssCursor::RowResize => CursorIcon::RowResize,
        CssCursor::NResize => CursorIcon::NResize,
        CssCursor::EResize => CursorIcon::EResize,
        CssCursor::SResize => CursorIcon::SResize,
        CssCursor::WResize => CursorIcon::WResize,
        CssCursor::NeResize => CursorIcon::NeResize,
        CssCursor::NwResize => CursorIcon::NwResize,
        CssCursor::SeResize => CursorIcon::SeResize,
        CssCursor::SwResize => CursorIcon::SwResize,
        CssCursor::EwResize => CursorIcon::EwResize,
        CssCursor::NsResize => CursorIcon::NsResize,
        CssCursor::NeswResize => CursorIcon::NeswResize,
        CssCursor::NwseResize => CursorIcon::NwseResize,
        CssCursor::ZoomIn => CursorIcon::ZoomIn,
        CssCursor::ZoomOut => CursorIcon::ZoomOut,
    }
}

/// Кламп scroll_y в `[0, max]`. NaN-input → 0 (защита от arithmetic errors).
fn clamp_scroll(target: f32, max: f32) -> f32 {
    if target.is_nan() {
        return 0.0;
    }
    target.clamp(0.0, max)
}

/// Полная высота контента в CSS px — `max(rect.y + rect.height)` по всем
/// rect-несущим командам display list-а. Используется для clamping-а scroll_y.
fn content_height_of(dl: &lumen_paint::DisplayList) -> f32 {
    use lumen_paint::DisplayCommand;
    let mut max_y = 0.0_f32;
    for cmd in dl {
        let r = match cmd {
            DisplayCommand::FillRect { rect, .. }
            | DisplayCommand::FillRoundedRect { rect, .. }
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::DrawBackgroundImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::DrawLinearGradient { rect, .. }
            | DisplayCommand::DrawRadialGradient { rect, .. }
            | DisplayCommand::DrawConicGradient { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. }
            | DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. } => rect,
            DisplayCommand::PopClip
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer => continue,
        };
        let bottom = r.y + r.height;
        if bottom > max_y {
            max_y = bottom;
        }
    }
    max_y
}

/// Полная ширина контента в CSS px — `max(rect.x + rect.width)` по всем
/// rect-несущим командам display list-а. Используется для clamping-а scroll_x.
fn content_width_of(dl: &lumen_paint::DisplayList) -> f32 {
    use lumen_paint::DisplayCommand;
    let mut max_x = 0.0_f32;
    for cmd in dl {
        let r = match cmd {
            DisplayCommand::FillRect { rect, .. }
            | DisplayCommand::FillRoundedRect { rect, .. }
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::DrawBackgroundImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::DrawLinearGradient { rect, .. }
            | DisplayCommand::DrawRadialGradient { rect, .. }
            | DisplayCommand::DrawConicGradient { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. }
            | DisplayCommand::PushMaskImage { rect, .. }
            | DisplayCommand::PushMaskLinearGradient { rect, .. }
            | DisplayCommand::PushMaskRadialGradient { rect, .. }
            | DisplayCommand::PushMaskConicGradient { rect, .. } => rect,
            DisplayCommand::PopClip
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer => continue,
        };
        let right = r.x + r.width;
        if right > max_x {
            max_x = right;
        }
    }
    max_x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_resolved_url(base: &str, href: &str) -> String {
        match ResourceBase::Url(base.to_owned()).resolve(href) {
            ResolvedResource::Url(u) => u,
            ResolvedResource::File(_) => panic!("expected Url"),
        }
    }

    #[test]
    fn resource_base_url_absolute_path() {
        assert_eq!(
            expect_resolved_url("https://example.com/path/page.html", "/style.css"),
            "https://example.com/style.css",
        );
    }

    #[test]
    fn resource_base_url_relative_same_dir() {
        assert_eq!(
            expect_resolved_url("https://example.com/path/page.html", "style.css"),
            "https://example.com/path/style.css",
        );
    }

    #[test]
    fn resource_base_url_relative_subdirectory() {
        assert_eq!(
            expect_resolved_url("https://example.com/path/page.html", "css/main.css"),
            "https://example.com/path/css/main.css",
        );
    }

    #[test]
    fn resource_base_url_root_base() {
        assert_eq!(
            expect_resolved_url("https://example.com/", "style.css"),
            "https://example.com/style.css",
        );
    }

    #[test]
    fn resource_base_url_http_scheme_with_port() {
        assert_eq!(
            expect_resolved_url("http://localhost:8080/index.html", "/css/app.css"),
            "http://localhost:8080/css/app.css",
        );
    }

    #[test]
    fn resource_base_url_absolute_href_passthrough() {
        // Абсолютный href с http/https-схемой ловится в начале ResourceBase::resolve
        // до Url::resolve — это позволяет href с другим scheme быть видимым как Url,
        // даже если base — File.
        let base = ResourceBase::Url("https://example.com/".to_owned());
        let res = base.resolve("https://cdn.example.com/style.css");
        match res {
            ResolvedResource::Url(u) => assert_eq!(u, "https://cdn.example.com/style.css"),
            ResolvedResource::File(_) => panic!("expected Url"),
        }
    }

    #[test]
    fn resource_base_file_resolves_relative() {
        let base = ResourceBase::File(PathBuf::from("samples/page.html"));
        let res = base.resolve("style.css");
        match res {
            ResolvedResource::File(p) => {
                assert_eq!(p, PathBuf::from("samples/style.css"));
            }
            ResolvedResource::Url(_) => panic!("expected File"),
        }
    }

    #[test]
    fn resource_base_file_absolute_url_passthrough() {
        let base = ResourceBase::File(PathBuf::from("samples/page.html"));
        let res = base.resolve("https://cdn.example.com/style.css");
        match res {
            ResolvedResource::Url(u) => assert_eq!(u, "https://cdn.example.com/style.css"),
            ResolvedResource::File(_) => panic!("expected Url"),
        }
    }

    #[test]
    fn resolve_str_url_base_relative() {
        let base = ResourceBase::Url("https://example.com/path/page.html".to_owned());
        assert_eq!(
            base.resolve_str("style.css"),
            "https://example.com/path/style.css"
        );
    }

    #[test]
    fn resolve_str_url_base_absolute_passthrough() {
        let base = ResourceBase::Url("https://example.com/page.html".to_owned());
        assert_eq!(
            base.resolve_str("https://cdn.example.com/lib.js"),
            "https://cdn.example.com/lib.js"
        );
    }

    #[test]
    fn resolve_str_file_base_yields_path_string() {
        let base = ResourceBase::File(PathBuf::from("/home/user/page.html"));
        let result = base.resolve_str("style.css");
        assert!(result.ends_with("style.css"), "got: {result}");
    }

    #[test]
    fn dispatch_preload_hints_emits_events() {
        use lumen_core::event::SubresourceKind;
        use lumen_html_parser::PreloadHint;
        use std::sync::{Arc, Mutex};

        struct CollectingSink(Mutex<Vec<Event>>);
        impl EventSink for CollectingSink {
            fn emit(&self, e: &Event) {
                self.0.lock().unwrap().push(e.clone());
            }
        }

        let sink: Arc<dyn EventSink> =
            Arc::new(CollectingSink(Mutex::new(Vec::new())));
        let base = ResourceBase::Url("https://example.com/".to_owned());
        let hints = vec![
            PreloadHint::Stylesheet { url: "reset.css".into() },
            PreloadHint::Script { url: "https://cdn.example.com/lib.js".into() },
        ];

        dispatch_preload_hints(&hints, &base, &sink, &mut std::collections::HashSet::new());

        let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
        let events = unsafe { (*sink_any).0.lock().unwrap() };
        assert_eq!(events.len(), 2);

        // CSS (High) сортируется перед JS (Medium) независимо от source-order
        let Event::SubresourceHintFound { url, kind, priority } = &events[0] else { panic!() };
        assert_eq!(url, "https://example.com/reset.css");
        assert_eq!(*kind, SubresourceKind::Stylesheet);
        assert_eq!(*priority, FetchPriority::High);

        let Event::SubresourceHintFound { url: url2, kind: kind2, priority: p2 } = &events[1] else { panic!() };
        assert_eq!(url2, "https://cdn.example.com/lib.js");
        assert_eq!(*kind2, SubresourceKind::Script);
        assert_eq!(*p2, FetchPriority::Medium);
    }

    #[test]
    fn dispatch_preload_hints_deduplicates_same_url() {
        use lumen_html_parser::PreloadHint;
        use std::sync::{Arc, Mutex};

        struct CollectingSink(Mutex<Vec<Event>>);
        impl EventSink for CollectingSink {
            fn emit(&self, e: &Event) {
                self.0.lock().unwrap().push(e.clone());
            }
        }

        let sink: Arc<dyn EventSink> =
            Arc::new(CollectingSink(Mutex::new(Vec::new())));
        let base = ResourceBase::Url("https://example.com/".to_owned());
        // rel="preload stylesheet" создаёт два хинта на один href
        let hints = vec![
            PreloadHint::Preload { url: "style.css".into(), as_kind: Some("style".into()) },
            PreloadHint::Stylesheet { url: "style.css".into() },
            PreloadHint::Stylesheet { url: "other.css".into() },
        ];

        dispatch_preload_hints(&hints, &base, &sink, &mut std::collections::HashSet::new());

        let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
        let events = unsafe { (*sink_any).0.lock().unwrap() };
        // style.css появляется дважды — должен emit-иться один раз
        assert_eq!(events.len(), 2, "expected 2 unique urls, got {}", events.len());
        let urls: Vec<_> = events.iter().map(|e| {
            let Event::SubresourceHintFound { url, .. } = e else { panic!() };
            url.as_str()
        }).collect();
        assert!(urls.contains(&"https://example.com/style.css"));
        assert!(urls.contains(&"https://example.com/other.css"));
    }

    #[test]
    fn dispatch_preload_hints_cross_call_dedup() {
        // Второй вызов с тем же seen-набором не должен повторно эмитить.
        use lumen_html_parser::PreloadHint;
        use std::sync::{Arc, Mutex};

        struct CollectingSink(Mutex<Vec<Event>>);
        impl EventSink for CollectingSink {
            fn emit(&self, e: &Event) { self.0.lock().unwrap().push(e.clone()); }
        }

        let sink: Arc<dyn EventSink> = Arc::new(CollectingSink(Mutex::new(Vec::new())));
        let base = ResourceBase::Url("https://example.com/".to_owned());
        let mut seen = std::collections::HashSet::new();

        // Первый вызов — ранний скан (streaming chunk)
        let early = vec![PreloadHint::Stylesheet { url: "reset.css".into() }];
        dispatch_preload_hints(&early, &base, &sink, &mut seen);

        // Второй вызов — финальный pipeline: те же хинты + новый
        let full = vec![
            PreloadHint::Stylesheet { url: "reset.css".into() },
            PreloadHint::Image { url: Some("hero.png".into()), srcset: None, sizes: None },
        ];
        dispatch_preload_hints(&full, &base, &sink, &mut seen);

        let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
        let events = unsafe { (*sink_any).0.lock().unwrap() };
        // reset.css — один раз (из первого вызова), hero.png — один раз (из второго)
        assert_eq!(events.len(), 2);
        let urls: Vec<_> = events.iter().map(|e| {
            let Event::SubresourceHintFound { url, .. } = e else { panic!() };
            url.as_str()
        }).collect();
        assert!(urls.contains(&"https://example.com/reset.css"));
        assert!(urls.contains(&"https://example.com/hero.png"));
    }

    #[test]
    fn dispatch_preload_hints_sorts_by_priority() {
        use lumen_html_parser::PreloadHint;
        use std::sync::{Arc, Mutex};

        struct CollectingSink(Mutex<Vec<Event>>);
        impl EventSink for CollectingSink {
            fn emit(&self, e: &Event) { self.0.lock().unwrap().push(e.clone()); }
        }

        let sink: Arc<dyn EventSink> = Arc::new(CollectingSink(Mutex::new(Vec::new())));
        let base = ResourceBase::Url("https://example.com/".to_owned());
        // Source-order: img (Low) → script (Medium) → css (High)
        let hints = vec![
            PreloadHint::Image { url: Some("hero.png".into()), srcset: None, sizes: None },
            PreloadHint::Script { url: "app.js".into() },
            PreloadHint::Stylesheet { url: "main.css".into() },
        ];

        dispatch_preload_hints(&hints, &base, &sink, &mut std::collections::HashSet::new());

        let sink_any = sink.as_ref() as *const dyn EventSink as *const CollectingSink;
        let events = unsafe { (*sink_any).0.lock().unwrap() };
        assert_eq!(events.len(), 3);

        // После сортировки: css(High) → js(Medium) → img(Low)
        let priorities: Vec<_> = events.iter().map(|e| {
            let Event::SubresourceHintFound { priority, .. } = e else { panic!() };
            *priority
        }).collect();
        assert_eq!(priorities, vec![FetchPriority::High, FetchPriority::Medium, FetchPriority::Low]);
    }

    #[test]
    fn collect_link_hrefs_finds_stylesheet() {
        let doc = lumen_html_parser::parse(
            r#"<html><head><link rel="stylesheet" href="style.css"></head><body></body></html>"#,
        );
        let mut hrefs = Vec::new();
        collect_link_hrefs(&doc, doc.root(), &mut hrefs);
        assert_eq!(hrefs, vec!["style.css"]);
    }

    #[test]
    fn collect_link_hrefs_ignores_non_stylesheet() {
        let doc = lumen_html_parser::parse(
            r#"<html><head><link rel="icon" href="favicon.ico"></head><body></body></html>"#,
        );
        let mut hrefs = Vec::new();
        collect_link_hrefs(&doc, doc.root(), &mut hrefs);
        assert!(hrefs.is_empty());
    }

    #[test]
    fn extract_title_basic() {
        let doc = lumen_html_parser::parse(
            r#"<html><head><title>Hello</title></head><body></body></html>"#,
        );
        assert_eq!(extract_title(&doc).as_deref(), Some("Hello"));
    }

    #[test]
    fn extract_title_cyrillic_and_entities() {
        // RCDATA-режим декодирует &amp; → '&' прямо в tokenizer-е.
        let doc = lumen_html_parser::parse(
            r#"<html><head><title>Дом &amp; Сад</title></head><body></body></html>"#,
        );
        assert_eq!(extract_title(&doc).as_deref(), Some("Дом & Сад"));
    }

    #[test]
    fn extract_title_collapses_whitespace() {
        let doc = lumen_html_parser::parse(
            "<html><head><title>  foo\n\t  bar  </title></head><body></body></html>",
        );
        assert_eq!(extract_title(&doc).as_deref(), Some("foo bar"));
    }

    #[test]
    fn extract_title_missing_is_none() {
        let doc = lumen_html_parser::parse("<html><body><p>x</p></body></html>");
        assert!(extract_title(&doc).is_none());
    }

    #[test]
    fn extract_title_empty_is_none() {
        let doc = lumen_html_parser::parse(
            "<html><head><title>   </title></head><body></body></html>",
        );
        assert!(extract_title(&doc).is_none());
    }

    #[test]
    fn extract_title_first_wins() {
        // Lenient: если страница объявила <title> дважды, берём первый.
        let doc = lumen_html_parser::parse(
            "<html><head><title>A</title><title>B</title></head><body></body></html>",
        );
        assert_eq!(extract_title(&doc).as_deref(), Some("A"));
    }

    #[test]
    fn window_title_with_page() {
        assert_eq!(window_title(Some("Foo")), "Foo — Lumen");
    }

    #[test]
    fn window_title_fallback() {
        // Fallback содержит версию пакета — проверяем префикс.
        let t = window_title(None);
        assert!(t.starts_with("Lumen "));
    }

    #[test]
    fn keybinding_f5_reload() {
        assert_eq!(
            keybinding_for(KeyCode::F5, ModifiersState::empty()),
            Some(KeyCommand::Reload),
        );
    }

    #[test]
    fn keybinding_ctrl_r_reload() {
        assert_eq!(
            keybinding_for(KeyCode::KeyR, ModifiersState::CONTROL),
            Some(KeyCommand::Reload),
        );
    }

    #[test]
    fn keybinding_plain_r_is_none() {
        // Без Ctrl — обычная буква, не команда. Защита от перехвата ввода
        // в омнибокс (когда он появится).
        assert_eq!(keybinding_for(KeyCode::KeyR, ModifiersState::empty()), None);
    }

    #[test]
    fn keybinding_ctrl_shift_r_is_none() {
        // Shift+Ctrl+R обычно «force-reload» в web-браузерах. Не делаем
        // сейчас (нет cache), но и не путаем с обычным reload.
        assert_eq!(
            keybinding_for(KeyCode::KeyR, ModifiersState::CONTROL | ModifiersState::SHIFT),
            None,
        );
    }

    #[test]
    fn keybinding_escape_exit() {
        assert_eq!(
            keybinding_for(KeyCode::Escape, ModifiersState::empty()),
            Some(KeyCommand::Exit),
        );
    }

    #[test]
    fn keybinding_ctrl_w_exit() {
        assert_eq!(
            keybinding_for(KeyCode::KeyW, ModifiersState::CONTROL),
            Some(KeyCommand::Exit),
        );
    }

    #[test]
    fn keybinding_ctrl_escape_is_none() {
        // Esc + любые модификаторы — не наша команда (рамп для будущего).
        assert_eq!(
            keybinding_for(KeyCode::Escape, ModifiersState::CONTROL),
            None,
        );
    }

    #[test]
    fn keybinding_unknown_key_is_none() {
        assert_eq!(keybinding_for(KeyCode::KeyA, ModifiersState::empty()), None);
        assert_eq!(keybinding_for(KeyCode::F1, ModifiersState::empty()), None);
    }

    #[test]
    fn keybinding_ctrl_f_opens_find() {
        assert_eq!(
            keybinding_for(KeyCode::KeyF, ModifiersState::CONTROL),
            Some(KeyCommand::FindOpen),
        );
    }

    #[test]
    fn keybinding_ctrl_l_opens_address_bar() {
        assert_eq!(
            keybinding_for(KeyCode::KeyL, ModifiersState::CONTROL),
            Some(KeyCommand::OpenAddressBar),
        );
    }

    #[test]
    fn keybinding_f6_opens_address_bar() {
        assert_eq!(
            keybinding_for(KeyCode::F6, ModifiersState::empty()),
            Some(KeyCommand::OpenAddressBar),
        );
    }

    #[test]
    fn keybinding_plain_f_opens_hints() {
        // F без модификаторов открывает hint-режим kbd-навигации.
        assert_eq!(
            keybinding_for(KeyCode::KeyF, ModifiersState::empty()),
            Some(KeyCommand::HintModeOpen)
        );
    }

    #[test]
    fn page_source_from_arg_url() {
        assert!(matches!(
            PageSource::from_arg(Some("https://example.com")),
            PageSource::Url(ref u) if u == "https://example.com"
        ));
        assert!(matches!(
            PageSource::from_arg(Some("http://localhost:8080")),
            PageSource::Url(_)
        ));
    }

    #[test]
    fn page_source_from_arg_file() {
        let s = PageSource::from_arg(Some("samples/page.html"));
        match s {
            PageSource::File(p) => assert_eq!(p, PathBuf::from("samples/page.html")),
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn page_source_from_arg_none_is_empty() {
        assert!(matches!(PageSource::from_arg(None), PageSource::Empty));
    }

    #[test]
    fn page_source_describe() {
        assert_eq!(PageSource::Empty.describe(), "(пустая вкладка)");
        assert_eq!(
            PageSource::Url("https://x.test".to_owned()).describe(),
            "https://x.test",
        );
        assert_eq!(
            PageSource::File(PathBuf::from("a.html")).describe(),
            "a.html",
        );
    }

    #[test]
    fn collect_link_hrefs_multiple() {
        let doc = lumen_html_parser::parse(
            r#"<html><head>
                <link rel="stylesheet" href="a.css">
                <link rel="stylesheet" href="b.css">
            </head><body></body></html>"#,
        );
        let mut hrefs = Vec::new();
        collect_link_hrefs(&doc, doc.root(), &mut hrefs);
        assert_eq!(hrefs, vec!["a.css", "b.css"]);
    }

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn dump_kind_from_flag_recognised() {
        assert_eq!(DumpKind::from_flag("--dump-source"), Some(DumpKind::Source));
        assert_eq!(DumpKind::from_flag("--dump-layout"), Some(DumpKind::Layout));
        assert_eq!(
            DumpKind::from_flag("--dump-display-list"),
            Some(DumpKind::DisplayList),
        );
    }

    #[test]
    fn dump_kind_from_flag_unknown() {
        assert_eq!(DumpKind::from_flag("--dump"), None);
        assert_eq!(DumpKind::from_flag("--dump-html"), None);
        assert_eq!(DumpKind::from_flag("samples/page.html"), None);
        assert_eq!(DumpKind::from_flag(""), None);
    }

    #[test]
    fn parse_cli_no_args_is_empty_window() {
        assert!(matches!(
            parse_cli(&args(&[])),
            Ok(CliMode::OpenWindow(PageSource::Empty))
        ));
    }

    #[test]
    fn parse_cli_single_target_is_window() {
        let cli = parse_cli(&args(&["samples/page.html"])).expect("ok");
        match cli {
            CliMode::OpenWindow(PageSource::File(p)) => {
                assert_eq!(p, PathBuf::from("samples/page.html"));
            }
            _ => panic!("expected OpenWindow(File)"),
        }
    }

    #[test]
    fn parse_cli_single_url_is_window() {
        let cli = parse_cli(&args(&["https://example.com"])).expect("ok");
        assert!(matches!(
            cli,
            CliMode::OpenWindow(PageSource::Url(ref u)) if u == "https://example.com"
        ));
    }

    #[test]
    fn parse_cli_dump_layout() {
        let cli = parse_cli(&args(&["--dump-layout", "samples/page.html"])).expect("ok");
        match cli {
            CliMode::Dump {
                source: PageSource::File(p),
                kind: DumpKind::Layout,
            } => assert_eq!(p, PathBuf::from("samples/page.html")),
            _ => panic!("expected Dump Layout File"),
        }
    }

    #[test]
    fn parse_cli_dump_source_with_url() {
        let cli = parse_cli(&args(&["--dump-source", "https://example.com"])).expect("ok");
        assert!(matches!(
            cli,
            CliMode::Dump {
                source: PageSource::Url(ref u),
                kind: DumpKind::Source,
            } if u == "https://example.com"
        ));
    }

    #[test]
    fn parse_cli_dump_display_list() {
        let cli = parse_cli(&args(&["--dump-display-list", "a.html"])).expect("ok");
        assert!(matches!(
            cli,
            CliMode::Dump {
                kind: DumpKind::DisplayList,
                ..
            }
        ));
    }

    #[test]
    fn parse_cli_dump_flag_without_target_errors() {
        // --dump-X в одиночку — нет цели для прогона pipeline-а.
        let err = parse_cli(&args(&["--dump-layout"])).unwrap_err();
        assert!(err.contains("требует"), "got: {err}");
    }

    #[test]
    fn parse_cli_unknown_flag_alone_errors() {
        let err = parse_cli(&args(&["--unknown"])).unwrap_err();
        assert!(err.contains("неизвестный"), "got: {err}");
    }

    #[test]
    fn parse_cli_two_args_first_is_target_errors() {
        // `lumen a.html b.html` — мы не знаем что делать; явная ошибка лучше,
        // чем «открыть первый, проигнорировать второй».
        let err = parse_cli(&args(&["a.html", "b.html"])).unwrap_err();
        assert!(err.contains("неизвестный"), "got: {err}");
    }

    #[test]
    fn parse_cli_dump_flag_then_flag_errors() {
        // `lumen --dump-layout --dump-source` — оба флаг, target нет.
        let err =
            parse_cli(&args(&["--dump-layout", "--dump-source"])).unwrap_err();
        assert!(err.contains("ожидался"), "got: {err}");
    }

    #[test]
    fn parse_cli_too_many_args_errors() {
        let err = parse_cli(&args(&["--dump-layout", "a.html", "b.html"])).unwrap_err();
        assert!(err.contains("много"), "got: {err}");
    }

    // ── Scroll-state helpers ─────────────────────────────────────────────────

    #[test]
    fn clamp_scroll_inside_range() {
        assert_eq!(clamp_scroll(50.0, 100.0), 50.0);
        assert_eq!(clamp_scroll(0.0, 100.0), 0.0);
        assert_eq!(clamp_scroll(100.0, 100.0), 100.0);
    }

    #[test]
    fn clamp_scroll_clamps_negative_to_zero() {
        assert_eq!(clamp_scroll(-5.0, 100.0), 0.0);
        assert_eq!(clamp_scroll(f32::NEG_INFINITY, 100.0), 0.0);
    }

    #[test]
    fn clamp_scroll_clamps_overshoot_to_max() {
        assert_eq!(clamp_scroll(200.0, 100.0), 100.0);
        assert_eq!(clamp_scroll(f32::INFINITY, 100.0), 100.0);
    }

    #[test]
    fn clamp_scroll_zero_max_keeps_at_zero() {
        // Контент помещается в viewport — max_scroll = 0.
        assert_eq!(clamp_scroll(50.0, 0.0), 0.0);
        assert_eq!(clamp_scroll(-5.0, 0.0), 0.0);
    }

    #[test]
    fn clamp_scroll_nan_defaults_to_zero() {
        assert_eq!(clamp_scroll(f32::NAN, 100.0), 0.0);
    }

    #[test]
    fn cursor_icon_thumb_hover_is_pointer() {
        assert_eq!(
            cursor_icon_for_hover(scrollbar::TrackClick::Thumb, false),
            CursorIcon::Pointer
        );
    }

    #[test]
    fn cursor_icon_track_above_is_default() {
        // Track-click тоже clickable (page-jump), но cursor-change на пустом
        // track-е был бы шумным — стандарт всех браузеров: только thumb.
        assert_eq!(
            cursor_icon_for_hover(scrollbar::TrackClick::Above, false),
            CursorIcon::Default
        );
    }

    #[test]
    fn cursor_icon_track_below_is_default() {
        assert_eq!(
            cursor_icon_for_hover(scrollbar::TrackClick::Below, false),
            CursorIcon::Default
        );
    }

    #[test]
    fn cursor_icon_off_scrollbar_is_default() {
        assert_eq!(
            cursor_icon_for_hover(scrollbar::TrackClick::None, false),
            CursorIcon::Default
        );
    }

    #[test]
    fn cursor_icon_drag_active_overrides_hover() {
        // Во время drag-а cursor должен «прилипнуть» к Pointer независимо
        // от текущей позиции курсора — winit шлёт CursorMoved за пределами
        // окна, hover-классификатор там вернёт None, но drag-флаг побеждает.
        assert_eq!(
            cursor_icon_for_hover(scrollbar::TrackClick::None, true),
            CursorIcon::Pointer
        );
        assert_eq!(
            cursor_icon_for_hover(scrollbar::TrackClick::Above, true),
            CursorIcon::Pointer
        );
    }

    #[test]
    fn page_step_is_below_full_viewport() {
        // 90% от viewport-а — оставляет overlap, чтобы при PageDown пользователь
        // не терял последнюю строку из вида.
        assert!((page_step(720.0) - 648.0).abs() < 0.01);
        assert!(page_step(720.0) < 720.0);
    }

    #[test]
    fn content_height_empty_list_is_zero() {
        assert_eq!(content_height_of(&Vec::new()), 0.0);
    }

    #[test]
    fn content_height_takes_max_bottom() {
        use lumen_core::geom::Rect;
        use lumen_layout::Color;
        use lumen_paint::DisplayCommand;
        let dl: lumen_paint::DisplayList = vec![
            DisplayCommand::FillRect {
                rect: Rect::new(0.0, 0.0, 100.0, 50.0),
                color: Color { r: 0, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect::new(0.0, 200.0, 100.0, 30.0),
                color: Color { r: 0, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect::new(0.0, 100.0, 100.0, 20.0),
                color: Color { r: 0, g: 0, b: 0, a: 255 },
            },
        ];
        // max(50, 230, 120) = 230
        assert!((content_height_of(&dl) - 230.0).abs() < 0.01);
    }

    #[test]
    fn content_height_ignores_pop_commands() {
        use lumen_paint::DisplayCommand;
        let dl: lumen_paint::DisplayList = vec![
            DisplayCommand::PopClip,
            DisplayCommand::PopOpacity,
            DisplayCommand::PopBlendMode,
        ];
        assert_eq!(content_height_of(&dl), 0.0);
    }

    // ── content_width_of ──────────────────────────────────────────────────────

    #[test]
    fn content_width_empty_list_is_zero() {
        assert_eq!(content_width_of(&Vec::new()), 0.0);
    }

    #[test]
    fn content_width_takes_max_right() {
        use lumen_core::geom::Rect;
        use lumen_layout::Color;
        use lumen_paint::DisplayCommand;
        let dl: lumen_paint::DisplayList = vec![
            DisplayCommand::FillRect {
                rect: Rect::new(0.0, 0.0, 100.0, 50.0),
                color: Color { r: 0, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect::new(300.0, 0.0, 80.0, 20.0),
                color: Color { r: 0, g: 0, b: 0, a: 255 },
            },
            DisplayCommand::FillRect {
                rect: Rect::new(150.0, 0.0, 60.0, 10.0),
                color: Color { r: 0, g: 0, b: 0, a: 255 },
            },
        ];
        // max(100, 380, 210) = 380
        assert!((content_width_of(&dl) - 380.0).abs() < 0.01);
    }

    #[test]
    fn content_width_ignores_pop_commands() {
        use lumen_paint::DisplayCommand;
        let dl: lumen_paint::DisplayList = vec![
            DisplayCommand::PopClip,
            DisplayCommand::PopOpacity,
            DisplayCommand::PopBlendMode,
        ];
        assert_eq!(content_width_of(&dl), 0.0);
    }

    // ── Scroll-keybindings ────────────────────────────────────────────────────

    #[test]
    fn keybinding_arrow_down_scrolls() {
        assert_eq!(
            keybinding_for(KeyCode::ArrowDown, ModifiersState::empty()),
            Some(KeyCommand::ScrollLineDown),
        );
        assert_eq!(
            keybinding_for(KeyCode::ArrowUp, ModifiersState::empty()),
            Some(KeyCommand::ScrollLineUp),
        );
    }

    #[test]
    fn keybinding_arrow_right_left_scroll_horizontal() {
        assert_eq!(
            keybinding_for(KeyCode::ArrowRight, ModifiersState::empty()),
            Some(KeyCommand::ScrollLineRight),
        );
        assert_eq!(
            keybinding_for(KeyCode::ArrowLeft, ModifiersState::empty()),
            Some(KeyCommand::ScrollLineLeft),
        );
    }

    #[test]
    fn keybinding_arrow_with_modifier_is_none() {
        // Ctrl+стрелка не наша — оставлено для возможной интеграции с
        // word-wise navigation в будущем (когда появится omnibox).
        assert_eq!(
            keybinding_for(KeyCode::ArrowDown, ModifiersState::CONTROL),
            None,
        );
    }

    #[test]
    fn keybinding_page_keys_scroll() {
        assert_eq!(
            keybinding_for(KeyCode::PageDown, ModifiersState::empty()),
            Some(KeyCommand::ScrollPageDown),
        );
        assert_eq!(
            keybinding_for(KeyCode::PageUp, ModifiersState::empty()),
            Some(KeyCommand::ScrollPageUp),
        );
    }

    #[test]
    fn keybinding_space_scrolls_page() {
        assert_eq!(
            keybinding_for(KeyCode::Space, ModifiersState::empty()),
            Some(KeyCommand::ScrollPageDown),
        );
        assert_eq!(
            keybinding_for(KeyCode::Space, ModifiersState::SHIFT),
            Some(KeyCommand::ScrollPageUp),
        );
    }

    #[test]
    fn keybinding_home_end_jump() {
        assert_eq!(
            keybinding_for(KeyCode::Home, ModifiersState::empty()),
            Some(KeyCommand::ScrollHome),
        );
        assert_eq!(
            keybinding_for(KeyCode::End, ModifiersState::empty()),
            Some(KeyCommand::ScrollEnd),
        );
    }

    // ── script execution gate ────────────────────────────────────────────────

    #[test]
    fn collect_inline_scripts_finds_inline() {
        let doc = lumen_html_parser::parse(
            r#"<html><head></head><body><script>console.log(1);</script></body></html>"#,
        );
        let mut scripts = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].contains("console.log"));
    }

    #[test]
    fn collect_inline_scripts_skips_empty() {
        let doc = lumen_html_parser::parse(
            r#"<html><head></head><body><script>   </script></body></html>"#,
        );
        let mut scripts = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts);
        assert!(scripts.is_empty());
    }

    #[test]
    fn collect_inline_scripts_multiple() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><script>a=1;</script><script>b=2;</script></body></html>"#,
        );
        let mut scripts = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts);
        assert_eq!(scripts.len(), 2);
    }

    #[test]
    fn run_scripts_blocked_by_sandbox() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><script>x=1;</script></body></html>"#,
        );
        let count = run_scripts(&doc, lumen_core::SandboxFlags::SCRIPTS, &lumen_core::NullJsRuntime);
        assert_eq!(count, 0);
    }

    #[test]
    fn run_scripts_allowed_calls_runtime() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><script>x=1;</script></body></html>"#,
        );
        // empty() — без ограничений, скрипты разрешены; NullJsRuntime → NotImplemented
        let count = run_scripts(&doc, lumen_core::SandboxFlags::empty(), &lumen_core::NullJsRuntime);
        assert_eq!(count, 1);
    }

    #[test]
    fn run_scripts_no_scripts_returns_zero() {
        let doc = lumen_html_parser::parse(
            r#"<html><head></head><body><p>no scripts</p></body></html>"#,
        );
        let count = run_scripts(&doc, lumen_core::SandboxFlags::empty(), &lumen_core::NullJsRuntime);
        assert_eq!(count, 0);
    }

    // ── navigation gate ──────────────────────────────────────────────────────

    #[test]
    fn navigation_gate_blocked_by_sandbox_returns_count() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><a href="/page1">link</a><a href="/page2">link2</a></body></html>"#,
        );
        assert_eq!(check_navigation_gate(&doc, lumen_core::SandboxFlags::NAVIGATION), 2);
    }

    #[test]
    fn navigation_gate_allowed_returns_zero() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><a href="/page1">link</a></body></html>"#,
        );
        assert_eq!(check_navigation_gate(&doc, lumen_core::SandboxFlags::empty()), 0);
    }

    #[test]
    fn navigation_gate_no_anchors_returns_zero() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><p>no links</p></body></html>"#,
        );
        assert_eq!(check_navigation_gate(&doc, lumen_core::SandboxFlags::NAVIGATION), 0);
    }
}
