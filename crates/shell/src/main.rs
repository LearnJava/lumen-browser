//! Lumen shell — точка входа браузера.
//!
//! Режимы запуска:
//! - `lumen` — открыть пустое окно.
//! - `lumen <path.html>` — распарсить файл, layout, paint, нарисовать в окне.
//! - `lumen <http(s)://...>` — загрузить страницу по сети, layout, paint.
//! - `lumen --dump-source <path-or-url>` — печать декодированного HTML в stdout.
//! - `lumen --dump-layout <path-or-url>` — печать layout-дерева в stdout.
//! - `lumen --dump-display-list <path-or-url>` — печать display list в stdout.
//!
//! Dump-режимы не создают окна и не инициализируют wgpu — pipeline прогоняется
//! до нужной фазы, результат сериализуется и пишется в stdout. Полезно для CI
//! (без GPU), отладки сложных страниц и сравнения вывода между версиями.
//!
//! Внешние CSS: `<link rel="stylesheet" href="...">` загружается с диска или
//! по сети — в зависимости от того, каким способом загружена страница.

mod find;
mod momentum_anim;
mod runtime;
mod scroll_anim;
mod scrollbar;

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use lumen_core::event::Event;
use lumen_core::ext::EventSink;
use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId, check_form_gate, check_navigation_gate};
use lumen_layout::LayoutBox;
use lumen_paint::{DisplayList, Renderer};
use winit::application::ApplicationHandler;

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
            // Прочие события (TabCreated, Navigation, …) в Phase 0 не emit-ятся,
            // print не нужен.
            _ => {}
        }
    }
}

/// Bundled-шрифт: статический Inter v4.1 Regular (~411 КБ),
/// SIL OFL 1.1, см. assets/fonts/OFL.txt.
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{CursorIcon, Window, WindowId};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_cli(&args) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    let event_sink: Arc<dyn EventSink> = Arc::new(StdoutEventSink);

    match cli {
        CliMode::Dump { source, kind } => run_dump_mode(&source, kind, event_sink),
        CliMode::OpenWindow(source) => run_window_mode(source, event_sink),
    }
}

fn run_window_mode(source: PageSource, event_sink: Arc<dyn EventSink>) -> ExitCode {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    let initial_viewport = Size::new(1024.0, 720.0);
    let (initial_page, layout_source) = match source.load(event_sink.clone(), initial_viewport) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка загрузки {}: {err}", source.describe());
            return ExitCode::FAILURE;
        }
    };

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(err) => {
            eprintln!("Не удалось создать event loop: {err}");
            return ExitCode::FAILURE;
        }
    };
    let content_height = content_height_of(&initial_page.display_list);
    let content_width = content_width_of(&initial_page.display_list);
    let mut app = Lumen {
        display_list: initial_page.display_list,
        title: initial_page.title,
        pending_images: initial_page.images,
        source,
        event_sink,
        modifiers: ModifiersState::empty(),
        window: None,
        renderer: None,
        runtime: runtime::EventLoop::new(),
        epoch: std::time::Instant::now(),
        find: find::FindState::default(),
        scroll_y: 0.0,
        scroll_x: 0.0,
        content_height,
        content_width,
        cursor_position: None,
        scroll_drag: None,
        scroll_anim: None,
        momentum_anim: None,
        touchpad_vel: (0.0, 0.0),
        touchpad_vel_time_ms: 0.0,
        last_cursor_icon: None,
        layout_source,
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
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp)?;
            print!("{}", lumen_layout::serialize_layout_tree(&parsed.layout));
            Ok(())
        }
        DumpKind::DisplayList => {
            let vp = Size::new(1024.0, 720.0);
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp)?;
            let dl = lumen_paint::build_display_list(&parsed.layout);
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
}

