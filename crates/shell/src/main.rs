//! Lumen shell — точка входа браузера.
//!
//! Режимы запуска:
//! - `lumen` — открыть пустое окно.
//! - `lumen <path.html>` — распарсить файл, layout, paint, нарисовать в окне.
//! - `lumen <http(s)://...>` — загрузить страницу по сети, layout, paint.
//! - `lumen --dump-source <path-or-url>` — печать декодированного HTML в stdout.
//! - `lumen --dump-layout <path-or-url>` — печать layout-дерева в stdout.
//! - `lumen --dump-display-list <path-or-url>` — печать display list в stdout.
//! - `lumen --print-to-pdf <out.pdf> <path-or-url>` — сохранить страницу как PDF (A4).
//! - `lumen --devtools-port <N>` — запустить DevTools WebSocket сервер на порту N.
//! - `lumen --bidi-port <N>` — запустить WebDriver BiDi WebSocket сервер на порту N.
//! - `lumen --mcp [url]` — MCP-сервер (stdio) для AI-агентов (Claude, Browser Use…).
//! - `lumen --mcp-port <N> [url]` — MCP-сервер на TCP порту N (отладка через netcat).
//!
//! Dump-режимы не создают окна и не инициализируют wgpu — pipeline прогоняется
//! до нужной фазы, результат сериализуется и пишется в stdout. Полезно для CI
//! (без GPU), отладки сложных страниц и сравнения вывода между версиями.
//!
//! Внешние CSS: `<link rel="stylesheet" href="...">` загружается с диска или
//! по сети — в зависимости от того, каким способом загружена страница.

mod address_bar;
mod animation_scheduler;
mod click_log;
mod backend_factory;
mod bidi;
mod config;
mod deterministic;
mod devtools;
mod download;
mod find;
mod forms;
mod gc_tick;
mod hints;
mod memory_poll;
mod input;
mod links;
mod momentum_anim;
mod notification;
mod omnibox;
mod panels;
mod platform;
mod reader_view;
mod source_view;
pub mod surface;
mod runtime;
mod scroll;
mod scroll_anim;
mod scrollbar;
mod session_persist;
mod tab_lifecycle;
mod tabs;
mod zoom;

use crate::tab_lifecycle::state::TabState;
use std::cell::Cell;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use lumen_core::event::{Event, FetchPriority, SubresourceKind};
use lumen_core::ext::{EventSink, HyphenationProvider, NullHyphenationProvider};
use lumen_encoding::KnuthLiangHyphenation;
use lumen_core::geom::{Point, Rect, Size};
use lumen_devtools::DevToolsServer;
use lumen_driver::BrowserSession;
use lumen_knowledge::HistoryFts;
use lumen_storage::session_export::{self, ExportedTab, SessionFile};
use lumen_storage::{BfCache, BfCacheEntry, History, SearchHistory};
use lumen_dom::{
    Document, NodeData, NodeId, check_form_gate, check_navigation_gate,
    collect_iframes, check_popup_gate,
};
use std::collections::HashMap;
use lumen_layout::{LayoutBox, Mat4, PaintOrder, SnapContainer, StackingTree, TransitionScheduler};
use lumen_layout::{collect_snap_containers, find_snap_target};
#[cfg(feature = "quickjs")]
use lumen_layout::{collect_computed_styles, collect_scroll_containers, set_scroll_position};
use lumen_layout::style::ComputedStyle;
use lumen_paint::{build_display_list_ordered, build_display_list_ordered_with_anim, hit_test, DisplayList, RenderBackend};
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
            Event::RequestFailed { url, stage, reason, .. } => {
                eprintln!("✗ {url} ({}: {reason})", stage.as_str());
            }
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
    // Load the fingerprint profile (9F.1) once, before any network or JS setup.
    // Absent config → engine defaults, so behaviour is unchanged out of the box.
    config::init_global(config::load().unwrap_or_default());

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (devtools_port, rest_args) = match extract_devtools_port(&args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (bidi_port, rest_args) = match extract_bidi_port(&rest_args) {
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
    let (click_log_flag, rest_args) = extract_click_log(&rest_args);
    click_log::init(click_log_flag);
    let (det_mode, rest_args) = deterministic::extract_deterministic(&rest_args);
    let (pdf_output, rest_args) = extract_print_to_pdf(&rest_args);
    let (mcp_mode, rest_args) = extract_mcp_mode(&rest_args);
    let (proxy, rest_args) = match extract_proxy(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка --proxy: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Если прокси передан в командной строке, переопределить конфиг.
    if let Some(proxy_str) = proxy {
        let mut cfg = config::global().clone();
        cfg.proxy = Some(proxy_str);
        config::init_global(cfg);
    }

    let cli = if let Some(output) = pdf_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::PrintToPdf { source, output }
    } else if let Some(mcp) = mcp_mode {
        CliMode::Mcp(mcp)
    } else {
        match parse_cli(&rest_args) {
            Ok(m) => m,
            Err(err) => {
                eprintln!("Ошибка аргументов: {err}");
                print_usage();
                return ExitCode::FAILURE;
            }
        }
    };

    if let Some(port) = devtools_port
        && let Err(e) = DevToolsServer::spawn(port)
    {
        eprintln!("Ошибка запуска DevTools на порту {port}: {e}");
        return ExitCode::FAILURE;
    }

    if let Some(port) = bidi_port
        && let Err(e) = bidi::spawn(port)
    {
        eprintln!("Ошибка запуска BiDi на порту {port}: {e}");
        return ExitCode::FAILURE;
    }

    let blocked_log = Arc::new(std::sync::Mutex::new(
        panels::shields_panel::BlockedLog::default(),
    ));
    let network_log = Arc::new(std::sync::Mutex::new(
        devtools::network_panel::NetworkLog::default(),
    ));
    // Sink chain: StdoutEventSink → NetworkLogSink → ShieldCountSink.
    // Each wrapper forwards to its inner sink, so all three observe every event.
    let event_sink: Arc<dyn EventSink> = Arc::new(panels::shields_panel::ShieldCountSink {
        inner: Arc::new(devtools::network_panel::NetworkLogSink {
            inner: Arc::new(StdoutEventSink),
            log: Arc::clone(&network_log),
        }),
        log: Arc::clone(&blocked_log),
    });

    // --import-session переопределяет источник страницы и начальный scroll.
    let (cli, initial_scroll) = match import_session {
        Some((session_source, scroll)) => (CliMode::OpenWindow(session_source), scroll),
        None => (cli, (0.0_f32, 0.0_f32)),
    };

    match cli {
        CliMode::Dump { source, kind } => run_dump_mode(&source, kind, event_sink),
        CliMode::OpenWindow(source) => run_window_mode(source, event_sink, blocked_log, network_log, initial_scroll, no_scrollbar, det_mode),
        CliMode::PrintToPdf { source, output } => run_print_to_pdf(&source, &output, event_sink),
        CliMode::Mcp(mcp) => run_mcp_mode(mcp),
    }
}

fn run_window_mode(
    source: PageSource,
    event_sink: Arc<dyn EventSink>,
    blocked_log: Arc<std::sync::Mutex<panels::shields_panel::BlockedLog>>,
    network_log: Arc<std::sync::Mutex<devtools::network_panel::NetworkLog>>,
    initial_scroll: (f32, f32),
    no_scrollbar: bool,
    deterministic: bool,
) -> ExitCode {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    // Wire navigator.clipboard to the OS clipboard (task #26). Process-global,
    // installed once; the JS bindings _lumen_clipboard_read/_write forward here.
    #[cfg(feature = "quickjs")]
    lumen_js::set_clipboard_provider(std::sync::Arc::new(
        platform::clipboard::PlatformClipboard,
    ));

    // Apply the fingerprint profile's navigator/screen/timezone values (9F.1).
    // Process-global; consumed by lumen_js when each page's JS context spins up.
    #[cfg(feature = "quickjs")]
    config::global().install_navigator();

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
    let (input_tx, input_rx) = input::channel();
    let (read_later_tx, read_later_rx) =
        std::sync::mpsc::channel::<(String, String, Vec<u8>)>();
    let mut app = Lumen {
        display_list: Vec::new(),
        tile_grid: lumen_paint::TileGrid::default_size(),
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
        snap_containers: Vec::new(),
        epoch: std::time::Instant::now(),
        find: find::FindState::default(),
        address_bar: address_bar::AddressBarState::default(),
        hint: hints::HintState::default(),
        scroll_y: initial_scroll.1,
        scroll_x: initial_scroll.0,
        content_height: 0.0,
        content_width: 0.0,
        dark_mode: false,
        cursor_position: None,
        hovered_nid: None,
        hovered_tab_idx: None,
        active_nid: None,
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
        idb_dir: lumen_idb_dir(),
        sw_backend: Arc::new(std::sync::Mutex::new(lumen_storage::store::InMemoryStorage::new())),
        cookie_jar: Arc::new(
            lumen_storage::CookieJar::open_in_memory().expect("cookie_jar init"),
        ),
        js_ctx: None,
        no_scrollbar,
        first_paint_delivered: false,
        first_contentful_paint_delivered: false,
        history_fts: HistoryFts::open_in_memory().expect("history_fts init"),
        search_history: SearchHistory::open_in_memory().expect("search_history init"),
        next_history_id: 1,
        hyp_provider: KnuthLiangHyphenation::new(),
        animated_gifs: HashMap::new(),
        gif_last_frame: HashMap::new(),
        image_cache: lumen_image::ImageDecodeCache::new(),
        input_rx,
        input_tx,
        focused_node: None,
        downloads: download::DownloadManager::new(),
        tab_strip: tabs::strip::TabStrip::new(),
        container_store: tabs::containers::ContainerStore::new(),
        bg_tabs: HashMap::new(),
        hibernated_tabs: HashMap::new(),
        tab_snapshots: lumen_storage::TabSnapshotStore::open_in_memory()
            .expect("tab_snapshots in-memory"),
        session_store: session_persist::open_store(),
        lifecycle_mgr: {
            let mut mgr = tab_lifecycle::TabLifecycleManager::new(
                tab_lifecycle::TierTimeouts::default(),
                8, // max 8 non-hibernated background tabs
            );
            // Register the initial blank tab (id=0) as the active tab.
            mgr.open_tab(0);
            mgr
        },
        lifecycle_last_tick: std::time::Instant::now(),
        split_view: None,
        vim_mode: None,
        vertical_tabs: panels::vertical_tabs::VerticalTabsPanel::new(),
        tree_tabs: panels::tree_tabs::TreeTabsPanel::new(),
        workspace_panel: panels::workspace_panel::WorkspacePanel::new(),
        workspaces: lumen_storage::Workspaces::open_in_memory()
            .expect("workspaces in-memory"),
        shields: panels::shields_panel::ShieldsPanel::new(blocked_log),
        permission: panels::permission_panel::PermissionPanel::new(),
        sidebar: panels::sidebar_panel::SidebarPanel::new(),
        bookmarks: lumen_storage::Bookmarks::open_in_memory().expect("bookmarks in-memory"),
        bookmark_panel: panels::bookmark_panel::BookmarkPanel::new(),
        history_store: History::open_in_memory().expect("history_store in-memory"),
        history_panel: panels::history_panel::HistoryPanel::new(),
        command_palette: panels::command_palette::CommandPalette::new(),
        focus: panels::focus_panel::FocusModePanel::new(),
        pip: panels::pip_window::PipWindow::new(),
        gesture: input::gesture::GestureRecognizer::new(),
        omnibox_aliases: lumen_storage::OmniboxAliases::open_in_memory()
            .expect("omnibox_aliases init"),
        notes: Vec::new(),
        read_later_store: lumen_knowledge::ReadLater::open_in_memory()
            .expect("read_later in-memory"),
        read_later_panel: panels::read_later_panel::ReadLaterPanel::new(),
        read_later_rx,
        read_later_tx,
        cookie_banner_dismiss: true,
        gc_tick: gc_tick::GcTick::new(),
        memory_poll: memory_poll::MemoryPollTick::new(memory_poll::platform_source()),
        cache_registry: lumen_core::ext::CacheRegistry::new(),
        deterministic,
        devtools_console: devtools::console_panel::ConsolePanel::new(),
        dom_inspector: devtools::inspector::DomInspectorPanel::new(),
        network_panel: devtools::network_panel::NetworkPanel::new(std::sync::Arc::clone(
            &network_log,
        )),
        privacy: panels::privacy_panel::PrivacyPanel::new(network_log),
        a11y_store: lumen_storage::A11yPrefs::open_in_memory()
            .expect("a11y_prefs in-memory"),
        a11y_panel: panels::a11y_panel::A11yPanel::new(),
        settings_store: lumen_storage::BrowserSettings::open_in_memory()
            .expect("settings in-memory"),
        settings_panel: panels::settings_panel::SettingsPanel::new(),
        shortcuts_panel: {
            let ks = lumen_storage::KeyboardShortcuts::open_in_memory()
                .expect("shortcuts in-memory");
            panels::shortcuts_panel::ShortcutsPanel::new(&ks.all())
        },
        fallbacks_preloaded: false,
        zoom_factor: zoom::ZOOM_DEFAULT,
        display_url: None,
        current_history_state_json: String::from("null"),
        fullscreen_nid: None,
        view_transition: None,
        archive: tabs::archive::TabArchive::new(),
        restore_spinner_start_ms: None,
        resize_active: None,
        reader_original_source: None,
    };
    // Restore the previous session only when launched without an explicit page
    // (no file/url argument and no --import-session), so we never clobber an
    // argv-requested page. Sets the active tab's source before `run_app`, so the
    // streaming load in `resumed` picks it up.
    if matches!(app.source, PageSource::Empty) {
        app.restore_session();
    }
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

/// Запустить `--print-to-pdf`: layout → paginate → render → PDF → файл.
fn run_print_to_pdf(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
) -> ExitCode {
    match do_print_to_pdf(source, output, event_sink) {
        Ok(page_count) => {
            eprintln!("PDF сохранён: {} ({page_count} стр.)", output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Ошибка --print-to-pdf {}: {err}", source.describe());
            ExitCode::FAILURE
        }
    }
}

/// A4 @ 96 DPI: 210 mm × 297 mm → 794 × 1123 px.
const PDF_PAGE_W: u32 = 794;
const PDF_PAGE_H: u32 = 1123;

fn do_print_to_pdf(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
) -> Result<usize, Box<dyn Error>> {
    use lumen_layout::{paginate, PaginationContext};
    use lumen_paint::{build_print_display_list, split_at_page_breaks, Renderer};

    let raw = source.load_bytes(event_sink.clone(), None)?;
    let vp = Size::new(PDF_PAGE_W as f32, PDF_PAGE_H as f32);
    let parsed = parse_and_layout(
        &raw.bytes,
        raw.content_type,
        &raw.base,
        &event_sink,
        vp,
        &mut std::collections::HashSet::new(),
        None,
        None,
        None,
        &NullHyphenationProvider,
        false, // headless PDF mode: no interactive JS needed
        false, // deterministic: not needed for PDF rendering
        false, // dark_mode: light mode for PDF output
        None,  // cookie_jar: not available in standalone PDF mode
    )?;

    let ctx = PaginationContext {
        page_width: PDF_PAGE_W as f32,
        page_height: PDF_PAGE_H as f32,
        margin_top: 48.0,
        margin_bottom: 48.0,
        margin_left: 48.0,
        margin_right: 48.0,
    };
    let mut pages = paginate(&parsed.layout, &ctx);
    let page_count_total = pages.len() as u32;
    // Attach @page margin-box data: page N of M at bottom-center.
    attach_page_boxes(&mut pages, page_count_total, &ctx);
    let cmds = build_print_display_list(&pages);
    let split_pages = split_at_page_breaks(cmds);

    let images = Renderer::render_print_pages(
        INTER_FONT.to_vec(),
        &split_pages,
        PDF_PAGE_W,
        PDF_PAGE_H,
    )?;

    let page_count = images.len();
    let pdf_bytes = encode_images_as_pdf(&images, PDF_PAGE_W, PDF_PAGE_H);
    std::fs::write(output, &pdf_bytes)?;
    Ok(page_count)
}

/// Attaches `PageBox` data to each page with default @page content: page N of M at bottom-center.
///
/// Uses a fixed-width measurer (8 px/char at any font size) for margin-box text layout,
/// matching the Phase 0 text-measurement approach used in layout tests. Shell has no
/// access to a real `TextMeasurer` outside the full layout pipeline, and margin-box
/// text is short (page numbers) so the approximation is acceptable.
fn attach_page_boxes(
    pages: &mut [lumen_layout::pagination::Page],
    total: u32,
    ctx: &lumen_layout::PaginationContext,
) {
    use lumen_layout::{MarginBoxPosition, PageBox, PageProperties, TextMeasurer};

    /// Fixed 8 px per character at any size — matches the Phase 0 layout test measurer.
    struct Fixed8;
    impl TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }

    let props = PageProperties {
        width: ctx.page_width,
        height: ctx.page_height,
        orientation: if ctx.page_width > ctx.page_height { "landscape".to_string() } else { "portrait".to_string() },
        margin_top: ctx.margin_top,
        margin_bottom: ctx.margin_bottom,
        margin_left: ctx.margin_left,
        margin_right: ctx.margin_right,
    };

    for page in pages.iter_mut() {
        let mut page_box = PageBox::new(page.number, props.clone());
        page_box.layout_margin_boxes();

        let label = format!("{} / {}", page.number + 1, total);
        let font_size = 10.0_f32;
        let line_height = font_size * 1.5;
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::BottomCenter) {
            mb.content = Some(label.clone());
            mb.layout_text(&label, font_size, line_height, &Fixed8);
        }

        page.page_box = Some(page_box);
    }
}

/// Кодирует набор растровых изображений в PDF-файл (по одному на страницу).
///
/// Размер страницы задаётся `page_w × page_h` в PDF-единицах (1 unit = 1 px @ 96 DPI).
/// Изображения встраиваются как DeviceRGB XObject без сжатия.
fn encode_images_as_pdf(images: &[lumen_image::Image], page_w: u32, page_h: u32) -> Vec<u8> {
    use pdf_writer::{Content, Name, Pdf, Rect, Ref};

    if images.is_empty() {
        return Pdf::new().finish();
    }

    let n = images.len() as i32;
    let mut pdf = Pdf::new();

    // Распределяем PDF-объект IDs:
    //   1            = catalog
    //   2            = page tree
    //   3 .. 3+n-1   = страницы
    //   3+n .. 3+2n-1 = потоки содержимого
    //   3+2n .. 3+3n-1 = image XObjects
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_ids: Vec<Ref> = (0..n).map(|i| Ref::new(3 + i)).collect();
    let content_ids: Vec<Ref> = (0..n).map(|i| Ref::new(3 + n + i)).collect();
    let image_ids: Vec<Ref> = (0..n).map(|i| Ref::new(3 + 2 * n + i)).collect();

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids(page_ids.iter().copied())
        .count(n);

    let media = Rect::new(0.0, 0.0, page_w as f32, page_h as f32);

    for (i, image) in images.iter().enumerate() {
        let idx = i as i32;
        let img_name = format!("Im{idx}");
        let img_w = image.width;
        let img_h = image.height;

        // Страница
        {
            let mut page = pdf.page(page_ids[i]);
            page.media_box(media);
            page.parent(page_tree_id);
            page.contents(content_ids[i]);
            page.resources()
                .x_objects()
                .pair(Name(img_name.as_bytes()), image_ids[i]);
        }

        // Поток содержимого: cm-матрица + Do оператор.
        // Матрица [w 0 0 -h 0 h] размещает изображение на всю страницу
        // и переворачивает по Y (PDF: начало координат внизу слева).
        let content_bytes = {
            let mut c = Content::new();
            c.save_state();
            c.transform([img_w as f32, 0.0, 0.0, -(img_h as f32), 0.0, img_h as f32]);
            c.x_object(Name(img_name.as_bytes()));
            c.restore_state();
            c.finish()
        };
        pdf.stream(content_ids[i], &content_bytes);

        // Image XObject: DeviceRGB без альфа-канала
        let rgba = image.to_rgba8();
        let rgb: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut xobj = pdf.image_xobject(image_ids[i], &rgb);
        xobj.width(img_w as i32);
        xobj.height(img_h as i32);
        xobj.color_space().device_rgb();
        xobj.bits_per_component(8);
    }

    pdf.finish()
}

fn run_dump(
    source: &PageSource,
    kind: DumpKind,
    event_sink: Arc<dyn EventSink>,
) -> Result<(), Box<dyn Error>> {
    let raw = source.load_bytes(event_sink.clone(), None)?;
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
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp, &mut std::collections::HashSet::new(), None, None, None, &NullHyphenationProvider, false, false, false, None)?;
            print!("{}", lumen_layout::serialize_layout_tree(&parsed.layout));
            Ok(())
        }
        DumpKind::DisplayList => {
            let vp = Size::new(1024.0, 720.0);
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp, &mut std::collections::HashSet::new(), None, None, None, &NullHyphenationProvider, false, false, false, None)?;
            let dl = paint_ordered(&parsed.layout);
            print!("{}", lumen_paint::serialize_display_list(&dl));
            Ok(())
        }
    }
}

fn print_usage() {
    eprintln!("Использование:");
    eprintln!("  lumen                                           — пустое окно");
    eprintln!("  lumen <path-or-url>                             — открыть страницу в окне");
    eprintln!("  lumen --dump-source <path-or-url>               — декодированный HTML в stdout");
    eprintln!("  lumen --dump-layout <path-or-url>               — layout-дерево в stdout");
    eprintln!("  lumen --dump-display-list <path-or-url>         — display list в stdout");
    eprintln!("  lumen --print-to-pdf <out.pdf> <path-or-url>   — сохранить страницу как PDF");
    eprintln!("  [--devtools-port <N>]                           — DevTools WS сервер (любой режим)");
    eprintln!("  [--bidi-port <N>]                               — WebDriver BiDi WS сервер (любой режим)");
    eprintln!("  [--proxy <url>]                                 — HTTP прокси (http://host:port или user:pass@host:port)");
    eprintln!("  --import-session <file.lsession>                — восстановить сессию из файла");
    eprintln!("  --mcp [url]                                     — MCP-сервер (stdio) для AI-агентов");
    eprintln!("  --mcp-port <N> [url]                            — MCP-сервер (TCP) на порту N");
}

