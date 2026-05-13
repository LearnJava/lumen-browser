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

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use lumen_core::event::Event;
use lumen_core::ext::EventSink;
use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId};
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
use winit::event::{ElementState, KeyEvent, Modifiers, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

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

    let initial_page = match source.load(event_sink.clone()) {
        Ok(page) => page,
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
    let mut app = Lumen {
        display_list: initial_page.display_list,
        title: initial_page.title,
        source,
        event_sink,
        modifiers: ModifiersState::empty(),
        window: None,
        renderer: None,
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
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink)?;
            print!("{}", lumen_layout::serialize_layout_tree(&parsed.layout));
            Ok(())
        }
        DumpKind::DisplayList => {
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink)?;
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

    fn load(&self, sink: Arc<dyn EventSink>) -> Result<LoadedPage, Box<dyn Error>> {
        if matches!(self, PageSource::Empty) {
            return Ok(LoadedPage::empty());
        }
        let raw = self.load_bytes(sink.clone())?;
        render_bytes(&raw.bytes, raw.content_type, &raw.base, sink)
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
}

impl LoadedPage {
    fn empty() -> Self {
        Self { display_list: DisplayList::new(), title: None }
    }
}

/// Действия shell-а, на которые мапятся клавиши. Изолированы от winit, чтобы
/// маппер был тестируем без event loop.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum KeyCommand {
    Reload,
    Exit,
}

/// Маппинг физической клавиши + модификаторов на shell-action.
///
/// F5 без модификаторов  → Reload.
/// Ctrl+R                → Reload.
/// Esc без модификаторов → Exit.
/// Ctrl+W                → Exit.
///
/// Прочие комбинации (Ctrl+Shift+R, F5+Ctrl, и т.д.) — пока None: не хотим
/// перехватывать привычные web-shortcuts (force-reload, etc.) до того, как
/// решим, что они должны делать.
fn keybinding_for(code: KeyCode, mods: ModifiersState) -> Option<KeyCommand> {
    let ctrl_only = mods == ModifiersState::CONTROL;
    let no_mods = mods.is_empty();
    match code {
        KeyCode::F5 if no_mods => Some(KeyCommand::Reload),
        KeyCode::KeyR if ctrl_only => Some(KeyCommand::Reload),
        KeyCode::Escape if no_mods => Some(KeyCommand::Exit),
        KeyCode::KeyW if ctrl_only => Some(KeyCommand::Exit),
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
                ResolvedResource::Url(resolve_url(base_url, href))
            }
        }
    }
}

enum ResolvedResource {
    File(PathBuf),
    Url(String),
}

/// Разрешить относительный `href` относительно `base_url`.
///
/// "/style.css" -> "https://host/style.css"
/// "css/a.css"  -> "https://host/path/css/a.css"
fn resolve_url(base_url: &str, href: &str) -> String {
    let (scheme, rest) = if let Some(r) = base_url.strip_prefix("https://") {
        ("https://", r)
    } else if let Some(r) = base_url.strip_prefix("http://") {
        ("http://", r)
    } else {
        return href.to_owned();
    };
    let authority = rest.find('/').map(|i| &rest[..i]).unwrap_or(rest);
    if href.starts_with('/') {
        format!("{scheme}{authority}{href}")
    } else {
        let path = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
        let dir = path.rfind('/').map(|i| &path[..=i]).unwrap_or("/");
        format!("{scheme}{authority}{dir}{href}")
    }
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

// ── Рендер ───────────────────────────────────────────────────────────────────

/// Результат фаз `decode → parse → layout` — общая часть для оконного и
/// dump-режимов. Поля владеют своими данными — нет ссылок наружу.
struct ParsedPage {
    document: Document,
    layout: LayoutBox,
    title: Option<String>,
    rule_count: usize,
}

fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
) -> Result<ParsedPage, Box<dyn Error>> {
    // Кодировку определяем по BOM -> <meta charset> -> эвристике. Это покрывает
    // и UTF-8 (большинство), и старые cp1251 / koi8-r / cp866 файлы.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    eprintln!("Кодировка: {}", encoding.name());

    let doc = lumen_html_parser::parse(&source);
    let title = extract_title(&doc);

    // Встроенные <style> + внешние <link rel=stylesheet>.
    let mut css = extract_style_blocks(&doc);
    css.push_str(&load_linked_stylesheets(&doc, base, sink));

    let sheet = lumen_css_parser::parse(&css);
    let viewport = Size::new(1024.0, 720.0);

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("ошибка разбора шрифта: {e}"))?;
    let measurer = lumen_paint::FontMeasurer::new(&font)
        .map_err(|e| format!("ошибка метрик шрифта: {e}"))?;

    let layout = lumen_layout::layout_measured(&doc, &sheet, viewport, &measurer);
    let rule_count = sheet.rules.len();
    Ok(ParsedPage {
        document: doc,
        layout,
        title,
        rule_count,
    })
}

fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
) -> Result<LoadedPage, Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink)?;
    let display_list = lumen_paint::build_display_list(&parsed.layout);
    println!(
        "Распарсено: {} DOM-узлов, {} CSS-правил, {} paint-команд",
        parsed.document.len(),
        parsed.rule_count,
        display_list.len()
    );
    Ok(LoadedPage {
        display_list,
        title: parsed.title,
    })
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
    source: PageSource,
    event_sink: Arc<dyn EventSink>,
    modifiers: ModifiersState,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl Lumen {
    /// Перезагрузить текущий источник: fetch/parse/layout/paint снова. На
    /// `PageSource::Empty` — no-op (грузить нечего). При ошибке — оставляем
    /// предыдущий display_list, печатаем причину в stderr.
    fn reload(&mut self) {
        if matches!(self.source, PageSource::Empty) {
            return;
        }
        println!("Reload: {}", self.source.describe());
        match self.source.load(self.event_sink.clone()) {
            Ok(page) => {
                self.display_list = page.display_list;
                self.title = page.title;
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
            .with_inner_size(LogicalSize::new(1024.0, 720.0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Не удалось создать окно: {err}");
                event_loop.exit();
                return;
            }
        };

        let renderer = match Renderer::new(window.clone(), INTER_FONT.to_vec()) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Не удалось инициализировать рендер: {err}");
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.renderer = Some(renderer);
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
            WindowEvent::RedrawRequested => {
                if let Some(r) = self.renderer.as_mut()
                    && let Err(err) = r.render(&self.display_list)
                {
                    eprintln!("Ошибка рендера: {err:?}");
                }
            }
            _ => {}
        }
    }
}

impl Lumen {
    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed || key_event.repeat {
            return;
        }
        let PhysicalKey::Code(code) = key_event.physical_key else {
            return;
        };
        let Some(cmd) = keybinding_for(code, self.modifiers) else {
            return;
        };
        match cmd {
            KeyCommand::Reload => self.reload(),
            KeyCommand::Exit => event_loop.exit(),
        }
    }
}

/// Достать чистый `ModifiersState` из обёртки `Modifiers` (winit 0.30 различает
/// "physical state" — Ctrl как клавиша — и "lock state"; для shortcuts нам
/// нужно физическое состояние).
fn winit_modifiers_state(mods: &Modifiers) -> ModifiersState {
    mods.state()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_url_absolute_path() {
        let r = resolve_url("https://example.com/path/page.html", "/style.css");
        assert_eq!(r, "https://example.com/style.css");
    }

    #[test]
    fn resolve_url_relative_same_dir() {
        let r = resolve_url("https://example.com/path/page.html", "style.css");
        assert_eq!(r, "https://example.com/path/style.css");
    }

    #[test]
    fn resolve_url_relative_subdirectory() {
        let r = resolve_url("https://example.com/path/page.html", "css/main.css");
        assert_eq!(r, "https://example.com/path/css/main.css");
    }

    #[test]
    fn resolve_url_root_base() {
        let r = resolve_url("https://example.com/", "style.css");
        assert_eq!(r, "https://example.com/style.css");
    }

    #[test]
    fn resolve_url_http_scheme() {
        let r = resolve_url("http://localhost:8080/index.html", "/css/app.css");
        assert_eq!(r, "http://localhost:8080/css/app.css");
    }

    #[test]
    fn resource_base_url_absolute_href_passthrough() {
        // Абсолютный href перехватывается в ResourceBase::resolve до вызова resolve_url.
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
}