/// Источник страницы. Запоминается в `Lumen`, чтобы reload (F5/Ctrl+R) мог
/// заново выполнить fetch/parse/layout/paint без аргументов командной строки.
#[derive(Debug, Clone)]
enum PageSource {
    /// Без аргументов — рисуем пустое окно. Reload no-op (грузить нечего).
    Empty,
    File(PathBuf),
    Url(String),
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
        }
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
                use lumen_network::HttpClient;

                let lumen_url = Url::parse(url)?;
                let client = HttpClient::new().with_sink(sink);
                let bytes = client.fetch(&lumen_url)?;
                eprintln!("Получено {} байт", bytes.len());
                Ok(RawPage {
                    bytes,
                    base: ResourceBase::Url(url.clone()),
                    content_type: Some("text/html"),
                })
            }
        }
    }

    fn load(
        &self,
        sink: Arc<dyn EventSink>,
        viewport: Size,
    ) -> Result<(LoadedPage, Option<LayoutSource>), Box<dyn Error>> {
        if matches!(self, PageSource::Empty) {
            return Ok((LoadedPage::empty(), None));
        }
        let raw = self.load_bytes(sink.clone())?;
        let (page, layout_source) =
            render_bytes(&raw.bytes, raw.content_type, &raw.base, sink, viewport)?;
        Ok((page, Some(layout_source)))
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
}

impl LoadedPage {
    fn empty() -> Self {
        Self {
            display_list: DisplayList::new(),
            title: None,
            images: Vec::new(),
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
    /// Скролл на одну строку вниз (стрелка вниз).
    ScrollLineDown,
    /// Скролл на одну строку вверх (стрелка вверх).
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
}

/// Маппинг физической клавиши + модификаторов на shell-action.
///
/// F5 без модификаторов  → Reload.
/// Ctrl+R                → Reload.
/// Esc без модификаторов → Exit.
/// Ctrl+W                → Exit.
/// Ctrl+F                → FindOpen.
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
    let no_mods = mods.is_empty();
    match code {
        KeyCode::F5 if no_mods => Some(KeyCommand::Reload),
        KeyCode::KeyR if ctrl_only => Some(KeyCommand::Reload),
        KeyCode::Escape if no_mods => Some(KeyCommand::Exit),
        KeyCode::KeyW if ctrl_only => Some(KeyCommand::Exit),
        KeyCode::KeyF if ctrl_only => Some(KeyCommand::FindOpen),
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
                use lumen_core::ext::NetworkTransport;
                use lumen_core::url::Url;
                use lumen_network::HttpClient;

                let client = HttpClient::new().with_sink(sink.clone());
                match Url::parse(&url).and_then(|u| client.fetch(&u)) {
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

/// Обходит DOM, для каждого `<img>` с непустым `src` скачивает байты, декодирует
/// через `lumen_image::decode` (PNG/JPEG dispatch по сигнатуре) и собирает
/// результаты в `Vec<(raw_src, Image)>`.
///
/// Побочный эффект: для тех `<img>`, у которых **не задан ни width, ни height**
/// атрибут, проставляет оба из декодированного изображения. Это HTML5 §10
/// «mapped attributes» — author CSS затем перекроет, если захочет. Если хоть
/// один из атрибутов уже задан author-ом, не лезем (предполагаем, что author
/// знает, что делает — например, форсирует aspect-ratio).
///
/// Ключ в результирующем Vec — raw href из `src` attribute, не resolved URL.
/// Это совпадает с тем, что хранит `BoxKind::Image { src, alt }` после layout,
/// поэтому `Renderer::register_image` будет видеть тот же ключ, который придёт
/// в `DisplayCommand::DrawImage.src` при рендеринге.
fn fetch_and_decode_images(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
) -> Vec<(String, lumen_image::Image)> {
    let mut entries: Vec<(NodeId, String, bool, bool)> = Vec::new();
    collect_img_entries(doc, doc.root(), &mut entries);

    let mut out: Vec<(String, lumen_image::Image)> = Vec::new();
    for (node_id, src, has_w, has_h) in entries {
        let bytes = match fetch_image_bytes(&src, base, sink) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Пропуск картинки {src}: {e}");
                continue;
            }
        };
        let image = match lumen_image::decode(&bytes) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Не декодируется {src}: {e}");
                continue;
            }
        };

        if !has_w && !has_h {
            apply_intrinsic_size(doc, node_id, image.width, image.height);
        }

        eprintln!(
            "Загружена картинка: {src} ({}×{}, {:?})",
            image.width, image.height, image.format
        );
        out.push((src, image));
    }
    out
}

