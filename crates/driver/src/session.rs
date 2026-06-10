//! Headless in-process браузерная сессия.
//!
//! Запускает весь pipeline движка (encode → parse → CSS → layout) в одном потоке
//! без winit-окна и wgpu-поверхности. Это «базовый клиент» BrowserSession:
//! все остальные реализации (winit, BiDi) можно строить на тех же примитивах.

use std::sync::Arc;

use lumen_core::error::{Error, Result};
use lumen_core::ext::NoopEventSink;
use lumen_core::geom::{Rect, Size};
use serde_json;
use lumen_dom::Document;
use lumen_dom::NodeData;
use lumen_dom::NodeId;
use lumen_layout::{computed_style_by_selector, LayoutBox};

use crate::{
    A11yNode, AxQuery, BoxModel, BrowserSession, ComputedProperties, ComputedStyleSnapshot,
    ConsoleEntry, FingerprintProfile, NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
    context::SessionContext,
    isolation::OriginIsolationContext,
};

/// Встроенный шрифт Inter-Regular (SIL OFL 1.1).
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// Размер viewport по умолчанию — 1024×720 (совпадает с graphic_tests).
const DEFAULT_VIEWPORT: Size = Size::new(1024.0, 720.0);

/// Состояние после успешной загрузки страницы.
struct SessionState {
    doc: Document,
    layout_root: LayoutBox,
    flat_tree: lumen_dom::FlatTree,
}

/// Headless in-process сессия браузера.
///
/// Запускает полный pipeline движка (HTML parse → CSS cascade → layout) без GPU.
/// `screenshot` недоступен до реализации задачи 8A.5 (tinyskia-cpu-raster).
///
/// # Пример
/// ```rust,no_run
/// use lumen_driver::{BrowserSession, InProcessSession};
///
/// let mut session = InProcessSession::new();
/// session.navigate("file:///tmp/page.html").unwrap();
/// let boxes = session.layout_snapshot().unwrap();
/// println!("{} боксов в layout", boxes.len());
/// ```
pub struct InProcessSession {
    /// Размер viewport в логических пикселях.
    viewport: Size,
    /// URL последней успешно загруженной страницы.
    current_url: String,
    /// DOM + layout после последней навигации; `None` до первого `navigate`.
    state: Option<SessionState>,
    /// Журнал сетевых запросов с последней навигации.
    net_log: Vec<NetworkEntry>,
    /// Журнал console.log/warn/error с последней навигации.
    con_log: Vec<ConsoleEntry>,
    /// Изолированный контекст сессии: cookies, storage, cache, fingerprint profile.
    context: SessionContext,
    /// Per-origin-group isolation (8E). `Some` when created via
    /// [`InProcessSession::with_origin_isolation`]; `None` for the default
    /// shared-context session created by [`InProcessSession::new`].
    isolation: Option<OriginIsolationContext>,
    /// Счётчик активных HTTP-запросов (0 = NetworkIdle).
    ///
    /// В синхронной модели всегда 0 после возврата из `navigate()`.
    /// Используется `wait(WaitCondition::NetworkIdle)`.
    active_network_requests: usize,
    /// Счётчик pending JS microtask/callback (0 = JsIdle).
    ///
    /// В headless-режиме без JS-движка всегда 0.
    /// Shell-интеграция: вызывать `set_pending_js_tasks()` при изменении очереди.
    pending_js_microtasks: usize,
}

impl InProcessSession {
    /// Создать сессию с viewport 1024×720.
    pub fn new() -> Self {
        Self {
            viewport: DEFAULT_VIEWPORT,
            current_url: String::new(),
            state: None,
            net_log: Vec::new(),
            con_log: Vec::new(),
            context: SessionContext::new(),
            isolation: None,
            active_network_requests: 0,
            pending_js_microtasks: 0,
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
            context: SessionContext::new(),
            isolation: None,
            active_network_requests: 0,
            pending_js_microtasks: 0,
        }
    }

    /// Create a session with per-origin-group isolation (Phase 1: 8E).
    ///
    /// Cookies, `localStorage`, `sessionStorage`, and `IndexedDB` are scoped to
    /// the origin group derived from `origin` (eTLD+1). Two sessions created
    /// with different origins from the same site (e.g. `www.example.com` and
    /// `api.example.com`) share the same origin group but have **independent**
    /// storage — each `with_origin_isolation` call returns a fully isolated
    /// session instance.
    ///
    /// # Example
    /// ```rust,no_run
    /// use lumen_driver::{BrowserSession, InProcessSession};
    ///
    /// let mut s1 = InProcessSession::with_origin_isolation("https://example.com");
    /// let mut s2 = InProcessSession::with_origin_isolation("https://example.com");
    /// // s1 and s2 have independent cookie jars, localStorage, and IDB.
    /// ```
    pub fn with_origin_isolation(origin: &str) -> Self {
        Self {
            viewport: DEFAULT_VIEWPORT,
            current_url: String::new(),
            state: None,
            net_log: Vec::new(),
            con_log: Vec::new(),
            context: SessionContext::new(),
            isolation: Some(OriginIsolationContext::new(origin)),
            active_network_requests: 0,
            pending_js_microtasks: 0,
        }
    }

    /// Access the per-origin-group isolation context, if this session was
    /// created via [`with_origin_isolation`](InProcessSession::with_origin_isolation).
    ///
    /// Returns `None` for sessions created with `new()` or `with_viewport()`.
    pub fn isolation_context(&self) -> Option<&OriginIsolationContext> {
        self.isolation.as_ref()
    }

    /// Mutable access to the per-origin-group isolation context.
    pub fn isolation_context_mut(&mut self) -> Option<&mut OriginIsolationContext> {
        self.isolation.as_mut()
    }

    /// Установить количество pending JS microtask/callback для условия `JsIdle`.
    ///
    /// Вызывается shell-интеграцией при изменении очереди QuickJS microtask loop.
    /// В headless-режиме без JS-движка счётчик остаётся 0.
    ///
    /// [`WaitCondition::JsIdle`] возвращает `true` когда счётчик == 0.
    pub fn set_pending_js_tasks(&mut self, count: usize) {
        self.pending_js_microtasks = count;
    }

    /// Build an [`HttpClient`] configured with this session's per-context
    /// fingerprint profile (task 9F.2).
    ///
    /// Applies the HTTP fingerprint profile (header order + Client Hints) and
    /// the derived TLS profile (cipher order, kx_groups, ALPN) so that
    /// [`set_fingerprint_profile`](BrowserSession::set_fingerprint_profile)
    /// actually changes the outgoing request signature — not just the stored
    /// value.
    ///
    /// [`HttpClient`]: lumen_network::HttpClient
    fn build_http_client(&self) -> lumen_network::HttpClient {
        lumen_network::HttpClient::new()
            .with_sink(Arc::new(NoopEventSink))
            .with_content_decoder(Arc::new(lumen_network::BrotliContentDecoder::new()))
            .with_fingerprint_profile(self.context.fingerprint_profile().to_http_profile())
    }

    /// Загрузить HTML-строку без навигации по URL. Используется для тестов.
    pub fn navigate_html(&mut self, html: &str) -> Result<()> {
        self.run_pipeline(html.as_bytes(), Some("text/html"), "about:blank".to_owned())
    }