/// Извлечь `--print-to-pdf <output.pdf>` из аргументов.
///
/// Возвращает `(Some(output_path), остальные_аргументы)` или `(None, все_аргументы)`.
fn extract_print_to_pdf(args: &[String]) -> (Option<std::path::PathBuf>, Vec<String>) {
    let mut i = 0;
    let mut output: Option<std::path::PathBuf> = None;
    let mut rest = Vec::new();

    while i < args.len() {
        if args[i] == "--print-to-pdf" && output.is_none() {
            i += 1;
            if let Some(path) = args.get(i) {
                output = Some(std::path::PathBuf::from(path));
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    if output.is_some() {
        (output, rest)
    } else {
        (None, args.to_vec())
    }
}

/// Извлечь `--mcp` / `--mcp-port N` из аргументов.
///
/// Возвращает `(Some(McpMode), остальные_аргументы)` или `(None, все_аргументы)`.
fn extract_mcp_mode(args: &[String]) -> (Option<McpMode>, Vec<String>) {
    let mut port: Option<u16> = None;
    let mut url: Option<String> = None;
    let mut mcp_found = false;
    let mut rest = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "--mcp" {
            mcp_found = true;
        } else if args[i] == "--mcp-port" {
            mcp_found = true;
            i += 1;
            if let Some(p) = args.get(i).and_then(|s| s.parse::<u16>().ok()) {
                port = Some(p);
            }
        } else if mcp_found && !args[i].starts_with("--") && url.is_none() {
            url = Some(args[i].clone());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    if mcp_found {
        (Some(McpMode { url, port }), rest)
    } else {
        (None, args.to_vec())
    }
}

/// Запустить MCP-сервер в headless-режиме.
///
/// Создаёт `InProcessSession`, опционально загружает URL, затем запускает
/// `McpServer` поверх stdio или TCP-транспорта. Блокирует до отключения клиента.
fn run_mcp_mode(mcp: McpMode) -> ExitCode {
    use lumen_driver::{BrowserSession, InProcessSession};
    use lumen_mcp::{McpServer, StdioTransport, TcpTransport};
    use std::net::TcpListener;

    let mut session = InProcessSession::new();
    if let Some(ref url) = mcp.url
        && let Err(e) = session.navigate(url)
    {
        eprintln!("MCP: ошибка загрузки {url}: {e}");
    }

    if let Some(port) = mcp.port {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("MCP: не удалось открыть порт {port}: {e}");
                return ExitCode::FAILURE;
            }
        };
        eprintln!("MCP listening on 127.0.0.1:{port}");
        match listener.accept() {
            Ok((stream, addr)) => {
                eprintln!("MCP connection from {addr}");
                match TcpTransport::from_stream(stream) {
                    Ok(transport) => {
                        let mut server = McpServer::new(session, transport);
                        let _ = server.run();
                    }
                    Err(e) => {
                        eprintln!("MCP: ошибка транспорта: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            Err(e) => {
                eprintln!("MCP: ошибка accept: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        eprintln!("MCP server ready (stdio)");
        let transport = StdioTransport::new();
        let mut server = McpServer::new(session, transport);
        let _ = server.run();
    }

    ExitCode::SUCCESS
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

/// Извлечь `--activity-log` (или `--click-log`) из аргументов.
/// Также активируется переменной окружения `LUMEN_ACTIVITY_LOG=1`.
fn extract_click_log(args: &[String]) -> (bool, Vec<String>) {
    let mut found = std::env::var("LUMEN_ACTIVITY_LOG").is_ok_and(|v| v == "1")
        || std::env::var("LUMEN_CLICK_LOG").is_ok_and(|v| v == "1");
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--activity-log" || arg == "--click-log" {
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

/// Извлечь `--bidi-port N` из аргументов, вернуть (port, остальные аргументы).
fn extract_bidi_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--bidi-port" {
            i += 1;
            let s = args.get(i).ok_or("--bidi-port требует номер порта")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("неверный порт: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// Извлечь `--proxy http://host:port` из аргументов.
fn extract_proxy(args: &[String]) -> Result<(Option<String>, Vec<String>), String> {
    let mut proxy: Option<String> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--proxy" {
            i += 1;
            let s = args.get(i).ok_or("--proxy требует адрес (http://host:port или https://host:port)")?;
            proxy = Some(s.clone());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((proxy, rest))
}

/// Источник страницы. Запоминается в `Lumen`, чтобы reload (F5/Ctrl+R) мог
/// заново выполнить fetch/parse/layout/paint без аргументов командной строки.
#[derive(Debug, Clone)]
enum PageSource {
    /// Без аргументов — рисуем пустое окно. Reload no-op (грузить нечего).
    Empty,
    File(PathBuf),
    Url(String),
    /// `about:blank` — пустой документ без сетевого запроса (HTML spec §7.5).
    /// `url_str()` возвращает "about:blank" для адресной строки и истории.
    AboutBlank,
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
    /// Overrides `source.url_str()` in the address bar for same-document entries.
    /// `None` for full-document navigation entries; `Some(url)` when this entry
    /// was created by `history.pushState` (the virtual URL at that point).
    display_url: Option<String>,
    /// State JSON for a same-document `history.pushState` entry.
    /// `None` → full navigation (popping this entry reloads the page).
    /// `Some(json)` → same-document (popping fires `popstate` with this state).
    same_doc_state_json: Option<String>,
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
pub(crate) trait PersistentJs {
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
    /// Deliver a LargestContentfulPaint entry to JS PerformanceObservers.
    ///
    /// Called when a large content element (>500px²) is rendered.
    /// `element_id` = NID; `size` = area in pixels; `render_time_ms` = render completion timestamp.
    #[allow(dead_code)]
    fn deliver_lcp_entry(&self, element_id: u32, size: u32, start_ms: f64, render_time_ms: f64);
    /// Deliver a LayoutShift entry to JS PerformanceObservers (CLS metric).
    ///
    /// Called when layout shift is detected during reflow (shift >5px).
    /// `value` = fractional shift distance; `had_input` = whether user input occurred recently.
    #[allow(dead_code)]
    fn deliver_layout_shift(&self, value: f64, had_input: bool);
    /// Push a fresh snapshot of computed CSS styles into the JS runtime.
    ///
    /// Called after every `relayout_page`. The JS side uses this for
    /// `window.getComputedStyle()` and CSS property reads.
    #[allow(dead_code)]
    fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>);
    /// Advance `document.readyState` to `"interactive"` and fire
    /// `readystatechange` + `DOMContentLoaded` on `document`.
    ///
    /// Call after HTML is fully parsed and inline scripts have run.
    #[allow(dead_code)]
    fn notify_dom_content_loaded(&self);
    /// Advance `document.readyState` to `"complete"` and fire
    /// `readystatechange` on `document` + `load` on `window`.
    ///
    /// Call after all subresources (images, fonts) are decoded and registered.
    #[allow(dead_code)]
    fn notify_window_loaded(&self);
    /// Notify all registered `MediaQueryList` instances that the viewport or
    /// user preferences changed (CSS Media Queries L4 §4.2). Each MQL whose
    /// `matches` flipped fires a `change` event on its listeners.
    ///
    /// Must be called after `update_viewport_size` so JS reads consistent
    /// dimensions. Shell calls it after every `relayout_page` and any
    /// `prefers-color-scheme` or `prefers-reduced-motion` toggle.
    #[allow(dead_code)]
    fn deliver_media_query_changes(&self, width: f32, height: f32, prefers_dark: bool, reduced_motion: bool);
    /// Poll all live `WebSocket` instances and deliver queued events to JS.
    ///
    /// Must be called on every event-loop step so that `onopen`/`onmessage`/
    /// `onclose`/`onerror` handlers fire promptly. Calls `_lumen_pump_websockets()`
    /// which drains `_lumen_ws_poll()` for every open handle.
    #[allow(dead_code)]
    fn pump_websockets(&self);
    /// Poll all live `EventSource` instances and deliver queued SSE events to JS.
    ///
    /// Must be called on every event-loop step so that `onopen`/`onmessage`/
    /// `onerror` handlers fire promptly. Calls `_lumen_pump_sse()` which drains
    /// `_lumen_sse_poll()` for every open handle (HTML Living Standard §9.2).
    #[allow(dead_code)]
    fn pump_sse(&self);
    /// Deliver messages posted by Web Worker threads to their `Worker` JS instances.
    ///
    /// Must be called on every event-loop tick alongside `tick_timers()` so that
    /// `onmessage` / `addEventListener('message', fn)` handlers fire promptly.
    #[allow(dead_code)]
    fn pump_workers(&self);
    /// Deliver messages posted to same-origin `BroadcastChannel` instances.
    ///
    /// Must be called on every event-loop tick alongside `pump_workers()` so
    /// that `onmessage` / `addEventListener('message', fn)` handlers fire when
    /// another context (tab/worker) broadcasts on a shared channel name.
    #[allow(dead_code)]
    fn pump_broadcast_channels(&self);
    /// Deliver messages posted by `SharedWorker` threads to this page's ports.
    ///
    /// Must be called on every event-loop tick alongside `pump_workers()` so that
    /// each client `port`'s `onmessage` / `addEventListener('message', fn)` fires
    /// when a shared worker replies (WHATWG HTML §10.2).
    #[allow(dead_code)]
    fn pump_shared_workers(&self);
    /// Drain OS notification requests queued by `new Notification(...)` in JS.
    ///
    /// Shell calls this in `about_to_wait` and forwards each entry to
    /// `notification::show_os_notification`. Returns an empty vec when no
    /// notifications were created since the last drain.
    #[allow(dead_code)]
    fn take_notification_requests(&self) -> Vec<(String, String)>;
    /// Purge JS-side per-node caches for nodes that have been detached from
    /// the DOM and have zero live JS references.
    ///
    /// Calls `_lumen_gc_collect(nids)` in QuickJS, which removes event-listener
    /// and input-value entries from `_lumen_listeners` / `_input_values` for
    /// the supplied node IDs.  Called by the shell's idle GC tick.
    #[allow(dead_code)]
    fn gc_collect(&self, dead_nids: &[u32]);
    /// Drain popup window requests queued by JS `window.open(...)`.
    ///
    /// Returns `(url, target, width_px, height_px)` tuples. Shell opens a new
    /// tab navigated to `url` for each entry. Returns an empty vec between
    /// `window.open()` calls.
    #[allow(dead_code)]
    fn take_window_open_requests(&self) -> Vec<(String, String, u32, u32)>;
    /// Drain `console.log/warn/error` messages buffered in the JS runtime.
    ///
    /// Each entry is `(level, text)` where level is 0=log, 1=warn, 2=error.
    /// Called by the shell in `about_to_wait` to feed the DevTools console panel.
    /// Returns an empty vec when no console calls have been made since last drain.
    #[allow(dead_code)]
    fn take_console_messages(&self) -> Vec<(u8, String)>;
    /// Push a fresh snapshot of per-node scroll state into the JS runtime.
    ///
    /// Maps NodeId index → `[scroll_x, scroll_y, scroll_width, scroll_height]`.
    /// Called after every `relayout_page` so JS reads `scrollTop`/`scrollLeft`/
    /// `scrollWidth`/`scrollHeight` consistently.
    #[allow(dead_code)]
    fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>);
    /// Drain programmatic scroll requests queued by JS (`scrollTo`/`scrollBy`/
    /// `scrollIntoView`/`scrollTop=`).
    ///
    /// Returns `(node_id, target_scroll_x, target_scroll_y)` tuples. Shell
    /// applies each via `set_scroll_position()`. Empty when none are pending.
    #[allow(dead_code)]
    fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)>;
    /// Drain `history.pushState` / `history.replaceState` URL-update notifications.
    ///
    /// Each entry is `(is_push, url, new_state_json)` where `is_push = true`
    /// means `pushState` (adds a same-document entry to nav_back) and `false`
    /// means `replaceState` (updates the displayed URL only).
    #[allow(dead_code)]
    fn take_history_url_updates(&self) -> Vec<(bool, String, String)>;
    /// Fire a `popstate` event in JS for a same-document back/forward navigation.
    ///
    /// `state_json` is the already-serialised state for the destination entry.
    /// `url` is the virtual address-bar URL to restore (may be empty).
    /// Calls `_lumen_deliver_popstate(state_json, url)` via `eval_js`.
    #[allow(dead_code)]
    fn fire_popstate(&self, state_json: &str, url: &str);
    /// Drain dirty `<canvas>` 2D pixel buffers for upload to the renderer.
    ///
    /// Returns `(node_index, width, height, rgba)` for every canvas drawn to
    /// since the last drain. Shell registers each as
    /// `Renderer::register_image("canvas:{nid}", ...)` and requests a repaint.
    /// Returns an empty vec when no canvas was drawn (HTML LS §4.12.4).
    #[allow(dead_code)]
    fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)>;
    /// Drain fullscreen requests queued by `element.requestFullscreen()` and
    /// `document.exitFullscreen()` (WHATWG Fullscreen §4).
    ///
    /// Each entry is `(enter, nid)`: `enter = true` means enter OS fullscreen
    /// for the element with the given node index; `false` means exit fullscreen
    /// (`nid` is ignored). Shell calls `window.set_fullscreen(Borderless)` /
    /// `window.set_fullscreen(None)` accordingly.
    #[allow(dead_code)]
    fn take_fullscreen_requests(&self) -> Vec<(bool, u32)>;
    /// Drain CSS View Transition events from `document.startViewTransition`.
    ///
    /// Shell drains these in `about_to_wait`: `Begin` captures old display list,
    /// `End` triggers relayout and starts 300 ms cross-fade.
    #[allow(dead_code)]
    fn take_view_transition_events(&self) -> Vec<ViewTransitionEvent>;
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
    fn deliver_lcp_entry(&self, element_id: u32, size: u32, start_ms: f64, render_time_ms: f64) {
        self.eval_js(&format!(
            "_lumen_deliver_lcp_entry({element_id}, {size}, {start_ms}, {render_time_ms})"
        ));
    }
    fn deliver_layout_shift(&self, value: f64, had_input: bool) {
        let had_input_js = if had_input { "true" } else { "false" };
        self.eval_js(&format!(
            "_lumen_deliver_layout_shift({}, 0, {had_input_js})",
            value
        ));
    }
    fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>) {
        self.rt.update_computed_styles(styles);
    }
    fn notify_dom_content_loaded(&self) {
        self.rt.notify_dom_content_loaded();
    }
    fn notify_window_loaded(&self) {
        self.rt.notify_window_loaded();
    }
    fn deliver_media_query_changes(&self, width: f32, height: f32, prefers_dark: bool, reduced_motion: bool) {
        let dark = if prefers_dark { "true" } else { "false" };
        let rm = if reduced_motion { "true" } else { "false" };
        self.eval_js(&format!(
            "if(typeof _lumen_deliver_media_changes==='function')_lumen_deliver_media_changes({width},{height},{dark},{rm});"
        ));
    }
    fn pump_websockets(&self) {
        self.eval_js("if(typeof _lumen_pump_websockets==='function')_lumen_pump_websockets();");
    }
    fn pump_sse(&self) {
        self.eval_js("if(typeof _lumen_pump_sse==='function')_lumen_pump_sse();");
    }
    fn pump_workers(&self) {
        self.rt.pump_workers();
    }
    fn pump_broadcast_channels(&self) {
        self.rt.pump_broadcast_channels();
    }
    fn pump_shared_workers(&self) {
        self.rt.pump_shared_workers();
    }
    fn take_notification_requests(&self) -> Vec<(String, String)> {
        self.rt
            .take_notification_requests()
            .into_iter()
            .map(|r| (r.title, r.body))
            .collect()
    }
    fn gc_collect(&self, dead_nids: &[u32]) {
        if dead_nids.is_empty() {
            return;
        }
        let arr = dead_nids
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.eval_js(&format!(
            "if(typeof _lumen_gc_collect==='function')_lumen_gc_collect([{arr}]);"
        ));
    }
    fn take_window_open_requests(&self) -> Vec<(String, String, u32, u32)> {
        self.rt
            .take_window_open_requests()
            .into_iter()
            .map(|r| (r.url, r.target, r.width, r.height))
            .collect()
    }
    fn take_console_messages(&self) -> Vec<(u8, String)> {
        self.rt.take_console_messages()
    }
    fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>) {
        self.rt.update_scroll_states(states);
    }
    fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)> {
        self.rt.take_scroll_requests()
    }
    fn take_history_url_updates(&self) -> Vec<(bool, String, String)> {
        self.rt
            .take_history_url_updates()
            .into_iter()
            .map(|u| match u {
                lumen_js::HistoryUrlUpdate::Push { url, new_state_json } => {
                    (true, url, new_state_json)
                }
                lumen_js::HistoryUrlUpdate::Replace { url, new_state_json } => {
                    (false, url, new_state_json)
                }
            })
            .collect()
    }
    fn fire_popstate(&self, state_json: &str, url: &str) {
        // Escape url for embedding in a JS string literal (single-quoted).
        let escaped = url.replace('\\', "\\\\").replace('\'', "\\'");
        // state_json is already valid JSON — embed directly without quoting.
        self.eval_js(&format!("_lumen_deliver_popstate({state_json}, '{escaped}')"));
    }
    fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)> {
        self.rt.flush_canvas_updates()
    }
    fn take_fullscreen_requests(&self) -> Vec<(bool, u32)> {
        self.rt
            .take_fullscreen_requests()
            .into_iter()
            .map(|r| match r {
                lumen_js::FullscreenRequest::Enter { nid } => (true, nid),
                lumen_js::FullscreenRequest::Exit => (false, 0),
            })
            .collect()
    }
    fn take_view_transition_events(&self) -> Vec<ViewTransitionEvent> {
        self.rt
            .take_view_transition_events()
            .into_iter()
            .map(|ev| match ev {
                lumen_js::ViewTransitionEvent::Begin => ViewTransitionEvent::Begin,
                lumen_js::ViewTransitionEvent::End => ViewTransitionEvent::End,
            })
            .collect()
    }
}

impl PageSource {
    fn from_arg(arg: Option<&str>) -> Self {
        match arg {
            Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
                PageSource::Url(s.to_owned())
            }
            Some("about:blank") => PageSource::AboutBlank,
            Some(s) => PageSource::File(PathBuf::from(s)),
            None => PageSource::Empty,
        }
    }

    fn describe(&self) -> String {
        match self {
            PageSource::Empty => "(пустая вкладка)".to_owned(),
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url(u) => u.clone(),
            PageSource::AboutBlank => "about:blank".to_owned(),
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
            PageSource::AboutBlank => Some("about:blank"),
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
            PageSource::Empty | PageSource::AboutBlank => return href.to_owned(),
        };
        base.resolve_str(href)
    }

    /// Прочитать байты страницы с диска или из сети, плюс вернуть базу для
    /// относительных URL и подсказку о content-type. Используется и обычным
    /// `load`, и dump-режимами.
    fn load_bytes(
        &self,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    ) -> Result<RawPage, Box<dyn Error>> {
        match self {
            PageSource::Empty => Err("источник пуст — нечего загружать".into()),
            PageSource::AboutBlank => Ok(RawPage {
                bytes: b"<!DOCTYPE html><html><head></head><body></body></html>".to_vec(),
                base: ResourceBase::Url("about:blank".to_owned()),
                content_type: Some("text/html"),
            }),
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
                let mut builder = HttpClient::new()
                    .with_sink(sink)
                    .with_content_decoder(std::sync::Arc::new(BrotliContentDecoder::new()));
                if let Some(jar) = cookie_jar {
                    builder = builder.with_cookie_jar(
                        Arc::new(lumen_storage::CookieJarProvider::new(jar)),
                        None,
                    );
                }
                let client = crate::config::global().apply_http(builder);
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

    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn load(
        &self,
        sink: Arc<dyn EventSink>,
        viewport: Size,
        ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
        idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
        sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
        hp: &dyn HyphenationProvider,
        cookie_banner_dismiss: bool,
    ) -> Result<(LoadedPage, Option<LayoutSource>, Option<Box<dyn PersistentJs>>), Box<dyn Error>> {
        if matches!(self, PageSource::Empty | PageSource::AboutBlank) {
            return Ok((LoadedPage::empty(), None, None));
        }
        let raw = self.load_bytes(sink.clone(), None)?;
        let (page, layout_source, js_ctx) =
            render_bytes(&raw.bytes, raw.content_type, &raw.base, sink, viewport, &mut std::collections::HashSet::new(), ls_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, false, false, None)?;
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
    /// Headless: страница рендерится постранично и сохраняется как PDF.
    PrintToPdf { source: PageSource, output: std::path::PathBuf },
    /// Headless: MCP-сервер для AI-агентов (Claude, Browser Use…).
    Mcp(McpMode),
}

/// Параметры MCP-режима.
#[derive(Debug, Clone)]
struct McpMode {
    /// Начальный URL (если указан).
    url: Option<String>,
    /// TCP-порт для `--mcp-port N`. None → stdio.
    port: Option<u16>,
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
    /// Multi-frame GIF animations decoded at load time. Keyed by the same src URL
    /// as `DrawImage.src`. Frame 0 of each entry is already in `images` so the
    /// renderer has a valid texture on first paint; subsequent frames are uploaded
    /// on each `RedrawRequested` tick via `Lumen::animated_gifs`.
    animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
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
            animated_gifs: Vec::new(),
            lazy_pairs: Vec::new(),
            layout_box: lumen_layout::LayoutBox {
                node: NodeId::from_index(0),
                rect: Rect::ZERO,
                style: lumen_layout::style::ComputedStyle::root(),
                kind: lumen_layout::BoxKind::Block,
                children: Vec::new(),
                col_span: 1,
                row_span: 1,
                svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
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
    /// Открыть новую вкладку (Ctrl+T).
    NewTab,
    /// Закрыть текущую вкладку или выйти, если вкладка последняя (Ctrl+W).
    CloseTab,
    /// Переключиться на следующую вкладку циклически (Ctrl+Tab).
    NextTab,
    /// Открыть/закрыть панель загрузок (Ctrl+Shift+J).
    DownloadsPanel,
    /// Открыть/закрыть split view (Ctrl+\): показывает активную и следующую
    /// вкладку рядом. При повторном нажатии закрывает split.
    SplitView,
    /// Переключить фокус между левой и правой панелями split view (Ctrl+M).
    SplitFocusSwitch,
    /// Включить/выключить Vim-режим навигации (Ctrl+Alt+V).
    VimModeToggle,
    /// Показать/скрыть вертикальную панель вкладок (Ctrl+B).
    ToggleVerticalTabs,
    /// Показать/скрыть tree-style панель вкладок (Ctrl+Shift+B).
    ToggleTreeTabs,
    /// Показать/скрыть панель воркспейсов (Ctrl+Shift+W).
    ToggleWorkspaces,
    /// Показать/скрыть панель Shields (Ctrl+Shift+S).
    ToggleShields,
    /// Показать/скрыть панель разрешений сайта (Ctrl+Shift+P, 7C.2).
    TogglePermissions,
    /// Включить/выключить авто-закрытие cookie-баннеров (Ctrl+Shift+K, 7C.3).
    ToggleCookieBannerDismiss,
    /// Показать/скрыть правую боковую панель (Ctrl+Shift+A, 7D.3).
    ToggleSidebar,
    /// Открыть/закрыть панель настроек доступности (Ctrl+Shift+Q, E-2).
    ToggleA11y,
    /// Показать/скрыть менеджер закладок (Ctrl+Shift+O, task #22).
    ToggleBookmarks,
    /// Показать/скрыть панель истории браузера (Ctrl+H, task D-5).
    ToggleHistory,
    /// Открыть/закрыть страницу настроек браузера (Ctrl+,, task D-7).
    ToggleSettings,
    /// Показать/скрыть командную палитру (Ctrl+K, §7E.2, task #23).
    ToggleCommandPalette,
    /// Войти/выйти из focus mode + Pomodoro (Ctrl+Shift+F, task #25, V4).
    ToggleFocusMode,
    /// Добавить текущую страницу в закладки (Ctrl+D).
    BookmarkCurrentPage,
    /// Показать/скрыть DevTools JS-консоль (F12, §7E.5).
    DevConsole,
    /// Показать/скрыть DevTools DOM-инспектор (Ctrl+Shift+I, §7E.1).
    DevInspector,
    /// Показать/скрыть DevTools панель сети (Ctrl+Shift+E, §7E.4).
    DevNetwork,
    /// Показать/скрыть privacy-панель сети (Ctrl+Shift+Y, V5).
    TogglePrivacy,
    /// Открыть/закрыть picture-in-picture окно видео (Ctrl+Shift+V, task #21).
    TogglePip,
    /// Показать/скрыть панель Read-later (Ctrl+Shift+R, §12.3).
    ToggleReadLater,
    /// Включить/выключить Reader View (F9, §D-3): clean article layout.
    ToggleReaderView,
    /// Открыть просмотр исходного кода текущей страницы (Ctrl+U, §D-2).
    ViewSource,
    /// Открыть/закрыть панель горячих клавиш (Ctrl+Shift+/, §D-4).
    ToggleShortcuts,
    /// Назначить контейнер активной вкладке (7D.2). Не привязано к клавише —
    /// диспатчится программно (контекстное меню вкладки / omnibox-команда
    /// `container <name>`). См. `tabs::containers::ContainerKind`.
    ///
    /// Конструируется через шелл-команды/omnibox в follow-up таске; пока
    /// гасим dead_code-предупреждение, чтобы `cargo clippy -D warnings` прошёл.
    #[allow(dead_code)]
    SetTabContainer(tabs::containers::ContainerKind),
    /// Увеличить масштаб страницы (Ctrl+=).
    ZoomIn,
    /// Уменьшить масштаб страницы (Ctrl+-).
    ZoomOut,
    /// Сбросить масштаб страницы к 100% (Ctrl+0).
    ZoomReset,
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
    let ctrl_and_shift = mods == (ModifiersState::CONTROL | ModifiersState::SHIFT);
    match code {
        KeyCode::F5 if no_mods => Some(KeyCommand::Reload),
        KeyCode::KeyR if ctrl_only => Some(KeyCommand::Reload),
        KeyCode::Escape if no_mods => Some(KeyCommand::Exit),
        KeyCode::KeyW if ctrl_only => Some(KeyCommand::CloseTab),
        KeyCode::KeyT if ctrl_only => Some(KeyCommand::NewTab),
        KeyCode::Tab if ctrl_only => Some(KeyCommand::NextTab),
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
        KeyCode::KeyJ if ctrl_and_shift => Some(KeyCommand::DownloadsPanel),
        // Ctrl+\ — toggle split view (show active + next tab side-by-side)
        KeyCode::Backslash if ctrl_only => Some(KeyCommand::SplitView),
        // Ctrl+M — move focus between left / right pane in split mode
        KeyCode::KeyM if ctrl_only => Some(KeyCommand::SplitFocusSwitch),
        // Ctrl+Alt+V — toggle Vim navigation mode
        KeyCode::KeyV if mods == (ModifiersState::CONTROL | ModifiersState::ALT) => {
            Some(KeyCommand::VimModeToggle)
        }
        // Ctrl+B — toggle vertical tab sidebar
        KeyCode::KeyB if ctrl_only => Some(KeyCommand::ToggleVerticalTabs),
        // Ctrl+Shift+B — toggle tree-style tab sidebar
        KeyCode::KeyB if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleTreeTabs)
        }
        // Ctrl+Shift+W — toggle workspace switcher bar
        KeyCode::KeyW if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleWorkspaces)
        }
        // Ctrl+Shift+S — toggle shields panel
        KeyCode::KeyS if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleShields)
        }
        // Ctrl+Shift+P — toggle per-site permission popover (7C.2)
        KeyCode::KeyP if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePermissions)
        }
        // Ctrl+Shift+K — toggle cookie-banner auto-dismiss (7C.3)
        KeyCode::KeyK if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleCookieBannerDismiss)
        }
        // Ctrl+Shift+A — toggle right sidebar web panel (7D.3)
        KeyCode::KeyA if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleSidebar)
        }
        // Ctrl+Shift+O — toggle bookmark manager panel (task #22)
        KeyCode::KeyO if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleBookmarks)
        }
        // Ctrl+H — toggle browser history panel (task D-5)
        KeyCode::KeyH if ctrl_only => Some(KeyCommand::ToggleHistory),
        // Ctrl+, — open browser settings (task D-7)
        KeyCode::Comma if ctrl_only => Some(KeyCommand::ToggleSettings),
        // Ctrl+Shift+F — toggle focus mode + Pomodoro timer (task #25, V4)
        KeyCode::KeyF if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleFocusMode)
        }
        // Ctrl+K — toggle the command palette (§7E.2)
        KeyCode::KeyK if ctrl_only => Some(KeyCommand::ToggleCommandPalette),
        // Ctrl+D — bookmark the current page
        KeyCode::KeyD if ctrl_only => Some(KeyCommand::BookmarkCurrentPage),
        // F12 — toggle DevTools JS console (§7E.5)
        KeyCode::F12 if no_mods => Some(KeyCommand::DevConsole),
        // Ctrl+Shift+I — toggle DevTools DOM inspector (§7E.1)
        KeyCode::KeyI if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::DevInspector)
        }
        // Ctrl+Shift+E — toggle DevTools network panel (§7E.4)
        KeyCode::KeyE if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::DevNetwork)
        }
        // Ctrl+Shift+Y — toggle privacy network panel (V5)
        KeyCode::KeyY if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePrivacy)
        }
        // Ctrl+Shift+V — toggle picture-in-picture video window (task #21)
        KeyCode::KeyV if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::TogglePip)
        }
        // Ctrl+Shift+Q — toggle accessibility settings panel (E-2)
        KeyCode::KeyQ if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleA11y)
        }
        // Ctrl+Shift+R — toggle Read-later panel (§12.3)
        KeyCode::KeyR if mods == (ModifiersState::CONTROL | ModifiersState::SHIFT) => {
            Some(KeyCommand::ToggleReadLater)
        }
        // F9 — toggle Reader View (§D-3)
        KeyCode::F9 if no_mods => Some(KeyCommand::ToggleReaderView),
        // Ctrl+U — view page source (§D-2)
        KeyCode::KeyU if ctrl_only => Some(KeyCommand::ViewSource),
        // Ctrl+Shift+/ — toggle keyboard shortcuts panel (§D-4)
        KeyCode::Slash if ctrl_and_shift => Some(KeyCommand::ToggleShortcuts),
        // Ctrl+= — zoom in
        KeyCode::Equal if ctrl_only => Some(KeyCommand::ZoomIn),
        // Ctrl+- — zoom out
        KeyCode::Minus if ctrl_only => Some(KeyCommand::ZoomOut),
        // Ctrl+0 — reset zoom
        KeyCode::Digit0 if ctrl_only => Some(KeyCommand::ZoomReset),
        _ => None,
    }
}

// ── Разрешение внешних ресурсов ──────────────────────────────────────────────

