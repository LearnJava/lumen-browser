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

use crate::{
    A11yNode, BoxModel, BrowserSession, ComputedProperties, ComputedStyleSnapshot,
    ConsoleEntry, InputCommand, NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
};

/// Встроенный шрифт Inter-Regular (SIL OFL 1.1).
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// Состояние после успешной загрузки страницы.
///
/// Содержит полный результат pipeline (parse → CSS → layout → paint),
/// включая GPU-специфичные данные для рендеринга через wgpu.
struct WinitSessionState {
    doc: Document,
    layout_root: LayoutBox,
    /// Display list for GPU rendering via wgpu.
    display_list: lumen_paint::DisplayList,
    /// Page title from `<title>` tag.
    title: Option<String>,
    /// Decoded images ready for GPU upload.
    images: Vec<(String, lumen_image::Image)>,
    /// Font provider for page-specific @font-face declarations.
    font_registry: std::sync::Arc<dyn lumen_core::FontProvider>,
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
    /// Current vertical scroll position in logical pixels.
    scroll_y: f32,
    /// Current horizontal scroll position in logical pixels.
    scroll_x: f32,
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
            scroll_y: 0.0,
            scroll_x: 0.0,
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
            scroll_y: 0.0,
            scroll_x: 0.0,
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
    /// Запустить полный pipeline (HTML parse -> CSS -> layout -> paint).
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

        // Build display list from layout tree.
        let display_list = lumen_paint::build_display_list(&layout_root);

        // Extract images from DOM and decode them.
        let images = extract_images(&doc);

        // Extract page title from <title> tag.
        let title = extract_title(&doc);

        // Create font_registry for @font-face declarations.
        let font_registry: std::sync::Arc<dyn lumen_core::FontProvider> =
            std::sync::Arc::new(lumen_font::SystemFontIndex::new());

