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
use lumen_core::ext::NoopEventSink;
use lumen_core::geom::Size;
use lumen_dom::Document;
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

    fn computed_style(&self, selector: &str) -> Result<Option<ComputedProperties>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("computed_style: WinitSession не реализована в 8A.7".into()))
    }

    fn computed_style_snapshot(&self, selector: &str) -> Result<Option<ComputedStyleSnapshot>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("computed_style_snapshot: WinitSession не реализована в 8A.7".into()))
    }

    fn layout_box_by_selector(&self, selector: &str) -> Result<Option<BoxModel>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("layout_box_by_selector: WinitSession не реализована в 8A.7".into()))
    }

    fn all_layout_boxes_by_selector(&self, selector: &str) -> Result<Vec<BoxModel>> {
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
        // TODO: реализовать в 8A.7
        // - Загрузить URL через NetworkTransport
        // - Запустить полный pipeline (парсинг, CSS, layout)
        // - Сохранить в state
        self.current_url = url.to_owned();
        Ok(())
    }

    fn click(&mut self, target: &Target) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("click: WinitSession не реализована в 8A.7".into()))
    }

    fn type_text(&mut self, target: &Target, text: &str) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("type_text: WinitSession не реализована в 8A.7".into()))
    }

    fn scroll(&mut self, target: &Target, delta: ScrollDelta) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("scroll: WinitSession не реализована в 8A.7".into()))
    }

    fn wait(&mut self, cond: WaitCondition, timeout_ms: u64) -> Result<()> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("wait: WinitSession не реализована в 8A.7".into()))
    }

    fn eval(&self, js: &str) -> Result<String> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("eval: WinitSession не реализована в 8A.7".into()))
    }

    fn query(&self, selector: &str) -> Result<Vec<NodeRef>> {
        // TODO: реализовать в 8A.7
        Err(Error::Other("query: WinitSession не реализована в 8A.7".into()))
    }
}
