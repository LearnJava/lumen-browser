//! Оконная сессия браузера через winit/wgpu.
//!
//! `WinitSession` реализует `BrowserSession` trait с использованием OS window (winit)
//! и GPU renderer (wgpu). Это — первый клиент `BrowserSession` для Lumen (см. ADR-006).
//!
//! # Архитектура
//!
//! WinitSession оборачивает основную Lumen логику (парсинг, CSS, layout, рендеринг)
//! и expose её через синхронный BrowserSession API. Внутри использует async event loop
//! через winit ApplicationHandler.
//!
//! **Фаза 1 (8A.7):** skeleton + основные методы (navigate, screenshot, layout_snapshot).
//! **Фаза 2 (8A.8):** полная миграция Lumen → WinitSession, shell переписан как клиент.

use std::sync::{Arc, Mutex};

use lumen_core::error::{Error, Result};
use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId};
use lumen_layout::LayoutBox;
use lumen_paint::Renderer;

use crate::{
    A11yNode, BoxModel, BrowserSession, ComputedProperties, ComputedStyleSnapshot,
    ConsoleEntry, NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
};

/// Состояние после успешной загрузки страницы.
struct WinitSessionState {
    doc: Document,
    layout_root: LayoutBox,
    renderer: Option<Renderer>,
}

/// Оконная сессия браузера.
///
/// Запускает полный pipeline движка с визуализацией через окно winit и GPU wgpu.
/// В текущей фазе (8A.7) это — skeleton; полная реализация будет в 8A.8.
///
/// # Пример
/// ```rust,no_run
/// use lumen_driver::{BrowserSession, WinitSession};
///
/// let mut session = WinitSession::new();
/// session.navigate("file:///path/to/page.html").unwrap();
/// let boxes = session.layout_snapshot().unwrap();
/// println!("{} боксов в layout", boxes.len());
/// ```
pub struct WinitSession {
    /// Размер viewport в логических пикселях.
    viewport: Size,
    /// URL последней успешно загруженной страницы.
    current_url: String,
    /// DOM + layout + renderer после последней навигации.
    state: Option<Arc<Mutex<WinitSessionState>>>,
    /// Журнал сетевых запросов с последней навигации.
    net_log: Vec<NetworkEntry>,
    /// Журнал console.log/warn/error с последней навигации.
    con_log: Vec<ConsoleEntry>,
}

impl WinitSession {
    /// Создать сессию с viewport 1024×720.
    pub fn new() -> Self {
        Self {
            viewport: Size::new(1024.0, 720.0),
            current_url: String::new(),
            state: None,
            net_log: Vec::new(),
            con_log: Vec::new(),
        }
    }

    /// Создать сессию с заданным размером viewport (логические пиксели).
    pub fn with_viewport(width: f32, height: f32) -> Self {
        Self {
            viewport: Size::new(width, height),
            current_url: String::new(),
            state: None,
            net_log: Vec::new(),
            con_log: Vec::new(),
        }
    }

    /// Получить текущее состояние сессии или вернуть ошибку.
    fn state(&self) -> Result<Arc<Mutex<WinitSessionState>>> {
        self.state
            .clone()
            .ok_or_else(|| {
                Error::Other("сессия не инициализирована — вызовите navigate() первым".into())
            })
    }
}

impl Default for WinitSession {
    fn default() -> Self {
        Self::new()
    }
}

impl WinitSession {
    /// Запустить полный pipeline (HTML parse -> CSS -> layout).
    fn run_pipeline(&mut self, bytes: &[u8], content_type: Option<&str>, url: String) -> Result<()> {
        // Временная реализация, использующая тот же код что и InProcessSession
        // TODO: вынести в общий helpers модуль

        const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

        let encoding = lumen_encoding::detect(bytes, content_type);
        let source = lumen_encoding::decode(encoding, bytes);

        let doc = lumen_html_parser::parse(&source);
        let css = extract_style_blocks(&doc);
        let sheet = lumen_css_parser::parse(&css);

        let font = lumen_font::Font::parse(INTER_FONT)
            .map_err(|e| Error::Other(format!("ошибка разбора Inter: {e}")))?;
        let measurer = lumen_paint::FontMeasurer::new(&font)
            .map_err(|e| Error::Other(format!("ошибка метрик Inter: {e}")))?;

        let layout_root = lumen_layout::layout_measured(&doc, &sheet, self.viewport, &measurer);

        self.current_url = url;
        self.state = Some(Arc::new(Mutex::new(WinitSessionState {
            doc,
            layout_root,
            renderer: None,
        })));
        Ok(())
    }
}