        self.current_url = url;
        self.state = Some(Arc::new(Mutex::new(WinitSessionState {
            doc,
            layout_root,
            display_list,
            title,
            images,
            font_registry,
        })));
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
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

/// Извлечь текст из <title> элемента.
fn extract_title(doc: &lumen_dom::Document) -> Option<String> {
    walk_find_title(doc, doc.root())
}

fn walk_find_title(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> Option<String> {
    let node = doc.get(id);
    if let lumen_dom::NodeData::Element { name, .. } = &node.data
        && name.local == "title"
    {
        // Собрать весь текст внутри <title>
        let mut text = String::new();
        for &child in &node.children {
            if let lumen_dom::NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        return if text.is_empty() { None } else { Some(text) };
    }
    for &child in &node.children {
        if let Some(title) = walk_find_title(doc, child) {
            return Some(title);
        }
    }
    None
}


/// Извлечь все <img> элементы из документа и их src.
fn extract_images(doc: &lumen_dom::Document) -> Vec<(String, lumen_image::Image)> {
    let mut images = Vec::new();
    walk_collect_images(doc, doc.root(), &mut images);
    images
}

fn walk_collect_images(
    doc: &lumen_dom::Document,
    id: lumen_dom::NodeId,
    images: &mut Vec<(String, lumen_image::Image)>,
) {
    let node = doc.get(id);
    if let lumen_dom::NodeData::Element { name, attrs, .. } = &node.data
        && name.local == "img"
    {
        // Найти src атрибут
        if let Some(src) = attrs.iter().find_map(|attr| {
            if attr.name.local == "src" {
                Some(attr.value.clone())
            } else {
                None
            }
        }) {
            // Для Phase 4b: пока просто используем пустой Image placeholder.
            // В Phase 4c будет реальная загрузка и декодирование изображений.
            let placeholder = lumen_image::Image {
                width: 1,
                height: 1,
                format: lumen_image::PixelFormat::Rgba8,
                data: vec![0, 0, 0, 255], // 1x1 transparent pixel
                icc_profile: None,
            };
            images.push((src, placeholder));
        }
    }
    for &child in &node.children {
        walk_collect_images(doc, child, images);
    }
}

// ── Вспомогательные функции ─────────────────────────────────────────────────

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
    let mt = lb.style.margin_top.to_px_opt().unwrap_or(0.0);
    let mr = lb.style.margin_right.to_px_opt().unwrap_or(0.0);
    let mb = lb.style.margin_bottom.to_px_opt().unwrap_or(0.0);
    let ml = lb.style.margin_left.to_px_opt().unwrap_or(0.0);
    let margin_box = lumen_core::geom::Rect {
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
#[derive(Debug, Default)]
struct SimpleSelector<'a> {
    tag: Option<&'a str>,
    id: Option<&'a str>,
    classes: Vec<&'a str>,
}

fn parse_simple_selector(s: &str) -> SimpleSelector<'_> {
    let mut sel = SimpleSelector::default();
    let mut rest = s;

    let tag_end = rest.find(['#', '.']).unwrap_or(rest.len());
    if tag_end > 0 {
        sel.tag = Some(&rest[..tag_end]);
    }
    rest = &rest[tag_end..];

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

/// Построить accessibility-дерево из DOM-структуры.
fn build_a11y_node(doc: &Document, id: NodeId) -> A11yNode {
    let node = doc.get(id);
    let (role, name) = match &node.data {
        NodeData::Element { name, attrs } => {
            let role = aria_role_for_tag(name.local.as_ref());
            let label = attrs
                .iter()
                .find(|a| a.name.local.eq_ignore_ascii_case("aria-label"))
                .or_else(|| attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case("alt")))
                .map(|a| a.value.clone())
                .unwrap_or_default();
            (role, label)
        }
        NodeData::Text(s) => ("text".into(), s.clone()),
        NodeData::Document => ("document".into(), String::new()),
        _ => (String::new(), String::new()),
    };

    let children = node
        .children
        .clone()
        .into_iter()
        .map(|child| build_a11y_node(doc, child))
        .filter(|n| !n.role.is_empty())
        .collect();

    A11yNode { role, name, children }
}

fn aria_role_for_tag(tag: &str) -> String {
    match tag {
        "button" => "button",
        "a" => "link",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
        "input" => "textbox",
        "img" => "img",
        "ul" | "ol" => "list",
        "li" => "listitem",
        "nav" => "navigation",
        "main" => "main",
        "header" => "banner",
        "footer" => "contentinfo",
        "section" | "article" => "region",
        "table" => "table",
        "tr" => "row",
        "td" | "th" => "cell",
        "form" => "form",
        "p" | "div" | "span" | "body" | "html" | "head" => "generic",
        _ => "",
    }
    .into()
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

/// Разрешить `Target` в координату точки клика (центр элемента или явная точка).
fn resolve_target_point(state: &WinitSessionState, target: &Target) -> Result<(f32, f32)> {
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

impl BrowserSession for WinitSession {
    // ── Ресурсы ────────────────────────────────────────────────────────────

    fn screenshot(&self) -> Result<Vec<u8>> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;

        let display_list = lumen_paint::build_display_list(&state.layout_root);
        let width = self.viewport.width as u32;
        let height = self.viewport.height as u32;
        let mut renderer = lumen_paint::Renderer::new_headless(INTER_FONT.to_vec(), width, height)
            .map_err(|e| Error::Other(format!("headless renderer: {e}")))?;

        let image = renderer
            .render_to_image(&display_list, 0.0, 0.0)
            .map_err(|e| Error::Other(format!("render_to_image: {e}")))?;

        lumen_image::encode_png_rgba8(&image)
            .map_err(|e| Error::Other(format!("PNG encoding: {e}")))
    }

    fn a11y_tree(&self) -> Result<A11yNode> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        Ok(build_a11y_node(&state.doc, state.doc.root()))
    }

    fn layout_snapshot(&self) -> Result<Vec<BoxModel>> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        let mut out = Vec::new();
        collect_boxes(&state.layout_root, &state.doc, &mut out);
        Ok(out)
    }

    fn computed_style(&self, selector: &str) -> Result<Option<ComputedProperties>> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        let Some(node_id) = find_first_by_selector(&state.doc, selector) else {
            return Ok(None);
        };
        let Some(lb) = find_layout_box(&state.layout_root, node_id) else {
            return Ok(None);
        };
        Ok(Some(style_to_properties(&lb.style)))
    }

    fn computed_style_snapshot(&self, selector: &str) -> Result<Option<ComputedStyleSnapshot>> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        Ok(lumen_layout::computed_style_by_selector(
            &state.layout_root,
            &state.doc,
            selector,
        ))
    }

    fn layout_box_by_selector(&self, selector: &str) -> Result<Option<BoxModel>> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        let Some(lb) = lumen_layout::find_box_by_selector(&state.layout_root, &state.doc, selector)
        else {
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
        let margin_box = lumen_core::geom::Rect {
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
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
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
            let margin_box = lumen_core::geom::Rect {
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
            let sink = std::sync::Arc::new(lumen_core::ext::NoopEventSink);
            let client = lumen_network::HttpClient::new()
                .with_sink(sink)
                .with_content_decoder(std::sync::Arc::new(
                    lumen_network::BrotliContentDecoder::new(),
                ));
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
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        let (x, y) = resolve_target_point(&state, target)?;

        // Phase 1 (8C): native input injection для mouse click.
        //
        // Требуется: WinitSessionHandler integration с event loop для обработки InputCommand.
        // Текущая реализация: заглушка (проверяет что элемент виден по координатам).
        // Полная реализация: injekt MouseClick в event loop → hit-test → JS dispatch с isTrusted=true.
        let _cmd = InputCommand::MouseClick { x, y };
        // TODO: enqueue в event loop через channel Sender<InputCommand>

        Ok(())
    }

    fn type_text(&mut self, target: &Target, text: &str) -> Result<()> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
        let _point = resolve_target_point(&state, target)?;

        // Phase 1 (8C): native input injection для keyboard input.
        //
        // Требуется: WinitSessionHandler integration с event loop для обработки InputCommand.
        // Текущая реализация: заглушка (проверяет что target найден).
        // Полная реализация: injekt KeyPress per char в event loop → input с isTrusted=true.
        for ch in text.chars() {
            let _cmd = InputCommand::KeyPress { char: ch };
            // TODO: enqueue в event loop через channel Sender<InputCommand>
        }
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
        // WinitSession получит его через задачу 8A.7 полную интеграцию.
        Err(Error::Other(
            "eval доступен после интеграции persistent JS runtime".into(),
        ))
    }

    fn query(&self, selector: &str) -> Result<Vec<NodeRef>> {
        let state = self.state()?;
        let state = state.lock().map_err(|e| Error::Other(format!("mutex: {e}")))?;
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
                .unwrap_or(lumen_core::geom::Rect::ZERO);
            out.push(NodeRef {
                node_id: id.index() as u32,
                tag_name,
                text_content,
                bounding_rect,
            });
        }
        Ok(out)
    }
}