/// Откуда загружена страница — нужно для разрешения относительных URL в `<link>`.
#[derive(Clone)]
pub(crate) enum ResourceBase {
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
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    ) -> lumen_network::HttpClient {
        use lumen_network::{BrotliContentDecoder, HttpClient, MixedContentMode};
        let mut builder = HttpClient::new()
            .with_sink(sink)
            .with_content_decoder(Arc::new(BrotliContentDecoder::new()));
        if let Some(jar) = cookie_jar {
            builder = builder.with_cookie_jar(
                Arc::new(lumen_storage::CookieJarProvider::new(jar)),
                None,
            );
        }
        let client = crate::config::global().apply_http(builder);
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

fn load_linked_stylesheets(doc: &Document, base: &ResourceBase, sink: &Arc<dyn EventSink>, cookie_jar: Option<Arc<lumen_storage::CookieJar>>) -> String {
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

                let client = base.http_client_for_subresource(sink.clone(), cookie_jar.clone());
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
/// Возвращает `(images, animated_gifs, lazy_pairs)`:
/// - `images` — декодированные картинки для немедленной регистрации в renderer-е
///   (включает frame 0 каждого анимированного GIF);
/// - `animated_gifs` — многокадровые GIF-анимации для тиканья в `RedrawRequested`;
/// - `lazy_pairs` — `(node_id_u32, url)` для `<img loading="lazy">`, которые
///   не загружаются сейчас и будут зарегистрированы через `_lumen_init_lazy_images`.
#[allow(clippy::type_complexity)]
fn fetch_and_decode_images(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: lumen_core::geom::Size,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> (Vec<(String, lumen_image::Image)>, Vec<(String, lumen_image::AnimatedGif)>, Vec<(u32, String)>) {
    let requests = lumen_layout::collect_image_requests(doc, viewport);

    let mut out: Vec<(String, lumen_image::Image)> = Vec::new();
    let mut anim_gifs: Vec<(String, lumen_image::AnimatedGif)> = Vec::new();
    let mut lazy_pairs: Vec<(u32, String)> = Vec::new();
    for req in requests {
        if req.is_lazy {
            // loading="lazy": defer until near viewport; register for proximity check.
            lazy_pairs.push((req.node_id.index() as u32, req.url));
            continue;
        }
        let bytes = match fetch_image_bytes(&req.url, base, sink, cookie_jar.clone()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Пропуск картинки {}: {e}", req.url);
                continue;
            }
        };

        // Animated GIF detection: decode all frames; store for animation if >1 frame.
        if lumen_image::is_gif(&bytes) {
            match lumen_image::decode_gif_animated(&bytes) {
                Ok(gif) if gif.frames.len() > 1 => {
                    let first = gif.frames[0].image.clone();
                    if !req.has_explicit_width && !req.has_explicit_height {
                        apply_intrinsic_size(doc, req.node_id, first.width, first.height);
                    }
                    eprintln!(
                        "Загружена GIF-анимация: {} ({}×{}, {} кадров)",
                        req.url, gif.width, gif.height, gif.frames.len()
                    );
                    out.push((req.url.clone(), first));
                    anim_gifs.push((req.url, gif));
                    continue;
                }
                Ok(gif) => {
                    // Single-frame GIF: treat as static image.
                    if let Some(frame) = gif.frames.into_iter().next() {
                        let img = frame.image;
                        if !req.has_explicit_width && !req.has_explicit_height {
                            apply_intrinsic_size(doc, req.node_id, img.width, img.height);
                        }
                        eprintln!(
                            "Загружена картинка (GIF, 1 кадр): {} ({}×{})",
                            req.url, img.width, img.height
                        );
                        out.push((req.url, img));
                    }
                    continue;
                }
                Err(e) => {
                    eprintln!("Не декодируется GIF {}: {e}", req.url);
                    continue;
                }
            }
        }

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
    (out, anim_gifs, lazy_pairs)
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
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
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
            let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
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
    /// Multi-frame GIF animations found in the DOM. See [`LoadedPage::animated_gifs`].
    animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
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

/// Frozen state of a background tab — moved in/out of `Lumen` on tab switch.
///
/// All per-page fields from `Lumen` live here while the tab is not active.
/// The active tab's state always lives directly in the `Lumen` struct fields.
struct PageSnapshot {
    display_list: DisplayList,
    title: Option<String>,
    pending_images: Vec<(String, lumen_image::Image)>,
    source: PageSource,
    runtime: runtime::EventLoop,
    animation_scheduler: animation_scheduler::AnimationScheduler,
    transition_scheduler: TransitionScheduler,
    prev_styles: HashMap<NodeId, ComputedStyle>,
    anim_frame: Option<lumen_layout::AnimationFrame>,
    layout_box: Option<lumen_layout::LayoutBox>,
    find: find::FindState,
    address_bar: address_bar::AddressBarState,
    hint: hints::HintState,
    scroll_y: f32,
    scroll_x: f32,
    content_height: f32,
    content_width: f32,
    layout_source: Option<LayoutSource>,
    pending_reload: Rc<Cell<bool>>,
    pending_js_navigate: Option<JsNavigateRequest>,
    stream_builder: Option<lumen_html_parser::IncrementalTreeBuilder>,
    stream_last_paint: std::time::Instant,
    preload_dispatched: std::collections::HashSet<String>,
    ime_composing: Option<String>,
    bfcache: BfCache,
    nav_back: Vec<NavEntry>,
    nav_fwd: Vec<NavEntry>,
    form_state: forms::FormState,
    validation_tooltip: Option<(Rect, String)>,
    color_picker_node: Option<NodeId>,
    ls_storage: HashMap<String, Arc<Mutex<lumen_core::WebStorage>>>,
    /// Directory for per-origin IndexedDB SQLite files. Cloned from the active
    /// tab's `idb_dir` when saving a snapshot; restored on tab switch-back.
    idb_dir: Option<std::path::PathBuf>,
    sw_backend: Arc<Mutex<dyn lumen_core::ext::StorageBackend>>,
    js_ctx: Option<Box<dyn PersistentJs>>,
    first_paint_delivered: bool,
    first_contentful_paint_delivered: bool,
    animated_gifs: HashMap<String, lumen_image::AnimatedGif>,
    gif_last_frame: HashMap<String, usize>,
    image_cache: lumen_image::ImageDecodeCache,
    /// Per-tab user zoom factor. Preserved when the tab goes to background.
    zoom_factor: f32,
    /// Virtual URL shown in the address bar when `history.pushState` /
    /// `history.replaceState` changed the displayed URL without a page load.
    /// `None` → use `source.url_str()`.  Reset to `None` on any full navigation.
    display_url: Option<String>,
    /// Serialised JS state object for the current history entry, mirrored from
    /// the JS side so the shell can store it in `NavEntry` when pushState fires.
    /// Initialised to `"null"` (the default initial `history.state`).
    current_history_state_json: String,
    /// Original page source preserved while Reader View (§D-3) is active.
    /// `None` = this tab is not in reader mode.
    reader_original_source: Option<PageSource>,
}

#[allow(clippy::too_many_arguments)]
fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    hp: &dyn HyphenationProvider,
    cookie_banner_dismiss: bool,
    deterministic: bool,
    dark_mode: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
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
    // fetch_provider пробрасывается в window.fetch(); ws_provider — в new WebSocket();
    // sse_provider — в new EventSource(). Все три используют один HttpClient.
    let (fetch_provider, ws_provider, sse_provider) = match base {
        ResourceBase::Url(_) => {
            let client = base.http_client_for_subresource(Arc::clone(sink), cookie_jar.clone());
            let arc_client = Arc::new(client);
            let fp: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsFetchProvider>);
            let wp: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsWebSocketProvider>);
            let sp: Option<Arc<dyn lumen_core::ext::JsSseProvider>> =
                Some(arc_client as Arc<dyn lumen_core::ext::JsSseProvider>);
            (fp, wp, sp)
        }
        ResourceBase::File(_) => (None, None, None),
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
        sse_provider,
        ls_store,
        idb_backend,
        sw_backend,
        cookie_banner_dismiss,
        deterministic,
    );
    // HTML LS §8.2.3 — after HTML parse + inline scripts: readyState → "interactive"
    // + DOMContentLoaded event. Fires before images/fonts are decoded.
    #[cfg(feature = "quickjs")]
    if let Some(js) = &js_ctx {
        js.notify_dom_content_loaded();
    }

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
    let (images, animated_gifs, lazy_pairs) = {
        let mut d = doc_arc.lock().unwrap();
        fetch_and_decode_images(&mut d, base, sink, viewport, cookie_jar.clone())
    };

    // Встроенные <style> + внешние <link rel=stylesheet>.
    let css = {
        let d = doc_arc.lock().unwrap();
        let mut css = extract_style_blocks(&d);
        css.push_str(&load_linked_stylesheets(&d, base, sink, cookie_jar.clone()));
        css
    };

    let sheet = lumen_css_parser::parse(&css);

    // @font-face: загружаем url()-источники до layout.
    // CSS: @font-face multi-font TextMeasurer — P1 нужно поддержать font-family в layout
    let font_registry = load_font_faces(&sheet.font_faces, base, sink, cookie_jar.clone());

    // Populate document.fonts with FontFace objects from @font-face rules.
    // Phase 1: store FontFace metadata; status marked as Loaded after successful load.
    {
        let mut d = doc_arc.lock().unwrap();
        for rule in &sheet.font_faces {
            let mut font_face = rule_to_font_face(rule);
            // Mark as Loaded if we successfully registered it in font_registry.
            // (The registry loads fonts during load_font_faces; if no errors, it's loaded.)
            font_face.status = lumen_dom::FontFaceStatus::Loaded;
            d.fonts_mut().add(font_face);
        }
    }

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("ошибка разбора шрифта: {e}"))?;
    // Многошрифтовый измеритель: Inter как fallback + @font-face семьи.
    // CSS: @font-face multi-font TextMeasurer — wired здесь.
    let mut measurer = lumen_paint::MultiFontMeasurer::new(&font)
        .map_err(|e| format!("ошибка метрик шрифта: {e}"))?;
    for rule in &sheet.font_faces {
        if !rule.family.is_empty()
            && let Some(bytes) = font_registry.face_bytes_for_family(&rule.family)
        {
            measurer.register_family(&rule.family, bytes);
        }
    }
    // Move font_registry into Arc after using it above (face_bytes_for_family).
    let font_provider: Arc<dyn lumen_core::FontProvider> = Arc::new(font_registry);

    let layout = {
        let d = doc_arc.lock().unwrap();
        lumen_layout::layout_measured_hyp(&d, &sheet, viewport, &measurer, hp, dark_mode)
    };

    // CSS Backgrounds L3 §3.10 — собираем `background-image: url(...)` уже
    // после layout-а (картинки фона не влияют на расчёт коробок). Декодируем
    // и добавляем к `images` тем же ключом, что эмиттер кладёт в
    // `DisplayCommand::DrawBackgroundImage.src`.
    let mut images = images;
    for (src, image) in fetch_and_decode_background_images(&layout, base, sink, cookie_jar.clone()) {
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
        animated_gifs,
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
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Vec<(String, lumen_image::Image)> {
    let urls = lumen_layout::collect_background_image_requests(layout);
    let mut out: Vec<(String, lumen_image::Image)> = Vec::new();
    for url in urls {
        let bytes = match fetch_image_bytes(&url, base, sink, cookie_jar.clone()) {
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
/// Convert a FontFaceRule from the CSS parser to a DOM FontFace object.
fn rule_to_font_face(rule: &lumen_css_parser::FontFaceRule) -> lumen_dom::FontFace {
    use lumen_css_parser::FontFaceSourceKind;

    let src_parts: Vec<String> = rule
        .sources
        .iter()
        .map(|src| {
            let kind_str = match src.kind {
                FontFaceSourceKind::Url => "url",
                FontFaceSourceKind::Local => "local",
            };
            format!("{}(\"{}\")", kind_str, src.value)
        })
        .collect();
    let src_str = src_parts.join(", ");

    lumen_dom::FontFace::new(
        rule.family.clone(),
        rule.style.as_deref().unwrap_or("normal").to_string(),
        rule.weight.as_deref().unwrap_or("400").to_string(),
        rule.stretch.clone(),
        rule.unicode_range.clone(),
        src_str,
    )
}

fn load_font_faces(
    font_faces: &[lumen_css_parser::FontFaceRule],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
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

            let raw = match fetch_image_bytes(&src.value, base, sink, cookie_jar.clone()) {
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

/// Traverse the layout tree and promote nodes with `will-change: transform/opacity/filter`
/// to their own GPU layers via `RenderBackend::promote_layer`.
///
/// Called after every relayout so the promoted-layer set stays current.
/// Nodes removed from the DOM are cleaned up automatically by `sync_promoted_layers`
/// (called by each backend's `promote_layer` impl via `LayerCache`).
fn promote_will_change_layers(lb: &LayoutBox, renderer: &mut dyn RenderBackend) {
    promote_will_change_rec(lb, renderer);
}

fn promote_will_change_rec(lb: &LayoutBox, renderer: &mut dyn RenderBackend) {
    let needs_layer = lb.style.will_change.iter().any(|p| {
        matches!(p.as_str(), "transform" | "opacity" | "filter")
    });
    if needs_layer {
        let w = lb.rect.width.max(1.0) as u32;
        let h = lb.rect.height.max(1.0) as u32;
        renderer.promote_layer(lb.node.index() as u32, w, h);
    }
    for child in &lb.children {
        promote_will_change_rec(child, renderer);
    }
}

/// Find the first `<video>` element in the layout tree (depth-first, document
/// order) and return its `(src, poster)` URLs.  Used by the picture-in-picture
/// window (task #21) to pick a video to embed.  Returns `None` when the page
/// has no `<video>`.
fn find_video_source(lb: &LayoutBox) -> Option<(String, String)> {
    if let lumen_layout::BoxKind::Video { src, poster } = &lb.kind {
        return Some((src.clone(), poster.clone()));
    }
    for child in &lb.children {
        if let Some(found) = find_video_source(child) {
            return Some(found);
        }
    }
    None
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
/// `dark_mode` is forwarded to `layout_measured_hyp` so `@media (prefers-color-scheme: dark)`
/// rules take effect on relayout (e.g. after OS theme change or window resize).
fn relayout_page(src: &LayoutSource, viewport: Size, hp: &dyn HyphenationProvider, dark_mode: bool) -> (DisplayList, lumen_layout::LayoutBox) {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let doc = src.document.lock().unwrap();
    let layout = lumen_layout::layout_measured_hyp(&doc, &src.stylesheet, viewport, &measurer, hp, dark_mode);
    drop(doc);
    let dl = paint_ordered(&layout);
    (dl, layout)
}

/// Extract `initial-scale` from the `<meta name=viewport>` of a page's document.
///
/// Returns `1.0` when the page has no viewport meta or omits `initial-scale`.
fn meta_initial_scale(src: &LayoutSource) -> f32 {
    src.document
        .lock()
        .ok()
        .and_then(|doc| doc.viewport_meta().map(|m| m.initial_scale))
        .unwrap_or(1.0)
}

/// Get-or-create the localStorage partition for the given `ResourceBase` origin.
/// Returns `None` for file: bases (no persistent origin-partitioned storage).
/// Returns the platform-specific directory for per-origin IndexedDB SQLite files,
/// creating it if it does not exist.
///
/// - Windows: `%APPDATA%\lumen\idb\`
/// - Unix:    `$HOME/.config/lumen/idb/`
/// - Fallback (env vars missing): `./lumen-idb/` (relative to working directory)
///
/// Returns `None` when directory creation fails — the caller falls back to
/// ephemeral in-memory IDB storage for the session.
fn lumen_idb_dir() -> Option<std::path::PathBuf> {
    let dir = if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .ok()
            .map(|p| std::path::PathBuf::from(p).join("lumen").join("idb"))
    } else {
        std::env::var("HOME")
            .ok()
            .map(|p| std::path::PathBuf::from(p).join(".config").join("lumen").join("idb"))
    }
    .unwrap_or_else(|| std::path::PathBuf::from("lumen-idb"));

    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("idb: не удалось создать директорию {}: {e}", dir.display());
        return None;
    }
    Some(dir)
}

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

/// Build the per-origin IndexedDB persistence handle for the given `ResourceBase`.
///
/// Returns `None` for `file:` bases (no origin storage).
/// When `idb_dir` is `Some`, opens or creates a dedicated SQLite file
/// `{idb_dir}/{sha256_hex(eTLD+1)[:16]}.db`; when `None` uses an ephemeral
/// in-memory store (tests / headless — no cross-reload persistence).
fn idb_store_for_base(
    base: &ResourceBase,
    idb_dir: Option<&std::path::Path>,
) -> Option<Arc<dyn lumen_core::ext::IdbBackend>> {
    let url = match base {
        ResourceBase::Url(u) => u.as_str(),
        ResourceBase::File(_) => return None,
    };
    idb_store_for_url(url, idb_dir)
}

/// Core IDB store builder — shared by [`idb_store_for_base`] and the reload path.
fn idb_store_for_url(
    url: &str,
    idb_dir: Option<&std::path::Path>,
) -> Option<Arc<dyn lumen_core::ext::IdbBackend>> {
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    let host = parsed.host();
    if host.is_empty() {
        return None;
    }
    // eTLD+1 for key derivation; falls back to raw host (IPs, localhost, unknown TLDs).
    let etld_plus_one = {
        use lumen_core::ext::PublicSuffixList;
        lumen_storage::PslProvider::new()
            .registrable_domain(host)
            .unwrap_or(host)
            .to_string()
    };
    if let Some(dir) = idb_dir {
        lumen_storage::IdbStore::for_origin(&etld_plus_one, dir).ok()
    } else {
        let origin = format!("{}://{}", parsed.scheme(), parsed.host());
        Some(Arc::new(lumen_storage::IdbStore::new(
            Arc::new(Mutex::new(lumen_storage::store::InMemoryStorage::new())),
            origin,
        )))
    }
}

/// Build the per-origin Service Worker registration persistence handle for the
/// given `ResourceBase`. Returns `None` for `file:` bases (no persistent storage).
/// The returned `SwStore` shares `backend`, so SW registrations survive page reloads.
fn sw_store_for_base(
    base: &ResourceBase,
    backend: &Arc<std::sync::Mutex<dyn lumen_core::ext::StorageBackend>>,
) -> Option<Arc<dyn lumen_core::ext::SwBackend>> {
    let origin = match base {
        ResourceBase::Url(u) => lumen_core::url::Url::parse(u).ok().map(|parsed| {
            let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", parsed.scheme(), parsed.host(), port)
        })?,
        ResourceBase::File(_) => return None,
    };
    Some(Arc::new(lumen_storage::SwStore::new(Arc::clone(backend), origin)))
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    hp: &dyn HyphenationProvider,
    cookie_banner_dismiss: bool,
    deterministic: bool,
    dark_mode: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Result<(LoadedPage, LayoutSource, Option<Box<dyn PersistentJs>>), Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink, viewport, preload_seen, ls_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, deterministic, dark_mode, cookie_jar)?;
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
            animated_gifs: parsed.animated_gifs,
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

/// Collect `<script>` elements from the DOM, separating classic from module scripts.
///
/// `scripts` receives classic `<script>` bodies (no `type` attribute, or `type=text/javascript`).
/// `module_scripts` receives `<script type=module>` bodies (HTML LS §8.1.3.1).
/// Both skip `<script src="...">` (external-only) and empty inline bodies.
fn collect_inline_scripts(
    doc: &Document,
    id: NodeId,
    scripts: &mut Vec<String>,
    module_scripts: &mut Vec<String>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_module = script_type.is_some_and(|t| t.eq_ignore_ascii_case("module"));
        let is_importmap = script_type.is_some_and(|t| t.eq_ignore_ascii_case("importmap"));

        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if !text.trim().is_empty() {
            if is_importmap {
                // Import maps are handled separately by the caller
                // For now, skip them here; caller will collect them separately
            } else if is_module {
                module_scripts.push(text);
            } else {
                scripts.push(text);
            }
        }
        return;
    }
    for &child in &node.children {
        collect_inline_scripts(doc, child, scripts, module_scripts);
    }
}

/// Collect the first `<script type="importmap">` import map from the document.
///
/// Returns the parsed ImportMap if found, or None if not present or invalid JSON.
#[cfg(feature = "quickjs")]
fn collect_import_map(doc: &Document) -> Option<lumen_js::esm::ImportMap> {
    collect_import_map_impl(doc, doc.root())
}

#[cfg(feature = "quickjs")]
fn collect_import_map_impl(
    doc: &Document,
    id: NodeId,
) -> Option<lumen_js::esm::ImportMap> {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_importmap = script_type.is_some_and(|t| t.eq_ignore_ascii_case("importmap"));

        if is_importmap {
            let mut text = String::new();
            for &child in &node.children {
                if let NodeData::Text(s) = &doc.get(child).data {
                    text.push_str(s);
                }
            }
            if let Some(map) = lumen_js::esm::ImportMap::parse(&text) {
                return Some(map);
            }
        }
    }
    for &child in &node.children {
        if let Some(map) = collect_import_map_impl(doc, child) {
            return Some(map);
        }
    }
    None
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
/// `sse_provider` пробрасывается в `new EventSource(url)`.
/// `ls_store` — localStorage partition для текущего origin (persists across reloads).
/// `None` = no network (sandboxed context или отключён quickjs feature).
#[allow(clippy::needless_return)] // `return` inside #[cfg] block is needed for correct control flow
#[allow(unused_variables, clippy::type_complexity, clippy::too_many_arguments)]
fn run_scripts_with_dom(
    doc: Document,
    sandbox: lumen_core::SandboxFlags,
    page_url: &str,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    cookie_banner_dismiss: bool,
    deterministic: bool,
) -> (Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Box<dyn PersistentJs>>) {
    let mut scripts: Vec<String> = Vec::new();
    let mut module_scripts: Vec<String> = Vec::new();
    collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut module_scripts);

    let doc_arc = Arc::new(Mutex::new(doc));

    if scripts.is_empty() && module_scripts.is_empty() {
        return (doc_arc, None, None);
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: заблокировано {} скрипт(ов) + {} модул(ей) (sandbox=scripts)",
            scripts.len(), module_scripts.len()
        );
        return (doc_arc, None, None);
    }

    #[cfg(feature = "quickjs")]
    {
        use lumen_core::ext::JsRuntime as _;
        match lumen_js::QuickJsRuntime::new() {
            Ok(rt) => {
                rt.set_cookie_banner_dismiss(cookie_banner_dismiss);
                if deterministic {
                    rt.set_deterministic_mode();
                }
                if let Err(e) = rt.install_dom(Arc::clone(&doc_arc), page_url, fetch_provider, ws_provider, sse_provider, ls_store, idb_backend, sw_backend) {
                    eprintln!("JS DOM init failed: {e}");
                }
                // Classic scripts run first (HTML LS §8.1.3 execution order).
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
                // Module scripts run after classic scripts (HTML LS §8.1.3.1 deferred).
                for src in &module_scripts {
                    match rt.eval_module(src) {
                        Ok(()) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "module: engine=quickjs, выполнение пропущено ({} байт)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("module error: {e}"),
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
        let _ = sse_provider;
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
    let mut _module_scripts: Vec<String> = Vec::new();
    collect_inline_scripts(doc, doc.root(), &mut scripts, &mut _module_scripts);
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
    /// Tile-based dirty-rect tracker. Updated on every display-list change via
    /// [`lumen_paint::TileGrid::update_from_diff`]. Dirty tiles are re-rendered
    /// on the next frame; clean tiles reuse the previous output (Phase 2).
    tile_grid: lumen_paint::TileGrid,
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
    renderer: Option<Box<dyn RenderBackend>>,
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
    /// CSS Scroll Snap L1 containers collected from `layout_box` after every
    /// layout update. Used by `start_smooth_scroll` / `scroll_x_by` to apply
    /// snap positions. Empty when `layout_box` is `None` or the page has no
    /// `scroll-snap-type` declarations. Cleared on navigation, recomputed on
    /// relayout / tab switch.
    snap_containers: Vec<SnapContainer>,
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
    /// OS-level `prefers-color-scheme` preference. `true` — система в тёмной теме.
    /// Читается из winit `Window::theme()` при создании окна и обновляется на
    /// `WindowEvent::ThemeChanged`. Прокидывается в JS `matchMedia` через
    /// `deliver_media_query_changes(.., self.dark_mode)`. Default `false` (light)
    /// до создания окна и в headless/deterministic-режимах (стабильность snapshot-ов).
    dark_mode: bool,
    /// Per-tab user zoom factor (100% = 1.0). Changed via Ctrl+= / Ctrl+- / Ctrl+0.
    ///
    /// Combined with `<meta viewport initial-scale>` to compute the effective CSS
    /// layout viewport: `effective = physical / (meta_scale * zoom_factor)`.
    /// Resets to 1.0 on tab switch (stored in `PageSnapshot` for background tabs).
    zoom_factor: f32,
    /// Последняя известная позиция курсора в **physical** пикселях (от winit).
    /// `None` пока курсор не вошёл в окно. Конвертируется в CSS px через
    /// `scale_factor()` непосредственно в hit-test / drag callback-ах.
    cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// DOM node currently under the mouse pointer (CSS `:hover` target).
    /// Updated on every `CursorMoved`; triggers relayout when it changes so
    /// `:hover` rules re-evaluate. `None` when cursor is outside the content area.
    hovered_nid: Option<NodeId>,
    /// Tab bar: index of the hovered tab for displaying tier-tooltip. Updated on
    /// every `CursorMoved` when cursor is over the tab bar (y < TAB_BAR_HEIGHT).
    /// `None` when cursor is outside the tab bar or no tabs exist.
    hovered_tab_idx: Option<usize>,
    /// DOM node whose mouse button is currently held down (CSS `:active` target).
    /// Set on `MouseInput(Pressed)`, cleared on `MouseInput(Released)`.
    active_nid: Option<NodeId>,
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
    /// Directory for per-origin IndexedDB SQLite files (`{sha256(eTLD+1)[:16]}.db`).
    /// `None` → ephemeral in-memory store per page (headless / tests).
    /// `Some(dir)` → each origin gets its own SQLite file in `dir`; data persists
    /// across page reloads and is shared across tabs of the same origin.
    idb_dir: Option<std::path::PathBuf>,
    /// Shared backend for Service Worker registration persistence. A per-origin
    /// `SwStore` is built over this for each page load so SW registrations survive
    /// page navigations within the session (same pattern as `idb_backend`).
    sw_backend: Arc<std::sync::Mutex<dyn lumen_core::ext::StorageBackend>>,
    /// Session-scoped cookie jar. Shared across all `HttpClient` instances so
    /// `Set-Cookie` headers received on one hop (including 3xx redirects) are
    /// sent back on subsequent requests to the same domain. In-memory in Phase 0;
    /// wired to a per-profile SQLite file in Phase 2.
    cookie_jar: Arc<lumen_storage::CookieJar>,
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
    /// Счётчик для генерирования rowid при индексировании в history_fts.
    /// Инкрементируется при каждой навигации на новую страницу.
    next_history_id: i64,
    /// Knuth–Liang hyphenation provider — реализует CSS `hyphens: auto`.
    /// Lazy-loads per-locale dictionaries on first use; cached for subsequent layouts.
    hyp_provider: KnuthLiangHyphenation,
    /// Multi-frame GIF animations keyed by the same src URL used in `DrawImage`.
    /// Populated at image-load time; cleared on page navigation.
    /// Single-frame GIFs are not stored here — handled as regular static images.
    animated_gifs: HashMap<String, lumen_image::AnimatedGif>,
    /// Last rendered frame index per animated GIF URL. Avoids redundant GPU texture
    /// re-uploads when `frame_index_at(elapsed_ms)` returns the same frame as the
    /// previous tick. Cleared together with `animated_gifs` on navigation.
    gif_last_frame: HashMap<String, usize>,
    /// CPU-side decoded image cache (ADR-008 §10E.4 scroll-discard).
    ///
    /// Stores one `ImageHandle` per image URL so far-away images can be evicted
    /// from RAM on scroll without discarding the GPU texture in the renderer.
    /// Cleared and repopulated on every page load; entries are dropped by
    /// `try_discard_offscreen_images` once an image leaves the
    /// `gate_image_requests` zone (viewport ± 2 screens).
    image_cache: lumen_image::ImageDecodeCache,
    /// Receiver side of the input injection channel (ADR-007 §8C).
    ///
    /// Drained each `about_to_wait`; commands are processed through the same
    /// hit-test / JS-dispatch path as real OS events.
    input_rx: input::InputReceiver,
    /// Sender side of the input injection channel — cloned for external callers.
    #[allow(dead_code)]
    input_tx: input::InputSender,
    /// The DOM node that received the last click (used as target for TypeText injection).
    ///
    /// `None` until the first click is processed.  Updated by `handle_click_at`.
    focused_node: Option<lumen_dom::NodeId>,
    /// Download manager: background download threads, progress channel, and
    /// panel visibility state. Panel toggled via Ctrl+Shift+J.
    downloads: download::DownloadManager,
    /// Tab strip state: open tabs (title, id) and active index.
    ///
    /// The ACTIVE tab's page state lives directly in the `Lumen` fields.
    /// Background tabs have their page state in `bg_tabs` keyed by `TabEntry::id`.
    tab_strip: tabs::strip::TabStrip,
    /// Per-`(origin, ContainerKind)` cookie/storage store ids (7D.2).
    ///
    /// Allocated lazily on first access; the actual cookie jar / storage
    /// dispatch picks up the store id as a partitioning key. Stored on the
    /// shell so isolation survives tab open/close/restore.
    container_store: tabs::containers::ContainerStore,
    /// Frozen page state for each background tab, keyed by `TabEntry::id`.
    ///
    /// `None` entry means the tab was opened but never loaded (blank new tab).
    bg_tabs: HashMap<usize, PageSnapshot>,
    /// Lightweight identity for hibernated (T3) tabs — keyed by `TabEntry::id`.
    ///
    /// When a background tab is promoted to Hibernated its full `PageSnapshot`
    /// is evicted from `bg_tabs` and stored in `tab_snapshots`; only this
    /// cheap struct remains in RAM.
    hibernated_tabs: HashMap<usize, tab_lifecycle::TabMetadata>,
    /// SQLite-backed blob store for T3 DOM snapshots (ADR-008 §10J).
    tab_snapshots: lumen_storage::TabSnapshotStore,
    /// SQLite-backed store for the last session — all open tabs at window close
    /// (§10I). Overwritten wholesale on `CloseRequested`, read back on launch to
    /// reopen the previous set of tabs. On-disk at `session_persist::SESSION_DB_PATH`.
    session_store: lumen_storage::SessionStore,
    /// Lifecycle tier manager — tracks T0→T4 transitions and LRU ordering.
    ///
    /// Synced with `tab_strip` on open/switch/close; `tick_idle` is polled
    /// from `about_to_wait` once per second to drive automatic hibernation.
    lifecycle_mgr: tab_lifecycle::TabLifecycleManager,
    /// Monotonic instant of the last `tick_lifecycle` call — used to throttle
    /// polling to approximately once per second.
    lifecycle_last_tick: std::time::Instant,
    /// Active split-view state. `None` = single-pane mode (normal).
    ///
    /// When `Some`, the window is divided into two side-by-side panes:
    /// left = active tab (live `Lumen` state), right = `SplitView::right`
    /// (frozen snapshot of another tab). `Ctrl+\` toggles; `Ctrl+M` switches focus.
    split_view: Option<panels::split_view::SplitView>,
    /// Vim keybinding mode state.  `None` = vim mode is off (default).
    ///
    /// Activated via `Ctrl+Alt+V`; deactivated via `Ctrl+Alt+V` again.
    /// When `Some`, [`VimMode::feed`] intercepts navigation keys before the
    /// global keybinding table.  [`VimState::Insert`] passes keys through.
    vim_mode: Option<input::vim::VimMode>,
    /// Vertical tab panel state. Toggled via Ctrl+B.
    ///
    /// When visible, the left `PANEL_WIDTH` CSS px of the window are occupied by
    /// the tab list and the page viewport shifts right accordingly.
    vertical_tabs: panels::vertical_tabs::VerticalTabsPanel,
    /// Tree-style tab panel state (7A.2): collapse/expand subtrees.
    ///
    /// Stores which subtrees are collapsed. Rendering delegate: see
    /// `panels::tree_tabs::build_panel`. Currently initialised alongside
    /// `vertical_tabs`; future toggle key will switch between flat/tree views.
    tree_tabs: panels::tree_tabs::TreeTabsPanel,
    /// Workspace switcher panel state (7A.3).
    ///
    /// Bottom-docked 32px bar showing named workspaces as coloured chips.
    /// `Ctrl+Shift+W` toggles.  When visible, `viewport_height_css()` subtracts
    /// `SWITCHER_HEIGHT` so the page layout does not overlap the bar.
    workspace_panel: panels::workspace_panel::WorkspacePanel,
    /// Persistent workspace storage — SQLite in-memory during testing; wired to
    /// a disk path in production via `Workspaces::open(path)`.
    workspaces: lumen_storage::Workspaces,
    /// Shields floating panel state (7C.4).
    ///
    /// Shows blocked-request counts per domain, and lets the user toggle
    /// request filtering on/off for the current site.  `Ctrl+Shift+S` toggles
    /// visibility.  Backed by a shared [`BlockedLog`] updated from the network
    /// thread via [`ShieldCountSink`].
    shields: panels::shields_panel::ShieldsPanel,
    /// Per-site permission popover state (7C.2).
    ///
    /// Shows camera/mic/notifications/clipboard grant state for the current
    /// page origin.  Each row has a toggle button cycling Ask → Allow → Deny.
    /// `Ctrl+Shift+P` toggles visibility.  State is in-memory only (no
    /// persistence across sessions).
    permission: panels::permission_panel::PermissionPanel,
    /// Right-docked sidebar web panel state (7D.3).
    ///
    /// Shows a secondary web viewport in a 300 CSS px slot at the right edge.
    /// `Ctrl+Shift+A` toggles visibility; `Lumen::open_sidebar_page` supplies
    /// the page display list.  When visible, `page_content_width_css()`
    /// subtracts [`panels::sidebar_panel::PANEL_WIDTH`] and `relayout()` fires.
    sidebar: panels::sidebar_panel::SidebarPanel,
    /// SQLite-backed bookmark store (in-memory for the session).
    ///
    /// Backs the bookmark manager panel. `@read-later <url>` omnibox commands and
    /// `Ctrl+D` (bookmark current page) write here; the panel reads via
    /// `Bookmarks::list_all` on every refresh.
    bookmarks: lumen_storage::Bookmarks,
    /// Bookmark manager panel state (task #22).
    ///
    /// Floating overlay anchored under the toolbar. `Ctrl+Shift+O` toggles
    /// visibility. Folder tree + bookmark list + search + drag-and-drop re-file
    /// (move bookmark to folder, persisted via `Bookmarks::set_folder`).
    bookmark_panel: panels::bookmark_panel::BookmarkPanel,
    /// SQLite-backed browsing history store (in-memory for the session, task D-5).
    ///
    /// Records each page visit. The history panel reads via `History::recent`
    /// (50 entries, grouped by date). `History::delete` / `History::clear` are
    /// called from the panel's delete and clear-all buttons.
    history_store: History,
    /// Browser history panel state (task D-5).
    ///
    /// Centred floating overlay. `Ctrl+H` toggles visibility. Shows recent pages
    /// grouped by date with search (via `HistoryFts`), delete per-entry, and a
    /// "Очистить всё" button.
    history_panel: panels::history_panel::HistoryPanel,
    /// Command palette modal state (task #23, §7E.2).
    ///
    /// `Ctrl+K` toggles a centred modal that fuzzy-searches across commands,
    /// bookmarks and history. While visible it captures all keyboard and pointer
    /// input; `↑/↓` move the selection, `Enter` activates, `Esc` closes.
    command_palette: panels::command_palette::CommandPalette,
    /// Focus mode + Pomodoro timer panel (task #25, V4).
    ///
    /// `Ctrl+Shift+F` enters a distraction-free focus mode: the tab bar is
    /// hidden and a compact Pomodoro countdown widget with an arc progress ring
    /// floats in the top-right corner. `Esc` exits focus mode (instead of
    /// quitting). The embedded `PomodoroTimer` is ticked from `about_to_wait`.
    focus: panels::focus_panel::FocusModePanel,
    /// Picture-in-picture floating video window (task #21).
    ///
    /// `Ctrl+Shift+V` opens a compact 320×180 card that keeps a tab's `<video>`
    /// element visible (poster placeholder) while the page scrolls or the user
    /// switches tabs. Implemented as an in-window overlay (the ad-hoc panel
    /// convention) — a true second OS window awaits multi-window support. The
    /// card can be dragged by its title bar.
    pip: panels::pip_window::PipWindow,
    /// Right-button drag gesture recognizer (§7B.3).
    ///
    /// Tracks right-button drags, classifies the trajectory into L/R/U/D/LD/RD,
    /// and maps each direction to a [`GestureAction`] via a configurable
    /// [`GestureMap`].  Default bindings: Left=Back, Right=Forward,
    /// LeftDown=CloseTab, RightDown=NewTab.
    gesture: input::gesture::GestureRecognizer,
    /// SQLite-backed omnibox bang-alias registry (§7B.4).
    ///
    /// Seeded with `!g` (Google) and `!gh` (GitHub) on startup.  Custom aliases
    /// are addable via `set(trigger, expansion)`.
    omnibox_aliases: lumen_storage::OmniboxAliases,
    /// In-session notes created via `@notes <text>` in the omnibox.
    ///
    /// Persisted in-memory for the session; each entry is a raw text string.
    /// Displayed nowhere yet — UI is a future task.
    notes: Vec<String>,
    /// §12.3 Read-later storage: persists HTML snapshots of saved pages.
    ///
    /// Populated by the `@read-later <url>` omnibox command: a background thread
    /// fetches the page HTML and calls `save()`. In-memory only (no SQLite path
    /// for the first ship — drop-in replacement once a `read_later.db` path is
    /// wired through the profile directory).
    read_later_store: lumen_knowledge::ReadLater,
    /// §12.3 Read-later panel state (Ctrl+Shift+R).
    read_later_panel: panels::read_later_panel::ReadLaterPanel,
    /// Channel receiver for completed background read-later fetches.
    ///
    /// Background threads send `(url, title, html_bytes)` here when done.
    /// Drained in `about_to_wait` to call `read_later_store.save()`.
    read_later_rx: std::sync::mpsc::Receiver<(String, String, Vec<u8>)>,
    /// Sender half of the read-later fetch channel (cloned into each background thread).
    read_later_tx: std::sync::mpsc::Sender<(String, String, Vec<u8>)>,
    /// Cookie-banner auto-dismiss preference (7C.3).
    ///
    /// When `true` (default) the JS shim in `lumen-js` auto-clicks consent-banner
    /// accept buttons on every page load. When `false` banners are shown normally.
    /// Toggle via `Ctrl+Shift+K` or a future settings UI.
    cookie_banner_dismiss: bool,
    /// Idle GC tick: drains dead DOM node IDs every 30 s and purges JS-side
    /// per-node caches (`_lumen_listeners`, `_input_values`) via `_lumen_gc_collect`.
    gc_tick: gc_tick::GcTick,
    /// Throttled OS memory pressure poller (ADR-008 §10H).
    ///
    /// Polled every 5 s in `about_to_wait`.  On `Medium` or `High` pressure,
    /// [`CacheRegistry::broadcast_pressure`] is called on `cache_registry`, and
    /// owned caches (`image_cache`, renderer `layer_cache`) are evicted directly.
    memory_poll: memory_poll::MemoryPollTick,
    /// Registry of cross-session shared caches (ADR-008 §10D.3).
    ///
    /// Caches registered here receive `on_memory_pressure` broadcasts from the
    /// poll loop.  Owned per-page caches (`image_cache`, layer cache) are evicted
    /// directly rather than through the registry to avoid shared-ownership overhead.
    cache_registry: lumen_core::ext::CacheRegistry,
    /// Deterministic render mode (8F).
    ///
    /// When `true` (`--deterministic` CLI flag): window opens at 1280×800,
    /// `Date.now()` is frozen at 0, `Math.random` uses a seeded PRNG, and
    /// `requestAnimationFrame` callbacks receive a 0 ms timestamp.
    /// Intended for snapshot testing and reproducible output.
    deterministic: bool,
    /// DevTools JS console panel (§7E.5).
    ///
    /// Captures `console.log/warn/error` output from the active page's JS runtime.
    /// Visible as a bottom overlay; toggled with `F12`.
    devtools_console: devtools::console_panel::ConsolePanel,
    /// DevTools DOM inspector panel (§7E.1).
    ///
    /// While active, hovering highlights the box under the cursor with a
    /// box-model overlay and clicking pins a node, showing its computed style
    /// in a right-docked side panel. Toggled with `Ctrl+Shift+I`.
    dom_inspector: devtools::inspector::DomInspectorPanel,
    /// DevTools network log panel (§7E.4).
    ///
    /// Shows a live log of HTTP requests (method / status / timing / URL),
    /// fed by `NetworkLogSink` from the engine's `EventSink`. Bottom overlay,
    /// toggled with `Ctrl+Shift+E`.
    network_panel: devtools::network_panel::NetworkPanel,
    /// Privacy network panel (V5).
    ///
    /// A privacy-focused, right-docked overlay sharing the same `NetworkLog` as
    /// [`network_panel`]: it presents the request stream as a newest-first log of
    /// tracker domains with blocked/allowed status and the matched filter rule,
    /// plus a blocked/allowed summary. Toggled with `Ctrl+Shift+Y`.
    ///
    /// [`network_panel`]: Lumen::network_panel
    privacy: panels::privacy_panel::PrivacyPanel,
    /// Persistent accessibility preferences store (task E-2).
    ///
    /// Backed by SQLite (in-memory for the session). Stores font-size
    /// multiplier, prefers-reduced-motion, forced-colors, and cursor size.
    /// Read on panel open; written when panel closes.
    a11y_store: lumen_storage::A11yPrefs,
    /// Accessibility settings panel overlay (task E-2, `Ctrl+Shift+Q`).
    ///
    /// A centred 300×260 px modal. Holds a working draft; on close the draft
    /// is persisted to `a11y_store` and media changes are re-delivered to JS.
    a11y_panel: panels::a11y_panel::A11yPanel,
    /// Persistent browser settings store (task D-7).
    ///
    /// Backed by SQLite (in-memory for the session). Stores homepage, search
    /// engine ID, shields, fingerprint mode, DoH, font size, theme, and
    /// download path. Read on panel open; written when panel closes.
    settings_store: lumen_storage::BrowserSettings,
    /// Settings page overlay state (task D-7, `about:settings`).
    ///
    /// `Ctrl+,` (or navigating to `about:settings`) toggles a centred
    /// 640×480 overlay with four tabbed sections: General, Privacy,
    /// Appearance, Downloads.
    settings_panel: panels::settings_panel::SettingsPanel,
    /// Keyboard shortcuts panel (Ctrl+Shift+/, §D-4).
    ///
    /// Shows all `KeyCommand` bindings with rebind-on-click support.
    shortcuts_panel: panels::shortcuts_panel::ShortcutsPanel,
    /// Whether the curated system-font fallback chain has been preloaded into
    /// the renderer (CSS Fonts L4 §5.3 codepoint cascade).
    ///
    /// The renderer can fall back per-glyph across loaded faces, but those
    /// faces must first be loaded via `Renderer::preload_curated_fallbacks`.
    /// Without it, CJK / emoji / RTL / Indic codepoints on pages with no
    /// explicit `font-family` for that script render as `.notdef`. Preloading
    /// is a one-time, idempotent operation (the curated families are system
    /// fonts, identical across pages), so this guard runs it once after the
    /// first page provides a `FontProvider`.
    fallbacks_preloaded: bool,
    /// Virtual URL shown in the address bar after `history.pushState` /
    /// `history.replaceState`.  `None` → use `source.url_str()`.
    /// Reset to `None` on any full navigation.
    display_url: Option<String>,
    /// Serialised JS state JSON for the current history entry, mirrored from JS
    /// so the shell can populate `NavEntry::same_doc_state_json` on pushState.
    /// `"null"` until a `pushState`/`replaceState` call updates it.
    current_history_state_json: String,
    /// Node ID of the currently fullscreen element, or `None` if not fullscreen.
    ///
    /// Set when `requestFullscreen()` is called in JS and cleared when
    /// `document.exitFullscreen()` or `Escape` exits fullscreen.  Used to deliver
    /// `_lumen_notify_fullscreen_exit()` when the OS exits fullscreen externally.
    fullscreen_nid: Option<u32>,
    /// Active CSS View Transition (CSS View Transitions L1 §4).
    ///
    /// Set when `document.startViewTransition(callback)` fires `_lumen_vt_end`.
    /// The `old_dl` snapshot fades out over the new display list for `duration_ms`.
    /// `None` when no transition is active.
    view_transition: Option<ViewTransitionState>,
    /// Tab auto-archive state (7A.5).
    ///
    /// Background tabs idle for more than `ARCHIVE_AFTER_MS` are moved here from
    /// the visible tab strip.  Only a title + URL string is retained; restoring
    /// opens a fresh navigation to that URL.  The archive button (rightmost 36 px
    /// of the tab bar) shows a count badge and toggles the archive panel.
    archive: tabs::archive::TabArchive,
    /// Timestamp (wall ms) when restore of a hibernated tab began.
    ///
    /// `Some(ms)` = spinner overlay is active; `None` = no restoration in progress.
    /// Set at the start of `restore_hibernated_tab` and cleared when restore completes.
    restore_spinner_start_ms: Option<f64>,
    /// Active element resize: `Some((node_id, start_x, start_y))` when user is dragging
    /// the resize grip. `None` when no resize is active.
    /// Set on MouseInput Pressed over a resize grip, cleared on MouseInput Released.
    /// During CursorMoved, width/height are updated via JS binding.
    resize_active: Option<(lumen_dom::NodeId, f32, f32)>,
    /// Original page source stored when Reader View (§D-3) is active.
    ///
    /// `Some` when the current page is showing the clean reader HTML (F9 toggle);
    /// `None` in normal browsing mode.  Toggling F9 again restores this source.
    reader_original_source: Option<PageSource>,
}

/// State for an in-progress CSS View Transition cross-fade (CSS View Transitions L1).
///
/// Holds the captured old display list and timing parameters.
struct ViewTransitionState {
    /// Display list captured before the JS callback mutated the DOM.
    old_dl: lumen_paint::DisplayList,
    /// Wall-clock epoch offset (ms) when the cross-fade animation started.
    start_ms: f64,
    /// Total cross-fade duration in milliseconds (currently 300 ms).
    duration_ms: f64,
}

/// CSS View Transitions L1 — event kind emitted by `document.startViewTransition`.
#[derive(Debug)]
#[allow(dead_code)]
enum ViewTransitionEvent {
    /// Callback is about to run — shell should snapshot the current frame.
    Begin,
    /// Callback finished — shell should relayout and start the cross-fade animation.
    End,
}

impl Lumen {
    /// Finds a layout box with a resize grip at position (x, y) in the layout tree.
    /// Returns the NodeId of that element, or None if no grip is found.
    /// This is used in B-7: CSS Resize property Phase 1 to detect mouse clicks on grips.
    fn find_resize_grip_node(
        &self,
        b: &lumen_layout::LayoutBox,
        x: f32,
        y: f32,
    ) -> Option<lumen_dom::NodeId> {
        // Check this box first
        if lumen_paint::point_on_resize_grip(b, x, y) {
            return Some(b.node);
        }

        // Recursively check children
        for child in &b.children {
            if let Some(nid) = self.find_resize_grip_node(child, x, y) {
                return Some(nid);
            }
        }

        None
    }

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
        // Apply <meta viewport initial-scale> + user zoom to derive the CSS layout viewport.
        let meta_scale = meta_initial_scale(src);
        let (css_w, css_h) =
            zoom::effective_viewport(vp_size.width, vp_size.height, meta_scale, self.zoom_factor);
        let viewport = Size::new(css_w, css_h);
        // Set interactive hover/focus/active state for this layout pass so that
        // :hover / :focus / :active / :focus-within CSS rules evaluate correctly.
        lumen_layout::set_interactive_state(self.hovered_nid, self.focused_node, self.active_nid);
        let (new_dl, lb) = relayout_page(src, viewport, &self.hyp_provider, self.dark_mode);
        lumen_layout::clear_interactive_state();
        self.content_height = content_height_of(&new_dl);
        self.content_width = content_width_of(&new_dl);
        self.tile_grid.update_from_diff(&self.display_list, &new_dl);
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
        // Promote nodes with will-change: transform/opacity/filter to GPU layers so
        // animation ticks can update only the layer matrix, bypassing relayout.
        // CSS: will-change — P4 wires ComputedStyle.will_change to promote_layer calls here.
        if let (Some(lb_ref), Some(r)) = (self.layout_box.as_ref(), self.renderer.as_mut()) {
            promote_will_change_layers(lb_ref, r.as_mut());
        }
        self.update_snap_containers();
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
        {
            // Lazy-load requests drained while `self` is borrowed immutably;
            // fetched after the borrow ends (fetch needs `&mut self`).
            let mut lazy_reqs: Vec<(u32, String)> = Vec::new();
            if let (Some(js), Some(lb_ref)) = (&self.js_ctx, self.layout_box.as_ref()) {
                js.update_layout_rects(collect_layout_rects(lb_ref));
                js.update_computed_styles(collect_computed_styles(lb_ref));
                js.update_viewport_size(viewport.width, viewport.height);
                js.deliver_layout_observers();
                // CSS MQ L4 §4.2: re-evaluate matchMedia() lists against the new
                // viewport. `dark_mode` mirrors the OS `prefers-color-scheme`,
                // read from winit at window creation / refreshed on ThemeChanged.
                js.deliver_media_query_changes(viewport.width, viewport.height, self.dark_mode, self.a11y_store.reduced_motion());
                // After fresh rects are in JS: fire lazy-load proximity check.
                // Images that entered the viewport+margin are queued by JS via
                // _lumen_request_lazy_image_load; we drain and fetch them below.
                js.deliver_lazy_images();
                lazy_reqs = js.take_lazy_image_requests();
                // Keep JS scroll-state cache in sync so scrollTop/scrollLeft reads
                // immediately after relayout return the correct clamped values.
                let scroll_states = collect_scroll_containers(lb_ref)
                    .iter()
                    .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                    .collect();
                js.update_scroll_states(scroll_states);
            }
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
            let bytes = match fetch_image_bytes(&url, &base, &self.event_sink, Some(Arc::clone(&self.cookie_jar))) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Lazy: пропуск {url}: {e}");
                    continue;
                }
            };

            // Animated GIF detection for lazy-loaded images.
            if lumen_image::is_gif(&bytes) {
                match lumen_image::decode_gif_animated(&bytes) {
                    Ok(gif) if gif.frames.len() > 1 => {
                        let first = gif.frames[0].image.clone();
                        if let Some(src) = self.layout_source.as_ref() {
                            let mut doc = src.document.lock().unwrap();
                            let node_id = NodeId::from_index(nid as usize);
                            apply_intrinsic_size(&mut doc, node_id, first.width, first.height);
                        }
                        eprintln!(
                            "Lazy GIF-анимация: {} ({}×{}, {} кадров)",
                            url, gif.width, gif.height, gif.frames.len()
                        );
                        if let Some(r) = self.renderer.as_mut() {
                            if let Err(e) = r.register_image(url.clone(), &first) {
                                eprintln!("Lazy GIF: не зарегистрирована {url}: {e}");
                            }
                            self.image_cache.insert(lumen_image::ImageKey::new(&url), first);
                        } else {
                            self.pending_images.push((url.clone(), first));
                        }
                        self.gif_last_frame.remove(&url);
                        self.animated_gifs.insert(url, gif);
                        continue;
                    }
                    Ok(gif) => {
                        if let Some(frame) = gif.frames.into_iter().next() {
                            let img = frame.image;
                            if let Some(src) = self.layout_source.as_ref() {
                                let mut doc = src.document.lock().unwrap();
                                let node_id = NodeId::from_index(nid as usize);
                                apply_intrinsic_size(&mut doc, node_id, img.width, img.height);
                            }
                            eprintln!("Lazy загружена (GIF, 1 кадр): {url} ({}×{})", img.width, img.height);
                            if let Some(r) = self.renderer.as_mut() {
                                if let Err(e) = r.register_image(url.clone(), &img) {
                                    eprintln!("Lazy: не зарегистрирована {url}: {e}");
                                }
                                self.image_cache.insert(lumen_image::ImageKey::new(&url), img);
                            } else {
                                self.pending_images.push((url, img));
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        eprintln!("Lazy: не декодируется GIF {url}: {e}");
                        continue;
                    }
                }
            }

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
                self.image_cache.insert(lumen_image::ImageKey::new(&url), image);
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
        click_log::log_fragment(&fragment, target_y.is_some());
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
        click_log::log_load_start(&self.source.describe());
        println!("Reload: {}", self.source.describe());

        // Phase 4c: попробовать загрузить через GpuSession (WinitSession)
        // для File и Url; fallback к старому пути для Snapshot
        let load_result = if let Some(page) = self.reload_via_gpu_session() {
            // WinitSession загрузка успешна
            Ok((page, None, None))
        } else {
            // Fallback к старому пути (PageSource::Snapshot, или ошибка WinitSession)
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            let ls_store = self.source.origin_str().map(|o| {
                Arc::clone(self.ls_storage.entry(o).or_insert_with(|| {
                    Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                }))
            });
            let idb_backend = self
                .source
                .url_str()
                .and_then(|u| idb_store_for_url(u, self.idb_dir.as_deref()));
            let sw_backend = self.source.origin_str().map(|o| {
                Arc::new(lumen_storage::SwStore::new(Arc::clone(&self.sw_backend), o))
                    as Arc<dyn lumen_core::ext::SwBackend>
            });
            self.source.load(self.event_sink.clone(), viewport, ls_store, idb_backend, sw_backend, &self.hyp_provider, self.cookie_banner_dismiss)
        };

        match load_result {
            Ok((page, new_layout_source, new_js_ctx)) => {
                // Drop JS closures before layout_source to release Arc<Mutex<Document>>
                // clones held inside QuickJS closures before LayoutSource's Arc drops.
                self.js_ctx = None;
                self.layout_source = new_layout_source;
                self.js_ctx = new_js_ctx;
                self.content_height = content_height_of(&page.display_list);
                self.content_width = content_width_of(&page.display_list);
                // On full page load, mark all tiles dirty — content has changed completely.
                self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
                self.display_list = page.display_list;
                self.animation_scheduler.clear();
                self.transition_scheduler = TransitionScheduler::new();
                self.prev_styles.clear();
                collect_box_styles(&page.layout_box, &mut self.prev_styles);
                self.layout_box = Some(page.layout_box);
                self.update_snap_containers();
                // Push initial layout geometry so JS can query bounding rects
                // immediately after page load (before the first relayout).
                #[cfg(feature = "quickjs")]
                if let (Some(js), Some(lb_ref)) = (&self.js_ctx, self.layout_box.as_ref()) {
                    let viewport = self.renderer.as_ref().map_or_else(
                        || Size::new(1024.0, 720.0),
                        |r| {
                            let s = r.viewport_size();
                            Size::new(s.width as f32, s.height as f32)
                        },
                    );
                    js.update_layout_rects(collect_layout_rects(lb_ref));
                    js.update_computed_styles(collect_computed_styles(lb_ref));
                    js.update_viewport_size(viewport.width, viewport.height);
                }
                self.title = page.title;
                if let Some(t) = &self.title {
                    self.tab_strip.set_active_title(t.as_str());
                }
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
                // Reset CPU image cache for the reloaded page (10E.4 scroll-discard).
                self.image_cache.clear();
                if let Some(r) = self.renderer.as_mut() {
                    // Старая GPU-cache картинок относится к предыдущей странице
                    // (даже если src совпадает, content мог измениться). Чистим
                    // и регистрируем заново.
                    r.clear_images();
                    for (src, image) in &page.images {
                        if let Err(err) = r.register_image(src.clone(), image) {
                            eprintln!("Картинка {src} не зарегистрирована: {err}");
                        }
                        self.image_cache.insert(lumen_image::ImageKey::new(src), image.clone());
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
                let title = self.title.as_deref().unwrap_or("");
                click_log::log_load_ok(&self.source.describe(), title);
                click_log::log_page_ready(&self.source.describe(), self.scroll_y);
            }
            Err(err) => {
                click_log::log_load_err(&self.source.describe(), &err.to_string());
                eprintln!("Ошибка reload {}: {err}", self.source.describe());
            }
        }
    }

    /// Попытаться загрузить страницу через GpuSession (WinitSession).
    /// Возвращает LoadedPage если успешно, иначе None (fallback к старому пути).
    ///
    /// Phase 4c: использует WinitSession::render_to_gpu() вместо inline pipeline
    /// для PageSource::File и PageSource::Url.
    fn reload_via_gpu_session(&mut self) -> Option<LoadedPage> {
        use lumen_driver::{WinitSession, GpuSession};

        // Преобразовать PageSource в URL для WinitSession
        let url = match &self.source {
            PageSource::File(path) => {
                format!("file://{}", path.display())
            }
            PageSource::Url(u) => u.clone(),
            _ => return None, // Snapshot и Empty обработаны отдельно
        };

        let viewport = self.renderer.as_ref().map_or_else(
            || Size::new(1024.0, 720.0),
            |r| {
                let s = r.viewport_size();
                Size::new(s.width, s.height)
            },
        );

        // Создать сессию с нужным viewport
        let mut session = WinitSession::with_viewport(viewport.width, viewport.height);

        // Загрузить страницу через WinitSession
        if session.navigate(&url).is_err() {
            return None;
        }

        // Получить RenderedPage через render_to_gpu()
        let rendered = match session.render_to_gpu() {
            Ok(r) => r,
            Err(_) => return None,
        };

        // Преобразовать RenderedPage в LoadedPage
        // Преобразовать lumen_driver::JsNavigateRequest в shell::JsNavigateRequest
        let js_navigate = rendered.js_navigate.map(|nav| {
            if nav.replace {
                JsNavigateRequest::Replace(nav.url)
            } else {
                JsNavigateRequest::Push(nav.url)
            }
        });

        Some(LoadedPage {
            display_list: rendered.display_list,
            title: rendered.title,
            images: rendered.images,
            animated_gifs: Vec::new(), // lumen-driver path has no animated GIF support yet
            lazy_pairs: Vec::new(), // Phase 4c: TODO integrate lazy loading
            layout_box: rendered.layout_box,
            font_registry: rendered.font_registry,
            js_navigate,
        })
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
        if matches!(self.source, PageSource::Empty | PageSource::AboutBlank) {
            return;
        }
        let source = self.source.clone();
        let sink = Arc::clone(&self.event_sink);
        let proxy = self.load_proxy.clone();
        let cookie_jar = Arc::clone(&self.cookie_jar);

        std::thread::spawn(move || {
            let raw = match source.load_bytes(Arc::clone(&sink), Some(cookie_jar)) {
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
        let viewport = Size::new(vp_size.width, vp_size.height);

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
        self.update_snap_containers();

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
        // Full page load: force all tiles dirty.
        self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
        self.display_list = page.display_list;
        self.animation_scheduler.clear();
        self.transition_scheduler = TransitionScheduler::new();
        self.prev_styles.clear();
        collect_box_styles(&page.layout_box, &mut self.prev_styles);
        self.layout_box = Some(page.layout_box);
        self.update_snap_containers();
        self.title = page.title.clone();
        if let Some(t) = &self.title {
            self.tab_strip.set_active_title(t.as_str());
        }
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

        // Индексировать страницу в history_fts для omnibox (@history) и записать
        // в history_store для панели истории (Ctrl+H).
        // Пропускаем Empty и File sources — только HTTP(S) и bfcache snapshots.
        if let Some(url) = self.source.url_str() {
            let title = page.title.as_deref().unwrap_or("");
            let _ = self.history_fts.index(self.next_history_id, url, title, "");
            self.next_history_id += 1;
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let _ = self.history_store.record_visit(url, title, now_secs);
        }
        // Clear GIF animation state from previous page.
        self.animated_gifs.clear();
        self.gif_last_frame.clear();
        // Populate animated GIFs from new page; reset frame tracking.
        for (url, gif) in page.animated_gifs {
            self.animated_gifs.insert(url, gif);
        }

        // Update shields panel domain and clear per-page blocked counts.
        {
            let domain = self.source.url_str().and_then(|u| {
                // Extract hostname from the loaded URL for the shields panel.
                let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))?;
                let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let host = &rest[..host_end];
                let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
                if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
            });
            self.shields.clear_log();
            self.shields.set_domain(domain);
        }

        // Clear the network panel log so each page starts with a fresh request list.
        self.network_panel.clear_log();

        // Update permission panel origin on navigation.
        {
            let origin = self.source.url_str().and_then(|u| {
                // Build bare origin (scheme + host) for permission keying.
                let scheme_end = u.find("://")?;
                let scheme = &u[..scheme_end + 3];
                let rest = &u[scheme_end + 3..];
                let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let host = &rest[..host_end];
                let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
                if host.is_empty() { None } else { Some(format!("{}{}", scheme, host.to_ascii_lowercase())) }
            });
            self.permission.set_origin(origin);
        }

        // Reset CPU image cache for the new page (10E.4 scroll-discard).
        self.image_cache.clear();
        if let Some(r) = self.renderer.as_mut() {
            r.set_font_provider(Some(page.font_registry.clone()));
            // Warm the curated system-font fallback chain once, now that a
            // FontProvider (this page's FontRegistry, which wraps the system
            // font index) is available. Loads emoji / CJK / RTL / Indic / Thai
            // faces into the renderer so the codepoint cascade can resolve
            // glyphs Inter lacks. One-time: the faces persist across pages and
            // the curated families are system fonts identical for every page.
            if !self.fallbacks_preloaded {
                r.preload_curated_fallbacks();
                self.fallbacks_preloaded = true;
            }
            r.clear_images();
            for (src, image) in &page.images {
                if let Err(err) = r.register_image(src.clone(), image) {
                    eprintln!("Картинка {src} не зарегистрирована: {err}");
                }
                self.image_cache.insert(lumen_image::ImageKey::new(src), image.clone());
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
        // HTML LS §8.2.3 — all resources loaded: readyState → "complete" + window.load event.
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            js.notify_window_loaded();
        }

        // If zoom or <meta viewport initial-scale> is active, relayout with the
        // correct effective viewport. The initial load used the raw physical size.
        let zoom = self.zoom_factor;
        let meta_scale = self.layout_source.as_ref().map(meta_initial_scale).unwrap_or(1.0);
        if (zoom - 1.0).abs() > 0.001 || (meta_scale - 1.0).abs() > 0.001 {
            self.relayout();
        }
    }
}

impl ApplicationHandler<LoadEvent> for Lumen {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let (win_w, win_h) = if self.deterministic { (1280.0, 800.0) } else { (1024.0, 720.0) };
        let attrs = Window::default_attributes()
            .with_title(window_title(self.title.as_deref()))
            .with_inner_size(LogicalSize::new(win_w, win_h))
            .with_position(LogicalPosition::new(0, 0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Не удалось создать окно: {err}");
                event_loop.exit();
                return;
            }
        };

        let mut renderer = match backend_factory::create_backend(window.clone(), INTER_FONT.to_vec()) {
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
            self.image_cache.insert(lumen_image::ImageKey::new(&src), image);
        }

        // CSS Media Queries L5 §5.2 — read the OS `prefers-color-scheme` once the
        // window exists. winit resolves it per platform (Win32 immersive dark mode,
        // macOS NSAppearance, Linux portal/XSettings); `None` → light fallback.
        // In deterministic/headless runs we keep light to preserve snapshot stability.
        if !self.deterministic {
            self.dark_mode = platform::dark_mode::theme_prefers_dark(window.theme());
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
                        Size::new(s.width, s.height)
                    },
                );
                let ls_store = ls_store_for_base(&raw.base, &mut self.ls_storage);
                let idb_backend = idb_store_for_base(&raw.base, self.idb_dir.as_deref());
                let sw_backend = sw_store_for_base(&raw.base, &self.sw_backend);
                match render_bytes(&raw.bytes, raw.content_type, &raw.base, self.event_sink.clone(), viewport, &mut self.preload_dispatched, ls_store, idb_backend, sw_backend, &self.hyp_provider, self.cookie_banner_dismiss, self.deterministic, self.dark_mode, Some(Arc::clone(&self.cookie_jar))) {
                    Ok((page, new_layout_source, new_js_ctx)) => {
                        click_log::log_load_ok(&self.source.describe(), page.title.as_deref().unwrap_or(""));
                        self.apply_loaded_page(page, Some(new_layout_source), new_js_ctx);
                        click_log::log_page_ready(&self.source.describe(), self.scroll_y);
                    }
                    Err(e) => {
                        click_log::log_load_err(&self.source.describe(), &e.to_string());
                        eprintln!("Ошибка финального render {}: {e}", self.source.describe());
                    }
                }
            }
            LoadEvent::LoadError(msg) => {
                click_log::log_load_err(&self.source.describe(), &msg);
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
        // WebSocket pump runs here too so onopen/onmessage/onclose fire promptly.
        if let Some(js) = &self.js_ctx {
            js.tick_timers();
            js.pump_websockets();
            js.pump_sse();
            js.pump_workers();
            js.pump_broadcast_channels();
            js.pump_shared_workers();
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

        // ── Canvas 2D: upload dirty <canvas> bitmaps to the renderer ──────────
        // JS Canvas 2D draws into per-node CPU buffers (lumen_canvas::Context2D).
        // Each frame we drain the dirty buffers and register them under the same
        // `canvas:{nid}` key the display list emits, then request a repaint.
        let canvas_updates = self
            .js_ctx
            .as_ref()
            .map(|js| js.flush_canvas_updates())
            .unwrap_or_default();
        if !canvas_updates.is_empty() {
            if let Some(r) = self.renderer.as_mut() {
                for (nid, w, h, rgba) in &canvas_updates {
                    let image = lumen_image::Image {
                        width: *w,
                        height: *h,
                        format: lumen_image::PixelFormat::Rgba8,
                        data: rgba.clone(),
                        icc_profile: None,
                    };
                    if let Err(e) = r.register_image(format!("canvas:{nid}"), &image) {
                        eprintln!("Canvas: не зарегистрирован canvas:{nid}: {e}");
                    }
                }
            }
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }

        // ── History API: pushState/replaceState URL updates ───────────────────
        // Drain URL-update notifications from history.pushState/replaceState.
        // pushState adds a same-document back-stack entry; replaceState updates
        // the displayed URL only.  Neither triggers a page load.
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            let updates = js.take_history_url_updates();
            for (is_push, url, new_state_json) in updates {
                if is_push {
                    // pushState: save current state to nav_back as same-doc entry.
                    let old_display = self.display_url.take();
                    let old_state = std::mem::replace(
                        &mut self.current_history_state_json,
                        new_state_json,
                    );
                    self.nav_back.push(NavEntry {
                        source: self.source.clone(),
                        scroll_x: self.scroll_x,
                        scroll_y: self.scroll_y,
                        display_url: old_display,
                        same_doc_state_json: Some(old_state),
                    });
                    self.display_url = Some(url);
                } else {
                    // replaceState: update URL + state, no nav_back push.
                    self.current_history_state_json = new_state_json;
                    self.display_url = Some(url);
                }
            }
        }

        // ── Native input injection (ADR-007 §8C) ─────────────────────────────
        // Drain injected commands and route through the same dispatch path as
        // real OS events so events have isTrusted=true.
        let injected: Vec<input::InputCommand> = self.input_rx.drain();
        for cmd in injected {
            match cmd {
                input::InputCommand::Click { x, y } => {
                    self.handle_click_at(x, y);
                }
                input::InputCommand::TypeText { text } => {
                    let chars: Vec<char> = text.chars().collect();
                    for ch in chars {
                        self.inject_char(ch);
                    }
                }
                input::InputCommand::MouseMove { x, y } => {
                    self.dispatch_mouse_move(x, y);
                }
                input::InputCommand::Scroll { x, y } => {
                    self.scroll_x = clamp_scroll(x, self.max_scroll_x());
                    self.scroll_y = clamp_scroll(y, (self.content_height - self.viewport_height_css()).max(0.0));
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
            }
        }

        // Download manager: drain completion events from background threads.
        self.downloads.poll();

        // §12.3 Read-later: drain completed background page fetches and persist.
        while let Ok((url, title, html)) = self.read_later_rx.try_recv() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let _ = self.read_later_store.save(&url, &title, &html, "", &[], now);
            self.refresh_read_later();
            self.request_redraw();
        }

        // Web Notifications API: deliver pending OS notifications queued by JS.
        if let Some(js) = &self.js_ctx {
            for (title, body) in js.take_notification_requests() {
                notification::show_os_notification(&title, &body);
            }
        }

        // window.open() popup requests: each entry opens a new tab and navigates it
        // to the requested URL.  Executed after the page render so the current tab
        // stays visible while the new tab loads.
        if let Some(js) = &self.js_ctx {
            let popups = js.take_window_open_requests();
            for (url, _target, _width, _height) in popups {
                self.open_new_tab();
                let url = if url.is_empty() {
                    "about:blank".to_owned()
                } else {
                    url
                };
                self.navigate_to(PageSource::Url(url));
            }
        }

        // Fullscreen API: apply OS fullscreen on requestFullscreen() / exitFullscreen().
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            for (enter, nid) in js.take_fullscreen_requests() {
                if enter {
                    self.fullscreen_nid = Some(nid);
                    if let Some(w) = self.window.as_ref() {
                        w.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                    }
                } else {
                    self.fullscreen_nid = None;
                    if let Some(w) = self.window.as_ref() {
                        w.set_fullscreen(None);
                    }
                }
            }
        }

        // CSS View Transitions API: drain snapshot/animation events from JS.
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            for event in js.take_view_transition_events() {
                match event {
                    ViewTransitionEvent::Begin => {
                        // Capture current display list as the "before" snapshot.
                        self.view_transition = Some(ViewTransitionState {
                            old_dl: self.display_list.clone(),
                            start_ms: 0.0,
                            duration_ms: 300.0,
                        });
                    }
                    ViewTransitionEvent::End => {
                        // Callback finished — relayout picks up DOM mutations,
                        // then the render step blends old_dl (fading out) over
                        // the new display list.
                        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                        if let Some(vt) = &mut self.view_transition {
                            vt.start_ms = now_ms;
                        }
                        self.relayout();
                        if let Some(w) = self.window.as_ref() {
                            w.request_redraw();
                        }
                    }
                }
            }
        }

        // DevTools console: drain JS console.log/warn/error messages into the panel.
        if let Some(js) = &self.js_ctx {
            let msgs = js.take_console_messages();
            if !msgs.is_empty() {
                self.devtools_console.push_batch(msgs);
                if self.devtools_console.visible {
                    self.request_redraw();
                }
            }
        }

        // JS scroll requests: drain programmatic scrolls queued by scrollTo/scrollBy/
        // scrollIntoView.  Scroll position is applied directly to the existing layout
        // tree (no CSS re-computation needed — scroll only affects paint offsets), the
        // display list is rebuilt cheaply, and JS scroll-state cache is updated so
        // subsequent scrollTop/scrollLeft reads return the new values.
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            let scroll_reqs = js.take_scroll_requests();
            if !scroll_reqs.is_empty()
                && let Some(lb) = self.layout_box.as_mut()
            {
                let mut changed = false;
                for (nid, x, y) in scroll_reqs {
                    if set_scroll_position(lb, NodeId::from_index(nid as usize), x, y) {
                        changed = true;
                    }
                }
                if changed {
                    // Rebuild display list with the updated scroll offsets.
                    let new_dl = paint_ordered(lb);
                    self.tile_grid.update_from_diff(&self.display_list, &new_dl);
                    self.display_list = new_dl;
                    // Sync JS cache so scrollTop/scrollLeft reads are accurate.
                    let states = collect_scroll_containers(lb)
                        .iter()
                        .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                        .collect();
                    js.update_scroll_states(states);
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
            }
        }

        // DOM GC idle tick: drain dead node IDs and purge JS-side per-node caches.
        // Runs every 30 s to free _lumen_listeners / _input_values entries for
        // nodes that were detached from the tree and have no live JS references.
        if let (Some(ls), Some(js)) = (self.layout_source.as_ref(), self.js_ctx.as_ref()) {
            let dead = {
                let doc = ls.document.lock().unwrap();
                self.gc_tick.poll(&doc)
            };
            if let Some(dead_nids) = dead {
                let ids: Vec<u32> = dead_nids
                    .iter()
                    .map(|n| n.index() as u32)
                    .collect();
                js.gc_collect(&ids);
            }
        }

        // Tab lifecycle: advance tier timers, trigger hibernation for overdue tabs.
        self.tick_lifecycle();

        // Focus mode (task #25): advance the Pomodoro countdown and keep
        // redrawing while the ring animates (only while active and running).
        if self.focus.active {
            self.focus.tick(now_ms);
            if self.focus.timer.running {
                self.request_redraw();
            }
        }

        // Memory pressure: poll OS every 5 s; evict caches on Medium+ pressure.
        if let Some(level) = self.memory_poll.tick(&mut self.cache_registry) {
            self.image_cache.on_memory_pressure(level);
            if let Some(renderer) = &mut self.renderer {
                renderer.on_layer_memory_pressure(level);
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
                JsNavigateRequest::Push(url) => {
                    click_log::log_js_nav("pushState/location.href", &url);
                    self.navigate_to(PageSource::Url(url));
                }
                JsNavigateRequest::Replace(url) => {
                    click_log::log_js_nav("replaceState/location.replace", &url);
                    self.navigate_replace(PageSource::Url(url));
                }
                JsNavigateRequest::Reload => {
                    click_log::log_js_nav("location.reload", &self.source.describe());
                    self.reload();
                }
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
                self.save_full_session();
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
            WindowEvent::ThemeChanged(theme) => {
                // OS switched light↔dark. Update the stored preference and re-run
                // layout: relayout() re-evaluates `@media (prefers-color-scheme)`
                // and pushes the new value to JS matchMedia listeners via
                // deliver_media_query_changes(.., self.dark_mode).
                let dark = platform::dark_mode::theme_prefers_dark(Some(theme));
                if dark != self.dark_mode {
                    self.dark_mode = dark;
                    self.relayout();
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
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
                // DevTools inspector: highlight the box under the cursor.
                if self.dom_inspector.visible {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let x_css = (position.x as f32) / dpr;
                    let y_css = (position.y as f32) / dpr;
                    let hovered = if y_css < tabs::strip::TAB_BAR_HEIGHT {
                        None
                    } else {
                        let (page_x, page_y) = self.page_point(x_css, y_css);
                        self.layout_box
                            .as_ref()
                            .and_then(|lb| hit_test(Point::new(page_x, page_y), lb))
                            .map(|r| r.node)
                    };
                    if self.dom_inspector.set_hovered(hovered) {
                        self.request_redraw();
                    }
                }
                // Feed current position to the gesture recognizer (right-drag tracking).
                {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    self.gesture.track(
                        (position.x as f32) / dpr,
                        (position.y as f32) / dpr,
                    );
                }
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
                // PiP window drag (task #21): follow the cursor while the title
                // bar is held, clamped to the window.
                if self.pip.dragging() {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let win_w = self.viewport_width_css();
                    let win_h = self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                    self.pip.drag_to(
                        (position.x as f32) / dpr,
                        (position.y as f32) / dpr,
                        win_w,
                        win_h,
                    );
                    self.request_redraw();
                }
                // CSS :hover tracking — find the element under the cursor and
                // trigger relayout when it changes so :hover rules re-evaluate.
                {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let x_css = (position.x as f32) / dpr;
                    let y_css = (position.y as f32) / dpr;
                    let new_hovered = if y_css < tabs::strip::TAB_BAR_HEIGHT {
                        None
                    } else {
                        let (page_x, page_y) = self.page_point(x_css, y_css);
                        self.layout_box
                            .as_ref()
                            .and_then(|lb| hit_test(Point::new(page_x, page_y), lb))
                            .map(|r| r.node)
                    };
                    if new_hovered != self.hovered_nid {
                        #[cfg(feature = "quickjs")]
                        let old_nid = self.hovered_nid;
                        self.hovered_nid = new_hovered;
                        self.relayout();
                        self.request_redraw();
                        // Dispatch hover-change events per W3C UI Events §17.5 / Pointer Events L2 §10.
                        #[cfg(feature = "quickjs")]
                        {
                            // Leave events on the element losing hover.
                            if let Some(old) = old_nid {
                                let nid = old.index() as u32;
                                self.js_pointer_event(nid, "pointerout",   x_css, y_css, 0, 0);
                                self.js_mouse_event(nid,   "mouseout",     x_css, y_css, 0, 0);
                                self.js_pointer_event(nid, "pointerleave", x_css, y_css, 0, 0);
                                self.js_mouse_event(nid,   "mouseleave",   x_css, y_css, 0, 0);
                            }
                            // Enter events on the element gaining hover.
                            if let Some(nw) = new_hovered {
                                let nid = nw.index() as u32;
                                self.js_pointer_event(nid, "pointerover",  x_css, y_css, 0, 0);
                                self.js_mouse_event(nid,   "mouseover",    x_css, y_css, 0, 0);
                                self.js_pointer_event(nid, "pointerenter", x_css, y_css, 0, 0);
                                self.js_mouse_event(nid,   "mouseenter",   x_css, y_css, 0, 0);
                            }
                        }
                    }
                }
                // Tab bar: update hovered_tab_idx for tooltip rendering.
                {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let x_css = (position.x as f32) / dpr;
                    let y_css = (position.y as f32) / dpr;
                    let win_w = self.viewport_width_css();
                    self.hovered_tab_idx = if y_css < tabs::strip::TAB_BAR_HEIGHT {
                        match tabs::strip::hit_test(&self.tab_strip, x_css, y_css, win_w) {
                            tabs::strip::TabHit::Tab(idx) => Some(idx),
                            _ => None,
                        }
                    } else {
                        None
                    };
                }
                // B-7: Active resize — update element width/height as mouse moves.
                #[cfg(feature = "quickjs")]
                if let Some((node_id, start_x, start_y)) = self.resize_active {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let x_css = (position.x as f32) / dpr;
                    let y_css = (position.y as f32) / dpr;
                    let delta_x = x_css - start_x;
                    let delta_y = y_css - start_y;
                    let nid_u32 = node_id.index() as u32;
                    self.eval_js(&format!(
                        "_lumen_apply_resize({}, {}, {});",
                        nid_u32, delta_x, delta_y
                    ));
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                self.hovered_tab_idx = None;
                self.resize_active = None; // Clear resize when cursor leaves window
                // Clear hover state when cursor leaves the window.
                if self.hovered_nid.is_some() {
                    // Dispatch leave events before clearing hovered state.
                    #[cfg(feature = "quickjs")]
                    if let Some(old) = self.hovered_nid {
                        let nid = old.index() as u32;
                        self.js_pointer_event(nid, "pointerout",   0.0, 0.0, 0, 0);
                        self.js_mouse_event(nid,   "mouseout",     0.0, 0.0, 0, 0);
                        self.js_pointer_event(nid, "pointerleave", 0.0, 0.0, 0, 0);
                        self.js_mouse_event(nid,   "mouseleave",   0.0, 0.0, 0, 0);
                    }
                    self.hovered_nid = None;
                    self.relayout();
                    self.request_redraw();
                }
                self.gesture.cancel();
                // Драг продолжается даже когда курсор вышел из окна — winit
                // продолжит слать CursorMoved-события за пределами client area,
                // пока зажата кнопка. Сбросим drag только на MouseInput Release
                // или если события прекратятся (мы не получим MouseInput, но
                // повторный CursorEntered/CursorMoved оживят drag — допустимо
                // для Phase 0).
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Right {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let (x_css, y_css) = self
                        .cursor_position
                        .map(|p| ((p.x as f32) / dpr, (p.y as f32) / dpr))
                        .unwrap_or((0.0, 0.0));
                    if state == ElementState::Pressed {
                        self.gesture.begin(x_css, y_css);
                    } else if state == ElementState::Released
                        && let Some(action) = self.gesture.finish()
                    {
                        self.execute_gesture_action(action, event_loop);
                    }
                } else if button != MouseButton::Left {
                    // Middle / back / forward — ignore.
                } else if state == ElementState::Pressed {
                    // CSS :active — set immediately on press so :active rules apply.
                    if self.active_nid != self.hovered_nid {
                        self.active_nid = self.hovered_nid;
                        self.relayout();
                        self.request_redraw();
                    }
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
                    // Fire mousedown + pointerdown on the hovered DOM element.
                    // Per W3C UI Events §17.6 + Pointer Events L2 §10 — fires before
                    // any default action (click). Only when cursor is over page content.
                    #[cfg(feature = "quickjs")]
                    if let Some(hov) = self.hovered_nid {
                        let nid = hov.index() as u32;
                        self.js_pointer_event(nid, "pointerdown", x_css, y_css, 0, 1);
                        self.js_mouse_event(nid, "mousedown", x_css, y_css, 0, 1);
                    }

                    // B-7: Check if click is on resize grip of any element in layout tree.
                    // If so, activate resize mode. This must be checked before other UI panels.
                    if let Some(ref layout_box) = self.layout_box
                        && let Some(nid) = self.find_resize_grip_node(layout_box, x_css, y_css) {
                            self.resize_active = Some((nid, x_css, y_css));
                            self.request_redraw();
                            return;
                        }

                    // Command palette (task #23): modal — captures every click.
                    // A click on a row activates it; a click on the scrim closes.
                    if self.command_palette.visible {
                        let win_w = self.viewport_width_css();
                        match panels::command_palette::hit_test(
                            &self.command_palette,
                            x_css,
                            y_css,
                            win_w,
                        ) {
                            panels::command_palette::PaletteHit::Row(filtered_idx) => {
                                self.command_palette.selected = filtered_idx;
                                if let Some(item) = self.command_palette.selected_item().cloned() {
                                    self.command_palette.close();
                                    self.activate_palette(&item, event_loop);
                                }
                            }
                            panels::command_palette::PaletteHit::Dismiss => {
                                self.command_palette.close();
                            }
                            panels::command_palette::PaletteHit::Inside => {}
                        }
                        self.request_redraw();
                        return;
                    }

                    // Focus mode widget (task #25): floating top-right card. A
                    // click on the ring pauses/resumes; the `×` corner exits.
                    if self.focus.active {
                        let win_w = self.viewport_width_css();
                        if let Some(hit) =
                            panels::focus_panel::hit_test(&self.focus, x_css, y_css, win_w)
                        {
                            match hit {
                                panels::focus_panel::FocusHit::TogglePause => {
                                    self.focus.timer.toggle_pause();
                                    if self.focus.timer.running {
                                        let now_ms =
                                            self.epoch.elapsed().as_secs_f64() * 1000.0;
                                        self.focus.tick(now_ms);
                                    }
                                }
                                panels::focus_panel::FocusHit::Exit => self.focus.exit(),
                            }
                            self.request_redraw();
                            return;
                        }
                    }

                    // Picture-in-picture window (task #21): floating draggable
                    // card. `×` closes, the centre button toggles play/pause, the
                    // title bar starts a drag, the body swallows the click.
                    if self.pip.active
                        && let Some(hit) = panels::pip_window::hit_test(&self.pip, x_css, y_css)
                    {
                        match hit {
                            panels::pip_window::PipHit::Close => self.pip.close(),
                            panels::pip_window::PipHit::PlayPause => self.pip.toggle_play(),
                            panels::pip_window::PipHit::Header => {
                                self.pip.begin_drag(x_css, y_css);
                            }
                            panels::pip_window::PipHit::Body => {}
                        }
                        self.request_redraw();
                        return;
                    }

                    // Tab bar occupies y = 0..TAB_BAR_HEIGHT — dispatch first.
                    if y_css < tabs::strip::TAB_BAR_HEIGHT {
                        let win_w = self.viewport_width_css();
                        // Archive panel: close on click-outside before checking button.
                        if self.archive.visible {
                            match tabs::archive::hit_test_panel(
                                &self.archive,
                                x_css,
                                y_css,
                                win_w,
                                tabs::strip::TAB_BAR_HEIGHT,
                            ) {
                                Some(tabs::archive::ArchiveHit::Restore(id)) => {
                                    if let Some(entry) = self.archive.take(id)
                                        && !entry.url.is_empty()
                                    {
                                        self.navigate_to(PageSource::Url(entry.url));
                                    }
                                    self.archive.close();
                                    self.request_redraw();
                                    return;
                                }
                                Some(tabs::archive::ArchiveHit::Dismiss(id)) => {
                                    self.archive.take(id);
                                    self.request_redraw();
                                    return;
                                }
                                Some(tabs::archive::ArchiveHit::Inside) => {
                                    self.request_redraw();
                                    return;
                                }
                                Some(tabs::archive::ArchiveHit::Outside) => {
                                    self.archive.close();
                                    self.request_redraw();
                                }
                                None => {}
                            }
                        }
                        // Archive toolbar button (rightmost 36 px of the tab bar).
                        if tabs::archive::hit_test_button(
                            x_css,
                            y_css,
                            win_w,
                            tabs::strip::TAB_BAR_HEIGHT,
                        ) {
                            self.archive.toggle();
                            self.request_redraw();
                            return;
                        }
                        // Tab area: pass effective width (excluding archive button).
                        let tab_area_w =
                            win_w - tabs::archive::ARCHIVE_BTN_W;
                        match tabs::strip::hit_test(&self.tab_strip, x_css, y_css, tab_area_w) {
                            tabs::strip::TabHit::Tab(idx) => self.switch_tab(idx),
                            tabs::strip::TabHit::Close(idx) => {
                                self.close_tab(idx, event_loop);
                            }
                            tabs::strip::TabHit::Empty => {}
                        }
                        return;
                    }
                    // Archive panel: close on click below tab bar when open.
                    if self.archive.visible {
                        let win_w = self.viewport_width_css();
                        match tabs::archive::hit_test_panel(
                            &self.archive,
                            x_css,
                            y_css,
                            win_w,
                            tabs::strip::TAB_BAR_HEIGHT,
                        ) {
                            Some(tabs::archive::ArchiveHit::Restore(id)) => {
                                if let Some(entry) = self.archive.take(id)
                                    && !entry.url.is_empty()
                                {
                                    self.navigate_to(PageSource::Url(entry.url));
                                }
                                self.archive.close();
                                self.request_redraw();
                                return;
                            }
                            Some(tabs::archive::ArchiveHit::Dismiss(id)) => {
                                self.archive.take(id);
                                self.request_redraw();
                                return;
                            }
                            Some(tabs::archive::ArchiveHit::Inside) => {
                                self.request_redraw();
                                return;
                            }
                            Some(tabs::archive::ArchiveHit::Outside) | None => {
                                self.archive.close();
                                self.request_redraw();
                            }
                        }
                    }

                    // Vertical tab panel: intercept clicks in x < PANEL_WIDTH area.
                    if self.vertical_tabs.visible
                        && x_css < panels::vertical_tabs::PANEL_WIDTH
                    {
                        let win_h = self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                        match panels::vertical_tabs::hit_test(
                            &self.tab_strip,
                            x_css,
                            y_css,
                            tabs::strip::TAB_BAR_HEIGHT,
                            win_h,
                        ) {
                            Some(panels::vertical_tabs::VTabHit::Tab(idx)) => {
                                self.switch_tab(idx);
                            }
                            Some(panels::vertical_tabs::VTabHit::Close(idx)) => {
                                self.close_tab(idx, event_loop);
                            }
                            Some(panels::vertical_tabs::VTabHit::Empty) | None => {}
                        }
                        return;
                    }

                    // Tree-style tab panel: intercept clicks in x < PANEL_WIDTH area.
                    if self.tree_tabs.visible
                        && x_css < panels::tree_tabs::PANEL_WIDTH
                    {
                        let win_h = self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                        match panels::tree_tabs::hit_test(
                            &self.tab_strip,
                            &self.tree_tabs,
                            x_css,
                            y_css,
                            tabs::strip::TAB_BAR_HEIGHT,
                            win_h,
                        ) {
                            Some(panels::tree_tabs::TreeTabHit::Tab(idx)) => {
                                self.switch_tab(idx);
                            }
                            Some(panels::tree_tabs::TreeTabHit::Close(idx)) => {
                                self.close_tab(idx, event_loop);
                            }
                            Some(panels::tree_tabs::TreeTabHit::Arrow(tab_id)) => {
                                let expanding = self.tree_tabs.collapsed.contains(&tab_id);
                                self.tree_tabs.toggle_collapsed(tab_id);
                                if expanding {
                                    // Purge stale collapse entries for tabs that were closed
                                    // while their parent subtree was hidden.
                                    let subtree = tabs::tree::subtree_ids(
                                        &self.tab_strip.tabs, tab_id,
                                    );
                                    let valid: std::collections::HashSet<usize> =
                                        self.tab_strip.tabs.iter().map(|t| t.id).collect();
                                    self.tree_tabs.collapsed.retain(|id| {
                                        valid.contains(id) || !subtree.contains(id)
                                    });
                                }
                                self.request_redraw();
                            }
                            Some(panels::tree_tabs::TreeTabHit::Empty) | None => {}
                        }
                        return;
                    }

                    // Shields floating panel (7C.4): top-right overlay.
                    if self.shields.visible {
                        let win_w = self.viewport_width_css();
                        let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                        if let Some(hit) = panels::shields_panel::hit_test(
                            &self.shields,
                            x_css,
                            y_css,
                            win_w,
                            tab_h,
                        ) {
                            match hit {
                                panels::shields_panel::ShieldsHit::Toggle => {
                                    self.shields.enabled = !self.shields.enabled;
                                    self.request_redraw();
                                }
                                panels::shields_panel::ShieldsHit::Close => {
                                    self.shields.visible = false;
                                    self.request_redraw();
                                }
                                panels::shields_panel::ShieldsHit::Empty => {}
                            }
                            return;
                        }
                    }

                    // Privacy network panel (V5): right-docked overlay.
                    if self.privacy.visible {
                        let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                        let win_w = self.viewport_width_css();
                        let win_h = self.viewport_height_css() + tab_h;
                        match panels::privacy_panel::hit_test(
                            &self.privacy,
                            x_css,
                            y_css,
                            win_w,
                            win_h,
                            tab_h,
                        ) {
                            panels::privacy_panel::PrivacyHit::Close => {
                                self.privacy.visible = false;
                                self.request_redraw();
                                return;
                            }
                            // Swallow clicks inside the panel so they don't reach
                            // the page underneath.
                            panels::privacy_panel::PrivacyHit::Inside => return,
                            panels::privacy_panel::PrivacyHit::Outside => {}
                        }
                    }

                    // Permission popover (7C.2): top-left overlay below tab bar.
                    if self.permission.visible {
                        let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                        if let Some(hit) = panels::permission_panel::hit_test(
                            &self.permission,
                            x_css,
                            y_css,
                            tab_h,
                        ) {
                            match hit {
                                panels::permission_panel::PermissionHit::Toggle(kind) => {
                                    self.permission.cycle_permission(kind);
                                    self.request_redraw();
                                }
                                panels::permission_panel::PermissionHit::Close => {
                                    self.permission.visible = false;
                                    self.request_redraw();
                                }
                                panels::permission_panel::PermissionHit::Empty => {}
                            }
                            return;
                        }
                    }

                    // §12.3 Read-later panel (Ctrl+Shift+R): right-docked overlay.
                    if self.read_later_panel.visible {
                        use panels::read_later_panel::ReadLaterHit;
                        let win_w = self.viewport_width_css();
                        let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                        let px = win_w - panels::read_later_panel::PANEL_W - 4.0;
                        let py = tab_h + 4.0;
                        let hit = panels::read_later_panel::hit_test(
                            x_css,
                            y_css,
                            px,
                            py,
                            &self.read_later_panel.entries,
                            self.read_later_panel.scroll_offset,
                        );
                        match hit {
                            ReadLaterHit::Close => {
                                self.read_later_panel.visible = false;
                                self.request_redraw();
                            }
                            ReadLaterHit::Open(id) => {
                                // Load from offline HTML snapshot.
                                if let Ok(Some(entry)) = self.read_later_store.get(id) {
                                    let html = String::from_utf8_lossy(&entry.html_snapshot)
                                        .into_owned();
                                    let base_url = entry.url.clone();
                                    let _ = self.read_later_store.set_status(
                                        id,
                                        lumen_knowledge::ReadStatus::Read,
                                    );
                                    self.read_later_panel.visible = false;
                                    self.navigate_to(PageSource::Snapshot { html, base_url });
                                }
                            }
                            ReadLaterHit::Delete(id) => {
                                let _ = self.read_later_store.delete(id);
                                self.refresh_read_later();
                                self.request_redraw();
                            }
                            ReadLaterHit::Inside => { /* swallow */ }
                            ReadLaterHit::Outside => {
                                self.read_later_panel.visible = false;
                                self.request_redraw();
                            }
                        }
                        return;
                    }

                    // Bookmark manager panel (task #22): floating overlay.
                    if self.bookmark_panel.visible {
                        let (ax, ay) = self.bookmark_anchor();
                        if let Some(hit) = panels::bookmark_panel::hit_test(
                            &self.bookmark_panel,
                            x_css,
                            y_css,
                            ax,
                            ay,
                        ) {
                            use panels::bookmark_panel::BookmarkHit;
                            match hit {
                                BookmarkHit::Close => {
                                    self.bookmark_panel.visible = false;
                                    self.bookmark_panel.search_active = false;
                                }
                                BookmarkHit::FocusSearch => {
                                    self.bookmark_panel.search_active = true;
                                }
                                BookmarkHit::SelectFolder(folder) => {
                                    self.bookmark_panel.selected_folder = folder;
                                    self.bookmark_panel.scroll_y = 0.0;
                                }
                                BookmarkHit::DeleteBookmark(id) => {
                                    if let Some(url) = self
                                        .bookmark_panel
                                        .entries
                                        .iter()
                                        .find(|e| e.id == id)
                                        .map(|e| e.url.clone())
                                    {
                                        let _ = self.bookmarks.delete(&url);
                                        self.refresh_bookmarks();
                                    }
                                }
                                BookmarkHit::Bookmark(id) => {
                                    // Begin a potential drag; open vs. re-file is
                                    // resolved on the matching mouse release.
                                    self.bookmark_panel.begin_drag(id);
                                }
                                BookmarkHit::Empty => {
                                    self.bookmark_panel.search_active = false;
                                }
                            }
                            self.request_redraw();
                            return;
                        }
                    }

                    // Accessibility settings panel (E-2): centred overlay.
                    if self.a11y_panel.visible {
                        let win_w = self.viewport_width_css();
                        let win_h = self.viewport_height_css();
                        use panels::a11y_panel::A11yHit;
                        let hit = panels::a11y_panel::hit_test(
                            &self.a11y_panel,
                            x_css,
                            y_css,
                            win_w,
                            win_h,
                        );
                        match hit {
                            A11yHit::Close => {
                                let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                                self.a11y_panel.visible = false;
                                self.deliver_a11y_media_changes();
                            }
                            A11yHit::FontMultiplier(v) => {
                                self.a11y_panel.draft.font_size_multiplier = v as f64;
                            }
                            A11yHit::ReducedMotion => {
                                self.a11y_panel.draft.reduced_motion =
                                    !self.a11y_panel.draft.reduced_motion;
                            }
                            A11yHit::ForcedColors => {
                                self.a11y_panel.draft.forced_colors =
                                    !self.a11y_panel.draft.forced_colors;
                            }
                            A11yHit::CursorSizeOption(size) => {
                                self.a11y_panel.draft.cursor_size = size;
                            }
                            A11yHit::Inside => { /* swallow */ }
                            A11yHit::Outside => {
                                let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                                self.a11y_panel.visible = false;
                                self.deliver_a11y_media_changes();
                            }
                        }
                        self.request_redraw();
                        return;
                    }

                    // Settings panel (task D-7): centred overlay.
                    if self.settings_panel.visible {
                        let win_w = self.viewport_width_css();
                        let win_h = self.viewport_height_css();
                        let sp_x = (win_w - panels::settings_panel::PANEL_W) * 0.5;
                        let sp_y = (win_h - panels::settings_panel::PANEL_H) * 0.5;
                        use panels::settings_panel::SettingsHit;
                        let hit = panels::settings_panel::hit_test(
                            &self.settings_panel,
                            x_css,
                            y_css,
                            sp_x,
                            sp_y,
                        );
                        match hit {
                            SettingsHit::Close => {
                                let draft = self.settings_panel.apply_draft();
                                let _ = self.settings_store.apply_snapshot(&draft);
                                self.settings_panel.visible = false;
                            }
                            SettingsHit::TabSelect(sec) => {
                                self.settings_panel.section = sec;
                                self.settings_panel.scroll_y = 0.0;
                            }
                            SettingsHit::ToggleShields => {
                                self.settings_panel.draft.shields_enabled =
                                    !self.settings_panel.draft.shields_enabled;
                            }
                            SettingsHit::ToggleDoh => {
                                self.settings_panel.draft.doh_enabled =
                                    !self.settings_panel.draft.doh_enabled;
                            }
                            SettingsHit::SetFingerprintMode(mode) => {
                                self.settings_panel.draft.fingerprint_mode = mode;
                            }
                            SettingsHit::SetTheme(theme) => {
                                self.settings_panel.draft.theme = theme;
                            }
                            SettingsHit::FontSizeDecrease => {
                                self.settings_panel.draft.font_size =
                                    (self.settings_panel.draft.font_size - 2.0).max(8.0);
                            }
                            SettingsHit::FontSizeIncrease => {
                                self.settings_panel.draft.font_size =
                                    (self.settings_panel.draft.font_size + 2.0).min(36.0);
                            }
                            SettingsHit::FocusHomepage => {
                                self.settings_panel.focused_input =
                                    Some(panels::settings_panel::SettingInput::Homepage);
                            }
                            SettingsHit::FocusDownloadPath => {
                                self.settings_panel.focused_input =
                                    Some(panels::settings_panel::SettingInput::DownloadPath);
                            }
                            SettingsHit::Inside => { /* swallow */ }
                            SettingsHit::Outside => {
                                let draft = self.settings_panel.apply_draft();
                                let _ = self.settings_store.apply_snapshot(&draft);
                                self.settings_panel.visible = false;
                            }
                        }
                        self.request_redraw();
                        return;
                    }

                    // Keyboard shortcuts panel (§D-4): centred overlay.
                    if self.shortcuts_panel.visible {
                        let win_w = self.viewport_width_css();
                        let win_h = self.viewport_height_css();
                        let kp_x = (win_w - panels::shortcuts_panel::PANEL_W) * 0.5;
                        let kp_y = (win_h - panels::shortcuts_panel::PANEL_H) * 0.5;
                        use panels::shortcuts_panel::ShortcutsHit;
                        let lx = x_css - kp_x;
                        let ly = y_css - kp_y;
                        if lx >= 0.0 && lx < panels::shortcuts_panel::PANEL_W
                            && ly >= 0.0 && ly < panels::shortcuts_panel::PANEL_H
                        {
                            match self.shortcuts_panel.hit_test(lx, ly) {
                                ShortcutsHit::Close => {
                                    self.shortcuts_panel.close();
                                }
                                ShortcutsHit::StartRebind(idx) => {
                                    self.shortcuts_panel.rebinding = Some(idx);
                                }
                                ShortcutsHit::Consumed => {}
                            }
                        } else {
                            self.shortcuts_panel.close();
                        }
                        self.request_redraw();
                        return;
                    }

                    // History panel (task D-5): centred floating overlay.
                    if self.history_panel.visible {
                        let (px, py) = self.history_panel_anchor();
                        use panels::history_panel::HistoryHit;
                        let hit =
                            panels::history_panel::hit_test(&self.history_panel, x_css, y_css, px, py);
                        match hit {
                            HistoryHit::Close => {
                                self.history_panel.visible = false;
                                self.history_panel.search_active = false;
                            }
                            HistoryHit::FocusSearch => {
                                self.history_panel.search_active = true;
                            }
                            HistoryHit::ClearAll => {
                                let _ = self.history_store.clear();
                                let _ = self.history_fts.clear();
                                self.refresh_history();
                            }
                            HistoryHit::Delete(id) => {
                                if let Some(url) = self
                                    .history_panel
                                    .rows
                                    .iter()
                                    .find_map(|r| {
                                        if let panels::history_panel::HistoryRow::Entry(e) = r {
                                            if e.id == id { Some(e.url.clone()) } else { None }
                                        } else {
                                            None
                                        }
                                    })
                                {
                                    let _ = self.history_store.delete(&url);
                                    self.refresh_history();
                                }
                            }
                            HistoryHit::Navigate(url) => {
                                self.history_panel.visible = false;
                                self.navigate_to(PageSource::Url(url));
                            }
                            HistoryHit::Inside => { /* swallow */ }
                            HistoryHit::Outside => {
                                self.history_panel.visible = false;
                                self.history_panel.search_active = false;
                            }
                        }
                        self.request_redraw();
                        return;
                    }

                    // Sidebar web panel (7D.3): right-docked panel.
                    if self.sidebar.visible {
                        let win_w = self.viewport_width_css();
                        let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                        let win_h = self.viewport_height_css() + tab_h;
                        if let Some(hit) = panels::sidebar_panel::hit_test(
                            &self.sidebar,
                            x_css,
                            y_css,
                            win_w,
                            tab_h,
                            win_h,
                        ) {
                            match hit {
                                panels::sidebar_panel::SidebarHit::Close => {
                                    self.sidebar.close();
                                    self.relayout();
                                    self.request_redraw();
                                }
                                panels::sidebar_panel::SidebarHit::Content
                                | panels::sidebar_panel::SidebarHit::Header => {}
                            }
                            return;
                        }
                    }

                    // Workspace switcher bar (7A.3): clicks in the bottom bar area.
                    if self.workspace_panel.visible {
                        let win_w = self.viewport_width_css();
                        let win_h = self.viewport_height_css()
                            + tabs::strip::TAB_BAR_HEIGHT
                            + panels::workspace_panel::SWITCHER_HEIGHT;
                        if let Some(hit) = panels::workspace_panel::hit_test(
                            &self.workspace_panel,
                            x_css,
                            y_css,
                            win_w,
                            win_h,
                        ) {
                            match hit {
                                panels::workspace_panel::WorkspaceHit::SwitchTo(id) => {
                                    self.workspace_panel.set_active(Some(id));
                                    self.request_redraw();
                                }
                                panels::workspace_panel::WorkspaceHit::DeleteWorkspace(id) => {
                                    // Never delete the last workspace — require at least one.
                                    if self.workspace_panel.workspaces.len() > 1 {
                                        let _ = self.workspaces.delete(id);
                                        self.refresh_workspaces();
                                        // If the deleted workspace was active, switch to first.
                                        if self.workspace_panel.active_id == Some(id) {
                                            let first_id = self
                                                .workspace_panel
                                                .workspaces
                                                .first()
                                                .map(|w| w.id);
                                            self.workspace_panel.set_active(first_id);
                                        }
                                        self.request_redraw();
                                    }
                                }
                                panels::workspace_panel::WorkspaceHit::NewWorkspace => {
                                    let n = self.workspace_panel.workspaces.len() + 1;
                                    let name = format!("Workspace {n}");
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_secs() as i64)
                                        .unwrap_or(0);
                                    if let Ok(id) =
                                        self.workspaces.create(&name, "#6482dc", "", None, now)
                                    {
                                        self.refresh_workspaces();
                                        self.workspace_panel.set_active(Some(id));
                                    }
                                    self.request_redraw();
                                }
                                panels::workspace_panel::WorkspaceHit::Empty => {}
                            }
                            return;
                        }
                    }

                    // Split-view focus routing: clicking in the right pane
                    // transfers focus there; clicking in the left pane transfers
                    // focus back. Right-pane clicks do not navigate (frozen pane).
                    if self.split_view.is_some() {
                        // Pre-compute before mutable borrow of split_view.
                        let split_x = (self.viewport_width_css() / 2.0).floor();
                        if let Some(ref mut sv) = self.split_view {
                            sv.focus_at(x_css, split_x);
                            if sv.cursor_in_right(x_css, split_x) {
                                // Right pane clicked — focus only, no link navigation.
                                self.request_redraw();
                                return;
                            }
                        }
                        // Left pane clicked — fall through to normal handling below.
                    }

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
                            self.handle_click_at(x_css, y_css);
                        }
                    }
                } else {
                    // Released — завершаем drag (если был) и сбрасываем resize.
                    self.resize_active = None;
                    // CSS :active — clear on release.
                    if self.active_nid.is_some() {
                        self.active_nid = None;
                        self.relayout();
                        self.request_redraw();
                    }
                    // Fire mouseup + pointerup on the hovered DOM element.
                    // Per W3C UI Events §17.6 + Pointer Events L2 §10.
                    #[cfg(feature = "quickjs")]
                    if let (Some(hov), Some(pos)) = (self.hovered_nid, self.cursor_position) {
                        let dpr = self.renderer.as_ref()
                            .map_or(1.0_f32, |r| r.scale_factor() as f32).max(1e-6);
                        let xu = (pos.x as f32) / dpr;
                        let yu = (pos.y as f32) / dpr;
                        let nid = hov.index() as u32;
                        self.js_pointer_event(nid, "pointerup", xu, yu, 0, 0);
                        self.js_mouse_event(nid, "mouseup", xu, yu, 0, 0);
                    }
                    // Bookmark drag-and-drop: if a bookmark drag is in progress,
                    // resolve the drop target. Dropping on a folder re-files the
                    // bookmark; dropping anywhere else opens it (a plain click).
                    if let Some(id) = self.bookmark_panel.take_drag() {
                        self.finish_bookmark_drop(id);
                        self.request_redraw();
                    }
                    // End a PiP window drag (task #21).
                    if self.pip.dragging() {
                        self.pip.end_drag();
                    }
                    self.scroll_drag = None;
                    // Курсор был «зафиксирован» как Pointer пока тянули
                    // thumb; теперь пересчитаем по hover-точке текущего
                    // положения курсора (CursorMoved-event на release сам
                    // не приходит, поэтому делаем вручную).
                    self.update_cursor_icon();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                // Privacy network panel intercepts the wheel while visible:
                // scroll the request list instead of the page.
                if self.privacy.visible {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, l) => l,
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
                    };
                    let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                    let win_h = self.viewport_height_css() + tab_h;
                    let body_h = panels::privacy_panel::list_body_height(win_h, tab_h);
                    if lines > 0.0 {
                        self.privacy.scroll_up(lines.abs().ceil() as usize);
                    } else if lines < 0.0 {
                        self.privacy.scroll_down(lines.abs().ceil() as usize, body_h);
                    }
                    self.request_redraw();
                    return;
                }
                // DevTools network panel intercepts the wheel while visible:
                // scroll the request list instead of the page.
                if self.network_panel.visible {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, l) => l,
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
                    };
                    if lines > 0.0 {
                        self.network_panel.scroll_up(lines.abs().ceil() as usize);
                    } else if lines < 0.0 {
                        self.network_panel.scroll_down(lines.abs().ceil() as usize);
                    }
                    self.request_redraw();
                    return;
                }
                // §12.3 Read-later panel intercepts the wheel while visible.
                if self.read_later_panel.visible {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, l) => l,
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
                    };
                    let max_scroll = self.read_later_panel.max_scroll();
                    if lines > 0.0 {
                        self.read_later_panel.scroll_up();
                    } else if lines < 0.0 {
                        self.read_later_panel.scroll_down(max_scroll);
                    }
                    self.request_redraw();
                    return;
                }
                // Bookmark panel intercepts the wheel while visible: scroll the
                // bookmark list rather than the page.
                if self.bookmark_panel.visible {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, l) => l,
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
                    };
                    // winit: wheel up → lines > 0 → scroll content up (scroll_y -=).
                    self.bookmark_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
                    self.request_redraw();
                    return;
                }
                // History panel intercepts the wheel while visible.
                if self.history_panel.visible {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, l) => l,
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
                    };
                    self.history_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
                    self.request_redraw();
                    return;
                }
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

                // In split mode, check if right pane is focused; route scroll there.
                let right_pane_focused = self
                    .split_view
                    .as_ref()
                    .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);

                match delta {
                    MouseScrollDelta::LineDelta(cols, lines) => {
                        // Mouse wheel: дискретные тики, momentum не нужен.
                        self.momentum_anim = None;
                        self.touchpad_vel = (0.0, 0.0);
                        let dx = -cols * 40.0;
                        let dy = -lines * 40.0;
                        let (dx_css, dy_css) = if shift { (dy, 0.0) } else { (dx, dy) };
                        if right_pane_focused {
                            let vh = self.viewport_height_css();
                            let vw = (self.viewport_width_css() / 2.0).floor();
                            if let Some(ref mut sv) = self.split_view {
                                if dy_css != 0.0 {
                                    let max =
                                        (sv.right.content_height - vh).max(0.0);
                                    sv.right.scroll_y =
                                        (sv.right.scroll_y + dy_css).clamp(0.0, max);
                                }
                                if dx_css != 0.0 {
                                    let max = (sv.right.content_width - vw).max(0.0);
                                    sv.right.scroll_x =
                                        (sv.right.scroll_x + dx_css).clamp(0.0, max);
                                }
                            }
                            self.request_redraw();
                        } else {
                            if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                            self.scroll_by_smooth(dy_css);
                        }
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
                                if right_pane_focused {
                                    let vh = self.viewport_height_css();
                                    if let Some(ref mut sv) = self.split_view
                                        && dy_css != 0.0
                                    {
                                        let max =
                                            (sv.right.content_height - vh).max(0.0);
                                        sv.right.scroll_y =
                                            (sv.right.scroll_y + dy_css).clamp(0.0, max);
                                    }
                                    self.request_redraw();
                                } else {
                                    if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                                    self.scroll_by_smooth(dy_css);
                                }
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
                                if right_pane_focused {
                                    let vh = self.viewport_height_css();
                                    if let Some(ref mut sv) = self.split_view
                                        && dy_css != 0.0
                                    {
                                        let max =
                                            (sv.right.content_height - vh).max(0.0);
                                        sv.right.scroll_y =
                                            (sv.right.scroll_y + dy_css).clamp(0.0, max);
                                    }
                                    self.request_redraw();
                                } else {
                                    if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                                    self.scroll_by_smooth(dy_css);
                                }
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
                // ADR-008 §10E.4: after scroll, evict CPU-decoded images beyond gate zone.
                self.try_discard_offscreen_images();

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

                // Step 2.5: GIF animation — update GPU textures for frames that changed.
                // Uses the same `epoch` as rAF timestamps so GIF timing is consistent
                // with CSS animations and JS. Runs before rAF so JS can read correct img.
                if !self.animated_gifs.is_empty() {
                    let elapsed_ms = self.epoch.elapsed().as_millis() as u64;

                    // Collect (url, frame_idx, frame_image) for frames that changed.
                    let updates: Vec<(String, usize, lumen_image::Image)> = {
                        let gifs = &self.animated_gifs;
                        let last = &self.gif_last_frame;
                        gifs.iter()
                            .filter_map(|(url, gif)| {
                                let idx = gif.frame_index_at(elapsed_ms);
                                if last.get(url).copied().unwrap_or(usize::MAX) != idx {
                                    Some((url.clone(), idx, gif.frames[idx].image.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    };

                    for (url, idx, image) in updates {
                        if let Some(r) = self.renderer.as_mut()
                            && let Err(e) = r.register_image(url.clone(), &image)
                        {
                            eprintln!("GIF кадр {url}[{idx}]: не зарегистрирован: {e}");
                        }
                        self.gif_last_frame.insert(url, idx);
                    }

                    // Request next redraw if any GIF still has more frames to show.
                    let gif_animating = {
                        let gifs = &self.animated_gifs;
                        gifs.values().any(|gif| match gif.loop_count {
                            lumen_image::GifLoopCount::Infinite => gif.frames.len() > 1,
                            lumen_image::GifLoopCount::Finite(n) => {
                                let total_ms: u64 =
                                    gif.frames.iter().map(lumen_image::AnimatedFrame::delay_ms).sum();
                                elapsed_ms < total_ms.saturating_mul(u64::from(n))
                            }
                        })
                    };
                    if gif_animating {
                        self.request_redraw();
                    }
                }

                // Step 3: rAF callbacks + microtask checkpoint.
                self.runtime.run_rendering_step(timestamp_ms);

                // Step 3.1: JS requestAnimationFrame callbacks.
                // Snapshot-pattern: callbacks registered during this call go into
                // the next frame. If any new rAF was registered (animation loop),
                // request another redraw immediately.
                // In deterministic mode (8F) pass 0.0 to suppress wall-clock jitter.
                if let Some(js) = &self.js_ctx {
                    let raf_ts = if self.deterministic { 0.0 } else { timestamp_ms };
                    js.run_animation_frame(raf_ts);
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

                // CSS View Transitions: fade old display list over new content.
                // Renders old_dl wrapped in PushOpacity(1-progress)/PopOpacity so it
                // fades out while the new display list (rendered underneath) fades in.
                // Runs at most `duration_ms`; after that, view_transition is cleared.
                let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                if let Some(ref vt) = self.view_transition {
                    let elapsed = now_ms - vt.start_ms;
                    let progress = (elapsed / vt.duration_ms).clamp(0.0, 1.0) as f32;
                    let alpha = 1.0 - progress;
                    if alpha > 0.0 {
                        let mut vt_cmds = Vec::with_capacity(vt.old_dl.len() + 2);
                        vt_cmds.push(lumen_paint::DisplayCommand::PushOpacity { alpha });
                        vt_cmds.extend_from_slice(&vt.old_dl);
                        vt_cmds.push(lumen_paint::DisplayCommand::PopOpacity);
                        // Prepend so old content renders before (under) UI panels.
                        vt_cmds.append(&mut overlay_buf);
                        overlay_buf = vt_cmds;
                        if let Some(w) = self.window.as_ref() {
                            w.request_redraw();
                        }
                    }
                }
                // Clear completed transition (separate borrow from the block above).
                let transition_done = self
                    .view_transition
                    .as_ref()
                    .is_some_and(|vt| now_ms - vt.start_ms >= vt.duration_ms);
                if transition_done {
                    self.view_transition = None;
                }

                // Hint overlay: viewport-locked бейджи kbd-навигации.
                // Добавляются последними → рисуются поверх scrollbar/tooltip.
                if self.hint.is_active() {
                    let mut hint_cmds = hints::build_hints_overlay(&self.hint, scroll_x, scroll_y);
                    overlay_buf.append(&mut hint_cmds);
                }

                // Download panel: viewport-locked bottom-right panel.
                // Rendered before the tab bar so it appears below the tab strip.
                if self.downloads.visible {
                    let win_size = self.window.as_ref().map_or((1024, 720), |w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    });
                    let mut dl_cmds = download::build_download_bar(&self.downloads, win_size);
                    overlay_buf.append(&mut dl_cmds);
                }

                // DevTools JS console panel: bottom overlay, toggled by F12.
                if self.devtools_console.visible {
                    let con_win_size = self.window.as_ref().map_or((1024, 720), |w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    });
                    let mut con_cmds = devtools::console_panel::build_console_panel(
                        &self.devtools_console,
                        con_win_size,
                    );
                    overlay_buf.append(&mut con_cmds);
                }

                // DevTools network panel: bottom overlay, toggled by Ctrl+Shift+E.
                if self.network_panel.visible {
                    self.network_panel.refresh();
                    let net_win_size = self.window.as_ref().map_or((1024, 720), |w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    });
                    let mut net_cmds = devtools::network_panel::build_network_panel(
                        &self.network_panel,
                        net_win_size,
                    );
                    overlay_buf.append(&mut net_cmds);
                }

                // Privacy network panel (V5): right-docked overlay, Ctrl+Shift+Y.
                if self.privacy.visible {
                    self.privacy.refresh();
                    let priv_win_size = self.window.as_ref().map_or((1024, 720), |w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    });
                    let mut priv_cmds = panels::privacy_panel::build_privacy_panel(
                        &self.privacy,
                        priv_win_size,
                        tabs::strip::TAB_BAR_HEIGHT,
                    );
                    overlay_buf.append(&mut priv_cmds);
                }

                // DevTools DOM inspector: right-docked computed-style side panel.
                // Viewport-locked; the box-model overlay for the hovered node is
                // emitted into the scrollable page layer below.
                if self.dom_inspector.visible {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    let (pw, ph) = self.window.as_ref().map_or((1024, 720), |w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    });
                    let win_css = (
                        (pw as f32 / dpr) as u32,
                        (ph as f32 / dpr) as u32,
                    );
                    let mut insp_cmds = devtools::inspector::build_inspector_panel(
                        &self.dom_inspector,
                        win_css,
                        tabs::strip::TAB_BAR_HEIGHT,
                    );
                    overlay_buf.append(&mut insp_cmds);
                }

                // Vertical tab panel: docked left sidebar, below the tab bar.
                // Rendered before the tab bar so tab bar draws on top.
                if self.vertical_tabs.visible {
                    let win_h = self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                    let mut vt_cmds = panels::vertical_tabs::build_panel(
                        &self.tab_strip,
                        tabs::strip::TAB_BAR_HEIGHT,
                        win_h,
                    );
                    overlay_buf.append(&mut vt_cmds);
                }

                // Tree-style tab panel (7A.2): same slot as vertical_tabs, but with
                // parent-child indentation and collapse/expand arrows.
                // Toggle via Ctrl+Shift+B; occupies the same PANEL_WIDTH as vertical_tabs.
                if self.tree_tabs.visible {
                    let win_h = self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                    let mut tt_cmds = panels::tree_tabs::build_panel(
                        &self.tab_strip,
                        &self.tree_tabs,
                        tabs::strip::TAB_BAR_HEIGHT,
                        win_h,
                    );
                    overlay_buf.append(&mut tt_cmds);
                }

                // Shields floating panel (7C.4): top-right overlay anchored below
                // the tab bar.  Refresh blocked counts before rendering.
                if self.shields.visible {
                    self.shields.refresh();
                    let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                    let win_w = self.viewport_width_css();
                    let mut sh_cmds = panels::shields_panel::build_panel(
                        &self.shields,
                        win_w,
                        tab_h,
                    );
                    overlay_buf.append(&mut sh_cmds);
                }

                // Permission popover (7C.2): top-left overlay anchored below the tab bar.
                if self.permission.visible {
                    let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                    let mut perm_cmds = panels::permission_panel::build_panel(
                        &self.permission,
                        tab_h,
                    );
                    overlay_buf.append(&mut perm_cmds);
                }

                // Sidebar web panel (7D.3): right-docked secondary viewport.
                if self.sidebar.visible {
                    let win_w = self.viewport_width_css();
                    let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                    let win_h = self.viewport_height_css() + tab_h;
                    let mut sb_cmds = panels::sidebar_panel::build_panel(
                        &self.sidebar,
                        win_w,
                        tab_h,
                        win_h,
                    );
                    overlay_buf.append(&mut sb_cmds);
                }

                // Workspace switcher bar (7A.3): bottom-docked horizontal strip.
                // Rendered before the tab bar so tab bar always draws on top.
                if self.workspace_panel.visible {
                    let win_w = self.viewport_width_css();
                    // Full window height including tab bar — bar is docked at bottom.
                    let win_h = self.viewport_height_css()
                        + tabs::strip::TAB_BAR_HEIGHT
                        + panels::workspace_panel::SWITCHER_HEIGHT;
                    let mut ws_cmds = panels::workspace_panel::build_panel(
                        &self.workspace_panel,
                        win_w,
                        win_h,
                    );
                    overlay_buf.append(&mut ws_cmds);
                }

                // Bookmark manager panel (task #22): floating overlay anchored
                // under the toolbar. Drawn above page/other overlays, below the
                // tab bar.
                if self.bookmark_panel.visible {
                    let (ax, ay) = self.bookmark_anchor();
                    let mut bm_cmds =
                        panels::bookmark_panel::build_panel(&self.bookmark_panel, ax, ay);
                    overlay_buf.append(&mut bm_cmds);
                }

                // Accessibility settings panel (E-2): centred overlay, Ctrl+Shift+Q.
                if self.a11y_panel.visible {
                    let win_w = self.viewport_width_css();
                    let win_h = self.viewport_height_css();
                    let win_size = (win_w as u32, win_h as u32);
                    let mut a11y_cmds =
                        panels::a11y_panel::build_a11y_panel(&self.a11y_panel, win_size);
                    overlay_buf.append(&mut a11y_cmds);
                }

                // Settings panel (task D-7): centred overlay, Ctrl+, or about:settings.
                if self.settings_panel.visible {
                    let win_w = self.viewport_width_css();
                    let win_h = self.viewport_height_css();
                    let sp_x = (win_w - panels::settings_panel::PANEL_W) * 0.5;
                    let sp_y = (win_h - panels::settings_panel::PANEL_H) * 0.5;
                    panels::settings_panel::build_panel(&self.settings_panel, &mut overlay_buf, sp_x, sp_y);
                }

                // Keyboard shortcuts panel (§D-4): centred floating overlay.
                if self.shortcuts_panel.visible {
                    let win_w = self.viewport_width_css();
                    let win_h = self.viewport_height_css();
                    let kp_x = (win_w - panels::shortcuts_panel::PANEL_W) * 0.5;
                    let kp_y = (win_h - panels::shortcuts_panel::PANEL_H) * 0.5;
                    self.shortcuts_panel.build_panel(&mut overlay_buf, kp_x, kp_y);
                }

                // History panel (task D-5): centred floating overlay.
                if self.history_panel.visible {
                    let win_w = self.viewport_width_css();
                    let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                    let mut hist_cmds =
                        panels::history_panel::build_panel(&self.history_panel, win_w, tab_h);
                    overlay_buf.append(&mut hist_cmds);
                }

                // §12.3 Read-later panel: right-docked overlay.
                if self.read_later_panel.visible {
                    let win_w = self.viewport_width_css();
                    let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                    let mut rl_cmds = panels::read_later_panel::build_panel(
                        &self.read_later_panel,
                        win_w,
                        tab_h,
                    );
                    overlay_buf.append(&mut rl_cmds);
                }

                // Tab bar: viewport-locked strip at y=0..TAB_BAR_HEIGHT.
                // Rendered last → always on top of all other overlays.
                // Hidden in focus mode (task #25) for a distraction-free view;
                // the page transform offset is left unchanged so toggling focus
                // mode never reflows content (the strip shows page background).
                if !self.focus.active {
                    let win_w = self.viewport_width_css();
                    // Tab strip uses the area to the left of the archive button.
                    let tab_area_w = win_w - tabs::archive::ARCHIVE_BTN_W;
                    let mut tab_cmds =
                        tabs::strip::build_tab_bar(&self.tab_strip, tab_area_w);
                    overlay_buf.append(&mut tab_cmds);
                    // Tab tier tooltip on hover.
                    if let Some(idx) = self.hovered_tab_idx
                        && let Some(tab) = self.tab_strip.tabs.get(idx) {
                            let tab_w = tab_area_w / self.tab_strip.tabs.len().max(1) as f32;
                            let tab_center_x = (idx as f32 + 0.5) * tab_w;
                            if let Some(mut tooltip_cmds) = tabs::strip::build_tab_tooltip(
                                tab,
                                tab_center_x,
                                tabs::strip::TAB_BAR_HEIGHT,
                            ) {
                                overlay_buf.append(&mut tooltip_cmds);
                            }
                        }
                    // Archive toolbar button (rightmost 36 px of tab bar).
                    let mut arch_btn = tabs::archive::build_button(
                        &self.archive,
                        win_w,
                        tabs::strip::TAB_BAR_HEIGHT,
                    );
                    overlay_buf.append(&mut arch_btn);
                    // Archive panel: floating drop-down anchored below the button.
                    let mut arch_panel = tabs::archive::build_panel(
                        &self.archive,
                        win_w,
                        tabs::strip::TAB_BAR_HEIGHT,
                    );
                    overlay_buf.append(&mut arch_panel);
                }

                // Command palette (task #23): modal — drawn above everything,
                // including the tab bar, with a full-window dimming scrim.
                if self.command_palette.visible {
                    let win_w = self.viewport_width_css();
                    let win_h =
                        self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                    let mut cp_cmds = panels::command_palette::build_panel(
                        &self.command_palette,
                        win_w,
                        win_h,
                    );
                    overlay_buf.append(&mut cp_cmds);
                }

                // Focus mode widget (task #25): floating Pomodoro card with an
                // arc progress ring, drawn on top of everything (including where
                // the now-hidden tab bar was).
                if self.focus.active {
                    let win_w = self.viewport_width_css();
                    let mut focus_cmds =
                        panels::focus_panel::build_panel(&self.focus, win_w);
                    overlay_buf.append(&mut focus_cmds);
                }

                // Picture-in-picture window (task #21) — drawn last so it floats
                // above all other chrome.
                if self.pip.active {
                    let mut pip_cmds = panels::pip_window::build_panel(&self.pip);
                    overlay_buf.append(&mut pip_cmds);
                }

                // Loading spinner for hibernated tab restore >200ms (10K.3).
                if let Some(start_ms) = self.restore_spinner_start_ms {
                    let elapsed_ms = now_ms - start_ms;
                    let win_w = self.viewport_width_css();
                    let win_h =
                        self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
                    if let Some(mut spinner) =
                        panels::restore_spinner::build_spinner(elapsed_ms, win_w, win_h)
                    {
                        overlay_buf.append(&mut spinner);
                        // Keep animating the spinner while it's visible.
                        self.request_redraw();
                    }
                }

                // Build the split-view combined DL before borrowing renderer,
                // so the immutable borrow of self.split_view ends first.
                let split_combined: Option<lumen_paint::DisplayList> = {
                    let base_ref: &[lumen_paint::DisplayCommand] = anim_dl
                        .as_deref()
                        .or(page_buf.as_deref())
                        .unwrap_or(&self.display_list);
                    if let Some(ref sv) = self.split_view {
                        let vp_w = self.viewport_width_css();
                        let tab_h = tabs::strip::TAB_BAR_HEIGHT;
                        let vp_full_h = self.viewport_height_css() + tab_h;
                        let split_x = (vp_w / 2.0).floor();
                        Some(sv.build_combined_dl(
                            base_ref,
                            scroll_y,
                            scroll_x,
                            split_x,
                            tab_h,
                            vp_full_h,
                        ))
                    } else {
                        None
                    }
                };

                // DevTools inspector box-model overlay, in page coordinates so it
                // rides the same scroll/tab-bar transform as the page content.
                // Built before borrowing the renderer to keep borrows disjoint.
                let inspector_box_dl: lumen_paint::DisplayList = if self.dom_inspector.visible {
                    if let Some(lb) = self.layout_box.as_ref() {
                        let vp = Size::new(
                            self.viewport_width_css(),
                            self.viewport_height_css(),
                        );
                        devtools::inspector::build_box_overlay(
                            &self.dom_inspector,
                            lb,
                            vp,
                            (0.0, 0.0),
                        )
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                if let Some(r) = self.renderer.as_mut() {
                    if let Some(combined) = split_combined {
                        // Split-view mode: combined DL with baked scroll; renderer gets 0,0.
                        if let Err(err) = r.render(&combined, &overlay_buf, 0.0, 0.0) {
                            eprintln!("Ошибка рендера (split): {err:?}");
                        }
                    } else {
                        // Normal single-pane mode: shift page below tab bar (and right of
                        // vertical tabs panel when it is visible).
                        let base: &[lumen_paint::DisplayCommand] = anim_dl
                            .as_deref()
                            .or(page_buf.as_deref())
                            .unwrap_or(&self.display_list);
                        let mut shifted: lumen_paint::DisplayList =
                            Vec::with_capacity(base.len() + 2);
                        let page_x_offset = if self.vertical_tabs.visible {
                            panels::vertical_tabs::PANEL_WIDTH
                        } else if self.tree_tabs.visible {
                            panels::tree_tabs::PANEL_WIDTH
                        } else {
                            0.0
                        };
                        shifted.push(lumen_paint::DisplayCommand::PushTransform {
                            matrix: Mat4::translation_2d(
                                page_x_offset,
                                tabs::strip::TAB_BAR_HEIGHT,
                            ),
                        });
                        shifted.extend_from_slice(base);
                        // Inspector box-model overlay rides inside the page transform.
                        shifted.extend_from_slice(&inspector_box_dl);
                        shifted.push(lumen_paint::DisplayCommand::PopTransform);
                        if let Err(err) = r.render(&shifted, &overlay_buf, scroll_y, scroll_x) {
                            eprintln!("Ошибка рендера: {err:?}");
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Lumen {
    /// Return a cloneable [`InputSender`] for injecting synthetic input events.
    ///
    /// Callers on any thread can use the sender to enqueue [`InputCommand`]s;
    /// they are drained and dispatched in `about_to_wait`.
    #[allow(dead_code)]
    pub fn input_sender(&self) -> input::InputSender {
        self.input_tx.clone()
    }

    /// Return the current keyboard modifier flags as a bitmask.
    ///
    /// Bit layout: bit0=ctrl, bit1=shift, bit2=alt, bit3=meta (super).
    #[cfg(feature = "quickjs")]
    fn mod_flags(&self) -> u8 {
        (self.modifiers.control_key() as u8)
            | ((self.modifiers.shift_key()  as u8) << 1)
            | ((self.modifiers.alt_key()    as u8) << 2)
            | ((self.modifiers.super_key()  as u8) << 3)
    }

    /// Dispatch a `MouseEvent` of the given `event_type` to DOM node `nid`.
    ///
    /// `button` = which button (0=left, 1=middle, 2=right).
    /// `buttons` = bitmask of currently-held buttons.
    /// Coordinates are CSS viewport pixels.
    #[cfg(feature = "quickjs")]
    fn js_mouse_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32, button: u8, buttons: u8) {
        if let Some(ctx) = &self.js_ctx {
            let script = format!(
                "_lumen_dispatch_mouse_event({}, '{}', {}, {}, {}, {}, {})",
                nid, event_type,
                x_css as i32, y_css as i32,
                button, buttons,
                self.mod_flags(),
            );
            ctx.eval_js(&script);
        }
    }

    /// Dispatch a `PointerEvent` of the given `event_type` to DOM node `nid`.
    ///
    /// Always uses pointerId=1, pointerType='mouse', isPrimary=true (mouse input).
    /// Non-bubbling types (`pointerenter`/`pointerleave`) have `bubbles:false` per spec.
    #[cfg(feature = "quickjs")]
    fn js_pointer_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32, button: u8, buttons: u8) {
        if let Some(ctx) = &self.js_ctx {
            let script = format!(
                "_lumen_dispatch_pointer_event({}, '{}', {}, {}, {}, {}, {})",
                nid, event_type,
                x_css as i32, y_css as i32,
                button, buttons,
                self.mod_flags(),
            );
            ctx.eval_js(&script);
        }
    }

    /// Dispatch a synthetic `mousemove` event at CSS-pixel viewport coordinates.
    ///
    /// Hit-tests the position (accounting for current scroll offset) and fires
    /// `_lumen_dispatch_mouse_event` with event type `"mousemove"`.  Used by
    /// [`input::humanlike::HumanLikeSender`] to trace Bézier-curve paths before
    /// a click.  No-op when there is no JS context or no element at the position.
    /// Also fires the matching W3C `pointermove` event per Pointer Events L2 §10.
    fn dispatch_mouse_move(&mut self, x_css: f32, y_css: f32) {
        let panel_x_offset = if self.vertical_tabs.visible {
            panels::vertical_tabs::PANEL_WIDTH
        } else if self.tree_tabs.visible {
            panels::tree_tabs::PANEL_WIDTH
        } else {
            0.0
        };
        let page_x = (x_css - panel_x_offset) + self.scroll_x;
        let page_y = (y_css - tabs::strip::TAB_BAR_HEIGHT) + self.scroll_y;
        let hit = self.layout_box.as_ref().and_then(|lb| {
            hit_test(Point::new(page_x, page_y), lb)
        });
        #[cfg(feature = "quickjs")]
        if let Some(result) = hit.as_ref() {
            let nid = result.node.index() as u32;
            // Pointer Events L2 §10.5 — pointermove fires before mousemove.
            self.js_pointer_event(nid, "pointermove", x_css, y_css, 0, 0);
            self.js_mouse_event(nid, "mousemove", x_css, y_css, 0, 0);
        }
        #[cfg(not(feature = "quickjs"))]
        let _ = hit;
    }

    /// Handle a left-button click at CSS-pixel viewport coordinates `(x_css, y_css)`.
    ///
    /// Used by both the winit `MouseInput::Pressed` handler and the injected
    /// [`InputCommand::Click`] path so both share identical dispatch logic.
    /// Convert viewport CSS-pixel coordinates `(x_css, y_css)` into page
    /// (document) coordinates, accounting for the current scroll offset and the
    /// left tabs panel width when visible. Mirrors the conversion used by
    /// [`Lumen::handle_click_at`] so hit tests stay consistent across input
    /// paths.
    fn page_point(&self, x_css: f32, y_css: f32) -> (f32, f32) {
        let panel_x_offset = if self.vertical_tabs.visible {
            panels::vertical_tabs::PANEL_WIDTH
        } else if self.tree_tabs.visible {
            panels::tree_tabs::PANEL_WIDTH
        } else {
            0.0
        };
        (
            (x_css - panel_x_offset) + self.scroll_x,
            (y_css - tabs::strip::TAB_BAR_HEIGHT) + self.scroll_y,
        )
    }

    fn handle_click_at(&mut self, x_css: f32, y_css: f32) {
        // Dismiss validation tooltip on any non-scrollbar click.
        self.validation_tooltip = None;
        let scroll_y = self.scroll_y;

        // DevTools inspector: a click pins the box under the cursor and shows
        // its computed style, suppressing normal navigation / JS dispatch.
        if self.dom_inspector.visible {
            let (page_x, page_y) = self.page_point(x_css, y_css);
            if let Some(hit) = self
                .layout_box
                .as_ref()
                .and_then(|lb| hit_test(Point::new(page_x, page_y), lb))
            {
                let node = hit.node;
                let label = self
                    .layout_source
                    .as_ref()
                    .map(|src| {
                        devtools::inspector::element_label(&src.document.lock().unwrap(), node)
                    })
                    .unwrap_or_else(|| format!("NodeId({})", node.index()));
                let props = self
                    .layout_box
                    .as_ref()
                    .and_then(|lb| devtools::inspector::find_box(lb, node))
                    .map(devtools::inspector::computed_style_map)
                    .unwrap_or_default();
                self.dom_inspector.select(node, label, props);
                self.request_redraw();
            }
            return;
        }

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
        // When the vertical/tree tabs panel is visible, page content is shifted
        // right by PANEL_WIDTH, so we subtract that offset to convert to page coords.
        // Page content is also shifted down by TAB_BAR_HEIGHT via PushTransform,
        // so we subtract that offset from y to get layout coordinates.
        let panel_x_offset = if self.vertical_tabs.visible {
            panels::vertical_tabs::PANEL_WIDTH
        } else if self.tree_tabs.visible {
            panels::tree_tabs::PANEL_WIDTH
        } else {
            0.0
        };
        let page_x = (x_css - panel_x_offset) + self.scroll_x;
        let page_y = (y_css - tabs::strip::TAB_BAR_HEIGHT) + self.scroll_y;
        let hit_result = self.layout_box.as_ref().and_then(|lb| {
            hit_test(Point::new(page_x, page_y), lb)
        });

        // Debug click log — активируется флагом --click-log или LUMEN_CLICK_LOG=1.
        // For click log: report both the hit box node (<p>) and the inline source_node
        // (<a> text node) so the log shows what find_link_href actually searches from.
        let click_log_hit: Option<(u32, String, String, String)> =
            if click_log::is_enabled() {
                hit_result.as_ref().and_then(|r| {
                    self.layout_source.as_ref().map(|src| {
                        let doc = src.document.lock().unwrap();
                        // Use source_node for tag/class info — it reveals the inline element.
                        let effective_id = r.source_node;
                        let node = doc.get(effective_id);
                        let (tag, id_attr, class_attr) =
                            if let NodeData::Element { name, attrs } = &node.data {
                                let id = attrs.iter()
                                    .find(|a| a.name.local == "id")
                                    .map(|a| a.value.as_str())
                                    .unwrap_or("");
                                let cls = attrs.iter()
                                    .find(|a| a.name.local == "class")
                                    .map(|a| a.value.as_str())
                                    .unwrap_or("");
                                (name.local.to_string(), id.to_owned(), cls.to_owned())
                            } else if let NodeData::Text(t) = &node.data {
                                // Show which text we clicked and note the parent element.
                                let parent_tag = node.parent
                                    .map(|pid| {
                                        let pn = doc.get(pid);
                                        if let NodeData::Element { name, .. } = &pn.data {
                                            format!("<{}>", name.local)
                                        } else {
                                            "?".to_owned()
                                        }
                                    })
                                    .unwrap_or_default();
                                let preview: String = t.chars().take(30).collect();
                                (format!("#text in {parent_tag}"), String::new(), format!("\"{preview}\""))
                            } else {
                                ("#other".to_owned(), String::new(), String::new())
                            };
                        (effective_id.index() as u32, tag, id_attr, class_attr)
                    })
                })
            } else {
                None
            };

        // Track focused node for TypeText injection and CSS :focus matching.
        let new_focused = hit_result.as_ref().map(|r| r.node);
        let focus_changed = new_focused != self.focused_node;
        self.focused_node = new_focused;
        // Trigger relayout if :focus state changed so :focus / :focus-within rules update.
        if focus_changed {
            self.relayout();
        }
        // Dispatch JS click event (bubbles from hit node to document).
        // Passes viewport coordinates and modifier key state so
        // handlers can read event.clientX/clientY/ctrlKey/etc.
        if let (Some(result), Some(ctx)) =
            (hit_result.as_ref(), self.js_ctx.as_ref())
        {
            let mod_flags: u8 =
                (self.modifiers.control_key() as u8)
                | ((self.modifiers.shift_key()  as u8) << 1)
                | ((self.modifiers.alt_key()    as u8) << 2)
                | ((self.modifiers.super_key()  as u8) << 3);
            let script = format!(
                "_lumen_dispatch_mouse_event({}, 'click', {}, {}, 0, 1, {})",
                result.node.index(),
                x_css as i32,
                y_css as i32,
                mod_flags,
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

        // Log form actions (non-link outcomes).
        if click_log::is_enabled() {
            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                node_id: *nid, tag, id_attr: id, class_attr: cls,
            });
            match &form_action {
                forms::FormClickAction::Nothing => {} // logged in the Nothing branch below
                forms::FormClickAction::ToggleCheckbox(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleCheckbox"),
                    });
                }
                forms::FormClickAction::ToggleRadio { .. } => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleRadio"),
                    });
                }
                forms::FormClickAction::OpenColorPicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenColorPicker"),
                    });
                }
                forms::FormClickAction::SubmitForm(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("SubmitForm"),
                    });
                }
            }
        }

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
                // Phase 3: HTML5 form submission algorithm integration.
                // Execute submit_form() which performs constraint validation.
                if let Some(src) = self.layout_source.as_ref() {
                    let doc = src.document.lock().unwrap();
                    if let Some(submit_event) = forms::build_form_submit_event(&doc, submit_node) {
                        match submit_event {
                            lumen_dom::FormSubmitEvent::Valid { action, method, fields } => {
                                // Form passed validation — collect fields and submit.
                                let body = forms::encode_form_fields(&fields);
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
                                        eprintln!("[forms] POST {} body={}", action, body);
                                    }
                                }
                            }
                            lumen_dom::FormSubmitEvent::Invalid { invalid_controls } => {
                                // Form contains invalid controls — show first error.
                                if let Some(&first_invalid) = invalid_controls.first() {
                                    if let Some(lb) = self.layout_box.as_ref()
                                        && let Some((rect, msg)) = forms::find_control_rect_and_error(lb, &doc, first_invalid)
                                    {
                                        self.validation_tooltip = Some((rect, msg));
                                        if let Some(w) = self.window.as_ref() {
                                            w.request_redraw();
                                        }
                                    }
                                    eprintln!(
                                        "forms: submit blocked — {} control(s) failed constraint validation",
                                        invalid_controls.len()
                                    );
                                }
                            }
                        }
                    }
                }
            }
            forms::FormClickAction::Nothing => {
                // ── Link click ───────────────────────────
                // No form control was activated — check if
                // the clicked node is inside an <a href>.
                // Use source_node (text node inside inline element) so find_link_href
                // can walk up and find the <a> parent: text → <a href="…"> → found.
                // Falls back to r.node for non-inline boxes.
                let href = hit_result.as_ref().and_then(|r| {
                    self.layout_source
                        .as_ref()
                        .and_then(|src| links::find_link_href(&src.document.lock().unwrap(), r.source_node))
                });
                if let Some(href) = href {
                    if let Some(frag) = links::fragment_only(&href) {
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkFragment(frag),
                            });
                        }
                        // Same-page fragment navigation.
                        self.navigate_fragment(frag.to_owned());
                    } else if links::is_navigable_href(&href) {
                        let resolved = self.source.resolve_href(&href);
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkNavigate {
                                    href: &href,
                                    resolved: &resolved,
                                },
                            });
                        }
                        let target = PageSource::from_arg(Some(&resolved));
                        self.navigate_to(target);
                    } else {
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkBlocked(&href),
                            });
                        }
                    }
                } else if click_log::is_enabled() {
                    let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                        node_id: *nid, tag, id_attr: id, class_attr: cls,
                    });
                    let outcome = if hit_result.is_none() {
                        click_log::ClickOutcome::NoHit
                    } else {
                        click_log::ClickOutcome::NoLink
                    };
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome,
                    });
                }
            }
        }
    }

    /// Inject a typed character into the focused element (TypeText injection path).
    ///
    /// Fires `keydown` → `input` → `keyup` JS events via `_lumen_dispatch_key_event`
    /// on the last-focused node so events have `isTrusted=true`.
    fn inject_char(&mut self, ch: char) {
        let Some(ctx) = self.js_ctx.as_ref() else { return };
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        let key = escape_js_string_char(ch);
        for event_type in &["keydown", "input", "keyup"] {
            let script = format!(
                "_lumen_dispatch_key_event({}, '{}', '{}', '{}', false, false, false, false)",
                node_id, event_type, key, key,
            );
            ctx.eval_js(&script);
        }
        if let Some(nav) = ctx.take_navigate_request() {
            self.pending_js_navigate = Some(nav);
        }
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = key_event.physical_key else {
            return;
        };

        // Командная палитра — модальный overlay: пока открыта, перехватывает все
        // клавиши (Esc/Enter/↑/↓/Backspace/печать). Ctrl+K (toggle) пропускается
        // в глобальный keybinding-путь ниже, чтобы закрыть палитру.
        if self.command_palette.visible
            && !(code == KeyCode::KeyK && self.modifiers == ModifiersState::CONTROL)
            && self.handle_palette_key(code, key_event, event_loop)
        {
            return;
        }

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

        // Bookmark panel search box: when focused, printable input + Backspace +
        // Esc route to the search query. Modified keys (Ctrl/Cmd) fall through so
        // global shortcuts (e.g. Ctrl+Shift+O to close) keep working.
        if self.bookmark_panel.visible
            && self.bookmark_panel.search_active
            && self.handle_bookmark_key(code, key_event)
        {
            return;
        }

        // History panel search box: printable input + Backspace + Esc route here.
        // Arrow keys scroll the list. Modified keys fall through for global shortcuts.
        if self.history_panel.visible && self.handle_history_key(code, key_event) {
            return;
        }

        // Settings panel text inputs + Esc. Modified keys fall through for global shortcuts.
        if self.settings_panel.visible && self.handle_settings_key(code, key_event) {
            return;
        }

        // Keyboard shortcuts panel — capture any keypress when rebinding (§D-4).
        if self.shortcuts_panel.visible && self.handle_shortcuts_key(code, key_event) {
            return;
        }

        // Vim keybinding mode: intercept navigation keys in Normal state.
        // In Insert state, PassThrough falls through to the keybinding table.
        if let Some(ref mut vm) = self.vim_mode {
            let action = vm.feed(code, self.modifiers);
            match action {
                input::vim::VimAction::PassThrough => {} // fall through below
                input::vim::VimAction::Consumed => return,
                input::vim::VimAction::Deactivate => {
                    self.vim_mode = None;
                    return;
                }
                input::vim::VimAction::EnterInsert | input::vim::VimAction::ExitInsert => {
                    return;
                }
                input::vim::VimAction::ScrollDown => {
                    self.scroll_active_pane(LINE_STEP_CSS_PX);
                    return;
                }
                input::vim::VimAction::ScrollUp => {
                    self.scroll_active_pane(-LINE_STEP_CSS_PX);
                    return;
                }
                input::vim::VimAction::ScrollHalfPageDown => {
                    let half = self.viewport_height_css() * 0.5;
                    self.scroll_active_pane(half);
                    return;
                }
                input::vim::VimAction::ScrollHalfPageUp => {
                    let half = self.viewport_height_css() * 0.5;
                    self.scroll_active_pane(-half);
                    return;
                }
                input::vim::VimAction::ScrollTop => {
                    self.scroll_active_pane_to(0.0);
                    return;
                }
                input::vim::VimAction::ScrollBottom => {
                    self.scroll_active_pane_to(f32::INFINITY);
                    return;
                }
                input::vim::VimAction::OpenFind => {
                    self.hint.close();
                    self.find.open();
                    self.request_redraw();
                    return;
                }
                input::vim::VimAction::OpenHints | input::vim::VimAction::OpenHintsNewTab => {
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
                    return;
                }
                input::vim::VimAction::Copy => {
                    // Copy the current page URL to the OS clipboard (task #26).
                    if let Some(url) = self.source.url_str() {
                        use lumen_core::ext::ClipboardProvider;
                        platform::clipboard::PlatformClipboard.write_text(url);
                        eprintln!("[vim] copy URL: {url}");
                    }
                    return;
                }
                input::vim::VimAction::HistoryBack => {
                    self.navigate_back();
                    return;
                }
                input::vim::VimAction::HistoryForward => {
                    self.navigate_forward();
                    return;
                }
            }
        }

        // Fullscreen API (WHATWG Fullscreen §4.6): Escape always exits fullscreen first.
        // If we are fullscreen and the user presses Escape (no repeat, no mods), exit
        // fullscreen before processing any other shortcut.
        if self.fullscreen_nid.is_some()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.fullscreen_nid = None;
            if let Some(w) = self.window.as_ref() {
                w.set_fullscreen(None);
            }
            // Notify JS so fullscreenchange fires and document.fullscreenElement clears.
            #[cfg(feature = "quickjs")]
            if let Some(js) = &self.js_ctx {
                js.eval_js("if(typeof _lumen_notify_fullscreen_exit==='function')_lumen_notify_fullscreen_exit()");
            }
            return;
        }

        // Focus mode (task #25): while active, Escape exits focus mode instead of
        // quitting the app. Ctrl+Shift+F falls through to the keybinding table so
        // it can toggle focus mode off.
        if self.focus.active
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.focus.exit();
            self.request_redraw();
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
                let current = self.current_display_url().to_owned();
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
            KeyCommand::ScrollLineDown => self.scroll_active_pane(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineUp => self.scroll_active_pane(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineRight => self.scroll_x_by(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineLeft => self.scroll_x_by(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollPageDown => {
                let vh = self.viewport_height_css();
                self.scroll_active_pane(page_step(vh));
            }
            KeyCommand::ScrollPageUp => {
                let vh = self.viewport_height_css();
                self.scroll_active_pane(-page_step(vh));
            }
            KeyCommand::ScrollHome => self.scroll_active_pane_to(0.0),
            KeyCommand::ScrollEnd => self.scroll_active_pane_to(f32::INFINITY),
            KeyCommand::NewTab => self.open_new_tab(),
            KeyCommand::CloseTab => {
                let idx = self.tab_strip.active;
                self.close_tab(idx, event_loop);
            }
            KeyCommand::NextTab => {
                let next = (self.tab_strip.active + 1) % self.tab_strip.len();
                self.switch_tab(next);
            }
            KeyCommand::DownloadsPanel => {
                self.downloads.toggle_visible();
                self.request_redraw();
            }
            KeyCommand::SplitView => {
                if self.split_view.is_some() {
                    self.split_view = None;
                } else {
                    self.toggle_split_view();
                }
                self.request_redraw();
            }
            KeyCommand::SplitFocusSwitch => {
                if let Some(ref mut sv) = self.split_view {
                    sv.toggle_focus();
                    self.request_redraw();
                }
            }
            KeyCommand::VimModeToggle => {
                if self.vim_mode.is_some() {
                    self.vim_mode = None;
                } else {
                    self.vim_mode = Some(input::vim::VimMode::new());
                }
            }
            KeyCommand::ToggleVerticalTabs => {
                self.vertical_tabs.toggle();
                // Viewport width changes — re-layout the current page.
                self.relayout();
                self.request_redraw();
            }
            KeyCommand::ToggleTreeTabs => {
                self.tree_tabs.toggle();
                // Viewport width changes when switching to/from tree view.
                self.relayout();
                self.request_redraw();
            }
            KeyCommand::ToggleWorkspaces => {
                self.workspace_panel.toggle();
                // Viewport height changes — re-layout so content doesn't hide under bar.
                self.relayout();
                self.request_redraw();
            }
            KeyCommand::ToggleShields => {
                self.shields.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePermissions => {
                self.permission.toggle();
                self.request_redraw();
            }
            KeyCommand::ToggleCookieBannerDismiss => {
                self.cookie_banner_dismiss = !self.cookie_banner_dismiss;
                // Preference takes effect on the next page load.
            }
            KeyCommand::ToggleSidebar => {
                self.sidebar.toggle();
                // Sidebar occupies right PANEL_WIDTH — relayout so main page
                // content width adjusts accordingly.
                self.relayout();
                self.request_redraw();
            }
            KeyCommand::ToggleBookmarks => {
                self.bookmark_panel.toggle();
                if self.bookmark_panel.visible {
                    self.refresh_bookmarks();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleHistory => {
                self.history_panel.toggle();
                if self.history_panel.visible {
                    self.refresh_history();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleA11y => {
                if self.a11y_panel.visible {
                    let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                    self.a11y_panel.visible = false;
                    self.deliver_a11y_media_changes();
                } else {
                    self.a11y_panel.load_draft(self.a11y_store.snapshot());
                    self.a11y_panel.visible = true;
                }
                self.request_redraw();
            }
            KeyCommand::ToggleSettings => {
                let snap = self.settings_store.snapshot();
                if self.settings_panel.visible {
                    // Flush draft to store on close.
                    let draft = self.settings_panel.apply_draft();
                    let _ = self.settings_store.apply_snapshot(&draft);
                    self.settings_panel.visible = false;
                } else {
                    self.settings_panel.open(snap);
                }
                self.request_redraw();
            }
            KeyCommand::ToggleCommandPalette => {
                self.command_palette.toggle();
                if self.command_palette.visible {
                    self.refresh_palette_items();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleFocusMode => {
                // Enter with a default-length Pomodoro; re-baseline the timer so
                // the elapsed gap before the panel opened is not counted.
                self.focus.toggle(panels::focus_panel::DEFAULT_POMODORO_MIN);
                if self.focus.active {
                    let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                    self.focus.tick(now_ms);
                }
                self.request_redraw();
            }
            KeyCommand::BookmarkCurrentPage => {
                self.bookmark_current_page();
                self.request_redraw();
            }
            KeyCommand::SetTabContainer(container) => {
                let idx = self.tab_strip.active;
                self.set_tab_container(idx, container);
            }
            KeyCommand::DevConsole => {
                self.devtools_console.toggle();
                self.request_redraw();
            }
            KeyCommand::DevInspector => {
                self.dom_inspector.toggle();
                self.request_redraw();
            }
            KeyCommand::DevNetwork => {
                self.network_panel.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePrivacy => {
                self.privacy.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePip => {
                self.toggle_pip();
                self.request_redraw();
            }
            KeyCommand::ToggleReadLater => {
                self.read_later_panel.toggle();
                if self.read_later_panel.visible {
                    self.refresh_read_later();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleReaderView => {
                self.toggle_reader_view();
            }
            KeyCommand::ViewSource => {
                self.show_view_source();
            }
            KeyCommand::ToggleShortcuts => {
                self.shortcuts_panel.toggle();
                self.request_redraw();
            }
            KeyCommand::ZoomIn => {
                self.zoom_factor = zoom::zoom_in(self.zoom_factor);
                self.relayout();
            }
            KeyCommand::ZoomOut => {
                self.zoom_factor = zoom::zoom_out(self.zoom_factor);
                self.relayout();
            }
            KeyCommand::ZoomReset => {
                self.zoom_factor = zoom::zoom_reset();
                self.relayout();
            }
        }
    }

    /// Toggle the picture-in-picture window (task #21).
    ///
    /// When closing, just hides the card.  When opening, scans the current page
    /// layout for the first `<video>` element and embeds its `src` / `poster`;
    /// if the page has no video, the card opens with a placeholder so the user
    /// still gets feedback (and can drag / close it).
    /// Re-deliver media query changes to JS after accessibility prefs change.
    ///
    /// Called when the a11y panel closes so `prefers-reduced-motion` MQLs fire.
    fn deliver_a11y_media_changes(&self) {
        #[cfg(feature = "quickjs")]
        if let Some(js) = &self.js_ctx {
            let w = self.viewport_width_css();
            let h = self.viewport_height_css();
            let dark = if self.dark_mode { "true" } else { "false" };
            let rm = if self.a11y_store.reduced_motion() { "true" } else { "false" };
            js.eval_js(&format!(
                "if(typeof _lumen_deliver_media_changes==='function')\
                 _lumen_deliver_media_changes({w},{h},{dark},{rm});"
            ));
        }
    }

    fn toggle_pip(&mut self) {
        if self.pip.active {
            self.pip.close();
            return;
        }
        let win_w = self.viewport_width_css();
        let win_h = self.viewport_height_css() + tabs::strip::TAB_BAR_HEIGHT;
        let (src, poster) = self
            .layout_box
            .as_ref()
            .and_then(find_video_source)
            .unwrap_or_default();
        let title = self.title.clone().unwrap_or_default();
        self.pip.open(src, poster, title, win_w, win_h);
    }

    /// Сохранить текущую страницу в bfcache и стек навигации,
    /// затем загрузить `source` как новую страницу.
    /// Очищает `nav_fwd` (аналог браузера при навигации вперёд из середины истории).
    fn navigate_to(&mut self, source: PageSource) {
        click_log::log_nav(&source.describe());
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
        // Push current page to back stack (full-doc entry: no same_doc_state_json).
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: None,
            same_doc_state_json: None,
        });
        // New navigation invalidates forward history and resets same-doc state.
        self.nav_fwd.clear();
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        // Load new page.
        self.source = source;
        self.reload();
    }

    /// Перейти на `source`, заменяя текущую запись истории (без push в back-stack).
    /// Аналог `history.replaceState` / `location.replace()` в браузере.
    fn navigate_replace(&mut self, source: PageSource) {
        // New navigation invalidates forward history but does NOT push to back stack.
        self.nav_fwd.clear();
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        self.source = source;
        self.reload();
    }

    /// Перейти на предыдущую страницу в истории (Alt+Left).
    fn navigate_back(&mut self) {
        let Some(prev) = self.nav_back.pop() else { return };

        if let Some(state_json) = prev.same_doc_state_json {
            // Same-document navigation: fire popstate, update address bar, don't reload.
            // Push current same-doc state to forward stack so Alt+Right restores it.
            let cur_display = self.display_url.take();
            let cur_state = std::mem::replace(
                &mut self.current_history_state_json,
                state_json.clone(),
            );
            self.nav_fwd.push(NavEntry {
                source: self.source.clone(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                display_url: cur_display,
                same_doc_state_json: Some(cur_state),
            });
            let url = prev.display_url.unwrap_or_default();
            self.display_url = if url.is_empty() { None } else { Some(url.clone()) };
            if let Some(js) = &self.js_ctx {
                js.fire_popstate(&state_json, &url);
            }
            self.request_redraw();
            return;
        }

        // Full-document navigation: restore page and reload.
        // Push current page to forward stack.
        let cur_display = self.display_url.take();
        let cur_state = std::mem::replace(
            &mut self.current_history_state_json,
            String::from("null"),
        );
        self.nav_fwd.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: cur_display,
            // If we were in a same-doc state before this full-page nav, record it.
            same_doc_state_json: if cur_state != "null" { Some(cur_state) } else { None },
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

        if let Some(state_json) = next.same_doc_state_json {
            // Same-document forward navigation: fire popstate, update address bar.
            let cur_display = self.display_url.take();
            let cur_state = std::mem::replace(
                &mut self.current_history_state_json,
                state_json.clone(),
            );
            self.nav_back.push(NavEntry {
                source: self.source.clone(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                display_url: cur_display,
                same_doc_state_json: Some(cur_state),
            });
            let url = next.display_url.unwrap_or_default();
            self.display_url = if url.is_empty() { None } else { Some(url.clone()) };
            if let Some(js) = &self.js_ctx {
                js.fire_popstate(&state_json, &url);
            }
            self.request_redraw();
            return;
        }

        // Full-document forward navigation.
        let cur_display = self.display_url.take();
        let cur_state = std::mem::replace(
            &mut self.current_history_state_json,
            String::from("null"),
        );
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: cur_display,
            same_doc_state_json: if cur_state != "null" { Some(cur_state) } else { None },
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

    /// Execute a gesture action produced by the right-button drag recognizer.
    fn execute_gesture_action(
        &mut self,
        action: input::gesture::GestureAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use input::gesture::GestureAction;
        match action {
            GestureAction::NavigateBack => self.navigate_back(),
            GestureAction::NavigateForward => self.navigate_forward(),
            GestureAction::NewTab => self.open_new_tab(),
            GestureAction::CloseTab => {
                let idx = self.tab_strip.active;
                self.close_tab(idx, event_loop);
            }
        }
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
                    self.handle_omnibox_commit(value);
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

    /// Process a committed omnibox value: resolve aliases, then navigate or act.
    ///
    /// Order: `sidebar:` prefix → bang aliases (`!g`) → `@notes` / `@read-later`
    /// → record in search_history → plain navigate.
    fn handle_omnibox_commit(&mut self, value: String) {
        // `view-source:<url>` — fetch and display syntax-highlighted source (§D-2).
        if let Some(target_url) = value.trim().strip_prefix("view-source:") {
            let target_url = target_url.trim().to_owned();
            self.show_view_source_for_url(&target_url);
            return;
        }

        // `about:settings` — open the browser settings overlay (task D-7).
        if value.trim() == "about:settings" {
            let snap = self.settings_store.snapshot();
            self.settings_panel.open(snap);
            self.request_redraw();
            return;
        }

        // `sidebar:<url>` — load the URL into the right-docked sidebar panel (7D.3).
        if let Some(sidebar_url) = value.strip_prefix("sidebar:") {
            let sidebar_url = sidebar_url.trim().to_owned();
            if !sidebar_url.is_empty() {
                let sink = Arc::clone(&self.event_sink);
                let src = PageSource::from_arg(Some(&sidebar_url));
                match src.load_bytes(sink, Some(Arc::clone(&self.cookie_jar))) {
                    Ok(raw) => {
                        self.open_sidebar_page(sidebar_url, &raw.bytes, String::new());
                    }
                    Err(err) => {
                        eprintln!("sidebar: не удалось загрузить {sidebar_url}: {err}");
                        // Open panel with placeholder so user sees feedback.
                        self.sidebar.open(sidebar_url);
                        self.relayout();
                        self.request_redraw();
                    }
                }
            }
            return;
        }

        let aliases = self.omnibox_aliases.list_all().unwrap_or_default();
        if let Some(action) = omnibox::resolve(&value, &aliases) {
            match action {
                omnibox::AliasAction::Navigate(url) => {
                    self.navigate_to(PageSource::from_arg(Some(&url)));
                }
                omnibox::AliasAction::CreateNote(text) => {
                    self.notes.push(text);
                }
                omnibox::AliasAction::SaveReadLater(url) => {
                    // Spawn a background thread to fetch the page HTML and title.
                    // The result is sent back through `read_later_tx` and processed
                    // in `about_to_wait` via `read_later_rx`.
                    let tx = self.read_later_tx.clone();
                    let url_clone = url.clone();
                    std::thread::spawn(move || {
                        use lumen_core::ext::NetworkTransport;
                        use lumen_core::url::Url;
                        use lumen_network::HttpClient;
                        let Ok(parsed) = Url::parse(&url_clone) else { return };
                        let Ok(html) = HttpClient::new().fetch(&parsed) else { return };
                        let title = panels::read_later_panel::extract_title_from_html(&html);
                        let title = if title.is_empty() { url_clone.clone() } else { title };
                        let _ = tx.send((url_clone, title, html));
                    });
                    // Also persist into the bookmark store under a dedicated
                    // folder so the bookmark manager panel shows it.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = self.bookmarks.add(
                        &url,
                        &url,
                        "/Read Later",
                        &["read-later".to_owned()],
                        "",
                        now,
                    );
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                }
            }
            return;
        }

        // No alias matched — plain URL or search query.
        if !value.contains("://") && !value.starts_with('@') {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let _ = self.search_history.record(&value, now);
        }
        self.navigate_to(PageSource::from_arg(Some(&value)));
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

    /// Handle a key while the bookmark panel search box is focused.
    ///
    /// Returns `true` when the key was consumed. Modified keys (Ctrl/Cmd) are
    /// *not* consumed so global shortcuts continue to work.
    fn handle_bookmark_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.bookmark_panel.search_active = false;
                self.request_redraw();
                true
            }
            KeyCode::Backspace => {
                self.bookmark_panel.backspace_search();
                self.request_redraw();
                true
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    self.bookmark_panel.append_search(text);
                    self.request_redraw();
                    return true;
                }
                false
            }
        }
    }

    /// Handle keyboard input when the history panel is visible.
    ///
    /// When `search_active`: printable chars → search query, Backspace → delete
    /// char, Escape → blur search (panel stays open). Arrow keys scroll the list.
    /// Returns `true` if the key was consumed.
    fn handle_history_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                if self.history_panel.search_active {
                    self.history_panel.search_active = false;
                } else {
                    self.history_panel.visible = false;
                }
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.history_panel.search_active => {
                self.history_panel.backspace_search();
                self.refresh_history();
                self.request_redraw();
                true
            }
            KeyCode::ArrowDown => {
                self.history_panel.scroll_by(LINE_STEP_CSS_PX);
                self.request_redraw();
                true
            }
            KeyCode::ArrowUp => {
                self.history_panel.scroll_by(-LINE_STEP_CSS_PX);
                self.request_redraw();
                true
            }
            _ => {
                if self.history_panel.search_active
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.history_panel.append_search(ch);
                        }
                        self.refresh_history();
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Handle keyboard input when the settings panel is visible.
    ///
    /// Printable chars go to the focused text input. Escape closes panel (flushing
    /// draft). Returns `true` if the key was consumed.
    fn handle_settings_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                let draft = self.settings_panel.apply_draft();
                let _ = self.settings_store.apply_snapshot(&draft);
                self.settings_panel.visible = false;
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.settings_panel.focused_input.is_some() => {
                self.settings_panel.backspace();
                self.request_redraw();
                true
            }
            _ => {
                if self.settings_panel.focused_input.is_some()
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.settings_panel.append_char(ch);
                        }
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Обрабатывает клавишный ввод для панели горячих клавиш (§D-4).
    ///
    /// Когда активен rebind mode (`rebinding.is_some()`): захватывает
    /// следующую клавишу и передаёт в `accept_rebind`. Esc отменяет rebind.
    /// Возвращает `true`, если событие поглощено.
    fn handle_shortcuts_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if key_event.repeat {
            return false;
        }
        if self.shortcuts_panel.rebinding.is_some() {
            if code == KeyCode::Escape {
                self.shortcuts_panel.cancel_rebind();
                self.request_redraw();
                return true;
            }
            let modifier = {
                let m = self.modifiers;
                let ctrl = m.control_key();
                let shift = m.shift_key();
                let alt = m.alt_key();
                match (ctrl, shift, alt) {
                    (true, true, false) => "ctrl+shift",
                    (true, false, true) => "ctrl+alt",
                    (true, false, false) => "ctrl",
                    (false, true, false) => "shift",
                    (false, false, true) => "alt",
                    _ => "",
                }
            };
            let key = format!("{:?}", code);
            let key = key.trim_start_matches("Key").trim_start_matches("Digit").to_string();
            self.shortcuts_panel.accept_rebind(modifier, &key);
            self.request_redraw();
            return true;
        }
        if code == KeyCode::Escape {
            self.shortcuts_panel.close();
            self.request_redraw();
            return true;
        }
        false
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
        // Hint-mode activations have no real mouse coordinates, so x/y are 0.
        #[cfg(feature = "quickjs")]
        if let Some(ctx) = self.js_ctx.as_ref() {
            let script = format!(
                "_lumen_dispatch_mouse_event({}, 'click', 0, 0, 0, 1, 0)",
                node_id.index()
            );
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

    /// Returns the URL to display in the address bar and use for history / bookmarks.
    ///
    /// When `history.pushState` / `history.replaceState` has updated the virtual
    /// URL without a page load, `display_url` overrides the real `source` URL.
    fn current_display_url(&self) -> &str {
        self.display_url
            .as_deref()
            .or_else(|| self.source.url_str())
            .unwrap_or("")
    }

    /// Текущая логическая (CSS px) высота viewport-а. Если окно ещё не создано —
    /// fallback на layout-viewport 720 px, который у нас hardcoded в pipeline.
    fn viewport_height_css(&self) -> f32 {
        let total = match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().height as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 720.0,
        };
        let ws_bar = if self.workspace_panel.visible {
            panels::workspace_panel::SWITCHER_HEIGHT
        } else {
            0.0
        };
        (total - tabs::strip::TAB_BAR_HEIGHT - ws_bar).max(0.0)
    }

    /// CSS px ширина viewport-а — полная ширина окна, нужна scrollbar-overlay-у
    /// для размещения у правого края. Fallback на layout-viewport 1024 px (тот
    /// же hardcoded размер, что и в pipeline до создания окна).
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

    /// CSS px ширина области контента страницы — полная ширина окна минус
    /// ширина вертикальных панелей вкладок (слева) и sidebar (справа), если
    /// они видимы. Используется для клампинга горизонтального скролла.
    fn page_content_width_css(&self) -> f32 {
        let left_offset = if self.vertical_tabs.visible {
            panels::vertical_tabs::PANEL_WIDTH
        } else if self.tree_tabs.visible {
            panels::tree_tabs::PANEL_WIDTH
        } else {
            0.0
        };
        let right_offset = if self.sidebar.visible {
            panels::sidebar_panel::PANEL_WIDTH
        } else {
            0.0
        };
        (self.viewport_width_css() - left_offset - right_offset).max(0.0)
    }

    /// Open the sidebar with `url` and populate it with a freshly-laid-out page.
    ///
    /// Parses `html_bytes` as HTML, lays it out at [`PANEL_WIDTH`]-wide viewport,
    /// and stores the display list in the sidebar panel.  Triggers a relayout of
    /// the main page when the sidebar becomes visible (page width changes).
    fn open_sidebar_page(&mut self, url: String, html_bytes: &[u8], page_title: String) {
        let was_visible = self.sidebar.visible;
        self.sidebar.open(url.clone());

        // Decode bytes and parse HTML.
        let encoding = lumen_encoding::detect(html_bytes, None);
        let source_str = lumen_encoding::decode(encoding, html_bytes);
        let doc = lumen_html_parser::parse(&source_str);
        let doc_title = if page_title.is_empty() {
            extract_title(&doc).unwrap_or_default()
        } else {
            page_title
        };

        // Collect inline <style> blocks (no external CSS fetch for sidebar).
        let css_text = extract_style_blocks(&doc);
        let sheet = lumen_css_parser::parse(&css_text);

        let doc_arc = Arc::new(Mutex::new(doc));
        let src = LayoutSource {
            document: doc_arc,
            stylesheet: sheet,
            html_source: None,
        };

        let sidebar_vp = Size::new(
            panels::sidebar_panel::PANEL_WIDTH,
            self.viewport_height_css().max(100.0),
        );
        let (dl, _lb) = relayout_page(&src, sidebar_vp, &self.hyp_provider, self.dark_mode);
        let content_h = content_height_of(&dl);
        self.sidebar.set_page(dl, doc_title, content_h);

        if !was_visible {
            self.relayout();
        }
        self.request_redraw();
    }

    /// Reload workspace list from SQLite storage into the panel cache.
    ///
    /// Call this after every `Workspaces::create`, `rename`, or `delete` so
    /// the panel renders up-to-date chips on the next redraw.
    fn refresh_workspaces(&mut self) {
        let entries = self
            .workspaces
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|w| {
                let accent = panels::workspace_panel::parse_ws_color(&w.color);
                panels::workspace_panel::WsEntry {
                    id: w.id,
                    name: w.name,
                    accent,
                }
            })
            .collect();
        self.workspace_panel.set_workspaces(entries);
    }

    /// Reload the bookmark list from storage into the panel cache.
    ///
    /// Call this after every bookmark mutation (add / delete / move) so the
    /// panel renders up-to-date rows on the next redraw.
    /// Reload the read-later entry list from the in-memory store into the panel cache.
    ///
    /// Called after every save/delete and when the panel opens.  Shows the 50
    /// most recent items (unread first, then read, then archived).
    /// Toggle Reader View (§D-3, F9).
    ///
    /// When entering reader mode: extracts the article region from the current
    /// page's HTML source, wraps it in a clean reading template, and re-renders
    /// it as an in-memory `PageSource::Snapshot` without a network round-trip.
    /// The original source is stashed in `reader_original_source`.
    ///
    /// When exiting: restores the stashed source and reloads.
    fn toggle_reader_view(&mut self) {
        if let Some(original) = self.reader_original_source.take() {
            // Exit reader mode — restore original page.
            self.source = original;
            self.reload();
            return;
        }

        // Enter reader mode — extract article from current HTML source.
        let html = match self.layout_source.as_ref().and_then(|ls| ls.html_source.as_deref()) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return, // nothing to extract from
        };

        let Some(article) = reader_view::extract_article(&html) else { return };
        let reader_html = reader_view::build_reader_html(&article);

        let base_url = self.source.url_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "about:reader".to_owned());

        self.reader_original_source = Some(self.source.clone());
        self.source = PageSource::Snapshot { html: reader_html, base_url };
        self.reload();
    }

    /// Show syntax-highlighted source of the current page (Ctrl+U, §D-2).
    ///
    /// Uses the already-parsed HTML stored in `layout_source.html_source`.
    /// No-op when the page has no HTML source (e.g. empty tab).
    fn show_view_source(&mut self) {
        let html = match self.layout_source.as_ref().and_then(|ls| ls.html_source.as_deref()) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return,
        };
        let url = self.source.url_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "about:source".to_owned());
        let source_html = source_view::build_view_source_html(&url, &html);
        self.navigate_to(PageSource::Snapshot {
            html: source_html,
            base_url: format!("view-source:{url}"),
        });
    }

    /// Fetch `url` and display its raw bytes as syntax-highlighted source (§D-2).
    ///
    /// Used when the user types `view-source:<url>` in the address bar.
    fn show_view_source_for_url(&mut self, url: &str) {
        let source = PageSource::from_arg(Some(url));
        let sink = Arc::clone(&self.event_sink);
        let jar = Arc::clone(&self.cookie_jar);
        match source.load_bytes(sink, Some(jar)) {
            Ok(raw) => {
                let html_str = String::from_utf8_lossy(&raw.bytes).into_owned();
                let source_html = source_view::build_view_source_html(url, &html_str);
                self.navigate_to(PageSource::Snapshot {
                    html: source_html,
                    base_url: format!("view-source:{url}"),
                });
            }
            Err(e) => {
                eprintln!("view-source: не удалось загрузить {url}: {e}");
            }
        }
    }

    fn refresh_read_later(&mut self) {
        let mut entries = self
            .read_later_store
            .list_by_status(lumen_knowledge::ReadStatus::Unread, 50)
            .unwrap_or_default();
        entries.extend(
            self.read_later_store
                .list_by_status(lumen_knowledge::ReadStatus::Read, 50)
                .unwrap_or_default(),
        );
        self.read_later_panel.refresh(entries);
    }

    fn refresh_bookmarks(&mut self) {
        let entries = self
            .bookmarks
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|b| panels::bookmark_panel::BmEntry {
                id: b.id,
                url: b.url,
                title: b.title,
                folder: b.folder,
            })
            .collect();
        self.bookmark_panel.set_data(entries);
    }

    /// Reload the history panel data from `history_store`.
    ///
    /// When `history_panel.query` is non-empty, uses `HistoryFts::search` for
    /// full-text matching. Otherwise falls back to `History::recent(50)`.
    fn refresh_history(&mut self) {
        let query = self.history_panel.query.trim().to_owned();
        let items: Vec<panels::history_panel::HistoryItem> = if query.is_empty() {
            self.history_store
                .recent(50)
                .unwrap_or_default()
                .into_iter()
                .map(|e| panels::history_panel::HistoryItem {
                    id: e.id,
                    url: e.url,
                    title: e.title,
                    visit_date: e.visit_date,
                    visit_count: e.visit_count,
                })
                .collect()
        } else {
            self.history_fts
                .search(&query, 50)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(i, hit)| panels::history_panel::HistoryItem {
                    id: i as i64 + 1,
                    url: hit.url,
                    title: hit.title,
                    visit_date: 0,
                    visit_count: 1,
                })
                .collect()
        };
        self.history_panel.set_items(items);
    }

    /// Top-left corner of the history panel in window-space CSS px.
    fn history_panel_anchor(&self) -> (f32, f32) {
        let win_w = self.viewport_width_css();
        let px = (win_w - panels::history_panel::PANEL_W) * 0.5;
        let py = tabs::strip::TAB_BAR_HEIGHT + 4.0;
        (px, py)
    }

    /// Rebuild the command-palette item list: curated commands, every bookmark,
    /// and — when the query is non-empty — matching history pages (FTS).
    ///
    /// History depends on the query (the FTS index has no "list all"), so this
    /// is called both on open and on every query edit. Commands and bookmarks
    /// are query-independent; the palette's own fuzzy filter ranks the union.
    fn refresh_palette_items(&mut self) {
        use panels::command_palette::{PaletteAction, PaletteItem};

        let mut items: Vec<PaletteItem> =
            PaletteAction::all().iter().copied().map(PaletteItem::command).collect();

        // Bookmarks (query-independent — fuzzy-filtered in the palette).
        for b in self.bookmarks.list_all().unwrap_or_default() {
            items.push(PaletteItem::bookmark(b.title, b.url));
        }

        // History: FTS needs a query, so only add hits once the user types.
        let query = self.command_palette.query.trim().to_owned();
        if !query.is_empty()
            && let Ok(hits) = self.history_fts.search(&query, 12)
        {
            for hit in hits {
                items.push(PaletteItem::history(hit.title, hit.url));
            }
        }

        self.command_palette.set_items(items);
    }

    /// Handle a key while the command palette modal is open.
    ///
    /// Always returns `true` (the modal swallows every key). `Esc` closes,
    /// `Enter` activates the selected item, `↑/↓` move the selection,
    /// `Backspace` edits the query, and printable characters extend it. Editing
    /// the query refreshes history results.
    fn handle_palette_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.command_palette.close();
                self.request_redraw();
            }
            KeyCode::ArrowDown if !key_event.repeat => {
                self.command_palette.select_next();
                self.request_redraw();
            }
            KeyCode::ArrowUp if !key_event.repeat => {
                self.command_palette.select_prev();
                self.request_redraw();
            }
            KeyCode::Enter if !key_event.repeat => {
                if let Some(item) = self.command_palette.selected_item().cloned() {
                    self.command_palette.close();
                    self.activate_palette(&item, event_loop);
                }
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.command_palette.backspace();
                self.refresh_palette_items();
                self.request_redraw();
            }
            _ => {
                // Ignore modified keys other than the toggle (handled globally).
                if self.modifiers.control_key() || self.modifiers.super_key() {
                    return false;
                }
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    self.command_palette.append(text);
                    self.refresh_palette_items();
                    self.request_redraw();
                }
            }
        }
        true
    }

    /// Execute the action behind a selected palette item: run the command, or
    /// navigate to the bookmark / history URL.
    fn activate_palette(
        &mut self,
        item: &panels::command_palette::PaletteItem,
        event_loop: &ActiveEventLoop,
    ) {
        use panels::command_palette::{PaletteAction, PaletteKind};
        match &item.kind {
            PaletteKind::Bookmark | PaletteKind::History => {
                if !item.url.is_empty() {
                    self.navigate_to(PageSource::from_arg(Some(&item.url)));
                }
            }
            PaletteKind::Command(action) => match action {
                PaletteAction::NewTab => self.open_new_tab(),
                PaletteAction::CloseTab => {
                    let idx = self.tab_strip.active;
                    self.close_tab(idx, event_loop);
                }
                PaletteAction::Reload => self.reload(),
                PaletteAction::NavigateBack => self.navigate_back(),
                PaletteAction::NavigateForward => self.navigate_forward(),
                PaletteAction::FindOnPage => {
                    self.hint.close();
                    self.find.open();
                }
                PaletteAction::OpenAddressBar => {
                    self.hint.close();
                    let current = self.current_display_url().to_owned();
                    self.address_bar.open(&current);
                }
                PaletteAction::ToggleBookmarks => {
                    self.bookmark_panel.toggle();
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                }
                PaletteAction::BookmarkCurrentPage => self.bookmark_current_page(),
                PaletteAction::ToggleVerticalTabs => {
                    self.vertical_tabs.toggle();
                    self.relayout();
                }
                PaletteAction::ToggleDevConsole => self.devtools_console.toggle(),
                PaletteAction::ToggleShields => self.shields.toggle(),
                PaletteAction::ToggleVimMode => {
                    if self.vim_mode.is_some() {
                        self.vim_mode = None;
                    } else {
                        self.vim_mode = Some(input::vim::VimMode::new());
                    }
                }
            },
        }
        self.request_redraw();
    }

    /// Add the current page to bookmarks (Ctrl+D).
    ///
    /// No-op when the current page has no URL (e.g. blank tab). The active tab
    /// title is used when available, otherwise the URL stands in as the title.
    fn bookmark_current_page(&mut self) {
        let url = self.current_display_url().to_owned();
        if url.is_empty() {
            return;
        };
        let title = self
            .tab_strip
            .tabs
            .get(self.tab_strip.active)
            .map(|t| t.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| url.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = self.bookmarks.add(&url, &title, "", &[], "", now);
        if self.bookmark_panel.visible {
            self.refresh_bookmarks();
        }
    }

    /// Top-left anchor of the bookmark panel overlay (just under the tab bar).
    fn bookmark_anchor(&self) -> (f32, f32) {
        (8.0, tabs::strip::TAB_BAR_HEIGHT + 4.0)
    }

    /// Resolve a bookmark drag release: dropping on a folder re-files the
    /// bookmark (`Bookmarks::set_folder`), dropping elsewhere opens it.
    fn finish_bookmark_drop(&mut self, id: i64) {
        let Some(cursor) = self.cursor_position else { return };
        let dpr = self
            .renderer
            .as_ref()
            .map_or(1.0_f32, |r| r.scale_factor() as f32)
            .max(1e-6);
        let x_css = (cursor.x as f32) / dpr;
        let y_css = (cursor.y as f32) / dpr;
        let Some(url) = self
            .bookmark_panel
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.url.clone())
        else {
            return;
        };
        let (ax, ay) = self.bookmark_anchor();
        match panels::bookmark_panel::hit_test(&self.bookmark_panel, x_css, y_css, ax, ay) {
            Some(panels::bookmark_panel::BookmarkHit::SelectFolder(folder)) => {
                // Re-file: `None` (the "All" row) moves the bookmark to root.
                let target = folder.unwrap_or_default();
                let _ = self.bookmarks.set_folder(&url, &target);
                self.refresh_bookmarks();
            }
            _ => {
                // Released over the same row / list / outside: treat as a click.
                self.bookmark_panel.visible = false;
                self.bookmark_panel.search_active = false;
                self.navigate_to(PageSource::from_arg(Some(&url)));
            }
        }
    }

    /// Максимальный валидный scroll_y: ничего не скроллим, если контент
    /// помещается в viewport. Иначе — `content_height − viewport_height`.
    fn max_scroll(&self) -> f32 {
        (self.content_height - self.viewport_height_css()).max(0.0)
    }

    /// Максимальный валидный scroll_x: 0 если контент помещается по ширине.
    ///
    /// Использует `page_content_width_css()` — полная ширина минус панель вкладок.
    fn max_scroll_x(&self) -> f32 {
        (self.content_width - self.page_content_width_css()).max(0.0)
    }

    /// Rebuild `snap_containers` from the current `layout_box`.
    ///
    /// Called whenever `layout_box` changes (relayout, page load, tab switch).
    /// Cheap when the page has no `scroll-snap-type` declarations (returns empty).
    fn update_snap_containers(&mut self) {
        match &self.layout_box {
            Some(lb) => self.snap_containers = collect_snap_containers(lb),
            None => self.snap_containers.clear(),
        }
    }

    /// Apply CSS Scroll Snap L1 to a proposed page-level Y scroll offset.
    ///
    /// Finds the snap container whose node matches the root layout box (html
    /// element), overrides its rect with the viewport dimensions (the snap port
    /// for page scroll is the viewport, not the full document), then calls
    /// `find_snap_target`. Returns `target_y` unchanged if no snap applies.
    fn apply_page_y_snap(&self, target_y: f32) -> f32 {
        let root_node = match &self.layout_box {
            Some(lb) => lb.node,
            None => return target_y,
        };
        let vw = self.viewport_width_css();
        let vh = self.viewport_height_css();
        for sc in &self.snap_containers {
            if sc.node == root_node {
                // Proximity threshold uses viewport size, not full document size.
                let mut sc_viewport = sc.clone();
                sc_viewport.rect = lumen_core::geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some((_, sy)) = find_snap_target(
                    &sc_viewport,
                    (self.scroll_x, self.scroll_y),
                    (self.scroll_x, target_y),
                ) {
                    return clamp_scroll(sy, self.max_scroll());
                }
            }
        }
        target_y
    }

    /// Apply CSS Scroll Snap L1 to a proposed page-level X scroll offset.
    ///
    /// Mirror of `apply_page_y_snap` for horizontal scroll.
    fn apply_page_x_snap(&self, target_x: f32) -> f32 {
        let root_node = match &self.layout_box {
            Some(lb) => lb.node,
            None => return target_x,
        };
        let vw = self.viewport_width_css();
        let vh = self.viewport_height_css();
        for sc in &self.snap_containers {
            if sc.node == root_node {
                let mut sc_viewport = sc.clone();
                sc_viewport.rect = lumen_core::geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some((sx, _)) = find_snap_target(
                    &sc_viewport,
                    (self.scroll_x, self.scroll_y),
                    (target_x, self.scroll_y),
                ) {
                    return clamp_scroll(sx, self.max_scroll_x());
                }
            }
        }
        target_x
    }

    /// Горизонтальный скролл на delta CSS px (инстантный).
    fn scroll_x_by(&mut self, delta: f32) {
        let clamped = clamp_scroll(self.scroll_x + delta, self.max_scroll_x());
        let snapped = self.apply_page_x_snap(clamped);
        if (snapped - self.scroll_x).abs() > f32::EPSILON {
            self.scroll_x = snapped;
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
    /// стартует (и текущая сбрасывается). Применяет CSS Scroll Snap L1 если
    /// страница объявляет `scroll-snap-type` на корневом элементе.
    fn start_smooth_scroll(&mut self, target: f32) {
        let max = self.max_scroll();
        let target_clamped = clamp_scroll(target, max);
        // Apply page-level CSS Scroll Snap L1: snap to the nearest declared
        // snap point before starting the animation.
        let target_clamped = self.apply_page_y_snap(target_clamped);
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

    /// Scroll the currently focused pane by `delta` CSS px.
    ///
    /// In split mode, routes to the right pane when it has focus; otherwise
    /// falls through to `scroll_by_smooth` for the left (active) pane.
    fn scroll_active_pane(&mut self, delta: f32) {
        // Pre-compute viewport height before mutably borrowing split_view.
        let vh = self.viewport_height_css();
        let right_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);
        if right_focused {
            if let Some(ref mut sv) = self.split_view {
                let max = (sv.right.content_height - vh).max(0.0);
                sv.right.scroll_y = (sv.right.scroll_y + delta).clamp(0.0, max);
            }
            self.request_redraw();
            return;
        }
        self.scroll_by_smooth(delta);
    }

    /// Scroll the currently focused pane to an absolute position.
    ///
    /// `target = f32::INFINITY` scrolls to the bottom of the pane's content.
    fn scroll_active_pane_to(&mut self, target: f32) {
        let vh = self.viewport_height_css();
        let right_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);
        if right_focused {
            if let Some(ref mut sv) = self.split_view {
                let max = (sv.right.content_height - vh).max(0.0);
                sv.right.scroll_y = target.clamp(0.0, max);
            }
            self.request_redraw();
            return;
        }
        self.start_smooth_scroll(target);
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

    /// Drop CPU-decoded images that have scrolled outside the gate zone (ADR-008 §10E.4).
    ///
    /// Called once per rendered frame (in `RedrawRequested`) after scroll advancement.
    /// No-op when the cache is empty or the layout tree or renderer is unavailable.
    fn try_discard_offscreen_images(&mut self) {
        let (Some(root), Some(renderer)) = (self.layout_box.as_ref(), self.renderer.as_ref()) else {
            return;
        };
        let vp_size = renderer.viewport_size();
        let viewport = Size::new(vp_size.width, vp_size.height);
        scroll::decode_gating::discard_offscreen_images(
            &mut self.image_cache,
            root,
            viewport,
            self.scroll_x,
            self.scroll_y,
        );
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
            // Page content is shifted down by TAB_BAR_HEIGHT via PushTransform.
            let page_x = x_css + self.scroll_x;
            let page_y = (y_css - tabs::strip::TAB_BAR_HEIGHT) + self.scroll_y;
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
            PageSource::Empty | PageSource::AboutBlank => return,
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

    /// Persist every open tab (URL + title + scroll + serialised DOM) to the
    /// SQLite session store on window close (§10I).
    ///
    /// Walks the tab strip in left-to-right order, pulling each tab's state from
    /// whichever slot holds it: the active tab from `self`, background tabs from
    /// `bg_tabs`, hibernated tabs from `tab_snapshots`. Tabs without a real URL
    /// (blank, never-loaded) are skipped. Silent — write errors do not block exit.
    fn save_full_session(&self) {
        let mut tabs: Vec<lumen_storage::PersistedTab> = Vec::new();
        let active_idx = self.tab_strip.active;
        for (idx, entry) in self.tab_strip.tabs.iter().enumerate() {
            let persisted = if idx == active_idx {
                source_url_string(&self.source).map(|url| lumen_storage::PersistedTab {
                    url,
                    title: self.title.clone().unwrap_or_default(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    is_active: true,
                    dom_blob: dom_blob_of(self.layout_source.as_ref()),
                })
            } else if let Some(snap) = self.bg_tabs.get(&entry.id) {
                source_url_string(&snap.source).map(|url| lumen_storage::PersistedTab {
                    url,
                    title: snap.title.clone().unwrap_or_default(),
                    scroll_x: snap.scroll_x,
                    scroll_y: snap.scroll_y,
                    is_active: false,
                    dom_blob: dom_blob_of(snap.layout_source.as_ref()),
                })
            } else if self.hibernated_tabs.contains_key(&entry.id) {
                // DOM blob already on disk in tab_snapshots — copy it over.
                match self.tab_snapshots.fetch(entry.id as i64) {
                    Ok(Some(data)) if !data.url.is_empty() => Some(lumen_storage::PersistedTab {
                        url: data.url,
                        title: data.title,
                        scroll_x: data.scroll_x,
                        scroll_y: data.scroll_y,
                        is_active: false,
                        dom_blob: data.dom_blob,
                    }),
                    _ => None,
                }
            } else {
                None // Blank / never-loaded tab.
            };
            if let Some(t) = persisted {
                tabs.push(t);
            }
        }

        if let Err(e) = self.session_store.save(&tabs) {
            eprintln!("session: не удалось сохранить сессию: {e}");
        }
    }

    /// Reopen the tabs saved by [`Self::save_full_session`] (§10I).
    ///
    /// Called once at launch only when the user started the browser with no
    /// explicit page (so we do not clobber an `argv`-requested page). The
    /// previously-active tab's source + scroll are installed into `self` so the
    /// normal load pipeline renders it; each background tab is parked via the
    /// hibernation machinery (`hibernated_tabs` + `tab_snapshots`) so switching
    /// to it reconstructs it from its DOM blob without a network round-trip.
    fn restore_session(&mut self) {
        let tabs = match self.session_store.load() {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => return,
            Err(e) => {
                eprintln!("session: не удалось прочитать сессию: {e}");
                return;
            }
        };
        let active_idx = session_persist::active_index(&tabs);

        // Rebuild the tab strip from scratch — one entry per restored tab, in
        // saved order. The strip starts with a single blank tab (id 0); reuse it.
        self.tab_strip.tabs.clear();
        self.tab_strip.next_id = 0;

        for (idx, tab) in tabs.into_iter().enumerate() {
            let id = self.tab_strip.next_id;
            self.tab_strip.next_id += 1;
            self.tab_strip.tabs.push(tabs::strip::TabEntry {
                id,
                title: if tab.title.is_empty() {
                    "Восстановленная вкладка".to_owned()
                } else {
                    tab.title.clone()
                },
                tab_state: TabState::Active,
                opener_id: None,
                container: tabs::containers::ContainerKind::None,
                last_activated_ms: 0.0,
            });
            self.lifecycle_mgr.open_tab(id as u64);

            if idx == active_idx {
                // Active tab: load fresh through the normal pipeline.
                self.source = PageSource::from_arg(Some(&tab.url));
                self.scroll_x = tab.scroll_x;
                self.scroll_y = tab.scroll_y;
                self.title = Some(tab.title);
            } else {
                // Background tab: park as hibernated so switch_tab restores it
                // from the DOM blob on demand.
                let data = lumen_storage::HibernatedTabData {
                    dom_blob: tab.dom_blob,
                    css_source: String::new(),
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    scroll_x: tab.scroll_x,
                    scroll_y: tab.scroll_y,
                };
                if self.tab_snapshots.store(id as i64, &data).is_ok() {
                    self.hibernated_tabs.insert(
                        id,
                        tab_lifecycle::TabMetadata { url: tab.url, title: tab.title },
                    );
                    let last = self.tab_strip.tabs.len() - 1;
                    self.tab_strip.set_tab_state(last, TabState::Hibernated);
                }
            }
        }

        self.tab_strip.active = active_idx.min(self.tab_strip.tabs.len().saturating_sub(1));
    }

    // ── Tab lifecycle: hibernation and restore ─────────────────────────────────

    /// Promote a background tab from T2→T3 (Hibernated) by serialising its DOM
    /// to SQLite and evicting the in-memory `PageSnapshot`.
    ///
    /// On failure (serialise error, SQLite error) the snapshot is put back into
    /// `bg_tabs` and the tab stays at T2.
    fn hibernate_bg_tab(&mut self, tab_id: usize) {
        let Some(snap) = self.bg_tabs.remove(&tab_id) else { return };

        // Serialise DOM via Document::to_bytes() (bincode).
        let (dom_blob, css_source) = if let Some(ls) = snap.layout_source.as_ref() {
            match ls.document.lock() {
                Ok(doc) => {
                    let blob = doc.to_bytes().unwrap_or_default();
                    let css = extract_style_blocks(&doc);
                    (blob, css)
                }
                Err(_) => (vec![], String::new()),
            }
        } else {
            (vec![], String::new())
        };

        let url = match &snap.source {
            PageSource::Url(u) => u.clone(),
            PageSource::File(p) => format!("file://{}", p.display()),
            PageSource::Snapshot { base_url, .. } => base_url.clone(),
            PageSource::Empty | PageSource::AboutBlank => String::new(),
        };
        let title = snap.title.clone().unwrap_or_default();
        let scroll_x = snap.scroll_x;
        let scroll_y = snap.scroll_y;

        let data = lumen_storage::HibernatedTabData {
            dom_blob,
            css_source,
            url: url.clone(),
            title: title.clone(),
            scroll_x,
            scroll_y,
        };

        if let Err(e) = self.tab_snapshots.store(tab_id as i64, &data) {
            eprintln!("Ошибка hibernate tab {tab_id}: {e}");
            // Rollback — keep the snapshot in RAM.
            self.bg_tabs.insert(tab_id, snap);
            return;
        }

        // Keep only lightweight metadata in RAM (scroll state stays in SQLite).
        self.hibernated_tabs.insert(
            tab_id,
            tab_lifecycle::TabMetadata { url, title },
        );

        // Update badge in the strip (T3 = grey dot).
        if let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) {
            self.tab_strip.set_tab_state(idx, tab_lifecycle::TabState::Hibernated);
        }
    }

    /// Restore a T3-hibernated tab into the active slot.
    ///
    /// Fetches the DOM blob from SQLite, reconstructs the `Document` via
    /// `Document::from_bytes()`, re-parses inline CSS, and re-runs
    /// layout+paint.  Returns `true` on success so `switch_tab` knows
    /// whether to fall back to a blank tab.
    fn restore_hibernated_tab(&mut self, tab_id: usize) -> bool {
        // Start spinner timer for long restore operations (>200ms).
        self.restore_spinner_start_ms = Some(self.epoch.elapsed().as_secs_f64() * 1000.0);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }

        let Some(meta) = self.hibernated_tabs.remove(&tab_id) else {
            self.restore_spinner_start_ms = None;
            return false;
        };

        // Pre-fill title from lightweight metadata for immediate window title update.
        self.title = Some(meta.title.clone());

        let data = match self.tab_snapshots.fetch(tab_id as i64) {
            Ok(Some(d)) => d,
            Ok(None) => {
                eprintln!("tab {tab_id}: snapshot missing (url={})", meta.url);
                // Put metadata back so the strip still shows Hibernated.
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
            Err(e) => {
                eprintln!("tab {tab_id}: snapshot read error (url={}): {e}", meta.url);
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
        };

        // Reconstruct Document from bincode blob.
        let doc = match Document::from_bytes(&data.dom_blob) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Ошибка десериализации DOM вкладки {tab_id}: {e}");
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
        };

        // Re-parse CSS from inline <style> blocks preserved in the DOM.
        let css = if data.css_source.is_empty() {
            extract_style_blocks(&doc)
        } else {
            data.css_source.clone()
        };
        let stylesheet = lumen_css_parser::parse(&css);

        // Rebuild a fresh PersistentJs runtime. The JS heap cannot be
        // serialised, so the page's inline <script> blocks are re-run against
        // the restored DOM. The runtime shares the returned Arc<Mutex<Document>>
        // with the layout tree so both observe the same document.
        self.js_ctx = None;
        let event_sink = self.event_sink.clone();
        let cookie_banner_dismiss = self.cookie_banner_dismiss;
        let deterministic = self.deterministic;
        let (document_arc, js_ctx) = tab_lifecycle::hibernate::restore_js_context(
            &data.url,
            doc,
            event_sink,
            &mut self.ls_storage,
            self.idb_dir.as_deref(),
            &self.sw_backend,
            cookie_banner_dismiss,
            deterministic,
            Some(Arc::clone(&self.cookie_jar)),
        );

        let layout_source = LayoutSource {
            document: Arc::clone(&document_arc),
            stylesheet,
            html_source: None,
        };

        // Re-run layout+paint with the current viewport (including zoom).
        let phys = self.renderer.as_ref().map_or_else(
            || (1024.0_f32, 720.0_f32),
            |r| {
                let s = r.viewport_size();
                (s.width, s.height)
            },
        );
        let meta_scale = meta_initial_scale(&layout_source);
        let (css_w, css_h) = zoom::effective_viewport(phys.0, phys.1, meta_scale, self.zoom_factor);
        let viewport = lumen_core::geom::Size::new(css_w, css_h);
        let (display_list, lb) = relayout_page(&layout_source, viewport, &self.hyp_provider, self.dark_mode);

        // Install into the active slot.
        self.display_list = display_list;
        self.title = Some(data.title);
        self.layout_source = Some(layout_source);
        self.layout_box = Some(lb);
        self.js_ctx = js_ctx;
        self.scroll_x = data.scroll_x;
        self.scroll_y = data.scroll_y;
        self.content_height = content_height_of(&self.display_list);
        self.content_width = content_width_of(&self.display_list);

        // Seed the restored runtime with layout geometry + viewport so JS can
        // query bounding rects immediately (mirrors the fresh-load path).
        #[cfg(feature = "quickjs")]
        if let (Some(js), Some(lb_ref)) = (&self.js_ctx, self.layout_box.as_ref()) {
            js.update_layout_rects(collect_layout_rects(lb_ref));
            js.update_computed_styles(collect_computed_styles(lb_ref));
            js.update_viewport_size(viewport.width, viewport.height);
        }

        // Remove the SQLite entry — it is no longer needed.
        let _ = self.tab_snapshots.delete(tab_id as i64);

        // Restore complete — hide the spinner overlay.
        self.restore_spinner_start_ms = None;

        true
    }

    /// Poll the lifecycle manager approximately once per second.
    ///
    /// Processes tier transitions returned by `tick_idle` + `lru_evict`:
    /// - `Hibernated` transitions evict the corresponding `bg_tabs` entry to SQLite.
    /// - Other transitions update the tab strip badge.
    fn tick_lifecycle(&mut self) {
        if self.lifecycle_last_tick.elapsed().as_secs() < 1 {
            return;
        }
        self.lifecycle_last_tick = std::time::Instant::now();

        let transitions = self.lifecycle_mgr.tick_idle(tab_lifecycle::MemoryPressure::Low);
        let evicted = self.lifecycle_mgr.lru_evict();

        for tr in transitions.into_iter().chain(evicted) {
            let tab_id = tr.tab_id as usize;

            if tr.to == tab_lifecycle::TabState::Hibernated {
                if self.bg_tabs.contains_key(&tab_id) {
                    self.hibernate_bg_tab(tab_id);
                }
                continue;
            }

            // Update strip badge for BackgroundOld (amber) or other tier changes.
            if let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) {
                self.tab_strip.set_tab_state(idx, tr.to);
            }
        }

        // Auto-archive (7A.5): move background tabs idle for > 12 h out of the
        // strip.  Only runs when there are ≥ 2 tabs (the active tab is never
        // archived) and the tab is not already hibernated (RAM already saved).
        if self.tab_strip.len() >= 2 {
            let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
            let threshold = tabs::archive::ARCHIVE_AFTER_MS;
            // Collect IDs to archive (avoiding borrow conflict on tab_strip).
            let to_archive: Vec<usize> = self
                .tab_strip
                .tabs
                .iter()
                .enumerate()
                .filter(|(i, t)| {
                    *i != self.tab_strip.active
                        && t.tab_state != tab_lifecycle::TabState::Hibernated
                        && (now_ms - t.last_activated_ms) > threshold
                })
                .map(|(_, t)| t.id)
                .collect();

            for tab_id in to_archive {
                // Guard: never archive down to 0 tabs.
                if self.tab_strip.len() <= 1 {
                    break;
                }
                let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) else {
                    continue;
                };
                let title = self.tab_strip.tabs[idx].title.clone();
                let container = self.tab_strip.tabs[idx].container;
                let url = self
                    .bg_tabs
                    .get(&tab_id)
                    .and_then(|s| s.source.url_str().map(|u| u.to_owned()))
                    .unwrap_or_default();
                self.archive.push(tabs::archive::ArchivedTab {
                    id: tab_id,
                    title,
                    url,
                    container,
                });
                // Evict in-memory snapshot and remove from strip + lifecycle.
                self.bg_tabs.remove(&tab_id);
                self.lifecycle_mgr.close_tab(tab_id as u64);
                self.tab_strip.remove(idx);
            }
        }
    }

    // ── Tab management ────────────────────────────────────────────────────────

    /// Move all per-page fields from `self` into a `PageSnapshot`.
    ///
    /// Called before switching to a different tab so the current page state can
    /// be frozen while the new tab becomes active.
    fn save_page_snapshot(&mut self) -> PageSnapshot {
        PageSnapshot {
            display_list: std::mem::take(&mut self.display_list),
            title: self.title.take(),
            pending_images: std::mem::take(&mut self.pending_images),
            source: self.source.clone(),
            runtime: std::mem::take(&mut self.runtime),
            animation_scheduler: std::mem::replace(
                &mut self.animation_scheduler,
                animation_scheduler::AnimationScheduler::new(),
            ),
            transition_scheduler: std::mem::take(&mut self.transition_scheduler),
            prev_styles: std::mem::take(&mut self.prev_styles),
            anim_frame: self.anim_frame.take(),
            layout_box: self.layout_box.take(),
            find: std::mem::take(&mut self.find),
            address_bar: std::mem::take(&mut self.address_bar),
            hint: std::mem::take(&mut self.hint),
            scroll_y: self.scroll_y,
            scroll_x: self.scroll_x,
            content_height: self.content_height,
            content_width: self.content_width,
            layout_source: self.layout_source.take(),
            pending_reload: std::mem::replace(
                &mut self.pending_reload,
                Rc::new(Cell::new(false)),
            ),
            pending_js_navigate: self.pending_js_navigate.take(),
            stream_builder: self.stream_builder.take(),
            stream_last_paint: self.stream_last_paint,
            preload_dispatched: std::mem::take(&mut self.preload_dispatched),
            ime_composing: self.ime_composing.take(),
            bfcache: std::mem::replace(&mut self.bfcache, BfCache::new(16)),
            nav_back: std::mem::take(&mut self.nav_back),
            nav_fwd: std::mem::take(&mut self.nav_fwd),
            form_state: std::mem::take(&mut self.form_state),
            validation_tooltip: self.validation_tooltip.take(),
            color_picker_node: self.color_picker_node.take(),
            ls_storage: std::mem::take(&mut self.ls_storage),
            idb_dir: self.idb_dir.clone(),
            sw_backend: std::mem::replace(
                &mut self.sw_backend,
                Arc::new(std::sync::Mutex::new(
                    lumen_storage::store::InMemoryStorage::new(),
                )),
            ),
            js_ctx: self.js_ctx.take(),
            first_paint_delivered: self.first_paint_delivered,
            first_contentful_paint_delivered: self.first_contentful_paint_delivered,
            animated_gifs: std::mem::take(&mut self.animated_gifs),
            gif_last_frame: std::mem::take(&mut self.gif_last_frame),
            image_cache: std::mem::replace(
                &mut self.image_cache,
                lumen_image::ImageDecodeCache::new(),
            ),
            zoom_factor: self.zoom_factor,
            display_url: self.display_url.take(),
            current_history_state_json: std::mem::replace(
                &mut self.current_history_state_json,
                String::from("null"),
            ),
            reader_original_source: self.reader_original_source.take(),
        }
    }

    /// Restore per-page fields from a `PageSnapshot` into `self`.
    ///
    /// Called after a tab switch to make a previously-frozen tab active again.
    fn restore_page_snapshot(&mut self, snap: PageSnapshot) {
        self.display_list = snap.display_list;
        self.title = snap.title;
        self.pending_images = snap.pending_images;
        self.source = snap.source;
        self.runtime = snap.runtime;
        self.animation_scheduler = snap.animation_scheduler;
        self.transition_scheduler = snap.transition_scheduler;
        self.prev_styles = snap.prev_styles;
        self.anim_frame = snap.anim_frame;
        self.layout_box = snap.layout_box;
        self.find = snap.find;
        self.address_bar = snap.address_bar;
        self.hint = snap.hint;
        self.scroll_y = snap.scroll_y;
        self.scroll_x = snap.scroll_x;
        self.content_height = snap.content_height;
        self.content_width = snap.content_width;
        self.layout_source = snap.layout_source;
        self.pending_reload = snap.pending_reload;
        self.pending_js_navigate = snap.pending_js_navigate;
        self.stream_builder = snap.stream_builder;
        self.stream_last_paint = snap.stream_last_paint;
        self.preload_dispatched = snap.preload_dispatched;
        self.ime_composing = snap.ime_composing;
        self.bfcache = snap.bfcache;
        self.nav_back = snap.nav_back;
        self.nav_fwd = snap.nav_fwd;
        self.form_state = snap.form_state;
        self.validation_tooltip = snap.validation_tooltip;
        self.color_picker_node = snap.color_picker_node;
        self.ls_storage = snap.ls_storage;
        self.idb_dir = snap.idb_dir;
        self.sw_backend = snap.sw_backend;
        self.js_ctx = snap.js_ctx;
        self.first_paint_delivered = snap.first_paint_delivered;
        self.first_contentful_paint_delivered = snap.first_contentful_paint_delivered;
        self.animated_gifs = snap.animated_gifs;
        self.gif_last_frame = snap.gif_last_frame;
        self.image_cache = snap.image_cache;
        self.zoom_factor = snap.zoom_factor;
        self.display_url = snap.display_url;
        self.current_history_state_json = snap.current_history_state_json;
        self.reader_original_source = snap.reader_original_source;
    }

    /// Reset all per-page fields to blank-tab defaults.
    ///
    /// Called after `save_page_snapshot()` to prepare `self` for a fresh tab
    /// before loading a URL or showing an empty page.
    fn reset_to_blank_tab(&mut self) {
        self.display_list = Vec::new();
        self.title = None;
        self.pending_images = Vec::new();
        self.source = PageSource::Empty;
        self.runtime = runtime::EventLoop::new();
        self.animation_scheduler = animation_scheduler::AnimationScheduler::new();
        self.transition_scheduler = TransitionScheduler::new();
        self.prev_styles = HashMap::new();
        self.anim_frame = None;
        self.layout_box = None;
        self.find = find::FindState::default();
        self.address_bar = address_bar::AddressBarState::default();
        self.hint = hints::HintState::default();
        self.scroll_y = 0.0;
        self.scroll_x = 0.0;
        self.content_height = 0.0;
        self.content_width = 0.0;
        self.layout_source = None;
        self.pending_reload = Rc::new(Cell::new(false));
        self.pending_js_navigate = None;
        self.stream_builder = None;
        self.stream_last_paint = std::time::Instant::now();
        self.preload_dispatched = std::collections::HashSet::new();
        self.ime_composing = None;
        self.bfcache = BfCache::new(16);
        self.nav_back = Vec::new();
        self.nav_fwd = Vec::new();
        self.form_state = HashMap::new();
        self.validation_tooltip = None;
        self.color_picker_node = None;
        self.ls_storage = HashMap::new();
        // idb_dir is session-level — intentionally not reset here.
        self.sw_backend = Arc::new(std::sync::Mutex::new(
            lumen_storage::store::InMemoryStorage::new(),
        ));
        self.js_ctx = None;
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        self.animated_gifs = HashMap::new();
        self.gif_last_frame = HashMap::new();
        self.image_cache = lumen_image::ImageDecodeCache::new();
        self.zoom_factor = zoom::ZOOM_DEFAULT;
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        self.reader_original_source = None;
        // Cancel in-flight scroll animations.
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.scroll_drag = None;
    }

    /// Open a new blank tab.
    fn open_new_tab(&mut self) {
        // In tree-style tab mode, new tabs become children of the active tab,
        // building the parent-child tree automatically.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let new_idx = if self.tree_tabs.visible {
            let opener_id = self.tab_strip.tabs[self.tab_strip.active].id;
            self.tab_strip.push_with_opener(opener_id, now_ms)
        } else {
            self.tab_strip.push_blank(now_ms)
        };
        let new_id = self.tab_strip.tabs[new_idx].id;
        // Save current page into bg_tabs under the old active tab's id.
        let old_active = self.tab_strip.active;
        let old_id = self.tab_strip.tabs[old_active].id;
        // Mark old tab as recently backgrounded so it gets a badge if it ages to T2.
        self.tab_strip.set_tab_state(old_active, TabState::BackgroundRecent);
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        self.tab_strip.active = new_idx;
        self.reset_to_blank_tab();
        // Register the new tab with the lifecycle manager.
        self.lifecycle_mgr.open_tab(new_id as u64);
        self.request_redraw();
    }

    /// Open or toggle split view (Ctrl+\).
    ///
    /// Picks the next tab after the active one for the right pane. If no other
    /// tab exists, does nothing (split requires at least two tabs).
    fn toggle_split_view(&mut self) {
        let tab_count = self.tab_strip.len();
        if tab_count < 2 {
            return;
        }
        let next_idx = (self.tab_strip.active + 1) % tab_count;
        let next_id = self.tab_strip.tabs[next_idx].id;

        let (dl, scroll_y, scroll_x, content_height, content_width) =
            if let Some(snap) = self.bg_tabs.get(&next_id) {
                (
                    snap.display_list.clone(),
                    snap.scroll_y,
                    snap.scroll_x,
                    snap.content_height,
                    snap.content_width,
                )
            } else if let Some(meta) = self.hibernated_tabs.get(&next_id) {
                // Hibernated tab: show a minimal placeholder with its title/url.
                let placeholder_dl = build_split_placeholder(&meta.url);
                (placeholder_dl, 0.0, 0.0, 0.0, 0.0)
            } else {
                // Blank/new tab — show empty pane.
                (vec![], 0.0, 0.0, 0.0, 0.0)
            };

        self.split_view = Some(panels::split_view::SplitView::new(
            next_id,
            dl,
            scroll_y,
            scroll_x,
            content_height,
            content_width,
        ));
    }

    /// Close the tab at `idx`. If it was the last tab, exits the app instead.
    fn close_tab(&mut self, idx: usize, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.tab_strip.len() == 1 {
            // Last tab — exit.
            event_loop.exit();
            return;
        }
        let closing_id = self.tab_strip.tabs[idx].id;
        // Remove from lifecycle manager.
        self.lifecycle_mgr.close_tab(closing_id as u64);
        if idx == self.tab_strip.active {
            // Closing the active tab: save nothing (it will be dropped),
            // restore the tab that will become active after removal.
            let new_active = self.tab_strip.remove(idx);
            let new_id = self.tab_strip.tabs[new_active].id;
            // Mark the newly-activated tab as Active so its badge clears.
            self.tab_strip.set_tab_state(new_active, TabState::Active);
            // Drop the current active page.
            self.reset_to_blank_tab();
            if let Some(snap) = self.bg_tabs.remove(&new_id) {
                self.restore_page_snapshot(snap);
            } else if self.hibernated_tabs.contains_key(&new_id) {
                // Target tab is hibernated — restore from SQLite.
                self.restore_hibernated_tab(new_id);
            }
        } else {
            // Closing a background tab: drop snapshot and any hibernated data.
            self.bg_tabs.remove(&closing_id);
            self.hibernated_tabs.remove(&closing_id);
            let _ = self.tab_snapshots.delete(closing_id as i64);
            self.tab_strip.remove(idx);
        }
        self.request_redraw();
    }

    /// Assign `kind` to tab at `idx` for task 7D.2.
    ///
    /// Pre-registers a cookie/storage store id for the active page's origin
    /// if one is known, so subsequent requests can be partitioned. UI
    /// border-top strip refreshes on the next redraw via `build_tab_bar`.
    fn set_tab_container(&mut self, idx: usize, kind: tabs::containers::ContainerKind) {
        if idx >= self.tab_strip.len() {
            return;
        }
        self.tab_strip.set_tab_container(idx, kind);
        // Pre-warm a store id for the active tab's origin so cookie/storage
        // dispatch can partition by container id without a later allocation
        // step. Best-effort only — non-active tabs are wired up the same way
        // the next time their page loads.
        if idx == self.tab_strip.active
            && let Some(url) = self.source.url_str()
            && let Some(origin) = origin_of_url(url)
        {
            self.container_store.get_or_create(&origin, kind);
        }
        self.request_redraw();
    }

    /// Switch to tab at `idx`. No-op if already active.
    ///
    /// Handles all three cases:
    /// - T1/T2 tab: restore full `PageSnapshot` from `bg_tabs` (in-memory, fast).
    /// - T3 Hibernated tab: restore from SQLite via `Document::from_bytes()`.
    /// - Blank new tab: reset to empty state.
    fn switch_tab(&mut self, idx: usize) {
        if idx == self.tab_strip.active || idx >= self.tab_strip.len() {
            return;
        }
        // Save current active tab, marking it BackgroundRecent in the strip.
        let old_active = self.tab_strip.active;
        let old_id = self.tab_strip.tabs[old_active].id;
        self.tab_strip.set_tab_state(old_active, TabState::BackgroundRecent);
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);

        // Sync lifecycle manager: deactivate old, activate new.
        let new_id = self.tab_strip.tabs[idx].id;
        self.lifecycle_mgr.activate_tab(new_id as u64);

        // Restore new active tab, marking it Active so any badge clears.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        self.tab_strip.active = idx;
        self.tab_strip.set_tab_state(idx, TabState::Active);
        self.tab_strip.update_last_activated(idx, now_ms);

        self.reset_to_blank_tab();

        if let Some(snap) = self.bg_tabs.remove(&new_id) {
            // T1/T2: fast in-memory restore.
            self.restore_page_snapshot(snap);
        } else if self.hibernated_tabs.contains_key(&new_id) {
            // T3: restore from SQLite — Document::from_bytes() + relayout.
            self.restore_hibernated_tab(new_id);
        }
        // Otherwise the tab is blank (never loaded) — leave reset state.

        self.request_redraw();
    }
}