    /// Загрузить байты по URL и запустить pipeline. Внутренняя реализация
    /// навигации, используемая также для тестов с прямой передачей HTML.
    fn run_pipeline(&mut self, bytes: &[u8], content_type: Option<&str>, url: String) -> Result<()> {
        // sessionStorage is cleared on top-level navigation (HTML LS §8.1).
        if let Some(iso) = &mut self.isolation {
            iso.clear_all_session_storage();
        }

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
        let flat_tree = lumen_dom::build_flat_tree(&doc);

        self.current_url = url;
        self.state = Some(SessionState { doc, layout_root, flat_tree });
        Ok(())
    }

    /// Получить текущее состояние сессии или вернуть ошибку.
    fn state(&self) -> Result<&SessionState> {
        self.state.as_ref().ok_or_else(|| {
            Error::Other("сессия не инициализирована — вызовите navigate() первым".into())
        })
    }

    /// Детерминированный CPU-рендер текущей страницы в RGBA8 (tiny-skia).
    ///
    /// В отличие от [`BrowserSession::screenshot`], который рендерит через GPU
    /// (wgpu, `new_headless`) и потому зависит от драйвера/платформы, этот путь
    /// растеризует display-list программно через `lumen-paint` feature
    /// `cpu-render`. Результат пиксельно идентичен на Windows/macOS/Linux —
    /// основа для snapshot-тестов уровня 3 (задача 8A.6).
    ///
    /// Текущий CPU-растеризатор покрывает геометрические примитивы
    /// (`FillRect`/`FillRoundedRect`/`DrawBorder`/`DrawOutline`), линейные,
    /// радиальные и конические градиенты, тесселированные SVG-пути, серый
    /// placeholder `<img>` (`DrawImage` — без зарегистрированных пикселей
    /// рисуется заглушка, как в GPU-fallback) и текст (`DrawText` — глифы
    /// bundled-шрифта Inter Regular растеризуются через `lumen_font::Rasterizer`
    /// и композитятся через coverage-маску).
    ///
    /// # Errors
    /// Возвращает `Err`, если сессия не инициализирована или растеризация
    /// не удалась.
    #[cfg(feature = "cpu-render")]
    pub fn screenshot_cpu_rgba(&self) -> Result<lumen_image::Image> {
        let state = self.state()?;
        let display_list = lumen_paint::build_display_list(&state.layout_root);
        let width = self.viewport.width as u32;
        let height = self.viewport.height as u32;
        lumen_paint::Renderer::render_to_image_cpu(width, height, &display_list, &[], 0.0, 0.0)
            .map_err(|e| Error::Other(format!("CPU rasterization: {e}")))
    }

    /// Детерминированный CPU-рендер текущей страницы в PNG (tiny-skia).
    ///
    /// Удобная обёртка над [`Self::screenshot_cpu_rgba`] для записи эталонов.
    ///
    /// # Errors
    /// Возвращает `Err`, если рендер или PNG-кодирование не удались.
    #[cfg(feature = "cpu-render")]
    pub fn screenshot_cpu_png(&self) -> Result<Vec<u8>> {
        let image = self.screenshot_cpu_rgba()?;
        lumen_image::encode_png_rgba8(&image)
            .map_err(|e| Error::Other(format!("PNG encoding: {e}")))
    }

    /// Строит [`lumen_paint::DisplayList`] из текущего состояния страницы.
    ///
    /// Используется в `CompareBackend` тестах (ADR-010 RB-8): тест загружает страницу
    /// через InProcessSession, получает display list, затем рендерит его двумя
    /// бэкендами через [`lumen_paint::CompareBackend`].
    ///
    /// # Errors
    /// Возвращает `Err` если сессия не инициализирована (navigate() не вызывался).
    pub fn display_list_for_compare(
        &self,
    ) -> Result<Vec<lumen_paint::DisplayCommand>> {
        let state = self.state()?;
        Ok(lumen_paint::build_display_list(&state.layout_root))
    }
}

impl Default for InProcessSession {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserSession for InProcessSession {
    // ── Ресурсы ────────────────────────────────────────────────────────────

    fn screenshot(&self) -> Result<Vec<u8>> {
        let state = self.state()?;

        // Build display list from layout tree.
        let display_list = lumen_paint::build_display_list(&state.layout_root);

        // Create headless renderer for off-screen rendering.
        let width = self.viewport.width as u32;
        let height = self.viewport.height as u32;
        let mut renderer = lumen_paint::Renderer::new_headless(INTER_FONT.to_vec(), width, height)
            .map_err(|e| Error::Other(format!("headless renderer: {e}")))?;

        // Render to image (RGBA8).
        let image = renderer.render_to_image(&display_list, 0.0, 0.0)
            .map_err(|e| Error::Other(format!("render_to_image: {e}")))?;

        // Encode to PNG.
        lumen_image::encode_png_rgba8(&image).map_err(|e| Error::Other(format!("PNG encoding: {e}")))
    }

    fn a11y_tree(&self) -> Result<A11yNode> {
        let state = self.state()?;
        let ax_tree = lumen_a11y::build_ax_tree(&state.doc, state.doc.root(), &state.flat_tree);
        Ok(ax_node_to_a11y(&ax_tree.root))
    }

    fn query_a11y(&self, query: &AxQuery) -> Result<Option<A11yNode>> {
        let ax_tree = self.a11y_tree()?;
        Ok(find_a11y_node(&ax_tree, query))
    }

    fn query_a11y_all(&self, query: &AxQuery) -> Result<Vec<A11yNode>> {
        let ax_tree = self.a11y_tree()?;
        let mut results = Vec::new();
        find_all_a11y_nodes(&ax_tree, query, &mut results);
        Ok(results)
    }

    fn layout_snapshot(&self) -> Result<Vec<BoxModel>> {
        let state = self.state()?;
        let mut out = Vec::new();
        collect_boxes(&state.layout_root, &state.doc, &mut out);
        Ok(out)
    }

    fn computed_style(&self, selector: &str) -> Result<Option<ComputedProperties>> {
        let state = self.state()?;
        let Some(node_id) = find_first_by_selector(&state.doc, selector) else {
            return Ok(None);
        };
        // Найти LayoutBox для этого node_id.
        let Some(lb) = find_layout_box(&state.layout_root, node_id) else {
            return Ok(None);
        };
        Ok(Some(style_to_properties(&lb.style)))
    }

    fn network_log(&self) -> Result<Vec<NetworkEntry>> {
        Ok(self.net_log.clone())
    }

    fn console_log(&self) -> Result<Vec<ConsoleEntry>> {
        Ok(self.con_log.clone())
    }

    fn computed_style_snapshot(&self, selector: &str) -> Result<Option<ComputedStyleSnapshot>> {
        let state = self.state()?;
        Ok(computed_style_by_selector(&state.layout_root, &state.doc, selector))
    }

    fn current_url(&self) -> &str {
        &self.current_url
    }

    // ── Инструменты ────────────────────────────────────────────────────────