impl WinitSession {
    /// Проверить выполнение условия ожидания.
    fn check_wait_condition(&self, cond: &WaitCondition) -> Result<bool> {
        match cond {
            WaitCondition::DocumentReady => {
                // После navigate() state установлен и document готов.
                Ok(self.state.is_some())
            }
            WaitCondition::Visible(selector) => {
                // Проверить что элемент присутствует в layout и видим.
                // В Phase 1 WinitSession: если layout_box существует, элемент видим
                // (нет CSS-скрытых элементов до полной реализации CSS properties).
                self.layout_box_by_selector(selector).map(|opt| opt.is_some())
            }
            WaitCondition::Stable(selector) => {
                // Стабильность layout: проверить что элемент есть в layout.
                // Для Phase 1: если элемент есть, то layout стабилен
                // (нет animation/JS изменений в Phase 1). Реальная проверка стабильности —
                // отслеживание layout-change ticks (ADR-008 §15) — в следующих фазах.
                Ok(self.layout_box_by_selector(selector)?.is_some())
            }
            WaitCondition::NetworkIdle => {
                // Нет активных network запросов. Для Phase 1: все fetch() синхронны и
                // завершаются до возврата navigate(). Поэтому всегда idle.
                // (Реальная проверка через NetworkTransport events + event sink — задача 8D.2)
                Ok(true)
            }
            WaitCondition::JsIdle => {
                // JS event loop пуст: нет pending microtasks, tasks, rAF.
                // Для Phase 1: нет JS engine (persistent-js-runtime — задача 8A.7).
                // Поэтому всегда idle.
                Ok(true)
            }
        }
    }
}

impl crate::GpuSession for WinitSession {
    fn render_to_gpu(&mut self) -> crate::Result<crate::RenderedPage> {
        let state = self.state()?;
        let state_locked = state.lock()
            .map_err(|e| Error::Other(format!("mutex: {e}")))?;

        Ok(crate::RenderedPage {
            display_list: state_locked.display_list.clone(),
            title: state_locked.title.clone(),
            images: state_locked.images.clone(),
            layout_box: state_locked.layout_root.clone(),
            font_registry: state_locked.font_registry.clone(),
            js_navigate: None, // Phase 4 doesn't support JS navigation yet (задача 8D)
        })
    }

    fn set_scroll(&mut self, delta: crate::ScrollDelta) -> crate::Result<bool> {
        // Apply relative scroll delta to current position (default behavior for ScrollDelta struct)
        self.scroll_x += delta.x;
        self.scroll_y += delta.y;
        // Phase 4: no relayout needed for scroll in headless (задача 4c)
        Ok(false)
    }

    fn scroll_position(&self) -> (f32, f32) {
        (self.scroll_x, self.scroll_y)
    }

    fn viewport_size(&self) -> lumen_core::geom::Size {
        self.viewport
    }

    fn set_viewport(&mut self, width: f32, height: f32) -> crate::Result<bool> {
        let old_size = self.viewport;
        self.viewport = lumen_core::geom::Size::new(width, height);

        // Check if viewport actually changed
        if old_size != self.viewport {
            // Phase 4: relayout would be needed if page was already loaded
            // For now, just return false since we don't have state
            Ok(self.state.is_some())
        } else {
            Ok(false)
        }
    }

    fn navigate_streaming<F>(&mut self, url: &str, _on_chunk: F) -> crate::Result<()>
    where
        F: FnMut(crate::RenderedPage),
    {
        // Phase 4: streaming is not yet implemented (требует event-loop).
        // For now, just do a normal navigate.
        self.navigate(url)?;
        Ok(())
    }
}