/// Достать чистый `ModifiersState` из обёртки `Modifiers` (winit 0.30 различает
/// "physical state" — Ctrl как клавиша — и "lock state"; для shortcuts нам
/// нужно физическое состояние).
fn winit_modifiers_state(mods: &Modifiers) -> ModifiersState {
    mods.state()
}

/// URL-строка из `PageSource` для записи в сессию, или `None` для `Empty`
/// (нечего восстанавливать). `File` → путь, `Snapshot` → `base_url`.
fn source_url_string(src: &PageSource) -> Option<String> {
    match src {
        PageSource::Empty | PageSource::AboutBlank => None,
        PageSource::File(p) => Some(p.display().to_string()),
        PageSource::Url(u) => Some(u.clone()),
        PageSource::Snapshot { base_url, .. } => Some(base_url.clone()),
    }
}

/// Bincode-сериализованный `Document` (`Document::to_bytes()`) для вкладки, или
/// пустой вектор, если страница не загружена либо сериализация не удалась.
/// Пустой blob на восстановлении означает fresh-navigate по URL.
fn dom_blob_of(layout_source: Option<&LayoutSource>) -> Vec<u8> {
    layout_source
        .and_then(|ls| ls.document.lock().ok())
        .and_then(|doc| doc.to_bytes().ok())
        .unwrap_or_default()
}