    fn navigate(&mut self, url: &str) -> Result<()> {
        self.net_log.clear();
        self.con_log.clear();

        if let Some(path) = url.strip_prefix("file://") {
            let bytes = std::fs::read(path)
                .map_err(|e| Error::Io(format!("не удалось прочитать {path}: {e}")))?;
            return self.run_pipeline(&bytes, None, url.to_owned());
        }

        if url.starts_with("http://") || url.starts_with("https://") {
            use lumen_core::ext::NetworkTransport;
            let lumen_url = lumen_core::url::Url::parse(url)
                .map_err(|e| Error::InvalidUrl(format!("{url}: {e}")))?;
            let client = self.build_http_client();
            self.active_network_requests += 1;
            let result = client.fetch(&lumen_url);
            self.active_network_requests = self.active_network_requests.saturating_sub(1);
            let bytes = result?;
            return self.run_pipeline(&bytes, Some("text/html"), url.to_owned());
        }

        // Допускаем прямой файловый путь без схемы.
        let bytes = std::fs::read(url)
            .map_err(|e| Error::Io(format!("не удалось прочитать {url}: {e}")))?;
        self.run_pipeline(&bytes, None, format!("file://{url}"))
    }

    fn click(&mut self, target: &Target) -> Result<()> {
        let state = self.state()?;
        let _point = resolve_target_point(state, target)?;
        // Phase 1 (8C): native input injection для mouse click.
        //
        // Headless (без JS runtime) может только проверить что элемент найден и виден.
        // Полный click с JS dispatch требует persistent JS runtime (задача 8A.7).
        // После интеграции: eval JS код который создаёт mousedown → mouseup → click
        // через QuickJS eval с isTrusted=true (через специальный JS API).
        Ok(())
    }

    fn type_text(&mut self, target: &Target, text: &str) -> Result<()> {
        let state = self.state()?;
        let _ = resolve_target_point(state, target)?;
        // Phase 1 (8C): native input injection для keyboard input.
        //
        // Headless не может обновить form field state без JS runtime.
        // После интеграции persistent JS runtime: eval JS для посимвольного ввода
        // с keydown → input → keyup событиями (isTrusted=true).
        let _ = text;  // unused in headless mode
        Ok(())
    }

    fn scroll(&mut self, _target: &Target, _delta: ScrollDelta) -> Result<()> {
        // Scroll state management — задача 8A.7 (shell-as-driver-client).
        Ok(())
    }

    fn wait(&mut self, cond: WaitCondition, timeout_ms: u64) -> Result<()> {
        use std::time::Instant;

        let start = Instant::now();
        const POLL_INTERVAL_MS: u64 = 10;

        loop {
            // Проверить условие
            if self.check_wait_condition(&cond)? {
                return Ok(());
            }

            // Проверить timeout
            if start.elapsed().as_millis() as u64 >= timeout_ms {
                return Err(Error::Other(format!(
                    "wait timeout после {timeout_ms} мс для условия {:?}",
                    match &cond {
                        WaitCondition::DocumentReady => "DocumentReady".to_string(),
                        WaitCondition::Visible(s) => format!("Visible({})", s),
                        WaitCondition::Stable(s) => format!("Stable({})", s),
                        WaitCondition::NetworkIdle => "NetworkIdle".to_string(),
                        WaitCondition::JsIdle => "JsIdle".to_string(),
                    }
                )));
            }

            // Подождать до следующей проверки
            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        }
    }

    fn eval(&self, _js: &str) -> Result<String> {
        // JS eval через QuickJS — задача persistent-js-runtime (уже в shell).
        // InProcessSession получит его через задачу 8A.7.
        Err(Error::Other(
            "eval доступен после интеграции persistent JS runtime (задача 8A.7)".into(),
        ))
    }

    fn query(&self, selector: &str) -> Result<Vec<NodeRef>> {
        let state = self.state()?;
        let ids = find_all_by_selector(&state.doc, selector);
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let node = state.doc.get(id);
            let tag_name = match &node.data {
                NodeData::Element { name, .. } => name.local.to_string(),
                _ => String::new(),
            };
            let text_content = collect_text(&state.doc, id);
            let bounding_rect = find_layout_box(&state.layout_root, id)
                .map(|lb| lb.rect)
                .unwrap_or(Rect::ZERO);
            out.push(NodeRef {
                node_id: id.index() as u32,
                tag_name,
                text_content,
                bounding_rect,
            });
        }
        Ok(out)
    }

    fn layout_box_by_selector(&self, selector: &str) -> Result<Option<BoxModel>> {
        let state = self.state()?;
        let Some(lb) = lumen_layout::find_box_by_selector(&state.layout_root, &state.doc, selector) else {
            return Ok(None);
        };

        let tag_name = {
            let node = state.doc.get(lb.node);
            match &node.data {
                NodeData::Element { name, .. } => name.local.to_string(),
                _ => String::new(),
            }
        };

        let r = lb.rect;
        let mt = lb.style.margin_top.to_px_opt().unwrap_or(0.0);
        let mr = lb.style.margin_right.to_px_opt().unwrap_or(0.0);
        let mb = lb.style.margin_bottom.to_px_opt().unwrap_or(0.0);
        let ml = lb.style.margin_left.to_px_opt().unwrap_or(0.0);
        let margin_box = Rect {
            x: r.x - ml,
            y: r.y - mt,
            width: r.width + ml + mr,
            height: r.height + mt + mb,
        };

        Ok(Some(BoxModel {
            node_id: lb.node.index() as u32,
            tag_name,
            border_box: r,
            margin_box,
        }))
    }

    fn all_layout_boxes_by_selector(&self, selector: &str) -> Result<Vec<BoxModel>> {
        let state = self.state()?;
        let boxes = lumen_layout::find_all_by_selector(&state.layout_root, &state.doc, selector);
        let mut out = Vec::with_capacity(boxes.len());

        for lb in boxes {
            let tag_name = {
                let node = state.doc.get(lb.node);
                match &node.data {
                    NodeData::Element { name, .. } => name.local.to_string(),
                    _ => String::new(),
                }
            };

            let r = lb.rect;
            let mt = lb.style.margin_top.to_px_opt().unwrap_or(0.0);
            let mr = lb.style.margin_right.to_px_opt().unwrap_or(0.0);
            let mb = lb.style.margin_bottom.to_px_opt().unwrap_or(0.0);
            let ml = lb.style.margin_left.to_px_opt().unwrap_or(0.0);
            let margin_box = Rect {
                x: r.x - ml,
                y: r.y - mt,
                width: r.width + ml + mr,
                height: r.height + mt + mb,
            };

            out.push(BoxModel {
                node_id: lb.node.index() as u32,
                tag_name,
                border_box: r,
                margin_box,
            });
        }

        Ok(out)
    }

    // ── Isolation & Fingerprinting (Task 8E, Phase 1) ────────────────────────

    fn fingerprint_profile(&self) -> FingerprintProfile {
        self.context.fingerprint_profile()
    }

    fn set_fingerprint_profile(&mut self, profile: FingerprintProfile) -> Result<()> {
        self.context.set_fingerprint_profile(profile)
    }

    fn user_agent(&self) -> String {
        self.context.user_agent()
    }

    fn set_user_agent(&mut self, ua: &str) -> Result<()> {
        self.context.set_user_agent(ua)
    }

    // ── Deterministic mode (Task 8F, Phase 1) ────────────────────────────────

    fn set_clock(&mut self, mode: crate::ClockMode) -> Result<()> {
        self.context.set_clock_mode(mode);
        Ok(())
    }

