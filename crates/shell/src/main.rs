//! Lumen shell — точка входа браузера.
//!
//! Режимы запуска:
//! - `lumen` — открыть пустое окно.
//! - `lumen <path.html>` — распарсить файл, layout, paint.
//! - `lumen <http(s)://...>` — загрузить страницу по сети, layout, paint.
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
use lumen_paint::{DisplayList, Renderer};
use winit::application::ApplicationHandler;

/// EventSink, который печатает сетевые события в stdout — это и есть
/// «network log» Phase 0, реализующий принцип №4 «каждый исходящий байт
/// виден». Позже заменится на структурированный UI-логгер.
struct StdoutEventSink;

impl EventSink for StdoutEventSink {
    fn emit(&self, event: &Event) {
        match event {
            Event::RequestStarted { url, .. } => println!("→ GET {url}"),
            Event::RequestCompleted { url, status, .. } => println!("← {status} {url}"),
            Event::RequestBlocked { url, reason, .. } => println!("✗ {url} ({reason})"),
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
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn main() -> ExitCode {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    let event_sink: Arc<dyn EventSink> = Arc::new(StdoutEventSink);

    let arg = std::env::args().nth(1);
    let initial_page = match arg {
        Some(ref s) if s.starts_with("http://") || s.starts_with("https://") => {
            match load_url(s, event_sink.clone()) {
                Ok(page) => page,
                Err(err) => {
                    eprintln!("Ошибка загрузки {s}: {err}");
                    return ExitCode::FAILURE;
                }
            }
        }
        Some(ref s) => {
            let path = PathBuf::from(s);
            match load_page(&path, event_sink.clone()) {
                Ok(page) => page,
                Err(err) => {
                    eprintln!("Ошибка загрузки {}: {err}", path.display());
                    return ExitCode::FAILURE;
                }
            }
        }
        None => LoadedPage::empty(),
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
        window: None,
        renderer: None,
    };
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("Ошибка event loop: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn load_url(url: &str, sink: Arc<dyn EventSink>) -> Result<LoadedPage, Box<dyn Error>> {
    use lumen_core::ext::NetworkTransport;
    use lumen_core::url::Url;
    use lumen_network::HttpClient;

    let lumen_url = Url::parse(url)?;
    let client = HttpClient::new().with_sink(sink.clone());
    let bytes = client.fetch(&lumen_url)?;
    println!("Получено {} байт", bytes.len());
    render_bytes(&bytes, Some("text/html"), &ResourceBase::Url(url.to_owned()), sink)
}

fn load_page(path: &PathBuf, sink: Arc<dyn EventSink>) -> Result<LoadedPage, Box<dyn Error>> {
    let bytes = std::fs::read(path)?;
    render_bytes(&bytes, None, &ResourceBase::File(path.clone()), sink)
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
                    println!("Загружен CSS: {}", path.display());
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

fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
) -> Result<LoadedPage, Box<dyn Error>> {
    // Кодировку определяем по BOM -> <meta charset> -> эвристике. Это покрывает
    // и UTF-8 (большинство), и старые cp1251 / koi8-r / cp866 файлы.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    println!("Кодировка: {}", encoding.name());

    let doc = lumen_html_parser::parse(&source);
    let title = extract_title(&doc);

    // Встроенные <style> + внешние <link rel=stylesheet>.
    let mut css = extract_style_blocks(&doc);
    css.push_str(&load_linked_stylesheets(&doc, base, &sink));

    let sheet = lumen_css_parser::parse(&css);
    let viewport = Size::new(1024.0, 720.0);

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("ошибка разбора шрифта: {e}"))?;
    let measurer = lumen_paint::FontMeasurer::new(&font)
        .map_err(|e| format!("ошибка метрик шрифта: {e}"))?;

    let layout = lumen_layout::layout_measured(&doc, &sheet, viewport, &measurer);
    let display_list = lumen_paint::build_display_list(&layout);

    println!(
        "Распарсено: {} DOM-узлов, {} CSS-правил, {} paint-команд",
        doc.len(),
        sheet.rules.len(),
        display_list.len()
    );
    Ok(LoadedPage { display_list, title })
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
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
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
}