fn collect_img_entries(
    doc: &Document,
    id: NodeId,
    out: &mut Vec<(NodeId, String, bool, bool)>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "img"
    {
        let src = attrs
            .iter()
            .find(|a| a.name.local.eq_ignore_ascii_case("src"))
            .map(|a| a.value.clone())
            .unwrap_or_default();
        if !src.is_empty() {
            let has_w = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
            let has_h = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
            out.push((id, src, has_w, has_h));
        }
        // <img> — void элемент, у него не бывает children, выходим.
        return;
    }
    for &child in &node.children {
        collect_img_entries(doc, child, out);
    }
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
            use lumen_core::ext::NetworkTransport;
            use lumen_core::url::Url;
            use lumen_network::HttpClient;

            let client = HttpClient::new().with_sink(sink.clone());
            let lumen_url = Url::parse(&url)?;
            Ok(client.fetch(&lumen_url)?)
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
    document: Document,
    stylesheet: lumen_css_parser::Stylesheet,
    layout: LayoutBox,
    title: Option<String>,
    rule_count: usize,
    /// Декодированные изображения, найденные при обходе DOM. См. [`LoadedPage::images`].
    images: Vec<(String, lumen_image::Image)>,
}

/// Источник для повторного layout без повторной загрузки/парсинга.
/// Хранится в `Lumen`; обновляется только при reload/load новой страницы.
struct LayoutSource {
    document: Document,
    stylesheet: lumen_css_parser::Stylesheet,
}

fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: Size,
) -> Result<ParsedPage, Box<dyn Error>> {
    // Кодировку определяем по BOM -> <meta charset> -> эвристике. Это покрывает
    // и UTF-8 (большинство), и старые cp1251 / koi8-r / cp866 файлы.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    eprintln!("Кодировка: {}", encoding.name());

    let mut doc = lumen_html_parser::parse(&source);
    let title = extract_title(&doc);

    // Гейт выполнения скриптов: Phase 0 — top-level документ не sandboxed
    // (SandboxFlags::empty()), поэтому блокировки не будет. NullJsRuntime
    // возвращает NotImplemented — это ожидаемое поведение до подключения QuickJS.
    run_scripts(&doc, lumen_core::SandboxFlags::empty(), &lumen_core::NullJsRuntime);

    // Гейт отправки форм: Phase 0 — top-level документ не sandboxed.
    check_form_gate(&doc, lumen_core::SandboxFlags::empty());

    // Гейт навигации: Phase 0 — top-level документ не sandboxed.
    check_navigation_gate(&doc, lumen_core::SandboxFlags::empty());

    // Fetch + decode <img src>. Должно идти ДО layout, потому что intrinsic
    // dimensions из декодированного изображения проставляются как HTML
    // presentational hints (width/height attribute) и потом подхватываются
    // style cascade. Errors silently пропускаются — битая картинка не валит
    // всю страницу, layout нарисует серый placeholder.
    let images = fetch_and_decode_images(&mut doc, base, sink);

    // Встроенные <style> + внешние <link rel=stylesheet>.
    let mut css = extract_style_blocks(&doc);
    css.push_str(&load_linked_stylesheets(&doc, base, sink));

    let sheet = lumen_css_parser::parse(&css);

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("ошибка разбора шрифта: {e}"))?;
    let measurer = lumen_paint::FontMeasurer::new(&font)
        .map_err(|e| format!("ошибка метрик шрифта: {e}"))?;

    let layout = lumen_layout::layout_measured(&doc, &sheet, viewport, &measurer);
    let rule_count = sheet.rules.len();
    Ok(ParsedPage {
        document: doc,
        stylesheet: sheet,
        layout,
        title,
        rule_count,
        images,
    })
}