    fn set_rng_seed(&mut self, seed: Option<u64>) -> Result<()> {
        self.context.set_rng_seed(seed);
        Ok(())
    }

    fn freeze_fingerprint(&mut self, profile: FingerprintProfile) -> Result<()> {
        self.context.set_fingerprint_profile(profile)?;
        self.context.freeze_fingerprint();
        Ok(())
    }
}

/// Adapter: InProcessSession также реализует базовый BrowserSession из lumen-core::ext.
/// Это позволяет использовать InProcessSession везде, где ожидается core::ext::BrowserSession.
impl lumen_core::ext::BrowserSession for InProcessSession {
    fn navigate(&mut self, url_or_path: &str) -> Result<()> {
        <Self as BrowserSession>::navigate(self, url_or_path)
    }

    fn screenshot(&self) -> Result<Vec<u8>> {
        <Self as BrowserSession>::screenshot(self)
    }

    fn a11y_tree(&self) -> Result<String> {
        // Сериализовать accessibility tree в JSON.
        let ax_node = <Self as BrowserSession>::a11y_tree(self)?;
        serde_json::to_string(&ax_node)
            .map_err(|e| Error::Other(format!("a11y_tree serialization: {e}")))
    }

    fn click(&mut self, selector: &str) -> Result<Option<String>> {
        // Phase 1: найти элемент, но неDispatchEvent (требует JS runtime).
        <Self as BrowserSession>::click(self, &Target::Selector(selector.to_string()))?;
        Ok(None)
    }

    fn type_text(&mut self, text: &str) -> Result<()> {
        let state = self.state()?;
        // Найти сфокусированный элемент или первый input.
        let selector = "input:not([type='hidden']), textarea, [contenteditable]";
        if let Some(node_id) = find_first_by_selector(&state.doc, selector) {
            <Self as BrowserSession>::type_text(self, &Target::NodeId(node_id.index() as u32), text)?;
        }
        Ok(())
    }

    fn scroll_by(&mut self, delta: f32) -> Result<f32> {
        // Phase 1: прокрутка документа (требует persistent window state).
        // Пока что заглушка — возвращаем текущую позицию (всегда 0).
        let _ = delta;
        Ok(0.0)
    }

    fn wait_for_navigation(&mut self) -> Result<String> {
        // В headless-режиме navigate() уже блокируется, поэтому это NOP.
        Ok(self.current_url.clone())
    }

    fn wait_for_idle(&mut self) -> Result<()> {
        // Phase 0 headless: нет JS/animations, поэтому всегда idle.
        Ok(())
    }

    fn viewport(&self) -> (u32, u32) {
        (self.viewport.width as u32, self.viewport.height as u32)
    }

    fn set_viewport(&mut self, width: u32, height: u32) -> Result<()> {
        self.viewport = Size::new(width as f32, height as f32);
        Ok(())
    }

    fn computed_style(&self, selector: &str) -> Result<String> {
        let props = <Self as BrowserSession>::computed_style(self, selector)?
            .ok_or_else(|| Error::NotFound(format!("элемент не найден: {selector}")))?;
        // Сериализовать properties в JSON.
        serde_json::to_string(&props.properties)
            .map_err(|e| Error::Other(format!("computed_style serialization: {e}")))
    }

    fn eval(&mut self, script: &str) -> Result<String> {
        <Self as BrowserSession>::eval(self, script)
    }

    fn set_clock(&mut self, mode: lumen_core::ClockMode) -> Result<()> {
        self.context.set_clock_mode(mode);
        Ok(())
    }

    fn set_rng_seed(&mut self, seed: Option<u64>) -> Result<()> {
        self.context.set_rng_seed(seed);
        Ok(())
    }

    fn deliver_lcp_entry(
        &mut self,
        element_id: i32,
        size: u32,
        start_ms: f64,
        render_time_ms: f64,
    ) -> Result<()> {
        let script = format!(
            "_lumen_deliver_lcp_entry({}, {}, {}, {})",
            element_id, size, start_ms, render_time_ms
        );
        self.eval(&script)?;
        Ok(())
    }

    fn deliver_layout_shift(&mut self, value: f64, session_id: u32, had_input: bool) -> Result<()> {
        let script = format!(
            "_lumen_deliver_layout_shift({}, {}, {})",
            value, session_id, had_input as u8
        );
        self.eval(&script)?;
        Ok(())
    }

    fn deliver_perf_entry(
        &mut self,
        entry_type: &str,
        name: &str,
        start_ms: f64,
        duration_ms: f64,
        detail_json: Option<&str>,
    ) -> Result<()> {
        let detail = detail_json.unwrap_or("null");
        // {:?} produces a Rust Debug string which is a valid JS double-quoted literal
        // for the ASCII/UTF-8 entry type and name values used in practice.
        let script = format!(
            "_lumen_deliver_perf_entry({:?}, {:?}, {}, {}, {})",
            entry_type, name, start_ms, duration_ms, detail
        );
        self.eval(&script)?;
        Ok(())
    }
}

// ── Вспомогательные функции ─────────────────────────────────────────────────

/// Извлечь содержимое всех `<style>` блоков из документа (рекурсивный обход).
fn extract_style_blocks(doc: &Document) -> String {
    let mut out = String::new();
    walk_style(doc, doc.root(), &mut out);
    out
}

fn walk_style(doc: &Document, id: NodeId, out: &mut String) {
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
        walk_style(doc, child, out);
    }
}

/// Рекурсивно собрать все LayoutBox в плоский список BoxModel.
fn collect_boxes(lb: &LayoutBox, doc: &Document, out: &mut Vec<BoxModel>) {
    let tag_name = {
        let node = doc.get(lb.node);
        match &node.data {
            NodeData::Element { name, .. } => name.local.to_string(),
            _ => String::new(),
        }
    };
    let r = lb.rect;
    // Margin-box: expand border-box by resolved margin values (fallback to 0 for auto/relative).
    let mt = lb.style.margin_top.to_px_opt().unwrap_or(0.0);
    let mr = lb.style.margin_right.to_px_opt().unwrap_or(0.0);
    let mb = lb.style.margin_bottom.to_px_opt().unwrap_or(0.0);
    let ml = lb.style.margin_left.to_px_opt().unwrap_or(0.0);
    let margin_box = Rect {
        x: r.x - ml,
        y: r.y - mt,
        width: r.width + ml + mr,
        height: r.height + mt + mb,
    };
    out.push(BoxModel {
        node_id: lb.node.index() as u32,
        tag_name,
        border_box: lb.rect,
        margin_box,
    });
    for child in &lb.children {
        collect_boxes(child, doc, out);
    }
}

/// Найти первый LayoutBox, принадлежащий узлу с данным NodeId.
fn find_layout_box(root: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
    if root.node == id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_layout_box(child, id) {
            return Some(found);
        }
    }
    None
}

/// Простой парсер CSS-селектора — поддерживает основные формы Phase 0.
///
/// Поддерживаемые паттерны:
/// - `"div"` — по тегу
/// - `"#id"` — по id
/// - `".class"` — по классу
/// - `"tag#id"`, `"tag.class"` — комбинации тега с id/классом
/// - `"tag.class1.class2"` — несколько классов
#[derive(Debug, Default)]
struct SimpleSelector<'a> {
    tag: Option<&'a str>,
    id: Option<&'a str>,
    classes: Vec<&'a str>,
}