/// Извлечь origin (`scheme://host[:port]`) из URL-строки (7D.2). Для file://
/// или невалидных URL возвращает `None`. Используется как ключ
/// `ContainerStore` для cookie/storage партиционирования.
fn origin_of_url(url: &str) -> Option<String> {
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    lumen_network::Origin::from_url(&parsed).ok().map(|o| o.to_string())
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
            | DisplayCommand::PushMaskConicGradient { rect, .. }
            | DisplayCommand::PushMaskLayer { rect, .. } => rect,
            DisplayCommand::DrawCrossFade { dest, .. } => dest,
            DisplayCommand::PopClip
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::PushScrollLayer { .. }
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BoxModelOverlay { .. } => continue,
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
            | DisplayCommand::PushMaskConicGradient { rect, .. }
            | DisplayCommand::PushMaskLayer { rect, .. } => rect,
            DisplayCommand::DrawCrossFade { dest, .. } => dest,
            DisplayCommand::PopClip
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PopTransform
            | DisplayCommand::PopMask
            | DisplayCommand::PopMaskLayer
            | DisplayCommand::DrawLayerSnapshot { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PopFilter
            | DisplayCommand::PushBackdropFilter { .. }
            | DisplayCommand::PopBackdropFilter
            | DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::PushScrollLayer { .. }
            | DisplayCommand::PopScrollLayer
            | DisplayCommand::DrawSvgPath { .. }
            | DisplayCommand::DrawScrollbar { .. }
            | DisplayCommand::PageBreak
            | DisplayCommand::BoxModelOverlay { .. } => continue,
        };
        let right = r.x + r.width;
        if right > max_x {
            max_x = right;
        }
    }
    max_x
}