/// Повторный layout+paint по сохранённому `LayoutSource` с новым viewport.
/// Парсинг HTML/CSS не выполняется — только layout и build_display_list.
fn relayout_page(src: &LayoutSource, viewport: Size) -> DisplayList {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let layout = lumen_layout::layout_measured(&src.document, &src.stylesheet, viewport, &measurer);
    lumen_paint::build_display_list(&layout)
}

fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
    viewport: Size,
) -> Result<(LoadedPage, LayoutSource), Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink, viewport)?;
    let display_list = lumen_paint::build_display_list(&parsed.layout);
    println!(
        "Распарсено: {} DOM-узлов, {} CSS-правил, {} paint-команд, {} картинок",
        parsed.document.len(),
        parsed.rule_count,
        display_list.len(),
        parsed.images.len(),
    );
    let layout_source = LayoutSource {
        document: parsed.document,
        stylesheet: parsed.stylesheet,
    };
    Ok((
        LoadedPage {
            display_list,
            title: parsed.title,
            images: parsed.images,
        },
        layout_source,
    ))
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

/// Выполнить inline `<script>` блоки если sandbox позволяет, иначе заблокировать.
///
/// `SandboxFlags::SCRIPTS` установлен — скрипты запрещены; функция логирует
/// количество заблокированных и возвращает 0. Иначе каждый скрипт передаётся
/// в `runtime.eval()`; в Phase 0 это всегда `NullJsRuntime` → `NotImplemented`.
/// Возвращает число скриптов, переданных в runtime.
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
    /// Эпоха для rAF-timestamp-ов в миллисекундах от старта shell-а
    /// (DOMHighResTimeStamp — HTML §8.1.5.1: «timestamp passed to callback
    /// should be the current high resolution time»).
    epoch: std::time::Instant,
    /// Состояние Ctrl+F. Открыт ли bar, текущий query и индекс активного
    /// совпадения. Содержимое поиска не сохраняется между reload-ами
    /// (close() полностью очищает state); это сознательно: после reload
    /// display list другой, и старые позиции совпадений уже невалидны.
    find: find::FindState,
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
}

impl Lumen {
    /// Повторный layout+paint при изменении размера viewport.
    /// Использует сохранённый `LayoutSource`; парсинг не повторяется.
    fn relayout(&mut self) {
        let Some(src) = self.layout_source.as_ref() else { return };
        let Some(r) = self.renderer.as_ref() else { return };
        let vp_size = r.viewport_size();
        let viewport = Size::new(vp_size.width as f32, vp_size.height as f32);
        let new_dl = relayout_page(src, viewport);
        self.content_height = content_height_of(&new_dl);
        self.content_width = content_width_of(&new_dl);
        self.display_list = new_dl;
        self.scroll_y = clamp_scroll(self.scroll_y, self.max_scroll());
        self.scroll_x = clamp_scroll(self.scroll_x, self.max_scroll_x());
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
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
        match self.source.load(self.event_sink.clone(), viewport) {
            Ok((page, new_layout_source)) => {
                self.layout_source = new_layout_source;
                self.content_height = content_height_of(&page.display_list);
                self.content_width = content_width_of(&page.display_list);
                self.display_list = page.display_list;
                self.title = page.title;
                // Display list другой → старые match-rect-ы невалидны.
                // Closing полностью сбрасывает query/active — пользователю
                // нужно открыть find заново после reload, что естественно.
                self.find.close();
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
            }
            Err(err) => {
                eprintln!("Ошибка reload {}: {err}", self.source.describe());
            }
        }
    }
}