fn parse_simple_selector(s: &str) -> SimpleSelector<'_> {
    let mut sel = SimpleSelector::default();
    let mut rest = s;

    // Тег: всё до первого '#' или '.'
    let tag_end = rest.find(['#', '.']).unwrap_or(rest.len());
    if tag_end > 0 {
        sel.tag = Some(&rest[..tag_end]);
    }
    rest = &rest[tag_end..];

    // ID и классы
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('#') {
            let end = r.find(['#', '.']).unwrap_or(r.len());
            sel.id = Some(&r[..end]);
            rest = &r[end..];
        } else if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['#', '.']).unwrap_or(r.len());
            sel.classes.push(&r[..end]);
            rest = &r[end..];
        } else {
            break;
        }
    }

    sel
}

fn node_matches_selector(doc: &Document, id: NodeId, sel: &SimpleSelector<'_>) -> bool {
    let node = doc.get(id);
    let NodeData::Element { name, attrs } = &node.data else {
        return false;
    };

    if let Some(tag) = sel.tag
        && !name.local.eq_ignore_ascii_case(tag)
    {
        return false;
    }

    if let Some(wanted_id) = sel.id {
        let actual_id = attrs
            .iter()
            .find(|a| a.name.local.eq_ignore_ascii_case("id"))
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if actual_id != wanted_id {
            return false;
        }
    }

    if !sel.classes.is_empty() {
        let class_attr = attrs
            .iter()
            .find(|a| a.name.local.eq_ignore_ascii_case("class"))
            .map(|a| a.value.as_str())
            .unwrap_or("");
        let actual_classes: Vec<&str> = class_attr.split_whitespace().collect();
        for wanted in &sel.classes {
            if !actual_classes.iter().any(|c| c == wanted) {
                return false;
            }
        }
    }

    true
}

/// Найти первый узел в документе, совпадающий с `selector`.
fn find_first_by_selector(doc: &Document, selector: &str) -> Option<NodeId> {
    let sel = parse_simple_selector(selector);
    find_first_match(doc, doc.root(), &sel)
}

fn find_first_match(doc: &Document, id: NodeId, sel: &SimpleSelector<'_>) -> Option<NodeId> {
    if node_matches_selector(doc, id, sel) {
        return Some(id);
    }
    for &child in &doc.get(id).children.clone() {
        if let Some(found) = find_first_match(doc, child, sel) {
            return Some(found);
        }
    }
    None
}

/// Найти все узлы, совпадающие с `selector`.
fn find_all_by_selector(doc: &Document, selector: &str) -> Vec<NodeId> {
    let sel = parse_simple_selector(selector);
    let mut out = Vec::new();
    find_all_match(doc, doc.root(), &sel, &mut out);
    out
}

fn find_all_match(doc: &Document, id: NodeId, sel: &SimpleSelector<'_>, out: &mut Vec<NodeId>) {
    if node_matches_selector(doc, id, sel) {
        out.push(id);
    }
    for &child in &doc.get(id).children.clone() {
        find_all_match(doc, child, sel, out);
    }
}

/// Собрать текстовое содержимое поддерева.
fn collect_text(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    walk_text(doc, id, &mut out);
    out
}

fn walk_text(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children {
        walk_text(doc, child, out);
    }
}

/// Convert `lumen_a11y::AXNode` into the driver's `A11yNode` (public API type).
fn ax_node_to_a11y(ax: &lumen_a11y::AXNode) -> A11yNode {
    use crate::A11yState;
    let state = A11yState {
        disabled: ax.state.disabled,
        checked: ax.state.checked,
        expanded: ax.state.expanded,
        hidden: ax.state.hidden,
        selected: ax.state.selected,
        pressed: ax.state.pressed,
        required: ax.state.required,
        readonly: ax.state.readonly,
        invalid: ax.state.invalid,
        level: ax.state.level,
    };
    A11yNode {
        node_id: ax.node_id.index() as u32,
        role: ax.role.as_str().to_owned(),
        name: ax.name.clone(),
        description: ax.description.clone(),
        placeholder: ax.placeholder.clone(),
        state,
        children: ax.children.iter().map(ax_node_to_a11y).collect(),
    }
}

/// Преобразовать ComputedStyle в карту свойство → строка.
fn style_to_properties(style: &lumen_layout::ComputedStyle) -> ComputedProperties {
    let mut m = std::collections::HashMap::new();

    m.insert("display".into(), format!("{:?}", style.display).to_lowercase());
    m.insert("color".into(), format_color(&style.color));
    m.insert(
        "background-color".into(),
        style
            .background_color
            .as_ref()
            .and_then(|c| (*c).to_color_opt())
            .map(|c| format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a))
            .unwrap_or_else(|| "transparent".into()),
    );
    m.insert("font-size".into(), format!("{:.2}px", style.font_size));
    m.insert("font-weight".into(), format!("{}", style.font_weight.0));
    m.insert("width".into(), format_opt_length(style.width.as_ref()));
    m.insert("height".into(), format_opt_length(style.height.as_ref()));
    m.insert("margin-top".into(), format_length_or_auto(&style.margin_top));
    m.insert("margin-right".into(), format_length_or_auto(&style.margin_right));
    m.insert("margin-bottom".into(), format_length_or_auto(&style.margin_bottom));
    m.insert("margin-left".into(), format_length_or_auto(&style.margin_left));
    m.insert("padding-top".into(), format_length(&style.padding_top));
    m.insert("padding-right".into(), format_length(&style.padding_right));
    m.insert("padding-bottom".into(), format_length(&style.padding_bottom));
    m.insert("padding-left".into(), format_length(&style.padding_left));

    ComputedProperties { properties: m }
}

fn format_color(c: &lumen_layout::Color) -> String {
    format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a)
}

fn format_length(l: &lumen_layout::Length) -> String {
    match l {
        lumen_layout::Length::Px(v) => format!("{:.2}px", v),
        lumen_layout::Length::Percent(v) => format!("{:.2}%", v),
        lumen_layout::Length::Em(v) => format!("{:.2}em", v),
        lumen_layout::Length::Rem(v) => format!("{:.2}rem", v),
        other => format!("{other:?}"),
    }
}

fn format_opt_length(l: Option<&lumen_layout::Length>) -> String {
    match l {
        None => "auto".into(),
        Some(len) => format_length(len),
    }
}

fn format_length_or_auto(l: &lumen_layout::LengthOrAuto) -> String {
    match l {
        lumen_layout::LengthOrAuto::Auto => "auto".into(),
        lumen_layout::LengthOrAuto::Length(len) => format_length(len),
    }
}

/// Разрешить `Target` в координату точки клика (центр элемента или явная точка).
fn resolve_target_point(state: &SessionState, target: &Target) -> Result<(f32, f32)> {
    match target {
        Target::Point { x, y } => Ok((*x, *y)),
        Target::Selector(sel) => {
            let id = find_first_by_selector(&state.doc, sel).ok_or_else(|| {
                Error::NotFound(format!("элемент не найден: {sel}"))
            })?;
            let lb = find_layout_box(&state.layout_root, id).ok_or_else(|| {
                Error::NotFound(format!("layout-бокс не найден для: {sel}"))
            })?;
            Ok((lb.rect.x + lb.rect.width / 2.0, lb.rect.y + lb.rect.height / 2.0))
        }
        Target::NodeId(raw_id) => {
            let id = NodeId::from_index(*raw_id as usize);
            let lb = find_layout_box(&state.layout_root, id).ok_or_else(|| {
                Error::NotFound(format!("layout-бокс не найден для node_id={raw_id}"))
            })?;
            Ok((lb.rect.x + lb.rect.width / 2.0, lb.rect.y + lb.rect.height / 2.0))
        }
    }
}

