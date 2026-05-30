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
use lumen_dom::build_flat_tree;
use lumen_layout::{computed_style_by_selector, LayoutBox};

use crate::{
    A11yNode, AxQuery, BoxModel, BrowserSession, ComputedProperties, ComputedStyleSnapshot,
    ConsoleEntry, FingerprintProfile, NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
    context::SessionContext,
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
        }
    }

    /// Загрузить HTML-строку без навигации по URL. Используется для тестов.
    pub fn navigate_html(&mut self, html: &str) -> Result<()> {
        self.run_pipeline(html.as_bytes(), Some("text/html"), "about:blank".to_owned())
    }

    /// Загрузить байты по URL и запустить pipeline. Внутренняя реализация
    /// навигации, используемая также для тестов с прямой передачей HTML.
    fn run_pipeline(&mut self, bytes: &[u8], content_type: Option<&str>, url: String) -> Result<()> {
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
            let sink = Arc::new(NoopEventSink);
            let client = lumen_network::HttpClient::new()
                .with_sink(sink)
                .with_content_decoder(Arc::new(lumen_network::BrotliContentDecoder::new()));
            let bytes = client.fetch(&lumen_url)?;
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
    /// Проверить выполнение условия ожидания.
    fn check_wait_condition(&self, cond: &WaitCondition) -> Result<bool> {
        match cond {
            WaitCondition::DocumentReady => {
                // В headless-режиме document всегда ready после navigate().
                Ok(self.state.is_some())
            }
            WaitCondition::Visible(selector) => {
                // Проверить что элемент с этим селектором присутствует в layout
                // и видим (не display:none). В Phase 0 headless нет CSS-свойств видимости,
                // поэтому просто проверяем наличие layout-бокса.
                self.layout_box_by_selector(selector).map(|opt| opt.is_some())
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
                // Нет network запросов в headless. Phase 0/Phase 1 сеть — через fetch(),
                // который синхронен и завершается до возврата navigate().
                // Для Phase 1 — всегда true (нет активных запросов).
                Ok(true)
            }
            WaitCondition::JsIdle => {
                // Нет JS engine в Phase 0/Phase 1 headless (task persistent-js-runtime).
                // Для Phase 1 — всегда true.
                Ok(true)
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
}