/// Build a minimal placeholder display list for a hibernated tab in split view.
///
/// Shows a dark grey background with the URL text — used when the hibernated
/// tab's full display list has been evicted from memory.
fn build_split_placeholder(url: &str) -> lumen_paint::DisplayList {
    use lumen_layout::{Color, FontStyle, FontWeight};
    use lumen_paint::DisplayCommand;

    let bg = Color { r: 30, g: 30, b: 35, a: 255 };
    let fg = Color { r: 180, g: 180, b: 190, a: 255 };
    vec![
        // Background fill — large enough to cover any viewport half.
        DisplayCommand::FillRect {
            rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 4096.0, height: 4096.0 },
            color: bg,
        },
        // URL label near vertical centre of a typical viewport half.
        DisplayCommand::DrawText {
            rect: lumen_core::geom::Rect { x: 16.0, y: 300.0, width: 480.0, height: 20.0 },
            text: url.to_owned(),
            font_size: 13.0,
            color: fg,
            font_family: vec![],
            font_weight: FontWeight(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            tab_size: 0.0,
        },
    ]
}

/// Escape a single character for safe embedding in a JS string literal.
///
/// Converts `ch` to an ASCII or `\uXXXX` escape so the character can be
/// used in `"..."` JS string arguments passed via `eval_js`.
fn escape_js_string_char(ch: char) -> String {
    match ch {
        '"' => r#"\""#.to_owned(),
        '\\' => r"\\".to_owned(),
        '\n' => r"\n".to_owned(),
        '\r' => r"\r".to_owned(),
        '\t' => r"\t".to_owned(),
        c if (c as u32) < 0x20 || (c as u32) > 0x7E => {
            format!("\\u{:04X}", c as u32)
        }
        c => c.to_string(),
    }
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
    fn keybinding_ctrl_shift_r_is_read_later() {
        // Ctrl+Shift+R → toggle Read-later panel (§12.3).
        assert_eq!(
            keybinding_for(KeyCode::KeyR, ModifiersState::CONTROL | ModifiersState::SHIFT),
            Some(KeyCommand::ToggleReadLater),
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
    fn keybinding_ctrl_w_close_tab() {
        assert_eq!(
            keybinding_for(KeyCode::KeyW, ModifiersState::CONTROL),
            Some(KeyCommand::CloseTab),
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
    fn page_source_about_blank_from_arg() {
        assert!(matches!(
            PageSource::from_arg(Some("about:blank")),
            PageSource::AboutBlank
        ));
    }

    #[test]
    fn page_source_about_blank_url_str() {
        assert_eq!(PageSource::AboutBlank.url_str(), Some("about:blank"));
    }

    #[test]
    fn page_source_about_blank_describe() {
        assert_eq!(PageSource::AboutBlank.describe(), "about:blank");
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

    // ── extract_print_to_pdf ─────────────────────────────────────────────────

    #[test]
    fn extract_print_to_pdf_basic() {
        let (output, rest) = extract_print_to_pdf(&args(&["--print-to-pdf", "out.pdf", "page.html"]));
        assert_eq!(output.as_deref(), Some(std::path::Path::new("out.pdf")));
        assert_eq!(rest, args(&["page.html"]));
    }

    #[test]
    fn extract_print_to_pdf_no_flag() {
        let (output, rest) = extract_print_to_pdf(&args(&["page.html"]));
        assert!(output.is_none());
        assert_eq!(rest, args(&["page.html"]));
    }

    #[test]
    fn extract_print_to_pdf_with_url_source() {
        let (output, rest) = extract_print_to_pdf(&args(&["--print-to-pdf", "result.pdf", "https://example.com"]));
        assert_eq!(output.as_deref(), Some(std::path::Path::new("result.pdf")));
        assert_eq!(rest, args(&["https://example.com"]));
    }

    #[test]
    fn extract_print_to_pdf_combined_with_other_flags() {
        // --print-to-pdf coexists with other pre-extracted flags.
        let (output, rest) = extract_print_to_pdf(&args(&["--print-to-pdf", "a.pdf", "b.html"]));
        assert!(output.is_some());
        assert_eq!(rest, args(&["b.html"]));
    }

    #[test]
    fn encode_images_as_pdf_empty() {
        let pdf = encode_images_as_pdf(&[], 100, 100);
        // Non-empty: at minimum the %PDF header.
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn encode_images_as_pdf_single_page() {
        let img = lumen_image::Image {
            width: 2,
            height: 2,
            format: lumen_image::PixelFormat::Rgba8,
            data: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255],
            icc_profile: None,
        };
        let pdf = encode_images_as_pdf(&[img], 2, 2);
        assert!(pdf.starts_with(b"%PDF-"));
        // PDF objects contain binary + ASCII text — search raw bytes for key strings.
        let contains = |needle: &[u8]| pdf.windows(needle.len()).any(|w| w == needle);
        assert!(contains(b"/Page") || contains(b"/MediaBox"),
            "expected /Page or /MediaBox in PDF output (len={})", pdf.len());
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
        let mut mods = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].contains("console.log"));
        assert!(mods.is_empty());
    }

    #[test]
    fn collect_inline_scripts_skips_empty() {
        let doc = lumen_html_parser::parse(
            r#"<html><head></head><body><script>   </script></body></html>"#,
        );
        let mut scripts = Vec::new();
        let mut mods = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
        assert!(scripts.is_empty());
        assert!(mods.is_empty());
    }

    #[test]
    fn collect_inline_scripts_multiple() {
        let doc = lumen_html_parser::parse(
            r#"<html><body><script>a=1;</script><script>b=2;</script></body></html>"#,
        );
        let mut scripts = Vec::new();
        let mut mods = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
        assert_eq!(scripts.len(), 2);
        assert!(mods.is_empty());
    }

    #[test]
    fn collect_inline_scripts_separates_modules() {
        let doc = lumen_html_parser::parse(
            r#"<html><body>
              <script>var x = 1;</script>
              <script type="module">export const y = 2;</script>
            </body></html>"#,
        );
        let mut scripts = Vec::new();
        let mut mods = Vec::new();
        collect_inline_scripts(&doc, doc.root(), &mut scripts, &mut mods);
        assert_eq!(scripts.len(), 1, "classic script counted");
        assert_eq!(mods.len(), 1, "module script counted");
        assert!(scripts[0].contains("var x"));
        assert!(mods[0].contains("export const y"));
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