impl InProcessSession {
    /// Возвращает полный набор computed-style свойств первого элемента,
    /// совпадающего с `selector`, в виде JSON-объекта (`{"prop":"value",...}`).
    ///
    /// Используется панелью DevTools «Computed» (lumen-plan §7E.2). В отличие от
    /// [`computed_style`](lumen_core::ext::BrowserSession::computed_style)
    /// (≈13 свойств), охватывает ~70 свойств `ComputedStyle` через
    /// `lumen_layout::computed_style_json`. Ключи отсортированы (детерминизм).
    ///
    /// Ошибка [`Error::NotFound`], если ни один элемент не совпал с селектором.
    pub fn computed_style_json(&self, selector: &str) -> Result<String> {
        let state = self.state()?;
        lumen_layout::computed_style_json_by_selector(&state.layout_root, &state.doc, selector)
            .ok_or_else(|| Error::NotFound(format!("элемент не найден: {selector}")))
    }

    /// Проверить выполнение условия ожидания.
    fn check_wait_condition(&self, cond: &WaitCondition) -> Result<bool> {
        match cond {
            WaitCondition::DocumentReady => {
                // В headless-режиме document всегда ready после navigate().
                Ok(self.state.is_some())
            }
            WaitCondition::Visible(selector) => {
                // 8D.1: элемент считается видимым, если:
                // (a) имеет layout-бокс (display:none не создаёт боксов),
                // (b) border-box имеет ненулевые размеры (width > 0 && height > 0).
                //
                // visibility:hidden оставляет элемент в layout с ненулевым боксом —
                // полная проверка требует computed_style "visibility", реализуется в Phase 2
                // когда ComputedStyle расширяется P4.
                let Some(bm) = self.layout_box_by_selector(selector)? else {
                    return Ok(false);
                };
                Ok(bm.border_box.width > 0.0 && bm.border_box.height > 0.0)
            }
            WaitCondition::Stable(selector) => {
                // Стабильность layout: в headless нет animation или JavaScript,
                // поэтому layout стабилен с самого начала. Для Phase 1 — всегда true.
                // (Реальная реализация через layout-change tracking — в WinitSession + shell)
                // Сначала проверяем что элемент существует в DOM, затем report stable.
                let state = self.state()?;
                let doc = &state.doc;
                let ids = find_all_by_selector(doc, selector);
                Ok(!ids.is_empty())
            }
            WaitCondition::NetworkIdle => {
                // 8D.2: нет активных сетевых запросов.
                // active_network_requests отслеживается в navigate() вокруг каждого
                // HTTP-fetch. После возврата из navigate() счётчик == 0.
                // Полная async-реализация — в WinitSession + shell (Phase 2).
                Ok(self.active_network_requests == 0)
            }
            WaitCondition::JsIdle => {
                // 8D.3: JS event loop пуст (нет pending microtask/task/rAF).
                // pending_js_microtasks устанавливается через set_pending_js_tasks().
                // В headless без JS-движка счётчик == 0 → всегда idle.
                // Shell-интеграция: обновлять счётчик из QuickJS execute_pending_job() loop.
                Ok(self.pending_js_microtasks == 0)
            }
        }
    }
}

/// Find first accessibility node matching query.
fn find_a11y_node(node: &A11yNode, query: &AxQuery) -> Option<A11yNode> {
    if matches_query(node, query) {
        return Some(node.clone());
    }
    for child in &node.children {
        if let Some(result) = find_a11y_node(child, query) {
            return Some(result);
        }
    }
    None
}

/// Find all accessibility nodes matching query (depth-first).
fn find_all_a11y_nodes(node: &A11yNode, query: &AxQuery, results: &mut Vec<A11yNode>) {
    if matches_query(node, query) {
        results.push(node.clone());
    }
    for child in &node.children {
        find_all_a11y_nodes(child, query, results);
    }
}

/// Check if accessibility node matches query criteria.
fn matches_query(node: &A11yNode, query: &AxQuery) -> bool {
    match query {
        AxQuery::Role { role, name } => {
            let role_matches = node.role.eq_ignore_ascii_case(role);
            if !role_matches {
                return false;
            }
            if let Some(name_filter) = name {
                node.name.to_lowercase().contains(&name_filter.to_lowercase())
            } else {
                true
            }
        }
        AxQuery::NameContains(name_filter) => {
            node.name.to_lowercase().contains(&name_filter.to_lowercase())
        }
    }
}