/// Извлечь содержимое всех <style> блоков из документа.
fn extract_style_blocks(doc: &lumen_dom::Document) -> String {
    let mut out = String::new();
    walk_style_blocks(doc, doc.root(), &mut out);
    out
}

fn walk_style_blocks(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    let node = doc.get(id);
    if let lumen_dom::NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let lumen_dom::NodeData::Text(s) = &doc.get(child).data {
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

impl BrowserSession for WinitSession {
    // ── Ресурсы ────────────────────────────────────────────────────────────

    fn screenshot(&self) -> Result<Vec<u8>> {
        // TODO: реализовать в 8A.7
        // - Получить LayoutBox из state
        // - Построить DisplayList
        // - Отрендерить через headless renderer (как в InProcessSession)
        // - Закодировать в PNG
        Err(Error::Other("screenshot: WinitSession не реализована в 8A.7".into()))
    }

    fn a11y_tree(&self) -> Result<A11yNode> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("a11y_tree: WinitSession не реализована в 8A.7".into()))
    }

    fn layout_snapshot(&self) -> Result<Vec<BoxModel>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("layout_snapshot: WinitSession не реализована в 8A.7".into()))
    }

    fn computed_style(&self, _selector: &str) -> Result<Option<ComputedProperties>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("computed_style: WinitSession не реализована в 8A.7".into()))
    }

    fn computed_style_snapshot(&self, _selector: &str) -> Result<Option<ComputedStyleSnapshot>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("computed_style_snapshot: WinitSession не реализована в 8A.7".into()))
    }

    fn layout_box_by_selector(&self, _selector: &str) -> Result<Option<BoxModel>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("layout_box_by_selector: WinitSession не реализована в 8A.7".into()))
    }

    fn all_layout_boxes_by_selector(&self, _selector: &str) -> Result<Vec<BoxModel>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("all_layout_boxes_by_selector: WinitSession не реализована в 8A.7".into()))
    }

    fn network_log(&self) -> Result<Vec<NetworkEntry>> {
        Ok(self.net_log.clone())
    }

    fn console_log(&self) -> Result<Vec<ConsoleEntry>> {
        Ok(self.con_log.clone())
    }

    fn current_url(&self) -> &str {
        &self.current_url
    }

    // ── Инструменты ────────────────────────────────────────────────────────

    fn navigate(&mut self, url: &str) -> Result<()> {
        // Phase 1: support file:// URLs only
        // Phase 2: add HTTP(S) support via NetworkTransport

        if url.starts_with("file://") {
            let path = &url[7..]; // strip "file://"
            let bytes = std::fs::read(path)
                .map_err(|e| Error::Other(format!("ошибка чтения файла {}: {}", path, e)))?;
            self.run_pipeline(&bytes, Some("text/html"), url.to_owned())
        } else if url.starts_with("http://") || url.starts_with("https://") {
            // TODO (Phase 2): implement HTTP loading
            Err(Error::Other("HTTP navigation не реализована в 8A.7".into()))
        } else {
            Err(Error::Other(format!("неподдерживаемый URL scheme: {}", url)))
        }
    }

    fn click(&mut self, _target: &Target) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("click: WinitSession не реализована в 8A.7".into()))
    }

    fn type_text(&mut self, _target: &Target, _text: &str) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("type_text: WinitSession не реализована в 8A.7".into()))
    }

    fn scroll(&mut self, _target: &Target, _delta: ScrollDelta) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("scroll: WinitSession не реализована в 8A.7".into()))
    }

    fn wait(&mut self, _cond: WaitCondition, _timeout_ms: u64) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("wait: WinitSession не реализована в 8A.7".into()))
    }

    fn eval(&self, _js: &str) -> Result<String> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("eval: WinitSession не реализована в 8A.7".into()))
    }

    fn query(&self, _selector: &str) -> Result<Vec<NodeRef>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("query: WinitSession не реализована в 8A.7".into()))
    }
}