impl ApplicationHandler for Lumen {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(window_title(self.title.as_deref()))
            .with_inner_size(LogicalSize::new(1024.0, 720.0))
            .with_maximized(true);

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
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
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
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
                self.relayout();
                // HTML §8.1.5.1, шаг 13: ResizeObserver delivery. В Phase 0
                // никто не зарегистрирован (JS engine отсутствует), но
                // future-proof: когда подключим QuickJS, JS-callback-и
                // получат сигнал автоматически.
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
                        scrollbar::TrackClick::None => {}
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
                // HTML §8.1.5.1 «Update the rendering»: перед собственно
                // отрисовкой выполняем rAF-callback-и с текущим timestamp-ом.
                // Callback-и (когда подключим JS) могут поменять DOM/style;
                // здесь они лишь Rust closure, но точка диспатча уже стоит.
                let timestamp_ms =
                    self.epoch.elapsed().as_secs_f64() * 1000.0;
                self.runtime.run_rendering_step(timestamp_ms);

                // Тик smooth-scroll-анимации. Делаем ДО построения display-list-а.
                if self.advance_scroll_anim() {
                    self.request_redraw();
                }

                // Тик momentum scroll. Запускается после TouchPhase::Ended тачпада.
                // Конкурирует с scroll_anim: если пользователь начал keyboard/wheel
                // scroll во время momentum — scroll_anim перекрывает (momentum тикает
                // отдельно через scroll_x/scroll_y напрямую без smooth-anim).
                if self.advance_momentum(timestamp_ms) {
                    self.request_redraw();
                }

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

                let scroll_y = self.scroll_y;
                let scroll_x = self.scroll_x;
                if let Some(r) = self.renderer.as_mut() {
                    let page: &[lumen_paint::DisplayCommand] = page_buf
                        .as_deref()
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

        // Когда find bar открыт — все клавиши идут в него: ввод символов,
        // Esc=close, Backspace=стирание, Enter/F3=next (Shift=prev). Это не
        // даёт случайно сработать Esc=Exit или Ctrl+R=Reload в момент поиска.
        if self.find.is_open() {
            self.handle_find_key(code, key_event);
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
            KeyCommand::Reload => self.reload(),
            KeyCommand::Exit => event_loop.exit(),
            KeyCommand::FindOpen => {
                self.find.open();
                self.request_redraw();
            }
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
        let hover = scrollbar::classify_track_click(
            x_css,
            y_css,
            self.scroll_y,
            self.content_height,
            self.viewport_width_css(),
            self.viewport_height_css(),
        );
        let desired = cursor_icon_for_hover(hover, self.scroll_drag.is_some());
        if self.last_cursor_icon != Some(desired) {
            window.set_cursor(desired);
            self.last_cursor_icon = Some(desired);
        }
    }

    /// Пересчитывает текущий список совпадений по `display_list` и `find.query`.
    /// Возвращает пустой Vec, если bar закрыт или запрос пустой. Используется
    /// и для рендера, и для счётчика `next`/`prev`.
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
        find::find_matches(&self.display_list, self.find.query(), &measurer)
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
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. } => rect,
            DisplayCommand::PopClip
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::DrawLayerSnapshot { .. } => continue,
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
            | DisplayCommand::DrawBorder { rect, .. }
            | DisplayCommand::DrawText { rect, .. }
            | DisplayCommand::DrawImage { rect, .. }
            | DisplayCommand::DrawOutline { rect, .. }
            | DisplayCommand::PushClipRect { rect, .. } => rect,
            DisplayCommand::PopClip
            | DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PopOpacity
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PopBlendMode
            | DisplayCommand::DrawLayerSnapshot { .. } => continue,
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
    fn keybinding_plain_f_is_none() {
        // Без Ctrl — обычная буква, не команда. Иначе посимвольный ввод
        // не работал бы (например, в будущем omnibox).
        assert_eq!(keybinding_for(KeyCode::KeyF, ModifiersState::empty()), None);
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