// ── Тесты ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrowserSession;

    fn make_session(html: &str) -> InProcessSession {
        let mut s = InProcessSession::new();
        let bytes = html.as_bytes().to_vec();
        s.run_pipeline(&bytes, Some("text/html"), "file://test".into())
            .expect("pipeline не запустился");
        s
    }

    #[test]
    fn navigate_local_html_produces_layout() {
        let mut s = InProcessSession::new();
        // Inline HTML через run_pipeline напрямую (нет реального файла).
        let html = r#"<!DOCTYPE html><html><body><div id="box" style="width:100px;height:50px;background:red"></div></body></html>"#;
        s.run_pipeline(html.as_bytes(), Some("text/html"), "file://test".into())
            .expect("pipeline");
        let boxes = s.layout_snapshot().expect("layout_snapshot");
        assert!(!boxes.is_empty());
    }

    #[test]
    fn query_by_tag_returns_nodes() {
        let s = make_session("<html><body><p>один</p><p>два</p></body></html>");
        let nodes = s.query("p").expect("query");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag_name, "p");
    }

    #[test]
    fn query_by_id_returns_single_node() {
        let s = make_session(r#"<html><body><div id="hero">H</div><div id="other">O</div></body></html>"#);
        let nodes = s.query("#hero").expect("query");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, nodes[0].node_id); // sanity
    }

    #[test]
    fn query_by_class_filters_correctly() {
        let s = make_session(r#"<html><body><span class="red big">A</span><span class="blue">B</span><span class="red small">C</span></body></html>"#);
        let nodes = s.query(".red").expect("query .red");
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn computed_style_returns_properties() {
        let s = make_session(r#"<html><body><div id="x" style="font-size:24px"></div></body></html>"#);
        let props = s.computed_style("#x").expect("computed_style");
        assert!(props.is_some());
    }

    #[test]
    fn a11y_tree_has_role() {
        let s = make_session("<html><body><button>OK</button></body></html>");
        let tree = s.a11y_tree().expect("a11y_tree");
        fn has_role(node: &A11yNode, role: &str) -> bool {
            if node.role == role {
                return true;
            }
            node.children.iter().any(|c| has_role(c, role))
        }
        assert!(has_role(&tree, "button"), "button роль не найдена");
    }

    #[test]
    fn screenshot_returns_png() {
        let s = make_session("<html><body><div style='background:red; width:100px; height:100px;'></div></body></html>");
        let png_bytes = s.screenshot().expect("screenshot should succeed");
        // PNG signature: 89 50 4E 47 0D 0A 1A 0A
        assert!(png_bytes.len() > 8, "PNG should have content");
        assert_eq!(&png_bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A], "PNG signature");
    }

    #[test]
    fn current_url_after_navigate() {
        let mut s = InProcessSession::new();
        s.run_pipeline(b"<html></html>", None, "file:///my/page.html".into())
            .expect("pipeline");
        assert_eq!(s.current_url(), "file:///my/page.html");
    }

    #[test]
    fn parse_simple_selector_tag_only() {
        let sel = parse_simple_selector("div");
        assert_eq!(sel.tag, Some("div"));
        assert!(sel.id.is_none());
        assert!(sel.classes.is_empty());
    }

    #[test]
    fn parse_simple_selector_id() {
        let sel = parse_simple_selector("#hero");
        assert!(sel.tag.is_none());
        assert_eq!(sel.id, Some("hero"));
    }

    #[test]
    fn parse_simple_selector_class() {
        let sel = parse_simple_selector(".red");
        assert!(sel.tag.is_none());
        assert_eq!(sel.classes, vec!["red"]);
    }

    #[test]
    fn parse_simple_selector_tag_and_class() {
        let sel = parse_simple_selector("span.red.big");
        assert_eq!(sel.tag, Some("span"));
        assert_eq!(sel.classes, vec!["red", "big"]);
    }

    #[test]
    fn wait_document_ready_succeeds_after_navigate() {
        let mut s = InProcessSession::new();
        s.run_pipeline(b"<html><body>text</body></html>", Some("text/html"), "file:///test".into())
            .expect("pipeline");
        // DocumentReady должен быть успешен сразу после navigate
        s.wait(WaitCondition::DocumentReady, 1000)
            .expect("wait DocumentReady");
    }

    #[test]
    fn wait_visible_element_succeeds() {
        let mut s = make_session(r#"<html><body><div id="box" style="width:100px;height:50px"></div></body></html>"#);
        // Элемент существует в layout, поэтому Visible должна быть true
        s.wait(WaitCondition::Visible("#box".into()), 1000)
            .expect("wait Visible");
    }

    #[test]
    fn wait_visible_nonexistent_element_times_out() {
        let mut s = make_session(r#"<html><body><div id="box"></div></body></html>"#);
        // Элемента с id="missing" нет, поэтому timeout
        let result = s.wait(WaitCondition::Visible("#missing".into()), 100);
        assert!(result.is_err(), "wait должен вернуть timeout");
        assert!(result.unwrap_err().to_string().contains("timeout"));
    }

    #[test]
    fn wait_stable_element_succeeds() {
        let mut s = make_session(r#"<html><body><span class="text">Hello</span></body></html>"#);
        // Layout стабилен с самого начала в headless
        s.wait(WaitCondition::Stable(".text".into()), 1000)
            .expect("wait Stable");
    }

    #[test]
    fn wait_network_idle_succeeds() {
        let mut s = make_session("<html><body>test</body></html>");
        // Network всегда idle в headless (нет async network)
        s.wait(WaitCondition::NetworkIdle, 1000)
            .expect("wait NetworkIdle");
    }

    #[test]
    fn wait_js_idle_succeeds() {
        let mut s = make_session("<html><body>test</body></html>");
        // JS всегда idle в Phase 1 headless (нет JS engine)
        s.wait(WaitCondition::JsIdle, 1000)
            .expect("wait JsIdle");
    }

    // ── 8D: Auto-wait tests ─────────────────────────────────────────────────

    #[test]
    fn wait_visible_with_dimensions_succeeds() {
        let mut s = make_session(
            r#"<html><body><div id="box" style="width:80px;height:40px;background:blue"></div></body></html>"#,
        );
        s.wait(WaitCondition::Visible("#box".into()), 1000)
            .expect("visible element with explicit size should be found");
    }

    #[test]
    fn wait_visible_zero_height_times_out() {
        // div without content has height=0 — not considered visible (8D.1).
        let mut s = make_session(r#"<html><body><div id="empty"></div></body></html>"#);
        let result = s.wait(WaitCondition::Visible("#empty".into()), 100);
        assert!(result.is_err(), "zero-height element must not satisfy Visible");
        assert!(result.unwrap_err().to_string().contains("timeout"));
    }

    #[test]
    fn wait_network_idle_immediately_after_file_navigate() {
        // File navigations don't touch active_network_requests → always 0.
        let mut s = make_session("<html><body>hi</body></html>");
        assert_eq!(s.active_network_requests, 0);
        s.wait(WaitCondition::NetworkIdle, 100)
            .expect("NetworkIdle must be true after file-based navigate");
    }

    #[test]
    fn wait_js_idle_false_when_tasks_pending() {
        let mut s = make_session("<html><body>test</body></html>");
        s.set_pending_js_tasks(5);
        let result = s.wait(WaitCondition::JsIdle, 100);
        assert!(result.is_err(), "JsIdle must timeout when pending tasks > 0");
        // Clear tasks → idle.
        s.set_pending_js_tasks(0);
        s.wait(WaitCondition::JsIdle, 100)
            .expect("JsIdle must succeed after clearing pending tasks");
    }

    #[test]
    fn set_pending_js_tasks_updates_counter() {
        let mut s = InProcessSession::new();
        assert_eq!(s.pending_js_microtasks, 0);
        s.set_pending_js_tasks(3);
        assert_eq!(s.pending_js_microtasks, 3);
        s.set_pending_js_tasks(0);
        assert_eq!(s.pending_js_microtasks, 0);
    }

    // ── Тесты для core::ext::BrowserSession adapter ────────────────────────

    #[test]
    fn core_session_navigate() {
        let s = make_session("<html><body><h1>Hello</h1></body></html>");
        // Already navigated via make_session, just verify the URL is set
        assert!(!s.current_url().is_empty());
    }

    #[test]
    fn core_session_screenshot() {
        let s = make_session("<html><body><div style='background:blue; width:50px; height:50px;'></div></body></html>");
        let png_bytes = lumen_core::ext::BrowserSession::screenshot(&s).expect("screenshot");
        assert!(png_bytes.len() > 8);
        assert_eq!(&png_bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn core_session_a11y_tree_json() {
        let s = make_session("<html><body><button>Click</button></body></html>");
        let json_str = lumen_core::ext::BrowserSession::a11y_tree(&s).expect("a11y_tree");
        // Verify it's valid JSON
        serde_json::from_str::<serde_json::Value>(&json_str)
            .expect("a11y_tree should be valid JSON");
    }

    #[test]
    fn core_session_computed_style_json() {
        let s = make_session(r#"<html><body><div id="x" style="color:red;font-size:20px"></div></body></html>"#);
        let json_str = lumen_core::ext::BrowserSession::computed_style(&s, "#x").expect("computed_style");
        let obj: serde_json::Value = serde_json::from_str(&json_str)
            .expect("computed_style should be valid JSON");
        // Verify we have a JSON object with properties
        assert!(obj.is_object());
    }

    #[test]
    fn computed_style_json_full_map() {
        let s = make_session(r#"<html><body><div id="x" style="color:red;font-size:20px"></div></body></html>"#);
        let json_str = s.computed_style_json("#x").expect("computed_style_json");
        let obj: serde_json::Value =
            serde_json::from_str(&json_str).expect("valid JSON object");
        let map = obj.as_object().expect("object");
        // Full map covers far more than the ~13-property BrowserSession::computed_style.
        assert!(map.len() > 30, "expected full property map, got {}", map.len());
        assert_eq!(map["color"], "rgb(255, 0, 0)");
        assert_eq!(map["font-size"], "20px");
        assert_eq!(map["display"], "block");
    }

    #[test]
    fn computed_style_json_missing_selector_errors() {
        let s = make_session(r#"<html><body><div></div></body></html>"#);
        assert!(s.computed_style_json("#nope").is_err());
    }

    #[test]
    fn core_session_viewport() {
        let s = make_session("<html><body></body></html>");
        let (w, h) = lumen_core::ext::BrowserSession::viewport(&s);
        assert_eq!(w, 1024);
        assert_eq!(h, 720);
    }

    #[test]
    fn core_session_set_viewport() {
        let mut s = make_session("<html><body></body></html>");
        lumen_core::ext::BrowserSession::set_viewport(&mut s, 800, 600).expect("set_viewport");
        let (w, h) = lumen_core::ext::BrowserSession::viewport(&s);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
    }

    // ── 9F.2: per-context fingerprint profile applied to the HTTP client ──────

    #[test]
    fn http_client_reflects_fingerprint_profile() {
        let mut s = InProcessSession::new();
        // Default (Standard) → Chrome HTTP + Standard TLS.
        let c = s.build_http_client();
        assert_eq!(c.fingerprint_profile(), lumen_network::HttpProfile::Chrome);
        assert_eq!(c.tls_profile(), lumen_network::TlsProfile::Standard);

        // Strict → Strict HTTP + Strict TLS (TLS profile is derived).
        BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Strict).unwrap();
        let c = s.build_http_client();
        assert_eq!(c.fingerprint_profile(), lumen_network::HttpProfile::Strict);
        assert_eq!(c.tls_profile(), lumen_network::TlsProfile::Strict);

        // Tor → TorBrowser HTTP + Tor TLS.
        BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Tor).unwrap();
        let c = s.build_http_client();
        assert_eq!(c.fingerprint_profile(), lumen_network::HttpProfile::TorBrowser);
        assert_eq!(c.tls_profile(), lumen_network::TlsProfile::Tor);
    }

    #[test]
    fn frozen_fingerprint_keeps_client_profile() {
        let mut s = InProcessSession::new();
        BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Strict).unwrap();
        s.context.freeze_fingerprint();
        // A change after freeze is rejected; the built client keeps Strict.
        assert!(BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Standard).is_err());
        assert_eq!(
            s.build_http_client().fingerprint_profile(),
            lumen_network::HttpProfile::Strict
        );
    }

    #[test]
    fn set_clock_frozen_stores_timestamp() {
        let mut sess = InProcessSession::new();
        lumen_core::ext::BrowserSession::set_clock(&mut sess, lumen_core::ClockMode::Frozen(1_700_000_000_000)).unwrap();
        assert_eq!(sess.context.frozen_clock_ms(), Some(1_700_000_000_000));
    }

    #[test]
    fn set_clock_real_clears_timestamp() {
        let mut sess = InProcessSession::new();
        lumen_core::ext::BrowserSession::set_clock(&mut sess, lumen_core::ClockMode::Frozen(42)).unwrap();
        lumen_core::ext::BrowserSession::set_clock(&mut sess, lumen_core::ClockMode::Real).unwrap();
        assert_eq!(sess.context.frozen_clock_ms(), None);
    }

    #[test]
    fn set_rng_seed_stores_and_clears() {
        let mut sess = InProcessSession::new();
        lumen_core::ext::BrowserSession::set_rng_seed(&mut sess, Some(12345)).unwrap();
        assert_eq!(sess.context.rng_seed(), Some(12345));
        lumen_core::ext::BrowserSession::set_rng_seed(&mut sess, None).unwrap();
        assert_eq!(sess.context.rng_seed(), None);
    }

    // ── Deterministic mode (8F) driver trait tests ────────────────────────────

    #[test]
    fn driver_set_clock_frozen() {
        let mut sess = InProcessSession::new();
        BrowserSession::set_clock(&mut sess, crate::ClockMode::Frozen(999_000)).unwrap();
        assert_eq!(sess.context.frozen_clock_ms(), Some(999_000));
    }

    #[test]
    fn driver_set_clock_real_clears() {
        let mut sess = InProcessSession::new();
        BrowserSession::set_clock(&mut sess, crate::ClockMode::Frozen(1)).unwrap();
        BrowserSession::set_clock(&mut sess, crate::ClockMode::Real).unwrap();
        assert_eq!(sess.context.frozen_clock_ms(), None);
    }

    #[test]
    fn driver_set_clock_monotonic_advances() {
        use lumen_core::ext::ClockMode;
        let mut sess = InProcessSession::new();
        BrowserSession::set_clock(&mut sess, ClockMode::Monotonic { step_ms: 10 }).unwrap();
        // First read returns 0.
        assert_eq!(sess.context.read_clock_ms(), Some(0));
        // Second read advances by step_ms.
        assert_eq!(sess.context.read_clock_ms(), Some(10));
        assert_eq!(sess.context.read_clock_ms(), Some(20));
    }

    #[test]
    fn driver_set_rng_seed() {
        let mut sess = InProcessSession::new();
        BrowserSession::set_rng_seed(&mut sess, Some(42)).unwrap();
        assert_eq!(sess.context.rng_seed(), Some(42));
        BrowserSession::set_rng_seed(&mut sess, None).unwrap();
        assert_eq!(sess.context.rng_seed(), None);
    }

    #[test]
    fn driver_freeze_fingerprint_locks_profile() {
        let mut sess = InProcessSession::new();
        BrowserSession::freeze_fingerprint(&mut sess, FingerprintProfile::Strict).unwrap();
        assert_eq!(sess.fingerprint_profile(), FingerprintProfile::Strict);
        // Profile is now locked — change must fail.
        assert!(BrowserSession::set_fingerprint_profile(&mut sess, FingerprintProfile::Standard).is_err());
    }

    #[test]
    fn driver_freeze_fingerprint_standard_profile() {
        let mut sess = InProcessSession::new();
        BrowserSession::freeze_fingerprint(&mut sess, FingerprintProfile::Standard).unwrap();
        assert_eq!(sess.fingerprint_profile(), FingerprintProfile::Standard);
        assert!(sess.context.is_fingerprint_frozen());
    }

    #[test]
    fn deterministic_config_apply() {
        use crate::determinism::DeterministicConfig;
        let mut sess = InProcessSession::new();
        let cfg = DeterministicConfig {
            clock: crate::ClockMode::Frozen(1234),
            rng_seed: Some(99),
            freeze_fingerprint: Some(FingerprintProfile::Strict),
        };
        cfg.apply(&mut sess).unwrap();
        assert_eq!(sess.context.frozen_clock_ms(), Some(1234));
        assert_eq!(sess.context.rng_seed(), Some(99));
        assert_eq!(sess.fingerprint_profile(), FingerprintProfile::Strict);
        assert!(sess.context.is_fingerprint_frozen());
    }
}
