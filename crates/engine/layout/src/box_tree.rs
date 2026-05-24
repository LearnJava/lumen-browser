//! Box tree: block-флоу + inline-флоу.
//!
//! Каждый DOM-элемент даёт один LayoutBox. Блочные элементы стэкаются
//! вертикально. Текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`,
//! `<strong>`, и т.д.) объединяются в `InlineRun` — анонимный бокс, в
//! котором слова переносятся как единый поток. Слова с одинаковым стилем
//! на одной строке объединяются в один фрагмент (→ один DrawText).
//!
//! Whitespace-only текст и комментарии пропускаются.

use lumen_core::geom::{Rect, Size};
use lumen_core::ext::{HyphenationProvider, NullHyphenationProvider};
use lumen_css_parser::Stylesheet;
use lumen_dom::{build_flat_tree, Document, FlatTree, NodeData, NodeId};
use lumen_html_parser::{
    PictureParams, SizesViewport, pick_img_source, pick_picture_source,
};

use crate::style::{
    apply_container_rules, clear_cq_context, compute_pseudo_element_style, compute_style,
    set_cq_context, AlignValue,
    BackgroundImage, BoxSizing, ClearSide, ContainFlags, ContainerContext, ContainerType, Content,
    ContentItem, ComputedStyle, Direction, Display, FlexBasis, FlexDirection, FlexWrap, FloatSide,
    GridAutoFlow, GridLine, GridTrackSize, Hyphens, Length, LengthOrAuto, ListStylePosition,
    ListStyleType, Overflow, OverflowWrap, Position, TextAlign, TextOverflow, TextWrapMode,
    VerticalAlign, WordBreak,
};
use crate::TextMeasurer;

/// HTML-имя элемента `<img>` для распознавания replaced-боксов в layout.
/// Tag-name в DOM хранится lower-case (HTML5 tree-builder), поэтому
/// сравнение точное, без `eq_ignore_ascii_case`.
fn is_image_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "img"
    )
}

/// HTML-имя `<picture>` — обёртка над `<source>`-кандидатами и одним
/// `<img>`-fallback-ом. Сам по себе пиктур ничего не рендерит, его
/// единственная роль — переадресовать source-selection на inner `<img>`.
fn is_picture_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "picture"
    )
}

/// Вид form control — используется в `BoxKind::FormControl` для paint-специализаций
/// (фокус-рамка, checkbox/radio indicator, placeholder, стрелка select и т.д.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormControlKind {
    /// `<input>` — carries input type (from `type` attribute) and initial
    /// checked state (from presence of `checked` attribute in DOM). Paint uses
    /// this to draw checkbox/radio indicators without re-querying the DOM.
    Input { input_type: lumen_dom::InputType, checked: bool },
    Button,
    Select,
    Textarea,
}

/// Является ли DOM-узел HTML form control-ом.
/// Tag-name хранится lower-case (HTML5 tree-builder).
fn is_form_control_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. }
            if matches!(name.local.as_str(), "input" | "button" | "select" | "textarea")
    )
}

/// Финальный URL картинки + author-объявленные intrinsic dimensions.
/// Заполняется `resolve_image_source` ниже — это адаптер `PickedSource`
/// из `lumen-html-parser`, плюс legacy-fallback на голый `src`-атрибут
/// для битых страниц, у которых picker отказал.
struct ImageSource {
    url: String,
    intrinsic_width: Option<u32>,
    intrinsic_height: Option<u32>,
}

/// Запрос на предзагрузку изображения: URL после picking-а по
/// `<picture>`/`srcset`/`sizes` плюс признаки явного задания размеров
/// author-ом (нужны shell для `apply_intrinsic_size`).
pub struct ImageRequest {
    pub node_id: NodeId,
    pub url: String,
    pub has_explicit_width: bool,
    pub has_explicit_height: bool,
}

/// Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов.
/// URL выбирается через тот же picker, что layout использует при построении
/// `BoxKind::Image { src }` — гарантирует совпадение ключей в
/// `Renderer::register_image` и `DisplayCommand::DrawImage.src`.
pub fn collect_image_requests(doc: &Document, viewport: Size) -> Vec<ImageRequest> {
    let mut out = Vec::new();
    collect_requests_inner(doc, doc.root(), viewport, &mut out);
    out
}

/// Обходит готовое layout-дерево и возвращает уникальные URL-ы из
/// `background-image: url(...)` (CSS Backgrounds L3 §3.10) — те же ключи,
/// что эмиттер кладёт в `DisplayCommand::DrawBackgroundImage.src`.
///
/// Background-image не участвует в расчёте размеров, поэтому собирается
/// уже после layout — shell вызывает функцию между layout-ом и paint-ом,
/// дозагружает байты и регистрирует через `Renderer::register_image`.
///
/// Возвращает `Vec<String>` (а не `Vec<ImageRequest>`): для background-image
/// нет node-anchored intrinsic-size hint-ов (CSS Backgrounds L3 §3.9 говорит
/// о `background-size` в стилях, intrinsic-размер картинки в layout не
/// влияет). Дубликаты отфильтрованы — одна и та же картинка на разных
/// элементах загружается один раз.
#[must_use]
pub fn collect_background_image_requests(root: &LayoutBox) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_bg_image_inner(root, &mut out);
    out
}

fn collect_bg_image_inner(b: &LayoutBox, out: &mut Vec<String>) {
    if let BackgroundImage::Url(src) = &b.style.background_image
        && !src.is_empty()
        && !out.iter().any(|u| u == src)
    {
        out.push(src.clone());
    }
    for child in &b.children {
        collect_bg_image_inner(child, out);
    }
}

fn collect_requests_inner(doc: &Document, id: NodeId, viewport: Size, out: &mut Vec<ImageRequest>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "img"
    {
        let has_explicit_width = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
        let has_explicit_height =
            attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
        let source = resolve_image_source(doc, id, viewport);
        if !source.url.is_empty() {
            out.push(ImageRequest {
                node_id: id,
                url: source.url,
                has_explicit_width,
                has_explicit_height,
            });
        }
        return; // void element — нет children
    }
    for &child in &node.children {
        collect_requests_inner(doc, child, viewport, out);
    }
}

/// Выбрать источник для `<img>`-элемента с учётом окружающего контекста:
///  1. Если parent — `<picture>`, прогоняем picture-picker
///     (выбирает `<source>` или fallback на `<img>` по `media`/`type`/
///     `srcset`/`sizes`).
///  2. Иначе — `<img>`-picker, учитывающий собственный `srcset`/`sizes`/`src`.
///  3. Если оба picker-а вернули `None` (нет ни `srcset`, ни `src`) —
///     fallback на голый `src` атрибут как раньше: для битой разметки
///     лучше отрисовать пустую коробку, чем ничего.
///
/// Phase 0: DPR=1.0 (layout не знает про device pixel ratio renderer-а —
/// это интегрирует P3 при relayout-on-resize), фильтр MIME-типов
/// выключен (`supported_types: None`), `prefers_dark` = false. Когда
/// эти значения появятся в layout-pipeline — заменим без изменения
/// сигнатуры picker-ов.
fn resolve_image_source(doc: &Document, img_id: NodeId, viewport: Size) -> ImageSource {
    let sizes_vp = SizesViewport {
        width_px: viewport.width,
        height_px: viewport.height,
        root_font_size_px: 16.0,
        prefers_dark: false,
    };
    let params = PictureParams { viewport: sizes_vp, dpr: 1.0, supported_types: None };

    if let Some(parent_id) = doc.get(img_id).parent
        && is_picture_element(doc, parent_id)
        && let Some(picked) = pick_picture_source(doc, parent_id, &params)
    {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    if let Some(picked) = pick_img_source(doc, img_id, sizes_vp, params.dpr) {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    let raw_src = doc.get(img_id).get_attr("src").unwrap_or("").to_string();
    ImageSource { url: raw_src, intrinsic_width: None, intrinsic_height: None }
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: NodeId,
    /// Border-box rectangle: (x, y) is the top-left corner after margin,
    /// (width, height) includes padding + border but NOT margin.
    pub rect: Rect,
    pub style: ComputedStyle,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
}

/// Отрезок inline-контента с собственным стилем (до layout).
#[derive(Debug, Clone)]
pub struct InlineSegment {
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved px space before this segment's first word:
    /// margin_left + border_left_width + padding_left of the inline element.
    pub pre_space: f32,
    /// Resolved px space after this segment's last word:
    /// padding_right + border_right_width + margin_right of the inline element.
    pub post_space: f32,
    /// True when this segment comes from inside an inline element box
    /// (not anonymous text directly in a block container). Used by the painter
    /// to know whether to draw the element's own background/border.
    pub is_element_box: bool,
    /// Non-None when this segment is an inline-replaced `<img>`. Contains the
    /// resolved image URL. `text` holds the alt attribute.
    pub img_src: Option<String>,
    /// Pre-computed pixel width for image segments (0.0 for text segments).
    pub img_width: f32,
    /// True when this segment represents a forced line break (CSS §4.1: newline
    /// in white-space: pre / pre-wrap text). `text` is empty in this case.
    pub forced_break: bool,
}

/// Позиционированный текстовый фрагмент в строке (после layout).
/// `x` — смещение от левого края inline-контейнера до начала ТЕКСТА
/// (после border+padding inline-элемента слева).
/// `width` — ширина текста фрагмента в пикселях.
/// `padding_left` / `padding_right` — разрешённые px padding-а inline-элемента
/// для этого фрагмента (ненулевые только для первого/последнего слова сегмента).
#[derive(Debug, Clone)]
pub struct InlineFrag {
    pub x: f32,
    pub width: f32,
    /// Vertical offset within the line box (CSS vertical-align). Positive = down.
    pub y_offset: f32,
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved padding_left of this frag's inline box start (0 if not a box start).
    pub padding_left: f32,
    /// Resolved padding_right of this frag's inline box end (0 if not a box end).
    pub padding_right: f32,
    /// True when this frag comes from an inline element box (not anonymous text).
    /// Used by the painter to draw element background/border.
    pub is_element_box: bool,
    /// Non-None when this frag represents an inline-replaced `<img>`.
    /// `text` holds the alt attribute; `width` is the rendered pixel width.
    pub img_src: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BoxKind {
    /// Block-уровневый бокс (элемент или корень документа).
    Block,
    /// Анонимный контейнер для потока inline-контента (текст + inline-элементы).
    /// `segments` — сырые отрезки до lay_out; `lines` — позиционированные строки
    /// после lay_out. Каждая строка — `Vec<InlineFrag>`.
    InlineRun {
        segments: Vec<InlineSegment>,
        lines: Vec<Vec<InlineFrag>>,
    },
    /// Анонимный контейнер для горизонтального потока `display: inline-block`
    /// элементов. Сами дочерние боксы хранятся в `LayoutBox.children`. При
    /// layout дети раскладываются горизонтально слева направо; высота строки
    /// = высота самого высокого дочернего элемента.
    InlineBlockRow,
    /// Replaced element: изображение (`<img>`). В Phase 0 — block-level
    /// (одна картинка занимает свою строку). `src` — путь / URL ресурса
    /// (декодирование откладывается на следующий шаг), `alt` — alternate-текст
    /// для отображения и AT, размеры берутся из `style.width`/`style.height`
    /// (которые могут происходить из CSS или HTML-атрибутов как
    /// presentational hints). Inline-replaced в InlineRun-е — отдельная задача.
    Image {
        src: String,
        alt: String,
    },
    /// Replaced element: HTML form control (`<input>`, `<button>`, `<select>`,
    /// `<textarea>`). Phase 0: block-level replaced. Размеры берутся из
    /// `style.width`/`style.height` (UA defaults из `apply_ua_form_controls`).
    /// `kind` зарезервирован для paint-специализаций в следующих фазах.
    FormControl {
        kind: FormControlKind,
    },
    /// CSS 2.1 §17 — строка таблицы (`display: table-row`). Дочерние
    /// боксы — ячейки (`display: table-cell`), которые раскладываются
    /// горизонтально слева направо. Высота строки = max высота ячейки.
    TableRow,
    /// Схлопнутый межэлементный пробел в InlineBlockRow.
    /// Не рисуется; участвует только как горизонтальный gap между
    /// inline-block соседями (CSS white-space collapsing §4.1.2).
    InlineSpace,
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
    /// CSS Lists L3 §2.1 — `::marker` pseudo-element for `display: list-item`.
    /// `text` — marker string for counter types (1., a., i., …); empty for bullet
    /// types (disc/circle/square) which are rendered as geometric shapes.
    /// `position` — inside/outside flow. `list_style_type` — used by the display-list
    /// emitter to choose geometric (disc/circle/square) vs text rendering.
    /// For `outside` (default) positioned left of the principal block, out of flow.
    Marker {
        text: String,
        position: ListStylePosition,
        list_style_type: ListStyleType,
    },
    /// CSS Display L3 §8 — `display: flow-root`. Establishes a Block Formatting
    /// Context: contains floats, prevents margin escape. Laid out identically to
    /// Block in Phase 0; BFC float-containment wired when float layout is added.
    /// CSS: flow-root
    FlowRoot,
    /// CSS Display L3 §7.2 — `display: contents`. The element itself generates no
    /// box. Children are flattened into the parent's formatting context by
    /// `flatten_contents()` during `build_box`. Must never appear in the final
    /// layout tree that reaches `lay_out`.
    Contents,
    /// CSS 2.1 §17 — table container (`display: table` / `display: inline-table`).
    /// Direct children are `TableRowGroup` or `TableRow` boxes. Layout computes
    /// global column widths across all rows before positioning each row.
    Table,
    /// CSS 2.1 §17 — row group (`display: table-row-group`, `table-header-group`,
    /// `table-footer-group`). Rendered as a transparent wrapper; rows inside are
    /// collected by the parent `Table` box during column-width computation.
    TableRowGroup,
}

pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let null_hp = NullHyphenationProvider;
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), None, viewport, init_pcb, &null_hp);
    // CSS Container Queries L1: second pass applies @container rules + re-layout.
    apply_container_styles(&mut root, doc, sheet, viewport, None, &null_hp);
    root
}

pub fn layout_measured(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    let null_hp = NullHyphenationProvider;
    layout_measured_hyp(doc, sheet, viewport, measurer, &null_hp)
}

/// Layout with a real hyphenation provider (for `hyphens: auto`).
pub fn layout_measured_hyp(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp);
    apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp);
    root
}

/// CSS Backgrounds L3 §2.11.2 — «The Canvas Background and the Root Element»:
/// если у root-элемента (`<html>`) нет собственного фона
/// (`background-color: transparent` И `background-image: none`), фон
/// `<body>` пропагируется на root box, а у `<body>` обнуляется. Это
/// покрывает legacy-страницы `body { background: red }`, где иначе фон
/// рисуется только в пределах body box-а и не достигает viewport-а
/// сверху / снизу.
///
/// Phase 0: переносим только два longhand-а — `background-color` и
/// `background-image`. Остальные `background-*` longhand-ы у body без
/// image не имеют визуального эффекта и сейчас не propagated; при
/// добавлении реального paint pattern fill-а их тоже нужно будет
/// перенести.
///
/// Structure: `doc.root()` — Document-узел; его ребёнок — `<html>`
/// element. Body — прямой ребёнок `<html>`. SVG / MathML root-ы пока не
/// учитываются (spec упоминает их отдельно).
fn propagate_canvas_background(doc: &Document, root: &mut LayoutBox) {
    let html_idx = root
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "html"));
    let Some(html_idx) = html_idx else {
        return;
    };

    let html_box = &mut root.children[html_idx];
    let html_has_bg = html_box.style.background_color.is_some()
        || !matches!(html_box.style.background_image, BackgroundImage::None);
    if html_has_bg {
        return;
    }

    let body_idx = html_box
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "body"));
    let Some(body_idx) = body_idx else {
        return;
    };

    let body = &mut html_box.children[body_idx];
    let body_has_bg = body.style.background_color.is_some()
        || !matches!(body.style.background_image, BackgroundImage::None);
    if !body_has_bg {
        return;
    }

    let bg_color = body.style.background_color.take();
    let bg_image = std::mem::replace(&mut body.style.background_image, BackgroundImage::None);
    html_box.style.background_color = bg_color;
    html_box.style.background_image = bg_image;
}

fn is_html_element_named(doc: &Document, id: NodeId, want: &str) -> bool {
    matches!(
        doc.get(id).element_name(),
        Some(q) if q.local.eq_ignore_ascii_case(want)
    )
}

/// Является ли DOM-узел inline-контентом (non-whitespace текст или inline-элемент).
///
/// `<img>` в Phase 0 — block-level replaced element, не inline-контент:
/// он порождает собственный `BoxKind::Image`, а не вливается в `InlineRun`.
/// Inline-replaced (картинка внутри строки текста) — отдельная задача;
/// до неё `<img>` всегда занимает свою строку, как `<div>`.
fn is_inline_content(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
) -> bool {
    match &doc.get(id).data {
        NodeData::Text(s) => !s.chars().all(char::is_whitespace),
        NodeData::Element { .. } => {
            if is_image_element(doc, id) || is_form_control_element(doc, id) {
                return false;
            }
            // Inline-семантика: чистый `inline` или его flex/grid-варианты.
            // Phase 0 layout не делает реального flex/grid — флэт-семантика
            // блока для outer-display, но inline-family остаётся inline.
            matches!(
                compute_style(doc, id, sheet, inherited, viewport).display,
                Display::Inline | Display::InlineFlex | Display::InlineGrid
            )
        }
        _ => false,
    }
}

/// Является ли DOM-узел `display: inline-block` элементом.
/// Возвращает false для изображений (`<img>` — replaced element).
fn is_inline_block(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { .. }
        if !is_image_element(doc, id)
            && !is_form_control_element(doc, id)
            && compute_style(doc, id, sheet, inherited, viewport).display
                == Display::InlineBlock
    )
}

/// Обнуляет box-model spacing анонимного контейнера (InlineRun / InlineBlockRow).
fn anon_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut s = parent.clone();
    s.margin_top = LengthOrAuto::ZERO;
    s.margin_right = LengthOrAuto::ZERO;
    s.margin_bottom = LengthOrAuto::ZERO;
    s.margin_left = LengthOrAuto::ZERO;
    s.padding_top = Length::Px(0.0);
    s.padding_right = Length::Px(0.0);
    s.padding_bottom = Length::Px(0.0);
    s.padding_left = Length::Px(0.0);
    s.background_color = None;
    s.width = None;
    s.height = None;
    s.min_width = None;
    s.max_width = None;
    s.min_height = None;
    s.max_height = None;
    s.border_top_width = 0.0;
    s.border_right_width = 0.0;
    s.border_bottom_width = 0.0;
    s.border_left_width = 0.0;
    s.box_sizing = BoxSizing::ContentBox;
    s
}

fn anon_inline_run(node: NodeId, parent: &ComputedStyle, segs: Vec<InlineSegment>) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: anon_style(parent),
        kind: BoxKind::InlineRun { segments: segs, lines: vec![] },
        children: vec![],
    }
}

fn anon_inline_block_row(node: NodeId, parent: &ComputedStyle, items: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: anon_style(parent),
        kind: BoxKind::InlineBlockRow,
        children: items,
    }
}

/// Рекурсивно собирает `InlineSegment`-ы из поддерева inline-контента.
fn collect_inline_segments(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    out: &mut Vec<InlineSegment>,
    flat: &FlatTree,
) {
    match &doc.get(id).data {
        NodeData::Text(s) if inherited.white_space.preserves_whitespace() => {
            // CSS Text L3 §4.1: white-space: pre/pre-wrap — preserve tabs and
            // newlines. Split on \n to produce forced-break segments.
            let style = inherited.clone();
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    out.push(InlineSegment {
                        text: String::new(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_width: 0.0,
                        forced_break: true,
                    });
                }
                if !line.is_empty() {
                    out.push(InlineSegment {
                        text: line.to_string(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_width: 0.0,
                        forced_break: false,
                    });
                }
            }
        }
        NodeData::Text(s) if !s.chars().all(char::is_whitespace) => {
            // text-transform применяется здесь, до wrapping и paint —
            // measurer считает ширину уже после преобразования.
            let text = inherited.text_transform.apply(s);
            out.push(InlineSegment {
                text,
                style: inherited.clone(),
                pre_space: 0.0,
                post_space: 0.0,
                is_element_box: false,
                img_src: None,
                img_width: 0.0,
                forced_break: false,
            });
        }
        NodeData::Text(_) => {}
        NodeData::Element { .. } => {
            let s = compute_style(doc, id, sheet, inherited, viewport);
            if s.display == Display::None {
                return;
            }
            // Inline-replaced image: emit as a fixed-width, non-breakable segment.
            if is_image_element(doc, id) {
                let src = resolve_image_source(doc, id, viewport);
                let em = s.font_size;
                let w = s.width
                    .as_ref()
                    .and_then(|l| l.resolve(em, None, viewport))
                    .or_else(|| src.intrinsic_width.map(|v| v as f32))
                    .unwrap_or(em * 2.0);
                let pre = s.margin_left.resolve_or_zero(em, 0.0, viewport)
                    + s.border_left_width
                    + s.padding_left.resolve_or_zero(em, 0.0, viewport);
                let post = s.padding_right.resolve_or_zero(em, 0.0, viewport)
                    + s.border_right_width
                    + s.margin_right.resolve_or_zero(em, 0.0, viewport);
                let alt = doc.get(id).get_attr("alt").unwrap_or("").to_string();
                out.push(InlineSegment {
                    text: alt,
                    style: s,
                    pre_space: pre,
                    post_space: post,
                    is_element_box: true,
                    img_src: Some(src.url),
                    img_width: w,
                    forced_break: false,
                });
                return;
            }
            // Compute horizontal inline box model: margin + border + padding.
            // Use em=font_size, cb=0 (% padding on inline elements is uncommon).
            let em = s.font_size;
            let pre = s.margin_left.resolve_or_zero(em, 0.0, viewport)
                + s.border_left_width
                + s.padding_left.resolve_or_zero(em, 0.0, viewport);
            let post = s.padding_right.resolve_or_zero(em, 0.0, viewport)
                + s.border_right_width
                + s.margin_right.resolve_or_zero(em, 0.0, viewport);
            let start = out.len();
            // CSS Pseudo-elements L4 §4 — ::before in inline formatting context.
            // Block pseudo-elements inside inline context are skipped (Phase 0).
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "before", sheet, &s, viewport)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, viewport, out);
            }
            let children: Vec<NodeId> = flat.children_of(doc, id).to_vec();
            for child_id in children {
                collect_inline_segments(doc, sheet, child_id, &s, viewport, out, flat);
            }
            // CSS Pseudo-elements L4 §4 — ::after in inline formatting context.
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "after", sheet, &s, viewport)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, viewport, out);
            }
            let added = out.len() - start;
            // Mark all segments from this element (including pseudo-element content)
            // as element boxes so the painter draws their background/border.
            for seg in &mut out[start..start + added] {
                seg.is_element_box = true;
            }
            if added > 0 && (pre > 0.0 || post > 0.0) {
                out[start].pre_space += pre;
                out[start + added - 1].post_space += post;
            }
        }
        _ => {}
    }
}

/// Injects a pseudo-element box (::before or ::after) into the children list.
///
/// `is_before = true` → prepend; `false` → append.
/// Inline pseudo-elements are merged into the adjacent InlineRun when possible.
/// Block pseudo-elements are inserted as separate Block boxes.
fn inject_pseudo(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    ps: Option<ComputedStyle>,
    is_before: bool,
) {
    let Some(ps) = ps else { return };
    match ps.display {
        Display::Inline
        | Display::InlineFlex
        | Display::InlineGrid
        | Display::InlineBlock => {
            let segs = content_to_inline_segments(&ps);
            if segs.is_empty() {
                return;
            }
            if is_before {
                match children.first_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        let mut new_segs = segs;
                        new_segs.extend(std::mem::take(segments));
                        *segments = new_segs;
                    }
                    _ => children.insert(0, anon_inline_run(parent_id, &ps, segs)),
                }
            } else {
                match children.last_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        segments.extend(segs);
                    }
                    _ => children.push(anon_inline_run(parent_id, &ps, segs)),
                }
            }
        }
        _ => {
            // Block-level pseudo-element.
            let inner_segs = content_to_inline_segments(&ps);
            let inner = if inner_segs.is_empty() {
                vec![]
            } else {
                vec![anon_inline_run(parent_id, &ps, inner_segs)]
            };
            let b = LayoutBox {
                node: parent_id,
                rect: Rect::ZERO,
                style: ps,
                kind: BoxKind::Block,
                children: inner,
            };
            if is_before {
                children.insert(0, b);
            } else {
                children.push(b);
            }
        }
    }
}

/// Extracts text from `Content::Items` and returns it as a single `InlineSegment`.
/// Only `ContentItem::String` is handled in Phase 0; other variants are skipped.
fn content_to_inline_segments(style: &ComputedStyle) -> Vec<InlineSegment> {
    let Content::Items(items) = &style.content else {
        return vec![];
    };
    let text: String = items
        .iter()
        .filter_map(|item| {
            if let ContentItem::String(s) = item {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect();
    if text.is_empty() {
        return vec![];
    }
    vec![InlineSegment {
        text,
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_width: 0.0,
        forced_break: false,
    }]
}

/// Builds inline segments for a pseudo-element and applies its own box model
/// spacing (margin + border + padding) as `pre_space` / `post_space`.
/// Used by `collect_inline_segments` to inject `::before` / `::after` content.
fn push_pseudo_inline_segs(ps: &ComputedStyle, viewport: Size, out: &mut Vec<InlineSegment>) {
    let mut segs = content_to_inline_segments(ps);
    if segs.is_empty() {
        return;
    }
    let em = ps.font_size;
    let pre = ps.margin_left.resolve_or_zero(em, 0.0, viewport)
        + ps.border_left_width
        + ps.padding_left.resolve_or_zero(em, 0.0, viewport);
    let post = ps.padding_right.resolve_or_zero(em, 0.0, viewport)
        + ps.border_right_width
        + ps.margin_right.resolve_or_zero(em, 0.0, viewport);
    if pre > 0.0 {
        segs[0].pre_space += pre;
    }
    if post > 0.0 {
        let last = segs.len() - 1;
        segs[last].post_space += post;
    }
    out.extend(segs);
}

/// CSS Lists L3 §2.1 — ordinal of a `<li>` among its element siblings (1-based).
fn li_ordinal(doc: &Document, id: NodeId) -> u32 {
    let Some(parent_id) = doc.get(id).parent else { return 1 };
    let mut n = 0u32;
    for &sib in &doc.get(parent_id).children.clone() {
        if matches!(&doc.get(sib).data, NodeData::Element { name, .. } if name.local.as_str() == "li") {
            n += 1;
            if sib == id {
                return n;
            }
        }
    }
    1
}

fn to_roman(n: u32, upper: bool) -> String {
    const VALS: &[(u32, &str, &str)] = &[
        (1000, "M", "m"), (900, "CM", "cm"), (500, "D", "d"), (400, "CD", "cd"),
        (100, "C", "c"), (90, "XC", "xc"), (50, "L", "l"), (40, "XL", "xl"),
        (10, "X", "x"), (9, "IX", "ix"), (5, "V", "v"), (4, "IV", "iv"), (1, "I", "i"),
    ];
    if n == 0 { return "0".to_string(); }
    let mut out = String::new();
    let mut rem = n;
    for &(val, up, lo) in VALS {
        while rem >= val {
            out.push_str(if upper { up } else { lo });
            rem -= val;
        }
    }
    out
}

fn to_alpha(n: u32, upper: bool) -> String {
    if n == 0 { return "0".to_string(); }
    let base = if upper { b'A' } else { b'a' };
    let mut out = String::new();
    let mut rem = n;
    while rem > 0 {
        rem -= 1;
        out.insert(0, (base + (rem % 26) as u8) as char);
        rem /= 26;
    }
    out
}

fn to_greek(n: u32) -> String {
    const GREEK: &[char] = &['α','β','γ','δ','ε','ζ','η','θ','ι','κ','λ','μ',
                              'ν','ξ','ο','π','ρ','σ','τ','υ','φ','χ','ψ','ω'];
    if n == 0 { return "0".to_string(); }
    let idx = ((n - 1) as usize) % GREEK.len();
    GREEK[idx].to_string()
}

/// CSS Lists L3 §2.1 — builds the marker string from `list-style-type` + ordinal.
/// Bullet types (Disc/Circle/Square) return "" — rendered as geometric shapes by
/// the display-list emitter (FillRoundedRect / DrawBorder / FillRect).
/// CSS: @counter-style — P4 extends with custom counter styles.
fn marker_text(lst: ListStyleType, ordinal: u32) -> String {
    match lst {
        ListStyleType::None   => String::new(),
        ListStyleType::Disc   => String::new(), // geometric: filled circle
        ListStyleType::Circle => String::new(), // geometric: hollow circle
        ListStyleType::Square => String::new(), // geometric: filled square
        ListStyleType::Decimal            => format!("{}. ", ordinal),
        ListStyleType::DecimalLeadingZero => format!("{:02}. ", ordinal),
        ListStyleType::LowerRoman => format!("{}. ", to_roman(ordinal, false)),
        ListStyleType::UpperRoman => format!("{}. ", to_roman(ordinal, true)),
        ListStyleType::LowerAlpha => format!("{}. ", to_alpha(ordinal, false)),
        ListStyleType::UpperAlpha => format!("{}. ", to_alpha(ordinal, true)),
        ListStyleType::LowerGreek => format!("{}. ", to_greek(ordinal)),
    }
}

/// CSS Lists L3 §2.1 — creates `BoxKind::Marker` and prepends to children.
/// Does nothing when `list-style-type: none` or `list-style-image` is set (P4).
fn inject_marker(parent_id: NodeId, children: &mut Vec<LayoutBox>, style: &ComputedStyle, ordinal: u32) {
    if matches!(style.list_style_type, ListStyleType::None) {
        return;
    }
    // CSS: list-style-image — P4 wires image markers.
    let text = marker_text(style.list_style_type, ordinal);
    let mut ms = ComputedStyle::root();
    ms.font_size    = style.font_size;
    ms.font_weight  = style.font_weight;
    ms.font_style   = style.font_style;
    ms.font_family  = style.font_family.clone();
    ms.line_height  = style.line_height;
    ms.color        = style.color;
    ms.display      = Display::Inline;
    children.insert(0, LayoutBox {
        node:     parent_id,
        rect:     Rect::ZERO,
        style:    ms,
        kind:     BoxKind::Marker {
            text,
            position:        style.list_style_position,
            list_style_type: style.list_style_type,
        },
        children: vec![],
    });
}

/// CSS Display L3 §7.2 — replaces each `BoxKind::Contents` child with its own
/// children in-place. Grandchildren are already flattened (recursive `build_box`
/// calls run `flatten_contents` on inner levels first).
fn flatten_contents(children: &mut Vec<LayoutBox>) {
    let mut i = 0;
    while i < children.len() {
        if matches!(children[i].kind, BoxKind::Contents) {
            let grandchildren = std::mem::take(&mut children[i].children);
            let gc_len = grandchildren.len();
            children.remove(i);
            for (j, gc) in grandchildren.into_iter().enumerate() {
                children.insert(i + j, gc);
            }
            // Don't advance i — a grandchild might itself be Contents (edge case
            // if the inner build_box somehow produced an un-flattened Contents).
            // Advancing by gc_len skips them all safely since they were already
            // flattened at their own build level.
            i += gc_len;
        } else {
            i += 1;
        }
    }
}

fn build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
) -> LayoutBox {
    let mut style = compute_style(doc, id, sheet, inherited, viewport);

    let kind = match &doc.get(id).data {
        // Shadow root nodes are infrastructure — never rendered directly.
        // The flat tree already maps host children to shadow root's children.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. } | NodeData::ShadowRoot { .. } => BoxKind::Skip,
        NodeData::Document | NodeData::Element { .. } => {
            if style.display == Display::None {
                BoxKind::Skip
            } else if is_image_element(doc, id) {
                let src = resolve_image_source(doc, id, viewport);
                let alt = doc.get(id).get_attr("alt").unwrap_or("").to_string();
                // Intrinsic dimensions у выбранного `<source>` действуют как
                // presentational hint: заполняют только пустые слоты, не
                // перекрывают ни CSS-каскад, ни собственные `<img width|
                // height>` атрибуты (последние уже легли в style через
                // `apply_image_presentational_hints`). HTML5 §10 «mapped
                // attributes»: hint = UA-rule с specificity 0.
                if style.width.is_none()
                    && let Some(w) = src.intrinsic_width
                {
                    style.width = Some(Length::Px(w as f32));
                }
                if style.height.is_none()
                    && let Some(h) = src.intrinsic_height
                {
                    style.height = Some(Length::Px(h as f32));
                }
                BoxKind::Image { src: src.url, alt }
            } else if is_form_control_element(doc, id) {
                let kind = {
                    let node = doc.get(id);
                    let tag = node.element_name()
                        .map(|q| q.local.as_str())
                        .unwrap_or("")
                        .to_owned();
                    match tag.as_str() {
                        "button"   => FormControlKind::Button,
                        "select"   => FormControlKind::Select,
                        "textarea" => FormControlKind::Textarea,
                        _ => {
                            let input_type = node.input_type()
                                .unwrap_or(lumen_dom::InputType::Text);
                            let checked = node.get_attr("checked").is_some();
                            FormControlKind::Input { input_type, checked }
                        }
                    }
                };
                BoxKind::FormControl { kind }
            } else if matches!(style.display, Display::TableRow) {
                BoxKind::TableRow
            } else if matches!(style.display, Display::Table | Display::InlineTable) {
                BoxKind::Table
            } else if matches!(
                style.display,
                Display::TableRowGroup
                    | Display::TableHeaderGroup
                    | Display::TableFooterGroup
            ) {
                BoxKind::TableRowGroup
            } else if matches!(style.display, Display::FlowRoot) {
                BoxKind::FlowRoot
            } else if matches!(style.display, Display::Contents) {
                BoxKind::Contents
            } else {
                BoxKind::Block
            }
        }
    };

    let mut children = Vec::new();
    if matches!(kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Contents | BoxKind::FormControl { .. } | BoxKind::TableRow | BoxKind::Table | BoxKind::TableRowGroup) {
        // CSS: :host, ::slotted — P4 wires shadow-scoped styles here
        let dom_children: Vec<NodeId> = flat.children_of(doc, id).to_vec();
        // CSS Grid L1 §6: all direct children of a grid/flex container are
        // "blockified" — they participate as individual items, not wrapped in
        // InlineRun. Skip the inline-collection logic for these containers.
        let is_item_container = matches!(
            style.display,
            Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
                | Display::TableRow
                | Display::Table | Display::InlineTable
                | Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
        );
        if is_item_container {
            for child_id in dom_children {
                let child_box = build_box(doc, sheet, child_id, &style, viewport, flat);
                if !matches!(child_box.kind, BoxKind::Skip) {
                    children.push(child_box);
                }
            }
        } else {
        let mut i = 0;
        while i < dom_children.len() {
            let child_id = dom_children[i];
            let is_inl = is_inline_content(doc, sheet, child_id, &style, viewport);
            let is_ib = !is_inl && is_inline_block(doc, sheet, child_id, &style, viewport);

            if is_inl || is_ib {
                // Унифицированный сбор inline-уровневого контента: inline-элементы
                // и inline-block элементы участвуют в ОДНОМ inline-контексте.
                // Межэлементный whitespace не прерывает поток.
                // Результат: InlineRun (чистый текст) или InlineBlockRow (смешанный).
                let mut row_items: Vec<LayoutBox> = Vec::new();
                let mut pending: Vec<InlineSegment> = Vec::new();
                // CSS §4.1.2 white-space collapsing: whitespace between
                // inline-level siblings collapses to a single space.
                let mut had_ws = false;

                loop {
                    if i >= dom_children.len() {
                        break;
                    }
                    let cid = dom_children[i];
                    match &doc.get(cid).data {
                        NodeData::Text(s) if s.chars().all(char::is_whitespace) => {
                            had_ws = true;
                            i += 1;
                            continue;
                        }
                        NodeData::Comment(_) | NodeData::Doctype { .. } => {
                            i += 1;
                            continue;
                        }
                        _ => {}
                    }
                    if is_inline_content(doc, sheet, cid, &style, viewport) {
                        collect_inline_segments(doc, sheet, cid, &style, viewport, &mut pending, flat);
                        had_ws = false;
                        i += 1;
                    } else if is_inline_block(doc, sheet, cid, &style, viewport) {
                        if !pending.is_empty() {
                            row_items.push(anon_inline_run(
                                id,
                                &style,
                                std::mem::take(&mut pending),
                            ));
                        }
                        // Whitespace between inline-blocks → collapsed space gap.
                        if had_ws && !row_items.is_empty() {
                            row_items.push(LayoutBox {
                                node: id,
                                rect: Rect::ZERO,
                                style: anon_style(&style),
                                kind: BoxKind::InlineSpace,
                                children: vec![],
                            });
                        }
                        row_items.push(build_box(doc, sheet, cid, &style, viewport, flat));
                        had_ws = false;
                        i += 1;
                    } else if matches!(doc.get(cid).data, NodeData::Element { .. })
                        && compute_style(doc, cid, sheet, &style, viewport).display
                            == Display::None
                    {
                        // display:none не прерывает inline-контекст — CSS §9.2.4.
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !pending.is_empty() {
                    row_items.push(anon_inline_run(id, &style, std::mem::take(&mut pending)));
                }

                match row_items.len() {
                    0 => {}
                    // Единственный чисто-текстовый run — без лишней обёртки.
                    1 if matches!(row_items[0].kind, BoxKind::InlineRun { .. }) => {
                        children.push(row_items.remove(0));
                    }
                    // Несколько элементов или inline-block → InlineBlockRow.
                    _ => {
                        children.push(anon_inline_block_row(id, &style, row_items));
                    }
                }
            } else {
                children.push(build_box(doc, sheet, child_id, &style, viewport, flat));
                i += 1;
            }
        }
        // CSS Pseudo-elements L4 §4 — inject ::before / ::after for block-flow.
        // Only for Block / FlowRoot (not FormControl, not flex/grid item containers).
        if matches!(kind, BoxKind::Block | BoxKind::FlowRoot) {
            let before_ps =
                compute_pseudo_element_style(doc, id, "before", sheet, &style, viewport);
            let after_ps =
                compute_pseudo_element_style(doc, id, "after", sheet, &style, viewport);
            inject_pseudo(id, &mut children, before_ps, true);
            inject_pseudo(id, &mut children, after_ps, false);
            // CSS Lists L3 §2.1 — inject ::marker for list items.
            // ::marker comes before ::before in document order.
            if style.display == Display::ListItem {
                let ordinal = li_ordinal(doc, id);
                inject_marker(id, &mut children, &style, ordinal);
            }
        }
        // CSS Display L3 §7.2 — flatten display:contents boxes into this context.
        // Each Contents child is replaced by its own children (already built and
        // recursively flattened). Runs after pseudo-element injection so ::before/
        // ::after on the contents element itself are preserved inside the box.
        flatten_contents(&mut children);
        } // end else (non-item-container)
    }

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style,
        kind,
        children,
    }
}

/// Phase 0 shrink-to-fit: возвращает «предпочтительную» ширину inline-block-бокса
/// (включая padding+border самого бокса). Алгоритм: если у бокса явная CSS `width` —
/// берём её; иначе рекурсивно ищем максимальную preferred_width среди потомков
/// и добавляем padding+border текущего бокса. Возвращает `None` если явных размеров
/// нет ни у бокса, ни у его потомков.
///
/// Для typed-Length полей используем em = font_size, cb_width = 0 как
/// аппроксимацию (shrink-to-fit не знает cb_width заранее).
fn preferred_inline_block_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    let s = &b.style;
    let em = s.font_size;
    // % ширины на этом этапе не разрешима — трактуем как отсутствие.
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    if let Some(w_len) = &s.width
        && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr
                + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return Some(outer.max(0.0));
    }
    // InlineBlockRow — горизонтальный поток: суммируем ширины детей + их margins.
    // InlineSpace — collapsed whitespace gap; его ширина = char_width(' ').
    // Остальные боксы (Block, Image и т.д.) — вертикальный поток: берём max.
    let content_w = if matches!(b.kind, BoxKind::InlineBlockRow) {
        let sum: f32 = b.children.iter().map(|c| {
            if matches!(c.kind, BoxKind::InlineSpace) {
                // Учитываем ширину collapsed space, чтобы при shrink-to-fit
                // не занижать ширину контейнера и не вызывать перенос соседних
                // inline-block элементов на следующую строку.
                return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
            }
            let cw = preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0);
            let cem = c.style.font_size;
            let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
            let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
            cw + ml + mr
        }).sum();
        sum
    } else {
        b.children
            .iter()
            .filter_map(|c| preferred_inline_block_width(c, measurer, viewport))
            .fold(0.0_f32, f32::max)
    };
    if content_w > 0.0 {
        Some(
            (content_w + pl + pr
                + s.border_left_width + s.border_right_width)
                .max(0.0),
        )
    } else {
        None
    }
}

/// CSS Intrinsic Sizing L3 §4 — max-content border-box width of `b`.
///
/// The max-content width is the width a box would use if line breaking were
/// suppressed: all content on one line. For block containers this is the
/// maximum over children's max-content widths. For `InlineRun` boxes it is
/// the sum of all segment text widths (no wrapping). Includes the box's own
/// padding + border in the returned value (border-box width).
///
/// Phase-0 approximation: only `char_width` per-character measurement is
/// available; inter-word spacing is included, but features like ligatures or
/// kerning are not. Word-break is not applied — text is treated as one run.
fn max_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Explicit non-intrinsic CSS width takes precedence (same logic as
    // preferred_inline_block_width).
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // max-content = all segments on one line (no wrapping).
            measurer.map_or(0.0, |m| {
                segments.iter().map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let ts = seg.style.tab_size * m.char_width(' ', seg.style.font_size);
                    measure_text_w(&seg.text, seg.style.font_size, ls, ts, m)
                }).sum()
            })
        }
        BoxKind::InlineBlockRow => {
            b.children.iter().map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).sum()
        }
        _ => {
            b.children.iter()
                .map(|c| max_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Intrinsic Sizing L3 §4 — min-content border-box width of `b`.
///
/// The min-content width is the narrowest a box can be without overflowing:
/// the width of the longest unbreakable content unit (word, image, etc.).
///
/// Phase-0 approximation: computes the max word width per `InlineRun` by
/// splitting on ASCII whitespace. This gives correct results for Latin text
/// but may overestimate for languages without whitespace-based word breaks.
fn min_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // min-content = longest single word across all segments.
            measurer.map_or(0.0, |m| {
                segments.iter().flat_map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let ts = seg.style.tab_size * m.char_width(' ', seg.style.font_size);
                    // Split on whitespace to find individual "words".
                    seg.text.split_whitespace().map(move |word| {
                        measure_text_w(word, seg.style.font_size, ls, ts, m)
                    })
                }).fold(0.0_f32, f32::max)
            })
        }
        BoxKind::InlineBlockRow => {
            // For inline-block row, min-content is the max over children.
            b.children.iter().map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return 0.0; // spaces are breakable
                }
                let cw = min_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).fold(0.0_f32, f32::max)
        }
        _ => {
            b.children.iter()
                .map(|c| min_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// Рекурсивно смещает rect.y всего поддерева на dy (для vertical-align).
fn shift_y_box(b: &mut LayoutBox, dy: f32) {
    b.rect.y += dy;
    for child in &mut b.children {
        shift_y_box(child, dy);
    }
}

/// Рекурсивно смещает rect всего поддерева на (dx, dy).
/// Используется при позиционировании абсолютных потомков.
fn shift_tree(b: &mut LayoutBox, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    b.rect.x += dx;
    b.rect.y += dy;
    for child in &mut b.children {
        shift_tree(child, dx, dy);
    }
}

// ─── CSS 2.1 §9.5 — Float context ────────────────────────────────────────────

/// CSS 2.1 §9.5 — tracks float placements within a single block formatting
/// context.  Simplified Phase-0 implementation: only axis-aligned rectangles,
/// no shape-outside wrapping.  All coordinates are in the same space as the
/// block container's content area (i.e. not relative to viewport).
struct FloatContext {
    /// Left floats: `(bottom_y, right_edge)` — right edge of the float margin
    /// box in content-area coordinates.  Active while `bottom_y > query_y`.
    left: Vec<(f32, f32)>,
    /// Right floats: `(bottom_y, left_edge)` — left edge of the float margin
    /// box.  Active while `bottom_y > query_y`.
    right: Vec<(f32, f32)>,
}

impl FloatContext {
    fn new() -> Self {
        Self { left: Vec::new(), right: Vec::new() }
    }

    /// Left boundary of available inline space at `y` (= rightmost right-edge
    /// of all left floats whose `bottom_y > y`).  Falls back to `default_x`.
    fn left_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.left
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, r)| *r)
            .fold(default_x, f32::max)
    }

    /// Right boundary of available inline space at `y` (= leftmost left-edge
    /// of all right floats whose `bottom_y > y`).  Falls back to `default_x`.
    fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        self.right
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, l)| *l)
            .fold(default_x, f32::min)
    }

    /// Record a left float occupying `[y_top, bottom_y)` with right margin
    /// edge at `right_edge`.
    fn add_left(&mut self, bottom_y: f32, right_edge: f32) {
        self.left.push((bottom_y, right_edge));
    }

    /// Record a right float occupying `[y_top, bottom_y)` with left margin
    /// edge at `left_edge`.
    fn add_right(&mut self, bottom_y: f32, left_edge: f32) {
        self.right.push((bottom_y, left_edge));
    }

    /// CSS 2.1 §9.5.2 — advance `y` past all floats on the given side.
    fn clear_y(&self, y: f32, side: ClearSide) -> f32 {
        let mut result = y;
        let do_left  = matches!(side, ClearSide::Left  | ClearSide::Both);
        let do_right = matches!(side, ClearSide::Right | ClearSide::Both);
        if do_left  { for (bot, _) in &self.left  { result = result.max(*bot); } }
        if do_right { for (bot, _) in &self.right { result = result.max(*bot); } }
        result
    }

    /// True when there are no active floats at all.
    fn is_empty(&self) -> bool {
        self.left.is_empty() && self.right.is_empty()
    }
}

/// `pcb` — rect positioned containing block (ближайший предок с position != static),
/// используется для layout абсолютно-позиционированных потомков.
#[allow(clippy::too_many_arguments)]
fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    // CSS 2.1 §10.5: definite content height of the containing block, or None if auto.
    // None means percentage heights on children compute to 'auto'.
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    if matches!(b.kind, BoxKind::Skip) {
        b.rect = Rect::new(start_x, start_y, 0.0, 0.0);
        return;
    }

    let s = b.style.clone();
    let em = s.font_size;
    let cb = available_width;

    // Резолвим typed Length-поля с known containing block.
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_right = s.margin_right.resolve_or_zero(em, cb, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb, viewport);

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;
    // Block: auto-ширина = весь доступный inline-размер контейнера.
    // Replaced element (Image): auto-ширина = intrinsic (0 в Phase 0, без
    // декодированных пикселей). Это CSS 2.1 §10.3.2 — replaced-боксы
    // НЕ растягиваются на весь контейнер при отсутствии width.
    let is_replaced = matches!(b.kind, BoxKind::Image { .. } | BoxKind::FormControl { .. });
    b.rect.width = if is_replaced {
        0.0
    } else {
        (available_width - margin_left - margin_right).max(0.0)
    };
    // Явная ширина (CSS width: Npx) перекрывает auto-ширину.
    // box-sizing определяет, к какой части бокса относится `width`:
    //   - content-box: width — это размер контента, padding+border прибавляются;
    //   - border-box: width — общий размер вместе с padding+border.
    if let Some(w_len) = &s.width {
        if w_len.is_intrinsic() {
            // CSS Intrinsic Sizing L3 §4 — min-content / max-content / fit-content.
            // max_content_outer_width / min_content_outer_width already include
            // the box's own padding+border (border-box width), so we assign directly.
            let avail_bb = (available_width - margin_left - margin_right).max(0.0);
            b.rect.width = match w_len {
                Length::MaxContent => max_content_outer_width(b, measurer, viewport),
                Length::MinContent => min_content_outer_width(b, measurer, viewport),
                Length::FitContent(max_arg) => {
                    let max_c = max_content_outer_width(b, measurer, viewport);
                    if let Some(arg) = max_arg {
                        // fit-content(<length>) = min(avail, max(min-content, arg))
                        let min_c = min_content_outer_width(b, measurer, viewport);
                        let arg_px = arg.resolve(em, Some(cb), viewport).unwrap_or(avail_bb);
                        // arg_px is a content-box length; convert to border-box:
                        let arg_bb = match s.box_sizing {
                            BoxSizing::ContentBox => arg_px + padding_left + padding_right
                                + s.border_left_width + s.border_right_width,
                            BoxSizing::BorderBox => arg_px,
                        };
                        max_c.min(min_c.max(arg_bb)).min(avail_bb)
                    } else {
                        // fit-content = min(available, max-content)
                        max_c.min(avail_bb)
                    }
                }
                _ => unreachable!(),
            };
        } else if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
            b.rect.width = match s.box_sizing {
                BoxSizing::ContentBox => (w + padding_left + padding_right
                    + s.border_left_width + s.border_right_width).max(0.0),
                BoxSizing::BorderBox => w.max(padding_left + padding_right + s.border_left_width + s.border_right_width),
            };
        }
    }
    // CSS 2.1 §10.4: tentative width → clamp в [min-width, max-width].
    // Intrinsic keywords in min-/max- also resolve to intrinsic values here.
    // Порядок «max сначала, потом min» автоматически даёт правило
    // «при min > max побеждает min». min-/max- интерпретируются в той же
    // box-sizing модели, что и width: content-box добавляет padding+border,
    // border-box оставляет как есть.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_left + padding_right
            + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(b, measurer, viewport))
        } else {
            max_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(max_w) = max_bb {
            b.rect.width = b.rect.width.min(max_w);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(b, measurer, viewport))
        } else {
            min_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(min_w) = min_bb {
            b.rect.width = b.rect.width.max(min_w);
        }
    }
    // Phase 0 shrink-to-fit для inline-block без явной CSS width.
    // Полный алгоритм (CSS 2.1 §10.3.9) требует двух проходов; здесь —
    // упрощение: ищем максимальную explicit-width среди потомков.
    if s.width.is_none() && s.display == Display::InlineBlock
        && let Some(pref_w) = preferred_inline_block_width(b, measurer, viewport)
    {
        b.rect.width = pref_w.min(b.rect.width);
    }

    // CSS 2.1 §10.3.3 — auto horizontal-margin centering for block-level
    // non-replaced elements in normal flow with an explicit CSS width.
    // Remaining inline space distributes to auto margins: both auto → equal
    // halves (centered block); only left auto → left takes all remaining;
    // only right auto → no x shift (right margin absorbs remainder silently).
    // Does not apply to: replaced, inline-block, flex/grid containers, floats,
    // or absolute/fixed positioned elements.
    let ml_is_auto = s.margin_left.is_auto();
    let mr_is_auto = s.margin_right.is_auto();
    if (ml_is_auto || mr_is_auto)
        && s.width.is_some()
        && !is_replaced
        && !matches!(
            s.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
        )
        && !matches!(s.float_side, FloatSide::Left | FloatSide::Right)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
    {
        let ml_fixed = if ml_is_auto { 0.0 } else { margin_left };
        let mr_fixed = if mr_is_auto { 0.0 } else { margin_right };
        let remaining = (available_width - b.rect.width - ml_fixed - mr_fixed).max(0.0);
        let ml_computed = if ml_is_auto && mr_is_auto {
            remaining / 2.0
        } else if ml_is_auto {
            remaining
        } else {
            ml_fixed
        };
        b.rect.x = start_x + ml_computed;
    }

    let content_x = b.rect.x + padding_left + s.border_left_width;
    let content_y = b.rect.y + padding_top + s.border_top_width;
    let content_width = (b.rect.width
        - padding_left - padding_right
        - s.border_left_width - s.border_right_width).max(0.0);

    // pcb для потомков: если текущий элемент positioned — он сам CB для абсолютных детей.
    // CSS Containment L3: contain:layout и contain:paint тоже устанавливают containing block.
    // Высота ещё неизвестна, используем 0 — корректируем after layout.
    let is_positioned = !matches!(s.position, Position::Static);
    let contain_establishes_cb = s.contain.0
        & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0 | ContainFlags::STRICT.0) != 0;
    let children_pcb = if is_positioned || contain_establishes_cb {
        Rect::new(b.rect.x, b.rect.y, b.rect.width, 0.0)
    } else {
        pcb
    };

    // InlineRun обрабатывается до основного match.
    if let BoxKind::InlineRun { segments, lines } = &mut b.kind {
        if let Some(m) = measurer {
            // white-space: nowrap / text-wrap-mode: nowrap → infinite max_width so
            // the line-breaker never wraps; word-spacing/letter-spacing logic unchanged.
            let wrap_width = if s.white_space.is_nowrap() || s.text_wrap_mode == TextWrapMode::Nowrap {
                f32::INFINITY
            } else {
                content_width
            };
            let text_indent_px = s.text_indent.resolve_or_zero(em, cb, viewport);
            *lines = wrap_inline_run(segments, wrap_width, s.font_size, text_indent_px, viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap);
            align_lines(lines, content_width, s.text_align, s.direction);
            let line_h = s.font_size * s.line_height;
            apply_inline_vertical_align(lines, line_h);
            // CSS UI L4 §10.1: text-overflow: ellipsis требует overflow != visible.
            if s.text_overflow == TextOverflow::Ellipsis
                && (s.overflow_x != Overflow::Visible || s.overflow_y != Overflow::Visible)
            {
                apply_text_overflow_ellipsis(lines, content_width, s.font_size, m);
            }
        } else {
            *lines = one_line_fallback(segments);
        }
        let line_count = lines.len().max(1);
        b.rect.height = line_count as f32 * (s.font_size * s.line_height);
        return;
    }

    // Абсолютно-позиционированные дети: (index, static_x, static_y).
    // Заполняется внутри Block-flow и обрабатывается после match.
    let mut abs_deferred: Vec<(usize, f32, f32)> = Vec::new();

    match &mut b.kind {
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::Image { .. } | BoxKind::FormControl { .. } => {
            // Flex containers dispatch to lay_out_flex before block-flow.
            if matches!(s.display, Display::Flex | Display::InlineFlex) {
                let content_height = lay_out_flex(
                    &mut b.children, &s, content_x, content_y, content_width, measurer, viewport,
                    children_pcb, hp,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = h_len.resolve(em, available_height, viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else if let Some((aw, ah)) = s.aspect_ratio
                    && aw > 0.0 && ah > 0.0
                {
                    (b.rect.width * ah / aw).max(0.0)
                } else {
                    let ch = if s.contain.0 & ContainFlags::SIZE.0 != 0 { 0.0 } else { content_height };
                    ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Grid containers dispatch to lay_out_grid before block-flow.
            if matches!(s.display, Display::Grid | Display::InlineGrid) {
                let content_height = lay_out_grid(
                    &mut b.children, &s, content_x, content_y, content_width, measurer, viewport,
                    children_pcb, hp,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = h_len.resolve(em, available_height, viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else if let Some((aw, ah)) = s.aspect_ratio
                    && aw > 0.0 && ah > 0.0
                {
                    (b.rect.width * ah / aw).max(0.0)
                } else {
                    let ch = if s.contain.0 & ContainFlags::SIZE.0 != 0 { 0.0 } else { content_height };
                    ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Image не имеет flow-детей, поэтому child-цикл просто пуст —
            // объединяем с Block, чтобы общий код width/height/min-max/borders
            // не дублировался. content_height = 0 для Image без явной высоты
            // даёт коробку только из padding+border (что для пустой картинки
            // визуально корректно).
            // CSS 2.1 §10.5: definite content height for children's height percentage resolution.
            // Only available when this element itself has an explicit height.
            let children_available_height: Option<f32> = if let Some(h_len) = &s.height
                && let Some(h) = h_len.resolve(em, available_height, viewport)
            {
                Some(match s.box_sizing {
                    BoxSizing::ContentBox => h,
                    BoxSizing::BorderBox => (h - padding_top - padding_bottom
                        - s.border_top_width - s.border_bottom_width).max(0.0),
                })
            } else {
                None
            };
            let content_height = if (s.column_count.is_some() || s.column_width.is_some())
                && !b.children.is_empty()
            {
                lay_out_multicol_children(
                    &mut b.children,
                    content_x, content_y, content_width,
                    &s, em, measurer, viewport, children_pcb, hp,
                )
            } else {
                // CSS 2.1 §9.5 — float context for this block formatting context.
                let mut fc = FloatContext::new();
                let container_right = content_x + content_width;

                let mut child_y = content_y;
                // CSS 2.1 §8.3.1: resolved bottom margin of the previous block-level child.
                // Adjacent Block/FlowRoot siblings collapse their margins (gap = max, not sum).
                // Inline runs, replaced elements, and floats break the collapsing chain.
                let mut prev_block_mb: f32 = 0.0;
                for (i, child) in b.children.iter_mut().enumerate() {
                    if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                        abs_deferred.push((i, content_x, child_y));
                        continue;
                    }
                    // CSS Lists L3 §2.4 — position ::marker outside or inside principal block.
                    if matches!(&child.kind, BoxKind::Marker { .. }) {
                        let (position, em, lh) = if let BoxKind::Marker { position, .. } = &child.kind {
                            (*position, child.style.font_size, child.style.line_height)
                        } else { unreachable!() };
                        let line_h = em * lh;
                        let marker_w = em * 1.5; // CSS: list-style-type determines exact width
                        match position {
                            ListStylePosition::Outside => {
                                // Out of flow: does not advance child_y.
                                child.rect = Rect::new(content_x - marker_w, child_y, marker_w, line_h);
                            }
                            ListStylePosition::Inside => {
                                child.rect = Rect::new(content_x, child_y, marker_w, line_h);
                                child_y += line_h;
                            }
                        }
                        continue;
                    }

                    // CSS 2.1 §9.5.2: clear — advance child_y past relevant floats.
                    if !fc.is_empty() && child.style.clear != ClearSide::None {
                        child_y = fc.clear_y(child_y, child.style.clear);
                    }

                    // CSS 2.1 §9.5.1: float box — placed out of normal flow.
                    if child.style.float_side != FloatSide::None {
                        let cem = child.style.font_size;
                        let avail_left  = fc.left_edge_at(child_y, content_x);
                        let avail_right = fc.right_edge_at(child_y, container_right);
                        let avail_w = (avail_right - avail_left).max(0.0);

                        // Shrink-to-fit width: explicit CSS width wins; otherwise use
                        // preferred content width clamped to available space.
                        let float_layout_w = if child.style.width.is_some() {
                            avail_w
                        } else {
                            preferred_inline_block_width(child, measurer, viewport)
                                .map(|pw| pw.min(avail_w))
                                .unwrap_or(avail_w)
                        };
                        lay_out(child, avail_left, child_y, float_layout_w,
                                children_available_height, measurer, viewport, children_pcb, hp);

                        let fml = child.style.margin_left.resolve_or_zero(cem, avail_w, viewport);
                        let fmr = child.style.margin_right.resolve_or_zero(cem, avail_w, viewport);
                        let fmt = child.style.margin_top.resolve_or_zero(cem, avail_w, viewport);
                        let fmb = child.style.margin_bottom.resolve_or_zero(cem, avail_w, viewport);
                        let fw  = child.rect.width;
                        let fh  = child.rect.height;

                        match child.style.float_side {
                            FloatSide::Left => {
                                let lx = fc.left_edge_at(child_y, content_x);
                                child.rect.x = lx + fml;
                                child.rect.y = child_y + fmt;
                                fc.add_left(child_y + fmt + fh + fmb, lx + fml + fw + fmr);
                            }
                            FloatSide::Right => {
                                let rx = fc.right_edge_at(child_y, container_right);
                                child.rect.x = rx - fmr - fw;
                                child.rect.y = child_y + fmt;
                                fc.add_right(child_y + fmt + fh + fmb, rx - fmr - fw - fml);
                            }
                            FloatSide::None => unreachable!(),
                        }
                        // Float does not advance child_y in normal flow.
                        continue;
                    }

                    // Normal flow: narrow x/width for active floats.
                    let flow_left  = fc.left_edge_at(child_y, content_x);
                    let flow_right = fc.right_edge_at(child_y, container_right);
                    let flow_w = (flow_right - flow_left).max(0.0);

                    // CSS 2.1 §8.3.1: collapse adjacent sibling block margins.
                    // Only Block/FlowRoot participate; other kinds break the chain.
                    // Formula: start_y = child_y - min(prev_mb, mt)
                    // so that lay_out's internal "+mt" yields child_y + max(prev_mb, mt).
                    let is_block = matches!(&child.kind, BoxKind::Block | BoxKind::FlowRoot);
                    let mt = child.style.margin_top
                        .resolve_or_zero(child.style.font_size, flow_w, viewport);
                    let start_y = if is_block {
                        child_y - prev_block_mb.min(mt.max(0.0))
                    } else {
                        child_y
                    };

                    lay_out(child, flow_left, start_y, flow_w,
                            children_available_height, measurer, viewport, children_pcb, hp);
                    if matches!(child.kind, BoxKind::Skip) {
                        // Zero-height; does not break the collapsing chain.
                        continue;
                    }
                    let child_mb = child.style.margin_bottom.resolve_or_zero(
                        child.style.font_size, content_width, viewport);
                    child_y = child.rect.y + child.rect.height + child_mb;
                    prev_block_mb = if is_block { child_mb.max(0.0) } else { 0.0 };
                }
                // CSS 2.1 §9.5: the container height must also enclose all floats.
                let float_bottom = fc.left.iter().chain(fc.right.iter())
                    .map(|(bot, _)| *bot)
                    .fold(child_y, f32::max);
                (float_bottom - content_y).max(0.0)
            };
            // Явная высота (CSS height: Npx) перекрывает auto-высоту по содержимому.
            // box-sizing работает симметрично width: content-box прибавляет
            // padding+border, border-box оставляет h как итоговую высоту.
            b.rect.height = if let Some(h_len) = &s.height {
                if let Some(h) = h_len.resolve(em, available_height, viewport) {
                    match s.box_sizing {
                        BoxSizing::ContentBox => h
                            + padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else {
                    content_height + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width
                }
            } else if let Some((aw, ah)) = s.aspect_ratio
                && aw > 0.0 && ah > 0.0
            {
                // CSS Sizing L4 §6.1: height auto + aspect-ratio → derive from width.
                // Phase 0: ratio applied in border-box space.
                (b.rect.width * ah / aw).max(0.0)
            } else {
                // CSS Containment L3 §3.3: contain:size suppresses children contribution
                // to auto height — intrinsic height = 0.
                let ch = if s.contain.0 & ContainFlags::SIZE.0 != 0 { 0.0 } else { content_height };
                ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
            };
            // CSS 2.1 §10.4: clamp [min-height, max-height]. Симметрия с
            // width: max сначала, потом min → «min побеждает max». Content
            // оверфлоу-ит коробку если min режет ниже — это правильное
            // поведение CSS.
            let outer_vert = |v: f32| match s.box_sizing {
                BoxSizing::ContentBox => v + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width,
                BoxSizing::BorderBox => v,
            };
            if let Some(max_len) = &s.max_height
                && let Some(max_h) = max_len.resolve(em, available_height, viewport)
            {
                b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
            }
            if let Some(min_len) = &s.min_height
                && let Some(min_h) = min_len.resolve(em, available_height, viewport)
            {
                b.rect.height = b.rect.height.max(outer_vert(min_h.max(0.0)));
            }
        }
        BoxKind::InlineBlockRow => {
            // Двухфазный горизонтальный layout с переносом строк и
            // vertical-align (CSS 2.1 §9.4.3 + §10.8).
            //
            // Фаза 1: расставляем детей по X, группируем в строки.
            // Фаза 2: применяем вертикальное выравнивание внутри каждой строки.
            //
            // rows: (row_y, row_max_h, Vec<child_index>)
            // IFC strut (CSS §10.8): descent шрифта родителя добавляется к
            // высоте строки только если в строке есть текстовый InlineRun.
            // Для строк из одних inline-block / replaced элементов strut не
            // нужен — их вертикальные размеры заданы margin-box, а Edge/Blink
            // в таких случаях strut'ом не расширяют line box, иначе ряды
            // inline-block-ов накапливают descender-зазор (BUG-023).
            let strut_descent = measurer.map_or(0.0, |m| m.descent_px(b.style.font_size));
            let mut rows: Vec<(f32, f32, Vec<usize>)> = Vec::new();
            let mut cur_x = content_x;
            let mut cur_y = content_y;
            let mut row_max_h: f32 = 0.0;
            let mut row_y = cur_y;
            let mut cur_row: Vec<usize> = Vec::new();
            let mut row_has_text = false;
            let mut total_h: f32 = 0.0;

            for i in 0..b.children.len() {
                // InlineSpace: collapsed whitespace gap — advance cur_x only.
                if matches!(b.children[i].kind, BoxKind::InlineSpace) {
                    let space_w = measurer.map_or(0.0, |m| m.char_width(' ', b.style.font_size));
                    cur_x += space_w;
                    continue;
                }
                let is_run = matches!(b.children[i].kind, BoxKind::InlineRun { .. });
                let child_avail = if is_run {
                    (content_width - (cur_x - content_x)).max(0.0)
                } else {
                    content_width
                };
                lay_out(&mut b.children[i], cur_x, cur_y, child_avail, None, measurer, viewport, children_pcb, hp);
                if matches!(b.children[i].kind, BoxKind::Skip) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let child_mr = b.children[i].style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                let child_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let child_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                let child_right = b.children[i].rect.x + b.children[i].rect.width + child_mr;
                let child_full_h = child_mt + b.children[i].rect.height + child_mb;

                if !is_run && child_right > content_x + content_width && cur_x > content_x {
                    // Строка завершена. row_spacing включает strut_descent
                    // только если в строке был текст (см. комментарий выше).
                    let row_spacing = row_max_h + if row_has_text { strut_descent } else { 0.0 };
                    rows.push((row_y, row_max_h, std::mem::take(&mut cur_row)));
                    total_h += row_spacing;
                    cur_y += row_spacing;
                    row_y = cur_y;
                    cur_x = content_x;
                    row_max_h = 0.0;
                    row_has_text = false;
                    lay_out(&mut b.children[i], cur_x, cur_y, content_width, None, measurer, viewport, children_pcb, hp);
                }
                cur_row.push(i);
                if is_run {
                    row_has_text = true;
                }
                cur_x = b.children[i].rect.x + b.children[i].rect.width + child_mr;
                row_max_h = row_max_h.max(child_full_h);
            }
            if !cur_row.is_empty() {
                rows.push((row_y, row_max_h, cur_row));
            }
            b.rect.height = total_h + row_max_h + if row_has_text { strut_descent } else { 0.0 };

            // Фаза 2: vertical-align. Для пустых inline-block элементов
            // baseline = нижний край margin-box (CSS 2.1 §10.8.1), поэтому
            // Baseline обрабатывается так же как Bottom.
            let mut adjustments: Vec<(usize, f32)> = Vec::new();
            for (_, row_h, child_idxs) in &rows {
                for &idx in child_idxs {
                    let child = &b.children[idx];
                    let c_em = child.style.font_size;
                    let child_mt = child.style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                    let child_mb = child.style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                    let child_full_h = child_mt + child.rect.height + child_mb;
                    let dy = match child.style.vertical_align {
                        VerticalAlign::Bottom | VerticalAlign::TextBottom | VerticalAlign::Baseline => {
                            row_h - child_full_h
                        }
                        VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                        VerticalAlign::Middle => (row_h - child_full_h) / 2.0,
                        _ => 0.0,
                    };
                    if dy > 0.001 {
                        adjustments.push((idx, dy));
                    }
                }
            }
            for (idx, dy) in adjustments {
                shift_y_box(&mut b.children[idx], dy);
            }
        }
        BoxKind::TableRow => {
            // CSS 2.1 §17.5 — table row: ячейки раскладываются горизонтально.
            // col_widths=None → per-row auto-distribution (standalone <tr> outside <table>).
            let row_h = lay_out_table_row(
                b, content_x, content_y, content_width, None, measurer, viewport, children_pcb, hp,
            );
            b.rect.height = if let Some(h_len) = &s.height
                && let Some(h) = h_len.resolve(em, available_height, viewport)
            {
                match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                }
            } else {
                row_h + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
        }
        BoxKind::Table => {
            // CSS 2.1 §17 — table container: compute global column widths, lay out rows.
            let content_height = lay_out_table(
                b, content_x, content_y, content_width, measurer, viewport, children_pcb, hp,
            );
            b.rect.height = if let Some(h_len) = &s.height
                && let Some(h) = h_len.resolve(em, available_height, viewport)
            {
                match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                }
            } else {
                content_height + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
        }
        BoxKind::TableRowGroup => {
            // CSS 2.1 §17 — row group standalone (outside a <table>): block-flow of rows.
            // When inside a Table, rows are handled directly by lay_out_table.
            let mut cur_y = content_y;
            for i in 0..b.children.len() {
                if !matches!(b.children[i].kind, BoxKind::TableRow) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                lay_out(&mut b.children[i], content_x, cur_y + c_mt, content_width, None, measurer, viewport, children_pcb, hp);
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb;
            }
            b.rect.height = (cur_y - content_y) + padding_top + padding_bottom
                + s.border_top_width + s.border_bottom_width;
        }
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::InlineSpace => unreachable!(),
        BoxKind::Skip => unreachable!(),
        BoxKind::Contents => unreachable!("display:contents boxes must be flattened before lay_out"),
        BoxKind::Marker { .. } => {
            // Rect is set by the parent's block-flow loop; nothing to do here.
        }
    }

    // CSS Positioned Layout L3 §4 — абсолютное / фиксированное позиционирование.
    // Деферированные дети (abs_deferred) собраны в Block-ветке выше.
    // Обрабатываем после finalize b.rect.height, чтобы знать высоту containing block.
    if !abs_deferred.is_empty() {
        let my_pcb = if is_positioned {
            // Padding-edge box (упрощение: border-edge, достаточно для Phase 1).
            Rect::new(b.rect.x, b.rect.y, b.rect.width, b.rect.height)
        } else {
            pcb
        };
        lay_out_abs_children(b, &abs_deferred, measurer, viewport, my_pcb, hp);
    }

    // CSS Positioned Layout L3 §9.4.3 — position: relative — смещение после normal flow.
    if matches!(s.position, Position::Relative) {
        let off_x = match &s.left {
            LengthOrAuto::Length(l) => l.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.right {
                LengthOrAuto::Length(r) => -(r.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        let off_y = match &s.top {
            LengthOrAuto::Length(t) => t.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.bottom {
                LengthOrAuto::Length(bot) => -(bot.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        if off_x != 0.0 || off_y != 0.0 {
            shift_tree(b, off_x, off_y);
        }
    }
}

/// CSS 2.1 §17.5 — simplified automatic table layout for a single row.
///
/// Алгоритм:
/// 1. Ячейки с явным CSS `width` используют его (content-box/border-box).
/// 2. Оставшаяся ширина делится поровну между ячейками без явной ширины.
/// 3. После layout все ячейки выравниваются по максимальной высоте строки.
///
/// Возвращает высоту строки (content height, без padding/border родителя).
#[allow(clippy::too_many_arguments)]
fn lay_out_table_row(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    col_widths: Option<&[f32]>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let cell_idxs: Vec<usize> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    let n = cell_idxs.len();
    if n == 0 {
        return 0.0;
    }

    // Шаг 1: определяем ширины ячеек — из col_widths если они переданы,
    // иначе вычисляем из явных CSS-ширин ячеек текущей строки.
    let resolved_w: Vec<f32> = if let Some(cw) = col_widths {
        // Pre-computed table-wide column widths; take min(n, cw.len()) columns.
        (0..n).map(|j| cw.get(j).copied().unwrap_or(0.0)).collect()
    } else {
        let mut explicit_w: Vec<Option<f32>> = Vec::with_capacity(n);
        let mut total_explicit = 0.0_f32;
        let mut auto_count: usize = 0;

        for &i in &cell_idxs {
            let c = &b.children[i];
            let em = c.style.font_size;
            if let Some(w_len) = &c.style.width
                && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
            {
                // Приводим к border-box, чтобы суммировать с другими ячейками.
                let border_w = match c.style.box_sizing {
                    BoxSizing::ContentBox => {
                        let pl = c.style.padding_left.resolve_or_zero(em, content_width, viewport);
                        let pr = c.style.padding_right.resolve_or_zero(em, content_width, viewport);
                        w + pl + pr + c.style.border_left_width + c.style.border_right_width
                    }
                    BoxSizing::BorderBox => w,
                };
                explicit_w.push(Some(border_w));
                total_explicit += border_w;
                continue;
            }
            explicit_w.push(None);
            auto_count += 1;
        }

        let auto_share = if auto_count > 0 {
            ((content_width - total_explicit) / auto_count as f32).max(0.0)
        } else {
            0.0
        };
        explicit_w.iter().map(|w| w.unwrap_or(auto_share)).collect()
    };

    // Шаг 2: раскладываем ячейки горизонтально.
    // When col_widths are pre-computed (table layout), the column width is
    // authoritative — cell's CSS `width` was only a hint for column computation.
    // Temporarily clear it so lay_out uses avail (the column width) as final width.
    let use_global = col_widths.is_some();
    let mut cur_x = content_x;
    for (j, &i) in cell_idxs.iter().enumerate() {
        let avail = resolved_w[j];
        let saved_width = if use_global { b.children[i].style.width.take() } else { None };
        lay_out(&mut b.children[i], cur_x, content_y, avail, None, measurer, viewport, pcb, hp);
        if use_global {
            b.children[i].style.width = saved_width;
        }
        let c = &b.children[i];
        let c_em = c.style.font_size;
        let mr = c.style.margin_right.resolve_or_zero(c_em, content_width, viewport);
        cur_x = c.rect.x + c.rect.width + mr;
    }

    // Шаг 3: нормализуем высоту — все ячейки = max высота строки.
    let row_h = cell_idxs
        .iter()
        .map(|&i| b.children[i].rect.height)
        .fold(0.0_f32, f32::max);
    for &i in &cell_idxs {
        b.children[i].rect.height = row_h;
    }

    row_h
}

/// CSS 2.1 §17 — table layout. Computes global column widths across all rows
/// (through `TableRowGroup` and direct `TableRow` children), then lays out
/// rows top-to-bottom in DOM order. Returns content height.
#[allow(clippy::too_many_arguments)]
fn lay_out_table(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let col_widths = compute_table_col_widths(b, content_width, viewport);

    let mut cur_y = content_y;
    let n = b.children.len();
    for i in 0..n {
        match b.children[i].kind {
            BoxKind::TableRow => {
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let row_x = content_x;
                let row_y = cur_y + c_mt;
                b.children[i].rect.x = row_x;
                b.children[i].rect.y = row_y;
                b.children[i].rect.width = content_width;
                let row_h = lay_out_table_row(
                    &mut b.children[i],
                    row_x, row_y, content_width,
                    Some(&col_widths),
                    measurer, viewport, pcb, hp,
                );
                let row_style_h = {
                    let s = &b.children[i].style;
                    if let Some(h_len) = &s.height
                        && let Some(h) = h_len.resolve(s.font_size, None, viewport)
                    {
                        let pt = s.padding_top.resolve_or_zero(s.font_size, content_width, viewport);
                        let pb = s.padding_bottom.resolve_or_zero(s.font_size, content_width, viewport);
                        match s.box_sizing {
                            BoxSizing::ContentBox => (h + pt + pb + s.border_top_width + s.border_bottom_width).max(0.0),
                            BoxSizing::BorderBox => h.max(pt + pb + s.border_top_width + s.border_bottom_width),
                        }
                    } else {
                        let pt = b.children[i].style.padding_top.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        let pb = b.children[i].style.padding_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        row_h + pt + pb + b.children[i].style.border_top_width + b.children[i].style.border_bottom_width
                    }
                };
                b.children[i].rect.height = row_style_h;
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb;
            }
            BoxKind::TableRowGroup => {
                let group_em = b.children[i].style.font_size;
                let g_mt = b.children[i].style.margin_top.resolve_or_zero(group_em, content_width, viewport);
                let group_y = cur_y + g_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = group_y;
                b.children[i].rect.width = content_width;
                let mut row_y = group_y;
                let n_rows = b.children[i].children.len();
                for r in 0..n_rows {
                    if !matches!(b.children[i].children[r].kind, BoxKind::TableRow) {
                        continue;
                    }
                    let r_em = b.children[i].children[r].style.font_size;
                    let r_mt = b.children[i].children[r].style.margin_top.resolve_or_zero(r_em, content_width, viewport);
                    b.children[i].children[r].rect.x = content_x;
                    b.children[i].children[r].rect.y = row_y + r_mt;
                    b.children[i].children[r].rect.width = content_width;
                    let row_h = lay_out_table_row(
                        &mut b.children[i].children[r],
                        content_x, row_y + r_mt, content_width,
                        Some(&col_widths),
                        measurer, viewport, pcb, hp,
                    );
                    let r_pt = b.children[i].children[r].style.padding_top.resolve_or_zero(r_em, content_width, viewport);
                    let r_pb = b.children[i].children[r].style.padding_bottom.resolve_or_zero(r_em, content_width, viewport);
                    let r_bor = b.children[i].children[r].style.border_top_width + b.children[i].children[r].style.border_bottom_width;
                    b.children[i].children[r].rect.height = row_h + r_pt + r_pb + r_bor;
                    let r_mb = b.children[i].children[r].style.margin_bottom.resolve_or_zero(r_em, content_width, viewport);
                    row_y = b.children[i].children[r].rect.y + b.children[i].children[r].rect.height + r_mb;
                }
                let g_pt = b.children[i].style.padding_top.resolve_or_zero(group_em, content_width, viewport);
                let g_pb = b.children[i].style.padding_bottom.resolve_or_zero(group_em, content_width, viewport);
                let g_bor = b.children[i].style.border_top_width + b.children[i].style.border_bottom_width;
                b.children[i].rect.height = (row_y - group_y) + g_pt + g_pb + g_bor;
                let g_mb = b.children[i].style.margin_bottom.resolve_or_zero(group_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + g_mb;
            }
            _ => {}
        }
    }
    (cur_y - content_y).max(0.0)
}

/// Scans `b`'s cells and updates `col_explicit` with the max explicit border-box
/// width for each column. Called once per row during the pre-layout scan.
fn scan_row_explicit_widths(
    row: &LayoutBox,
    col_explicit: &mut Vec<Option<f32>>,
    content_width: f32,
    viewport: Size,
) {
    let cells: Vec<_> = row
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    for (j, cell) in cells.iter().enumerate() {
        let em = cell.style.font_size;
        let w_border = if let Some(w_len) = &cell.style.width
            && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
        {
            let bw = match cell.style.box_sizing {
                BoxSizing::ContentBox => {
                    let pl = cell.style.padding_left.resolve_or_zero(em, content_width, viewport);
                    let pr = cell.style.padding_right.resolve_or_zero(em, content_width, viewport);
                    w + pl + pr + cell.style.border_left_width + cell.style.border_right_width
                }
                BoxSizing::BorderBox => w,
            };
            Some(bw)
        } else {
            None
        };
        if j >= col_explicit.len() {
            col_explicit.resize(j + 1, None);
        }
        col_explicit[j] = match (col_explicit[j], w_border) {
            (Some(existing), Some(new)) => Some(existing.max(new)),
            (Some(existing), None) => Some(existing),
            (None, v) => v,
        };
    }
}

/// Computes per-column widths for a `BoxKind::Table` element by scanning all rows
/// (direct and inside `TableRowGroup` children). Returns a `Vec<f32>` of border-box
/// widths, one per column.
fn compute_table_col_widths(b: &LayoutBox, content_width: f32, viewport: Size) -> Vec<f32> {
    let mut col_explicit: Vec<Option<f32>> = Vec::new();

    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => {
                scan_row_explicit_widths(child, &mut col_explicit, content_width, viewport);
            }
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        scan_row_explicit_widths(row, &mut col_explicit, content_width, viewport);
                    }
                }
            }
            _ => {}
        }
    }

    let n_cols = col_explicit.len();
    if n_cols == 0 {
        return Vec::new();
    }

    let total_explicit: f32 = col_explicit.iter().filter_map(|w| *w).sum();
    let auto_count = col_explicit.iter().filter(|w| w.is_none()).count();
    let auto_share = if auto_count > 0 {
        ((content_width - total_explicit) / auto_count as f32).max(0.0)
    } else {
        0.0
    };

    col_explicit.iter().map(|w| w.unwrap_or(auto_share)).collect()
}

/// CSS Multi-column Layout L1 — lays out `children` into N columns.
/// Returns content height (max column height, without padding/border).
#[allow(clippy::too_many_arguments)]
fn lay_out_multicol_children(
    children: &mut [LayoutBox],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    s: &ComputedStyle,
    em: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let cb = content_width;
    let col_gap = s.column_gap.resolve_or_zero(em, cb, viewport).max(0.0);

    // Compute column count from column-count / column-width.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
                let n_from_w = ((content_width + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport)
                && w > 0.0
            {
                ((content_width + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }.max(1);

    let col_w = ((content_width - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);

    // Collect flow (non-abs) child indices.
    let flow_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if flow_idxs.is_empty() {
        return 0.0;
    }

    // First pass at (0, 0) to measure intrinsic heights.
    for &i in &flow_idxs {
        lay_out(&mut children[i], 0.0, 0.0, col_w, None, measurer, viewport, pcb, hp);
    }

    // Outer height of each child = margin_top + rect.height + margin_bottom.
    let outer_h: Vec<f32> = children
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if flow_idxs.contains(&i) {
                let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
                let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
                mt + c.rect.height + mb
            } else {
                0.0
            }
        })
        .collect();

    // Target column height for balanced distribution.
    let total_h: f32 = flow_idxs.iter().map(|&i| outer_h[i]).sum();
    let target_h = (total_h / n_cols as f32).ceil().max(1.0);
    // Count-based per-column cap for balanced distribution when heights are equal/zero.
    let per_col_cap = flow_idxs.len().div_ceil(n_cols as usize);

    // Greedy column assignment (height + count guard).
    let mut child_col = vec![0usize; children.len()];
    let mut col_fill = vec![0.0f32; n_cols as usize];
    let mut col_count = vec![0usize; n_cols as usize];
    let mut cur_col = 0usize;
    for &i in &flow_idxs {
        let height_overflow = col_fill[cur_col] + outer_h[i] > target_h && outer_h[i] > 0.0;
        let count_overflow = col_count[cur_col] >= per_col_cap;
        if cur_col + 1 < n_cols as usize && (height_overflow || count_overflow) {
            cur_col += 1;
        }
        child_col[i] = cur_col;
        col_fill[cur_col] += outer_h[i];
        col_count[cur_col] += 1;
    }

    // Second pass: final positioning.
    let mut col_y = vec![content_y; n_cols as usize];
    for &i in &flow_idxs {
        let col = child_col[i];
        let col_x = content_x + col as f32 * (col_w + col_gap);
        lay_out(&mut children[i], col_x, col_y[col], col_w, None, measurer, viewport, pcb, hp);
        let mb = children[i]
            .style
            .margin_bottom
            .resolve_or_zero(children[i].style.font_size, col_w, viewport);
        col_y[col] = children[i].rect.y + children[i].rect.height + mb;
    }

    // Content height = tallest column.
    col_y.iter().map(|&cy| cy - content_y).fold(0.0f32, f32::max)
}

/// Positions absolutely/fixed-positioned deferred children of `parent`.
/// Called after parent's height is finalized so `my_pcb` is complete.
fn lay_out_abs_children(
    parent: &mut LayoutBox,
    deferred: &[(usize, f32, f32)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    for &(idx, static_x, static_y) in deferred {
        let cs = parent.children[idx].style.clone();
        let c_em = cs.font_size;

        let cb = if matches!(cs.position, Position::Fixed) {
            Rect::new(0.0, 0.0, viewport.width, viewport.height)
        } else {
            my_pcb
        };

        let left = cs.left.resolve(c_em, cb.width, viewport);
        let right = cs.right.resolve(c_em, cb.width, viewport);
        let top = cs.top.resolve(c_em, cb.height, viewport);
        let bottom = cs.bottom.resolve(c_em, cb.height, viewport);

        // Доступная ширина для layout абсолютного child.
        let avail_w = if left.is_some() && right.is_some() && cs.width.is_none() {
            (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0)
        } else {
            cb.width
        };

        lay_out(&mut parent.children[idx], 0.0, 0.0, avail_w, None, measurer, viewport, my_pcb, hp);

        let c_ml = cs.margin_left.resolve_or_zero(c_em, cb.width, viewport);
        let c_mr = cs.margin_right.resolve_or_zero(c_em, cb.width, viewport);
        let c_mt = cs.margin_top.resolve_or_zero(c_em, cb.height, viewport);
        let c_mb = cs.margin_bottom.resolve_or_zero(c_em, cb.height, viewport);

        let child = &mut parent.children[idx];

        // Desired border-left edge.
        let new_x = match (left, right) {
            (Some(l), _)    => cb.x + l + c_ml,
            (None, Some(r)) => cb.x + cb.width - r - c_mr - child.rect.width,
            (None, None)    => static_x + c_ml,
        };
        // Desired border-top edge.
        let new_y = match (top, bottom) {
            (Some(t), _)    => cb.y + t + c_mt,
            (None, Some(bv)) => cb.y + cb.height - bv - c_mb - child.rect.height,
            (None, None)    => static_y + c_mt,
        };

        let dx = new_x - child.rect.x;
        let dy = new_y - child.rect.y;
        shift_tree(child, dx, dy);
    }
}

/// CSS Flexbox L1 §9 — single-line flex layout (Phase 0).
///
/// Алгоритм:
/// 1. Для каждого flex-item вычисляем hypothetical main size из flex-basis.
/// 2. Распределяем free space через flex-grow / flex-shrink.
/// 3. Раскладываем items с учётом justify-content и align-items.
///
/// Ограничения Phase 0: `nowrap` only (multi-line — задача 4B.5);
/// column-direction: cross-axis = container width (auto stretch).
///
/// Возвращает `content_height` (вертикальный размер контентной зоны контейнера).
#[allow(clippy::too_many_arguments)]
fn lay_out_flex(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let is_column = matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
    let is_reverse = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let is_wrap = matches!(s.flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let is_wrap_reverse = matches!(s.flex_wrap, FlexWrap::WrapReverse);

    // Indices of non-Skip children (actual flex items).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();
    // CSS Flexbox L1 §4 — stable sort by `order` (same-order items keep source order).
    item_idxs.sort_by_key(|&i| children[i].style.order);

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Container main size (for row: width; for column: 0 = auto, computed from items).
    let container_main = if is_column { 0.0 } else { content_width };

    // CSS Box Alignment §8: gap is fixed space between items, subtracted before flex-grow/shrink.
    let em = s.font_size;
    // item_gap: gap between items along the main axis.
    // cross_gap: gap between flex lines along the cross axis (wrap only).
    let item_gap = if is_column {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };
    let cross_gap = if is_column {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };

    // Step 1 — preliminary layout for intrinsic sizes.
    let cb = content_width;
    for &i in &item_idxs {
        lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, pcb, hp);
    }

    // Compute hypothetical main sizes for all items (outer = including margins).
    let all_hyp: Vec<f32> = item_idxs
        .iter()
        .map(|&i| {
            let item = &children[i];
            let is = &item.style;
            let iem = is.font_size;
            let m_l = is.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = is.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
            match &is.flex_basis {
                FlexBasis::Auto | FlexBasis::Content => {
                    if is_column { item.rect.height + m_t + m_b } else { item.rect.width + m_l + m_r }
                }
                FlexBasis::Length(l) => {
                    let base = l.resolve(iem, Some(cb), viewport).unwrap_or(0.0).max(0.0);
                    if is_column { base + m_t + m_b } else { base + m_l + m_r }
                }
            }
        })
        .collect();

    // Step 2 — break items into flex lines.
    // Wrap only applies to row direction (column wrapping requires known container height, Phase 0: skip).
    let lines: Vec<Vec<usize>> = if is_wrap && !is_column && container_main > 0.0 {
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut cur_line: Vec<usize> = Vec::new();
        let mut cur_main = 0.0_f32;
        for (k, &item_main) in all_hyp.iter().enumerate() {
            let gap = if cur_line.is_empty() { 0.0 } else { item_gap };
            if !cur_line.is_empty() && cur_main + gap + item_main > container_main {
                lines.push(cur_line);
                cur_line = vec![k];
                cur_main = item_main;
            } else {
                cur_line.push(k);
                cur_main += gap + item_main;
            }
        }
        if !cur_line.is_empty() {
            lines.push(cur_line);
        }
        lines
    } else {
        vec![(0..item_idxs.len()).collect()]
    };

    // Step 3–5: process each line (grow/shrink, justify, position, align).
    // cross_cursor tracks the current cross-axis offset across lines.
    let mut cross_cursor = 0.0_f32;

    let n_lines = lines.len();
    let ordered_line_idxs: Vec<usize> = if is_wrap_reverse {
        (0..n_lines).rev().collect()
    } else {
        (0..n_lines).collect()
    };

    for li in &ordered_line_idxs {
        let line_keys = &lines[*li]; // keys into item_idxs
        let n = line_keys.len();

        // Per-line hyp mains (mutable for grow/shrink).
        let mut hyp_mains: Vec<f32> = line_keys.iter().map(|&k| all_hyp[k]).collect();

        // Free space after gaps.
        let line_gap_total = if n > 1 { item_gap * (n - 1) as f32 } else { 0.0 };
        let total_hyp: f32 = hyp_mains.iter().sum();
        let free_space = if is_column { 0.0 } else { container_main - total_hyp - line_gap_total };

        if free_space > 0.0 {
            let total_grow: f32 = line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).sum();
            if total_grow > 0.0 {
                for (j, &k) in line_keys.iter().enumerate() {
                    let grow = children[item_idxs[k]].style.flex_grow;
                    hyp_mains[j] += free_space * (grow / total_grow);
                }
            }
        } else if free_space < 0.0 {
            let weights: Vec<f32> = line_keys
                .iter()
                .enumerate()
                .map(|(j, &k)| children[item_idxs[k]].style.flex_shrink * hyp_mains[j])
                .collect();
            let total_weight: f32 = weights.iter().sum();
            if total_weight > 0.0 {
                for j in 0..n {
                    hyp_mains[j] = (hyp_mains[j] + free_space * (weights[j] / total_weight)).max(0.0);
                }
            }
        }

        // Justify-content within the line.
        let resolved_main: f32 = hyp_mains.iter().sum();
        let remaining = if is_column { 0.0 } else { (container_main - resolved_main - line_gap_total).max(0.0) };
        let (jc_start, jc_gap) = match s.justify_content {
            AlignValue::End => (remaining, 0.0),
            AlignValue::Center => (remaining / 2.0, 0.0),
            AlignValue::SpaceBetween => {
                if n <= 1 { (0.0, 0.0) } else { (0.0, remaining / (n - 1) as f32) }
            }
            AlignValue::SpaceAround => {
                let per = remaining / n as f32;
                (per / 2.0, per)
            }
            AlignValue::SpaceEvenly => {
                let per = remaining / (n + 1) as f32;
                (per, per)
            }
            _ => (0.0, 0.0),
        };

        // Final layout: position items along main axis.
        let ordered_keys: Vec<usize> = if is_reverse { (0..n).rev().collect() } else { (0..n).collect() };
        let mut main_cursor = jc_start;

        for &j in &ordered_keys {
            let k = line_keys[j];
            let i = item_idxs[k];
            let outer_main = hyp_mains[j];
            let item_s = children[i].style.clone();
            let iem = item_s.font_size;
            let m_l = item_s.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = item_s.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = item_s.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = item_s.margin_bottom.resolve_or_zero(iem, cb, viewport);

            if is_column {
                let inner_main = (outer_main - m_t - m_b).max(0.0);
                children[i].style.height = Some(Length::Px(inner_main));
                lay_out(
                    &mut children[i],
                    content_x + m_l,
                    content_y + main_cursor + m_t,
                    content_width - m_l - m_r,
                    None,
                    measurer,
                    viewport,
                    pcb,
                    hp,
                );
                main_cursor += outer_main + item_gap + jc_gap;
            } else {
                let inner_main = (outer_main - m_l - m_r).max(0.0);
                children[i].style.width = Some(Length::Px(inner_main));
                lay_out(
                    &mut children[i],
                    content_x + main_cursor + m_l,
                    content_y + cross_cursor + m_t,
                    inner_main,
                    None,
                    measurer,
                    viewport,
                    pcb,
                    hp,
                );
                main_cursor += outer_main + item_gap + jc_gap;
            }
        }

        // Align-items on cross axis for this line.
        let line_cross: f32 = if is_column {
            0.0 // column cross axis (width) not handled in wrap Phase 0
        } else {
            line_keys.iter().map(|&k| children[item_idxs[k]].rect.height).fold(0.0_f32, f32::max)
        };

        if !is_column {
            for &k in line_keys {
                let i = item_idxs[k];
                let item = &mut children[i];
                let is = &item.style;
                let iem = is.font_size;
                let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
                let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
                let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
                let outer_cross = item.rect.height + m_t + m_b;
                match align {
                    AlignValue::End => {
                        item.rect.y = content_y + cross_cursor + line_cross - outer_cross + m_t;
                    }
                    AlignValue::Center => {
                        item.rect.y = content_y + cross_cursor + m_t + (line_cross - outer_cross) / 2.0;
                    }
                    AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                        let stretch_h = (line_cross - m_t - m_b).max(item.rect.height);
                        if item.rect.height < stretch_h {
                            item.rect.height = stretch_h;
                        }
                        item.rect.y = content_y + cross_cursor + m_t;
                    }
                    _ => {
                        item.rect.y = content_y + cross_cursor + m_t;
                    }
                }
            }
        }

        cross_cursor += line_cross + cross_gap;
    }

    // Remove trailing gap from cross_cursor.
    let total_cross = if n_lines > 1 {
        cross_cursor - cross_gap
    } else {
        cross_cursor
    };

    if is_column {
        // Column: return main-axis height (main_cursor from last line).
        // Re-compute from stored item positions.
        item_idxs
            .iter()
            .map(|&i| children[i].rect.y + children[i].rect.height - content_y)
            .fold(0.0_f32, f32::max)
    } else {
        total_cross
    }
}

/// CSS Grid Layout Level 1 — grid container layout.
///
/// Implements a Phase-0 subset of the grid layout algorithm (CSS Grid L1 §12):
///
/// - Explicit track lists (grid-template-columns / rows) with px, fr, auto.
/// - `repeat(N, size)` expansion.
/// - `minmax(min, max)` — min side used for sizing.
/// - Integer line numbers (positive only), `span N`, and `auto` placement.
/// - `grid-auto-flow: row | column` (no dense packing).
/// - `gap` / `column-gap` / `row-gap` between cells.
/// - `align-items` / `justify-items` within cells.
///
/// Returns the total content height of the grid.
#[allow(clippy::too_many_arguments)]
fn lay_out_grid(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let em = s.font_size;

    // Indices of actual items (non-Skip).
    let item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Gap between tracks.
    let col_gap = s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0);
    let row_gap = s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0);

    // Determine explicit track counts.
    let n_explicit_cols = s.grid_template_columns.len().max(1);

    // --- Step 1: Resolve placements for every item ---
    // placement: (col_start, col_end, row_start, row_end) all 1-based inclusive/exclusive.
    let mut placements: Vec<(u32, u32, u32, u32)> = vec![(0, 0, 0, 0); item_idxs.len()];

    let row_flow = !matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);

    // Pass 1: items with fully explicit placements.
    for (k, &i) in item_idxs.iter().enumerate() {
        let is = &children[i].style;

        // Resolve named area references first (grid-area: <name> shorthand or
        // individual grid-{row,column}-{start,end}: <name> values).
        let (named_cs, named_ce, named_rs, named_re) = {
            let has_named = matches!(&is.grid_column_start, GridLine::Named(_))
                || matches!(&is.grid_column_end, GridLine::Named(_))
                || matches!(&is.grid_row_start, GridLine::Named(_))
                || matches!(&is.grid_row_end, GridLine::Named(_));
            if has_named && !s.grid_template_areas.is_empty() {
                resolve_named_lines(
                    &is.grid_column_start,
                    &is.grid_column_end,
                    &is.grid_row_start,
                    &is.grid_row_end,
                    &s.grid_template_areas,
                )
            } else {
                (0, 0, 0, 0)
            }
        };

        // For each axis: use resolved named value if non-zero, else fall back to
        // the normal numeric/span resolver.
        let cs = if named_cs != 0 { named_cs } else { resolve_grid_line(&is.grid_column_start, n_explicit_cols as u32) };
        let ce = if named_ce != 0 { named_ce } else { resolve_grid_line_end(&is.grid_column_end, cs, n_explicit_cols as u32) };
        let rs = if named_rs != 0 { named_rs } else { resolve_grid_line(&is.grid_row_start, 0) };
        let re = if named_re != 0 { named_re } else { resolve_grid_line_end(&is.grid_row_end, rs, 0) };

        // `grid-column: span N` → start=Span(N), end=Auto → cs=0, ce=0.
        // resolve_grid_line returns 0 for Span-on-start, losing the count.
        // Recover the span so Pass 2 can use it for placement sizing.
        let ce = if ce == 0 {
            match &is.grid_column_start { GridLine::Span(n) => *n, _ => 0 }
        } else { ce };
        let re = if re == 0 {
            match &is.grid_row_start { GridLine::Span(n) => *n, _ => 0 }
        } else { re };

        if cs != 0 && rs != 0 {
            // Fully explicit: both axes known.
            placements[k] = (cs, ce, rs, re);
        } else if cs != 0 {
            // Column position fixed, row auto; preserve row-span if declared.
            placements[k] = (cs, ce, 0, re);
        } else if rs != 0 {
            // Row position fixed, column auto; preserve col-span if declared.
            placements[k] = (0, ce, rs, re);
        } else if ce > 0 || re > 0 {
            // Both axes auto but at least one span is declared (e.g. grid-column:span 2).
            // Store so pass-2 can recover the span via `end - 0 = span`.
            placements[k] = (0, ce, 0, re);
        }
        // All-auto no spans: stays (0,0,0,0) → span=1 in pass 2.
    }

    // Pass 2: auto-place remaining items — CSS Grid L1 §8.5 auto-placement algorithm.
    //
    // Two packing modes:
    //   Sparse (grid-auto-flow: row | column): cursor only moves forward.
    //   Dense  (grid-auto-flow: row dense | column dense): each item scans from
    //          (1,1) so it can fill gaps left by larger items.
    //
    // Occupancy HashSet replaces the O(k²) overlap scan from Pass 1 with O(1)
    // per-cell lookups.
    let dense = matches!(s.grid_auto_flow, GridAutoFlow::RowDense | GridAutoFlow::ColumnDense);
    let mut occupied: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for &(cs, ce, rs, re) in &placements {
        if cs != 0 && rs != 0 {
            for r in rs..re {
                for c in cs..ce {
                    occupied.insert((c, r));
                }
            }
        }
    }

    let mut cursor_row: u32 = 1;
    let mut cursor_col: u32 = 1;

    for (k, _) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs != 0 && rs != 0 {
            continue; // explicitly placed
        }

        let col_span = if ce > cs { ce - cs } else { 1 };
        let row_span = if re > rs { re - rs } else { 1 };

        if row_flow {
            let fixed_cs = if cs != 0 { cs } else { 0 };
            let fixed_ce = if cs != 0 { ce } else { 0 };

            // Dense packing starts each scan from (1,1); sparse continues from cursor.
            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            loop {
                let try_c   = if fixed_cs != 0 { fixed_cs } else { scan_c };
                let try_ce_val = if fixed_cs != 0 { fixed_ce } else { try_c + col_span };

                // Bounds: item must fit within explicit column count (or 1-col fallback).
                let fits = (try_ce_val - 1) <= n_explicit_cols as u32 || n_explicit_cols == 1;
                let cell_free = fits && (try_c..try_ce_val)
                    .all(|c| (scan_r..scan_r + row_span).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (try_c, try_ce_val, scan_r, scan_r + row_span);
                    for r in scan_r..scan_r + row_span {
                        for c in try_c..try_ce_val {
                            occupied.insert((c, r));
                        }
                    }
                    // Track highest placed row for grid-size calculation.
                    cursor_row = cursor_row.max(scan_r);
                    if !dense {
                        cursor_col = try_ce_val;
                        if cursor_col > n_explicit_cols as u32 {
                            cursor_col = 1;
                            cursor_row += 1;
                        }
                    }
                    break;
                }

                // Advance scan position.
                if fixed_cs != 0 {
                    scan_r += 1;
                    scan_c = 1;
                } else {
                    scan_c += 1;
                    if scan_c > n_explicit_cols as u32 {
                        scan_c = 1;
                        scan_r += 1;
                    }
                }
            }
        } else {
            // Column flow: fill top-to-bottom, wrap to next column.
            let n_explicit_rows = s.grid_template_rows.len().max(1) as u32;
            let fixed_rs = if rs != 0 { rs } else { 0 };
            let fixed_re = if rs != 0 { re } else { 0 };

            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            loop {
                let try_r      = if fixed_rs != 0 { fixed_rs } else { scan_r };
                let try_re_val = if fixed_rs != 0 { fixed_re } else { try_r + row_span };

                let fits = (try_re_val - 1) <= n_explicit_rows || n_explicit_rows == 1;
                let cell_free = fits && (scan_c..scan_c + col_span)
                    .all(|c| (try_r..try_re_val).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (scan_c, scan_c + col_span, try_r, try_re_val);
                    for r in try_r..try_re_val {
                        for c in scan_c..scan_c + col_span {
                            occupied.insert((c, r));
                        }
                    }
                    cursor_col = cursor_col.max(scan_c);
                    if !dense {
                        cursor_row = try_re_val;
                        if cursor_row > n_explicit_rows {
                            cursor_row = 1;
                            cursor_col += 1;
                        }
                    }
                    break;
                }

                if fixed_rs != 0 {
                    scan_c += 1;
                    scan_r = 1;
                } else {
                    scan_r += 1;
                    if scan_r > n_explicit_rows {
                        scan_r = 1;
                        scan_c += 1;
                    }
                }
            }
        }
    }

    // --- Step 2: Determine total grid dimensions ---
    let n_cols = placements.iter().map(|&(_, ce, _, _)| ce.saturating_sub(1)).max().unwrap_or(1)
        .max(n_explicit_cols as u32);
    let n_rows = placements.iter().map(|&(_, _, _, re)| re.saturating_sub(1)).max().unwrap_or(1);

    // --- Step 3: Compute column widths ---
    // Compute fixed column widths.
    let mut col_widths: Vec<f32> = (0..n_cols)
        .map(|c| {
            let ts = grid_track(c, &s.grid_template_columns, &s.grid_auto_columns);
            match ts {
                GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                _ => 0.0, // fr / auto resolved later
            }
        })
        .collect();

    // Total gap between columns.
    let total_col_gap = if n_cols > 1 { col_gap * (n_cols - 1) as f32 } else { 0.0 };
    let fixed_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
    let free_col = (content_width - fixed_col_total).max(0.0);

    // Distribute fr among column tracks.
    let total_fr: f32 = (0..n_cols)
        .map(|c| grid_track(c, &s.grid_template_columns, &s.grid_auto_columns).fr().unwrap_or(0.0))
        .sum();
    let auto_col_count = (0..n_cols)
        .filter(|&c| matches!(
            grid_track(c, &s.grid_template_columns, &s.grid_auto_columns),
            GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent
        ))
        .count();

    // For auto columns, divide remaining free space equally (after fr).
    let fr_width = if total_fr > 0.0 { free_col / total_fr } else { 0.0 };
    let auto_col_width = if auto_col_count > 0 && total_fr == 0.0 {
        free_col / auto_col_count as f32
    } else {
        0.0
    };

    for c in 0..n_cols {
        match grid_track(c, &s.grid_template_columns, &s.grid_auto_columns) {
            GridTrackSize::Fr(f) => col_widths[c as usize] = (f * fr_width).max(0.0),
            GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent => {
                col_widths[c as usize] = auto_col_width;
            }
            _ => {}
        }
    }

    // Column start offsets.
    let mut col_offsets: Vec<f32> = Vec::with_capacity(n_cols as usize);
    let mut x_off = 0.0_f32;
    for c in 0..n_cols {
        col_offsets.push(x_off);
        x_off += col_widths[c as usize] + if c < n_cols - 1 { col_gap } else { 0.0 };
    }

    // --- Step 4: Layout items to measure row heights ---
    // Explicit row heights.
    let mut row_heights: Vec<f32> = (0..n_rows)
        .map(|r| {
            match grid_track(r, &s.grid_template_rows, &s.grid_auto_rows) {
                GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                _ => 0.0,
            }
        })
        .collect();

    // Layout each item in its cell to determine content height.
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, _re) = placements[k];
        if cs == 0 || rs == 0 {
            continue; // unplaced (should not happen after auto-placement)
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let cell_w: f32 = if c1 > c0 {
            col_widths[c0..c1].iter().sum::<f32>() + col_gap * (c1 - c0 - 1) as f32
        } else {
            col_widths[c0]
        };
        // Layout at temporary position (y=0) to get intrinsic height.
        lay_out(&mut children[i], content_x + col_offsets[c0], 0.0, cell_w, None, measurer, viewport, pcb, hp);
        // Update auto row heights.
        let r0 = (rs - 1) as usize;
        if r0 < row_heights.len()
            && matches!(
                grid_track(r0 as u32, &s.grid_template_rows, &s.grid_auto_rows),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::Fr(_)
            )
        {
            let item_h = children[i].rect.height;
            if item_h > row_heights[r0] {
                row_heights[r0] = item_h;
            }
        }
    }

    // Resolve fr row heights.
    let total_row_gap = if n_rows > 1 { row_gap * (n_rows - 1) as f32 } else { 0.0 };
    let fixed_row_total: f32 = row_heights.iter().sum::<f32>() + total_row_gap;
    // If container has explicit height, distribute fr rows from it.
    let container_h = s.height.as_ref().and_then(|h| h.resolve(em, Some(content_width), viewport));
    let free_row = container_h.map(|h| (h - fixed_row_total).max(0.0)).unwrap_or(0.0);
    let total_row_fr: f32 = (0..n_rows)
        .map(|r| grid_track(r, &s.grid_template_rows, &s.grid_auto_rows).fr().unwrap_or(0.0))
        .sum();
    if total_row_fr > 0.0 && free_row > 0.0 {
        let fr_h = free_row / total_row_fr;
        for r in 0..n_rows {
            if let Some(f) = grid_track(r, &s.grid_template_rows, &s.grid_auto_rows).fr() {
                row_heights[r as usize] = (f * fr_h).max(row_heights[r as usize]);
            }
        }
    }

    // Row top offsets.
    let mut row_offsets: Vec<f32> = Vec::with_capacity(n_rows as usize);
    let mut y_off = 0.0_f32;
    for r in 0..n_rows {
        row_offsets.push(y_off);
        y_off += row_heights[r as usize] + if r < n_rows - 1 { row_gap } else { 0.0 };
    }

    // --- Step 5: Final positioning pass ---
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            // Unplaced — stack below grid content.
            lay_out(&mut children[i], content_x, content_y + y_off, content_width, None, measurer, viewport, pcb, hp);
            y_off += children[i].rect.height;
            continue;
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let r0 = (rs - 1).min(n_rows - 1) as usize;
        let r1 = (re - 1).min(n_rows) as usize;

        let cell_x = content_x + col_offsets[c0];
        let cell_y = content_y + row_offsets[r0];
        let cell_w: f32 = if c1 > c0 {
            col_widths[c0..c1].iter().sum::<f32>() + col_gap * (c1 - c0 - 1) as f32
        } else {
            col_widths[c0]
        };
        let cell_h: f32 = if r1 > r0 {
            row_heights[r0..r1].iter().sum::<f32>() + row_gap * (r1 - r0 - 1) as f32
        } else {
            row_heights[r0]
        };

        // Re-layout with final cell width.
        lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp);

        let item = &mut children[i];
        let is = &item.style;
        let iem = is.font_size;
        let m_t = is.margin_top.resolve_or_zero(iem, content_width, viewport);
        let m_b = is.margin_bottom.resolve_or_zero(iem, content_width, viewport);
        let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
        let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);

        // align-items (cross / block axis within cell).
        let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
        let item_outer_h = item.rect.height + m_t + m_b;
        match align {
            AlignValue::End => {
                item.rect.y = cell_y + cell_h - item.rect.height - m_b;
            }
            AlignValue::Center => {
                item.rect.y = cell_y + (cell_h - item_outer_h) / 2.0 + m_t;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                if item.rect.height < cell_h - m_t - m_b {
                    item.rect.height = (cell_h - m_t - m_b).max(item.rect.height);
                }
                item.rect.y = cell_y + m_t;
            }
            _ => {
                item.rect.y = cell_y + m_t;
            }
        }

        // justify-items (inline axis within cell).
        let justify = if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self };
        let item_outer_w = item.rect.width + m_l + m_r;
        match justify {
            AlignValue::End => {
                item.rect.x = cell_x + cell_w - item.rect.width - m_r;
            }
            AlignValue::Center => {
                item.rect.x = cell_x + (cell_w - item_outer_w) / 2.0 + m_l;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                item.rect.x = cell_x + m_l;
            }
            _ => {
                item.rect.x = cell_x + m_l;
            }
        }
    }

    y_off
}

/// Return the track size for track index `idx` (0-based) from a template list,
/// falling back to `auto_track` for implicit tracks beyond the template.
fn grid_track<'a>(idx: u32, template: &'a [GridTrackSize], auto_track: &'a GridTrackSize) -> &'a GridTrackSize {
    template.get(idx as usize).unwrap_or(auto_track)
}

/// Resolve a `GridLine` to a 1-based track number, or 0 if auto.
fn resolve_grid_line(line: &GridLine, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => 0,
        GridLine::Line(n) => {
            if *n > 0 {
                *n as u32
            } else if n_tracks > 0 {
                // Negative line numbers count from the end.
                (n_tracks as i32 + 1 + n).max(1) as u32
            } else {
                1
            }
        }
        GridLine::Span(_) => 0, // span on start — auto
    }
}

/// Resolve a grid-line end given start position and span.
fn resolve_grid_line_end(line: &GridLine, start: u32, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => {
            if start > 0 { start + 1 } else { 0 }
        }
        GridLine::Line(n) => {
            if *n > 0 {
                (*n as u32).max(start + 1)
            } else if n_tracks > 0 {
                let abs = (n_tracks as i32 + 1 + n).max(1) as u32;
                abs.max(start + 1)
            } else {
                start + 1
            }
        }
        GridLine::Span(n) => {
            // When start is known: end = start + span.
            // When start is auto (0): store span N directly so pass-2 placement
            // can use `re - rs = N - 0 = N` to recover the span count.
            if start > 0 { start + n } else { *n }
        }
    }
}

/// CSS Grid L1 §7.3 — locate a named area in `grid-template-areas`.
///
/// Returns `(row_start, row_end, col_start, col_end)` as 1-based exclusive
/// line numbers, or `None` if the name is not found. Handles rectangular
/// area shapes only (CSS Grid L1 requires areas to be rectangular).
fn find_named_area(areas: &[Vec<String>], name: &str) -> Option<(u32, u32, u32, u32)> {
    let mut row_start: Option<u32> = None;
    let mut row_end: Option<u32> = None;
    let mut col_start: Option<u32> = None;
    let mut col_end: Option<u32> = None;
    for (r, row) in areas.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell == name {
                let rs = (r + 1) as u32;
                let re = (r + 2) as u32;
                let cs = (c + 1) as u32;
                let ce = (c + 2) as u32;
                row_start = Some(row_start.map_or(rs, |v: u32| v.min(rs)));
                row_end   = Some(row_end.map_or(re,   |v: u32| v.max(re)));
                col_start = Some(col_start.map_or(cs, |v: u32| v.min(cs)));
                col_end   = Some(col_end.map_or(ce,   |v: u32| v.max(ce)));
            }
        }
    }
    Some((row_start?, row_end?, col_start?, col_end?))
}

/// Resolve named grid-line references for a single item against the
/// container's `grid-template-areas`. Returns `(col_start, col_end, row_start, row_end)`.
///
/// When all four placement properties are `Named(same_name)` (set by
/// `grid-area: <name>` shorthand), the area bounds are looked up once and
/// applied to all four axes. Mixed named/unnamed configurations fall back
/// to `Auto` (0) for any unresolved axis.
fn resolve_named_lines(
    col_start: &GridLine,
    col_end: &GridLine,
    row_start: &GridLine,
    row_end: &GridLine,
    areas: &[Vec<String>],
) -> (u32, u32, u32, u32) {
    // When grid-area: <name> sets all four to Named(name), resolve as one area.
    if let (
        GridLine::Named(n_cs),
        GridLine::Named(n_ce),
        GridLine::Named(n_rs),
        GridLine::Named(n_re),
    ) = (col_start, col_end, row_start, row_end)
        && n_cs == n_ce
        && n_ce == n_rs
        && n_rs == n_re
        && let Some((rs, re, cs, ce)) = find_named_area(areas, n_cs)
    {
        return (cs, ce, rs, re);
    }
    // Partial Named references: each axis resolved independently.
    let cs = if let GridLine::Named(n) = col_start {
        find_named_area(areas, n).map_or(0, |(_, _, cs, _)| cs)
    } else { 0 };
    let ce = if let GridLine::Named(n) = col_end {
        find_named_area(areas, n).map_or(0, |(_, _, _, ce)| ce)
    } else { 0 };
    let rs = if let GridLine::Named(n) = row_start {
        find_named_area(areas, n).map_or(0, |(rs, _, _, _)| rs)
    } else { 0 };
    let re = if let GridLine::Named(n) = row_end {
        find_named_area(areas, n).map_or(0, |(_, re, _, _)| re)
    } else { 0 };
    (cs, ce, rs, re)
}

/// Strips U+00AD (soft hyphens) from a word and collects break positions
/// (byte offsets in the returned display string).
fn strip_soft_hyphens(raw: &str) -> (String, Vec<usize>) {
    let mut display = String::with_capacity(raw.len());
    let mut positions: Vec<usize> = Vec::new();
    for ch in raw.chars() {
        if ch == '\u{00AD}' {
            positions.push(display.len());
        } else {
            display.push(ch);
        }
    }
    (display, positions)
}

/// Measures text width (letter_spacing applied between each character).
/// `tab_size` is used for `\t` characters; pass 0.0 when text contains no tabs.
fn measure_text_w(text: &str, font_size: f32, letter_spacing: f32, tab_size: f32, m: &dyn TextMeasurer) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' { tab_size } else { m.char_width(c, font_size) };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// Tries to find a hyphenation break in `display` that fits within `available_w`.
/// `break_positions` are byte offsets in `display` (already sorted ascending).
/// Returns `(prefix_with_hyphen, suffix)` for the rightmost fitting break, or `None`.
fn try_hyp_break(
    display: &str,
    available_w: f32,
    font_size: f32,
    letter_spacing: f32,
    m: &dyn TextMeasurer,
    break_positions: &[usize],
) -> Option<(String, String)> {
    if break_positions.is_empty() || available_w <= 0.0 {
        return None;
    }
    let hyphen_w = m.char_width('-', font_size) + letter_spacing;
    // Try from rightmost to leftmost — most characters on current line preferred.
    for &pos in break_positions.iter().rev() {
        if !display.is_char_boundary(pos) || pos == 0 {
            continue;
        }
        let prefix = &display[..pos];
        let prefix_w = measure_text_w(prefix, font_size, letter_spacing, 0.0, m);
        if prefix_w + hyphen_w <= available_w {
            let mut pfx = prefix.to_string();
            pfx.push('-');
            return Some((pfx, display[pos..].to_string()));
        }
    }
    None
}

/// Разбивает потоковые сегменты на строки.
///
/// Алгоритм: жадный word-wrap + опциональные переносы (hyphens: manual/auto).
/// Слова одного стиля на одной строке сливаются
/// Returns the byte offset where `word` must be split so the prefix fits within
/// `avail_px`. Guarantees at least one character in the prefix to prevent
/// infinite loops when even a single character is wider than `avail_px`.
/// Returns `word.len()` when the whole word fits.
fn char_break_offset(
    word: &str,
    avail_px: f32,
    font_size: f32,
    ls: f32,
    m: &dyn TextMeasurer,
) -> usize {
    let mut w = 0.0_f32;
    for (char_idx, (byte_pos, ch)) in word.char_indices().enumerate() {
        let cw = m.char_width(ch, font_size);
        // Width of prefix ending at this char: sum(cw + ls) - ls.
        // For first char: width = cw (no trailing letter-spacing).
        let prefix_w = if char_idx == 0 { cw } else { w + ls + cw };
        if prefix_w > avail_px {
            if char_idx == 0 {
                // Even the first char overflows — emit it to avoid infinite loop.
                return byte_pos + ch.len_utf8();
            }
            return byte_pos;
        }
        w = prefix_w;
    }
    word.len()
}

/// в один `InlineFrag`. Сегменты обрабатываются по одному, чтобы учитывать
/// `pre_space` / `post_space` (inline box model: margin + border + padding).
/// `white_space` controls whether whitespace is preserved (pre/pre-wrap).
#[allow(clippy::too_many_arguments)]
fn wrap_inline_run(
    segments: &[InlineSegment],
    max_width: f32,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
) -> Vec<Vec<InlineFrag>> {
    let space_w = m.char_width(' ', container_font_size);

    let mut result: Vec<Vec<InlineFrag>> = Vec::new();
    let mut current_line: Vec<InlineFrag> = Vec::new();
    // CSS Text L3 §7.1: text-indent только на первой строке.
    let mut current_x = text_indent;

    for seg in segments {
        // Forced line break from \n in white-space: pre/pre-wrap text.
        if seg.forced_break {
            result.push(std::mem::take(&mut current_line));
            current_x = 0.0;
            continue;
        }

        // Pre-mode: whitespace preserved, no word wrapping, tabs are tab_size wide.
        if white_space.preserves_whitespace() {
            if seg.text.is_empty() {
                continue;
            }
            let style = &seg.style;
            let em = style.font_size;
            let ls = style.letter_spacing;
            let tab_size = style.tab_size;
            let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_x += seg.pre_space;
            let frag_x = current_x;
            let frag_w = measure_text_w(&seg.text, em, ls, tab_size, m);
            current_line.push(InlineFrag {
                x: frag_x,
                y_offset: 0.0,
                width: frag_w,
                text: seg.text.clone(),
                style: style.clone(),
                padding_left: pad_l,
                padding_right: pad_r,
                is_element_box: seg.is_element_box,
                img_src: None,
            });
            current_x += frag_w + seg.post_space;
            continue;
        }

        // Image segments are fixed-width, non-breakable inline replaced elements.
        if let Some(img_src) = &seg.img_src {
            let img_w = seg.img_width;
            let gap = if current_line.is_empty() { 0.0 } else { space_w };
            if !current_line.is_empty() && current_x + gap + seg.pre_space + img_w > max_width {
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
            }
            let line_gap = if current_line.is_empty() { 0.0 } else { space_w };
            current_x += line_gap + seg.pre_space;
            let em = seg.style.font_size;
            let pad_l = seg.style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = seg.style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_line.push(InlineFrag {
                x: current_x,
                y_offset: 0.0,
                width: img_w,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: pad_l,
                padding_right: pad_r,
                is_element_box: true,
                img_src: Some(img_src.clone()),
            });
            current_x += img_w + seg.post_space;
            continue;
        }

        // Collect words; split_whitespace preserves U+00AD within tokens.
        let raw_words: Vec<&str> = seg.text.split_whitespace().collect();
        if raw_words.is_empty() {
            continue;
        }
        let style = &seg.style;
        let em = style.font_size;
        let ls = style.letter_spacing;
        let ws = style.word_spacing;
        let inter_word = space_w + ls + ws;

        // Resolved padding for this segment's inline box (for paint use).
        let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
        let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);

        let n = raw_words.len();
        for (wi, raw_word) in raw_words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;

            // Strip soft hyphens for display + collect hyphenation break positions.
            let (display_word, shy_positions) = strip_soft_hyphens(raw_word);

            // Space that the inline box model contributes at the word boundaries.
            let pre = if is_seg_first { seg.pre_space } else { 0.0 };
            let post = if is_seg_last { seg.post_space } else { 0.0 };

            let word_w = measure_text_w(&display_word, style.font_size, ls, 0.0, m);
            let gap = if current_line.is_empty() { 0.0 } else { inter_word };

            // Wrap: слово не влезает (но первое слово строки добавляем всегда).
            let needs_wrap = !current_line.is_empty()
                && current_x + gap + pre + word_w > max_width;

            if needs_wrap {
                // CSS Text L3 §6: try hyphenation before hard wrap.
                let hyph_result = if hyphens != Hyphens::None {
                    let mut break_pts = shy_positions.clone();
                    if hyphens == Hyphens::Auto && !display_word.is_empty() {
                        let auto_pts = hp.hyphenate(&display_word, "");
                        break_pts.extend_from_slice(&auto_pts);
                        break_pts.sort_unstable();
                        break_pts.dedup();
                    }
                    let avail = max_width - current_x - gap - pre;
                    try_hyp_break(&display_word, avail, style.font_size, ls, m, &break_pts)
                } else {
                    None
                };

                if let Some((pfx, sfx)) = hyph_result {
                    // Emit prefix (with trailing '-') to current line, then wrap.
                    let pfx_w = measure_text_w(&pfx, style.font_size, ls, 0.0, m);
                    current_x += gap + pre;
                    current_line.push(InlineFrag {
                        x: current_x,
                        y_offset: 0.0,
                        width: pfx_w,
                        text: pfx,
                        style: style.clone(),
                        padding_left: if is_seg_first { pad_l } else { 0.0 },
                        padding_right: 0.0,
                        is_element_box: seg.is_element_box,
                        img_src: None,
                    });
                    result.push(std::mem::take(&mut current_line));
                    current_x = 0.0;
                    // Emit suffix as first fragment on new line.
                    let sfx_w = measure_text_w(&sfx, style.font_size, ls, 0.0, m);
                    current_line.push(InlineFrag {
                        x: 0.0,
                        y_offset: 0.0,
                        width: sfx_w,
                        text: sfx,
                        style: style.clone(),
                        padding_left: 0.0,
                        padding_right: if is_seg_last { pad_r } else { 0.0 },
                        is_element_box: seg.is_element_box,
                        img_src: None,
                    });
                    current_x += sfx_w + post;
                    continue;
                }

                // CSS Text L3 §5.1: word-break: break-all — char-break at the
                // current line position before wrapping.
                if word_break == WordBreak::BreakAll {
                    let gap_w = if current_line.is_empty() { 0.0 } else { inter_word };
                    current_x += gap_w + pre;
                    let mut rest = display_word.as_str();
                    let mut first_chunk = true;
                    while !rest.is_empty() {
                        let avail = (max_width - current_x).max(0.0);
                        let split = char_break_offset(rest, avail, style.font_size, ls, m);
                        let head = &rest[..split];
                        let tail = &rest[split..];
                        if !head.is_empty() {
                            let head_w = measure_text_w(head, style.font_size, ls, 0.0, m);
                            current_line.push(InlineFrag {
                                x: current_x,
                                y_offset: 0.0,
                                width: head_w,
                                text: head.to_string(),
                                style: style.clone(),
                                padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                                padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                                is_element_box: seg.is_element_box,
                                img_src: None,
                            });
                            current_x += head_w;
                            first_chunk = false;
                        }
                        rest = tail;
                        if !rest.is_empty() {
                            result.push(std::mem::take(&mut current_line));
                            current_x = 0.0;
                        }
                    }
                    current_x += post;
                    continue;
                }

                // No hyphenation break found — normal wrap.
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
            }

            // CSS Text L3 §8.1: overflow-wrap: break-word / anywhere — char-break
            // words that are wider than the container (won't fit on any line).
            // word-break: break-word is a legacy alias for overflow-wrap: break-word.
            let ow_char_break = (word_break == WordBreak::BreakWord
                || matches!(overflow_wrap, OverflowWrap::BreakWord | OverflowWrap::Anywhere))
                && word_w > max_width;
            if ow_char_break {
                let line_gap_ow = if current_line.is_empty() { 0.0 } else { inter_word };
                current_x += line_gap_ow + pre;
                let mut rest = display_word.as_str();
                let mut first_chunk = true;
                while !rest.is_empty() {
                    let avail = (max_width - current_x).max(0.0);
                    let split = char_break_offset(rest, avail, style.font_size, ls, m);
                    let head = &rest[..split];
                    let tail = &rest[split..];
                    if !head.is_empty() {
                        let head_w = measure_text_w(head, style.font_size, ls, 0.0, m);
                        current_line.push(InlineFrag {
                            x: current_x,
                            y_offset: 0.0,
                            width: head_w,
                            text: head.to_string(),
                            style: style.clone(),
                            padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                            padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                            is_element_box: seg.is_element_box,
                            img_src: None,
                        });
                        current_x += head_w;
                        first_chunk = false;
                    }
                    rest = tail;
                    if !rest.is_empty() {
                        result.push(std::mem::take(&mut current_line));
                        current_x = 0.0;
                    }
                }
                current_x += post;
                continue;
            }

            let line_gap = if current_line.is_empty() { 0.0 } else { inter_word };
            current_x += line_gap + pre;
            let frag_x = current_x;

            // Слияние: только когда нет pre/post space у данного слова
            // и предыдущий фраг тоже не заканчивается inline-box-ом.
            let no_box = pre == 0.0 && post == 0.0;
            let merged = if no_box {
                if let Some(last) = current_line.last_mut() {
                    if last.style.text_rendering_eq(style) && last.padding_right == 0.0 {
                        last.text.push(' ');
                        last.text.push_str(&display_word);
                        last.width += inter_word + word_w;
                        current_x += word_w;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !merged {
                current_line.push(InlineFrag {
                    x: frag_x,
                    y_offset: 0.0,
                    width: word_w,
                    text: display_word,
                    style: style.clone(),
                    padding_left: if is_seg_first { pad_l } else { 0.0 },
                    padding_right: if is_seg_last { pad_r } else { 0.0 },
                    is_element_box: seg.is_element_box,
                    img_src: None,
                });
                current_x += word_w;
            }

            current_x += post;
        }
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    result
}

/// Сдвигает фрагменты каждой строки по text-align + direction.
/// `Start`/`End` разрешаются в Left/Right по direction (CSS Text L3 §7.1).
/// Для RTL фрагменты зеркалируются относительно content_width.
fn align_lines(
    lines: &mut [Vec<InlineFrag>],
    content_width: f32,
    text_align: TextAlign,
    direction: Direction,
) {
    let is_rtl = direction == Direction::Rtl;
    // Resolve Start/End to physical Left/Right.
    let physical = match text_align {
        TextAlign::Start => if is_rtl { TextAlign::Right } else { TextAlign::Left },
        TextAlign::End   => if is_rtl { TextAlign::Left  } else { TextAlign::Right },
        other => other,
    };
    for line in lines.iter_mut() {
        let Some(last) = line.last() else { continue };
        let line_width = last.x + last.width;
        if is_rtl {
            // Mirror positions within the line block, then align the block.
            // `right_gap` = space to the right of the mirrored line block.
            let right_gap = match physical {
                TextAlign::Right  => (content_width - line_width).max(0.0),
                TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
                _                 => 0.0, // Left / flush-left for RTL end
            };
            for frag in line.iter_mut() {
                frag.x = line_width - (frag.x + frag.width) + right_gap;
            }
        } else {
            let offset = match physical {
                TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
                TextAlign::Right  => (content_width - line_width).max(0.0),
                _                 => 0.0,
            };
            if offset > 0.0 {
                for frag in line.iter_mut() {
                    frag.x += offset;
                }
            }
        }
    }
}

/// CSS 2.1 §10.8 — применяет вертикальное выравнивание к inline-фрагментам.
/// Записывает `y_offset` (смещение от верхнего края line-box, вниз — положительное).
/// `line_h` = font_size * line_height контейнера.
///
/// Half-leading (§10.8.1): когда line-height > content-area, разница делится пополам
/// и добавляется выше и ниже content-area. Для `baseline` — фрагмент сдвигается вниз
/// на `half_leading = (line_h - frag_h) / 2`, чтобы content-area была центрирована.
fn apply_inline_vertical_align(lines: &mut [Vec<InlineFrag>], line_h: f32) {
    for line in lines.iter_mut() {
        for frag in line.iter_mut() {
            // frag_h: content area height ≈ font-size (ascent + descent for normal line-height).
            let frag_h = frag.style.font_size;
            // CSS 2.1 §10.8.1: half-leading pushes content area away from line-box edges.
            let half_leading = ((line_h - frag_h) / 2.0).max(0.0);
            frag.y_offset = match frag.style.vertical_align {
                // Baseline: content area centred via half-leading (top = half_leading).
                VerticalAlign::Baseline => half_leading,
                // Top/TextTop: fragment top-aligned to line-box top edge.
                VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                // Bottom/TextBottom: fragment bottom-aligned to line-box bottom edge.
                VerticalAlign::Bottom | VerticalAlign::TextBottom => (line_h - frag_h).max(0.0),
                // Middle: visual midpoint of fragment at midpoint of line-box.
                VerticalAlign::Middle => ((line_h - frag_h) / 2.0).max(0.0),
                // sub/super: relative shift from baseline (~0.8 * frag_h from frag top).
                VerticalAlign::Sub => half_leading + frag_h * 0.15,
                VerticalAlign::Super => half_leading - frag_h * 0.35,
                // CSS: positive length = shift up (above baseline) → negative screen y.
                VerticalAlign::Length(px) => half_leading - px,
                VerticalAlign::Percent(p) => half_leading - (p / 100.0 * line_h),
            };
        }
    }
}

/// Без измерителя: помещаем всё в одну строку. Ширина каждого фрагмента
/// без шрифтовых метрик неизвестна — оставляем 0.0; text-decoration в этом
/// режиме не рисуется. layout() для финального рендеринга всё равно ходит
/// через layout_measured().
fn one_line_fallback(segments: &[InlineSegment]) -> Vec<Vec<InlineFrag>> {
    let mut frags: Vec<InlineFrag> = Vec::new();
    for seg in segments {
        // Image segment: emit with pre-computed width, don't merge with text.
        if let Some(img_src) = &seg.img_src {
            frags.push(InlineFrag {
                x: 0.0,
                y_offset: 0.0,
                width: seg.img_width,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: true,
                img_src: Some(img_src.clone()),
            });
            continue;
        }
        let text: String = seg.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let merged = if let Some(last) = frags.last_mut() {
            if last.style.text_rendering_eq(&seg.style) && last.img_src.is_none() {
                last.text.push(' ');
                last.text.push_str(&text);
                true
            } else {
                false
            }
        } else {
            false
        };
        if !merged {
            frags.push(InlineFrag {
                x: 0.0,
                y_offset: 0.0,
                width: 0.0,
                text,
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
            });
        }
    }
    if frags.is_empty() { vec![] } else { vec![frags] }
}

/// CSS UI L4 §10.1 — усекает фрагменты строк, выходящих за `max_width`,
/// добавляя символ «…» (U+2026). Вызывается только когда `text-overflow:
/// ellipsis` И `overflow` создаёт clip.
fn apply_text_overflow_ellipsis(
    lines: &mut [Vec<InlineFrag>],
    max_width: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
) {
    let ellipsis = '\u{2026}'; // …
    let ellipsis_w = m.char_width(ellipsis, font_size);

    for line in lines.iter_mut() {
        let line_end = line.last().map(|f| f.x + f.width).unwrap_or(0.0);
        if line_end <= max_width {
            continue;
        }

        // Максимальная ширина для текстового контента перед «…».
        let budget = (max_width - ellipsis_w).max(0.0);

        // Ищем первый фрагмент, чьё начало выходит за budget.
        let cut = line.iter().position(|f| f.x > budget);

        match cut {
            Some(0) => {
                // Первый фрагмент уже за budget — показываем только «…».
                line[0].text = ellipsis.to_string();
                line[0].width = ellipsis_w;
                line.truncate(1);
            }
            Some(fi) => {
                // Усекаем фрагмент fi-1, удаляем fi и далее.
                let avail = budget - line[fi - 1].x;
                truncate_frag_with_ellipsis(&mut line[fi - 1], avail, font_size, m, ellipsis, ellipsis_w);
                line.truncate(fi);
            }
            None => {
                // Все фрагменты начинаются в пределах budget, но последний
                // выходит за max_width — усекаем его.
                let last = line.len() - 1;
                let avail = budget - line[last].x;
                truncate_frag_with_ellipsis(&mut line[last], avail, font_size, m, ellipsis, ellipsis_w);
            }
        }
    }
}

fn truncate_frag_with_ellipsis(
    frag: &mut InlineFrag,
    avail: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
    ellipsis: char,
    ellipsis_w: f32,
) {
    let mut buf = String::new();
    let mut w = 0.0_f32;
    for ch in frag.text.chars() {
        let cw = m.char_width(ch, font_size);
        if w + cw > avail {
            break;
        }
        buf.push(ch);
        w += cw;
    }
    buf.push(ellipsis);
    frag.text = buf;
    frag.width = w + ellipsis_w;
}

/// CSS Container Queries L1: second-pass after layout.
///
/// Walks the laid-out box tree looking for elements that establish containers
/// (`container-type: size | inline-size`). For each container, resolves its
/// content dimensions from the first-pass layout rect, re-applies matching
/// `@container` rules to all descendants, then re-lays out those descendants
/// so that layout-affecting properties (width, height, display, …) take effect.
///
/// Phase 0 limitations:
/// - Only block-flow children are re-laid out (Flex/Grid children use first-pass positions).
/// - Nested containers are processed outermost-first (inner containers are re-entered in
///   the same walk, but they use the parent container's context for their own re-layout).
pub fn apply_container_styles(
    root: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    hp: &dyn HyphenationProvider,
) {
    // No container rules in this sheet → fast path.
    if sheet.container_rules.is_empty() {
        return;
    }
    let pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    apply_container_inner(root, doc, sheet, viewport, measurer, pcb, hp);
}

fn apply_container_inner(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    let is_container = !matches!(b.style.container_type, ContainerType::Normal);
    if is_container {
        // Derive content dimensions from already-laid-out rect + style.
        let em = b.style.font_size;
        let bw = b.rect.width;
        let pad_l = b.style.padding_left.resolve_or_zero(em, bw, viewport);
        let pad_r = b.style.padding_right.resolve_or_zero(em, bw, viewport);
        let pad_t = b.style.padding_top.resolve_or_zero(em, bw, viewport);
        let pad_b = b.style.padding_bottom.resolve_or_zero(em, bw, viewport);
        let content_w = (bw - pad_l - pad_r
            - b.style.border_left_width - b.style.border_right_width).max(0.0);
        let content_h_val = (b.rect.height - pad_t - pad_b
            - b.style.border_top_width - b.style.border_bottom_width).max(0.0);
        let content_h = if matches!(b.style.container_type, ContainerType::Size) {
            Some(content_h_val)
        } else {
            None // inline-size: height not queryable
        };
        let ctx = ContainerContext {
            width: content_w,
            height: content_h,
            names: b.style.container_name.clone(),
        };
        // Re-apply container rules to all direct + indirect descendants.
        for child in &mut b.children {
            re_style_subtree(child, doc, sheet, &ctx, viewport);
        }
        // Re-lay out block-flow children with updated styles.
        let content_x = b.rect.x + pad_l + b.style.border_left_width;
        let content_y = b.rect.y + pad_t + b.style.border_top_width;
        let avail_h: Option<f32> = content_h;
        let child_pcb = if !matches!(b.style.position, Position::Static) {
            Rect::new(b.rect.x, b.rect.y, b.rect.width, b.rect.height)
        } else {
            pcb
        };
        // Expose this container's dimensions to cq* unit resolution during re-layout.
        set_cq_context(content_w, content_h);
        let mut child_y = content_y;
        for child in &mut b.children {
            if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                // Re-lay out against new pcb but don't advance child_y.
                lay_out(child, content_x, child_y, content_w, avail_h, measurer, viewport, child_pcb, hp);
                continue;
            }
            lay_out(child, content_x, child_y, content_w, avail_h, measurer, viewport, child_pcb, hp);
            if matches!(child.kind, BoxKind::Skip) {
                continue;
            }
            let child_mb = child.style.margin_bottom
                .resolve_or_zero(child.style.font_size, content_w, viewport);
            child_y = child.rect.y + child.rect.height + child_mb;
        }
        clear_cq_context();
        // After re-layout, recurse into children to catch nested containers.
        // Each nested container will set its own cq* context during its own re-layout.
        for child in &mut b.children {
            apply_container_inner(child, doc, sheet, viewport, measurer, child_pcb, hp);
        }
    } else {
        // Not a container — just recurse looking for container descendants.
        for child in &mut b.children {
            apply_container_inner(child, doc, sheet, viewport, measurer, pcb, hp);
        }
    }
}

/// Recursively re-applies container rules to a subtree.
/// Stops descending into elements that are themselves containers (they will
/// be processed by `apply_container_inner` with their own context).
fn re_style_subtree(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
) {
    if !matches!(b.kind, BoxKind::Skip) {
        apply_container_rules(&mut b.style, doc, b.node, sheet, ctx, viewport);
    }
    // Don't propagate into nested containers — they'll build their own context.
    if matches!(b.style.container_type, ContainerType::Normal) {
        for child in &mut b.children {
            re_style_subtree(child, doc, sheet, ctx, viewport);
        }
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::geom::Size;

    fn layout_div(css: &str, viewport_w: f32, viewport_h: f32) -> super::LayoutBox {
        let html = "<div></div>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(viewport_w, viewport_h));
        // html box > body box > div box
        fn find_empty_block(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::Block) && child.children.is_empty() {
                    return Some(child);
                }
                if let Some(found) = find_empty_block(child) {
                    return Some(found);
                }
            }
            None
        }
        find_empty_block(&root).cloned().expect("empty Block not found in layout tree")
    }

    #[test]
    fn aspect_ratio_height_from_width() {
        // width: 200px, aspect-ratio: 2/1 → height should be 100px border-box
        let div = layout_div("div { width: 200px; aspect-ratio: 2/1; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.rect.height, 100.0);
    }

    #[test]
    fn aspect_ratio_16_9() {
        // width: 160px, aspect-ratio: 16/9 → height = 160 * 9/16 = 90px
        let div = layout_div("div { width: 160px; aspect-ratio: 16/9; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 160.0);
        assert!((div.rect.height - 90.0).abs() < 0.5, "height={}", div.rect.height);
    }

    #[test]
    fn aspect_ratio_explicit_height_wins() {
        // Explicit height overrides aspect-ratio.
        let div = layout_div("div { width: 200px; height: 50px; aspect-ratio: 2/1; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.rect.height, 50.0);
    }

    #[test]
    fn aspect_ratio_no_height_without_ratio() {
        // Without aspect-ratio, height collapses to 0 for empty div.
        let div = layout_div("div { width: 200px; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.rect.height, 0.0);
    }

    // ── Hyphenation helpers ───────────────────────────────────────────────────

    #[test]
    fn strip_soft_hyphens_removes_shy_and_collects_positions() {
        let (disp, pos) = super::strip_soft_hyphens("hy\u{00AD}phen");
        assert_eq!(disp, "hyphen");
        assert_eq!(pos, vec![2]); // break point between 'y' and 'p'
    }

    #[test]
    fn strip_soft_hyphens_multiple_breaks() {
        // "su\u{AD}per\u{AD}man"
        let (disp, pos) = super::strip_soft_hyphens("su\u{00AD}per\u{00AD}man");
        assert_eq!(disp, "superman");
        assert_eq!(pos, vec![2, 5]);
    }

    #[test]
    fn strip_soft_hyphens_no_shy_returns_empty_positions() {
        let (disp, pos) = super::strip_soft_hyphens("hello");
        assert_eq!(disp, "hello");
        assert!(pos.is_empty());
    }

    #[test]
    fn measure_text_w_empty_is_zero() {
        struct ZeroMeasurer;
        impl super::super::TextMeasurer for ZeroMeasurer {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let m = ZeroMeasurer;
        assert_eq!(super::measure_text_w("", 16.0, 0.0, 0.0, &m), 0.0);
    }

    #[test]
    fn measure_text_w_three_chars_no_spacing() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // 3 chars × 8px − 0 letter-spacing = 24px
        let w = super::measure_text_w("abc", 16.0, 0.0, 0.0, &Fixed8);
        assert_eq!(w, 24.0);
    }

    #[test]
    fn try_hyp_break_finds_rightmost_fitting_split() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // "superman" → break positions [2, 5] (su|per|man)
        // Each char = 8px; hyphen = 8px.
        // If available_w = 32px: "su-" = 3×8 = 24 ≤ 32 ✓, "super-" = 6×8 = 48 > 32
        // So rightmost fitting = pos 2 ("su-" / "perman")
        let m = Fixed8;
        let result = super::try_hyp_break("superman", 32.0, 16.0, 0.0, &m, &[2, 5]);
        assert_eq!(result, Some(("su-".to_string(), "perman".to_string())));
    }

    #[test]
    fn try_hyp_break_prefers_rightmost_break() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // "superman" → break positions [2, 5]; available = 56px
        // "super-" = 6×8 = 48 ≤ 56 ✓ → prefer pos 5 over pos 2
        let m = Fixed8;
        let result = super::try_hyp_break("superman", 56.0, 16.0, 0.0, &m, &[2, 5]);
        assert_eq!(result, Some(("super-".to_string(), "man".to_string())));
    }

    #[test]
    fn try_hyp_break_returns_none_when_nothing_fits() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // Only 10px available; minimum "su-" = 24px
        let m = Fixed8;
        let result = super::try_hyp_break("superman", 10.0, 16.0, 0.0, &m, &[2, 5]);
        assert!(result.is_none());
    }

    #[test]
    fn wrap_inline_run_soft_hyphen_breaks_word_on_manual() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens};
        use lumen_core::geom::Size;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // Segment: "hi hy\u{AD}phen" — two words; 'hi' fills line, 'hy\u{AD}phen' needs break.
        // char=10, max_width=60:
        //   "hi"=20px fits; then gap(10)+60=90>60 → wrap attempted.
        //   avail = 60-20-10 = 30; "hy-"=30 ≤ 30 → break at pos 2.
        let seg = InlineSegment {
            text: "hi hy\u{00AD}phen".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
        };

        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(&[seg], 60.0, 16.0, 0.0, Size::new(800.0, 600.0), &m, Hyphens::Manual, &hp, crate::style::WhiteSpace::Normal, crate::style::WordBreak::Normal, crate::style::OverflowWrap::Normal);
        assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
        // Line 1 has both "hi" and "hy-" merged or as separate frags.
        let line1_text: String = lines[0].iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(line1_text.contains("hi"), "line1={line1_text}");
        assert!(line1_text.contains("hy-"), "line1={line1_text}");
        assert_eq!(lines[1].len(), 1);
        assert_eq!(lines[1][0].text, "phen");
    }

    #[test]
    fn wrap_inline_run_hyphens_none_no_break_on_shy() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens};
        use lumen_core::geom::Size;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // Same segment, Hyphens::None → soft hyphen ignored, full word wraps to new line unbroken.
        let seg = InlineSegment {
            text: "hi hy\u{00AD}phen".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
        };
        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(&[seg], 60.0, 16.0, 0.0, Size::new(800.0, 600.0), &m, Hyphens::None, &hp, crate::style::WhiteSpace::Normal, crate::style::WordBreak::Normal, crate::style::OverflowWrap::Normal);
        assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
        // Line 1 has only "hi"; line 2 has "hyphen" (whole, no hyphen char).
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].text, "hi");
        let line2_text = &lines[1][0].text;
        assert_eq!(line2_text, "hyphen", "soft-hyphen should be stripped: {line2_text}");
    }

    // ── char_break_offset ────────────────────────────────────────────────────

    #[test]
    fn char_break_offset_all_fit() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // "abc" = 3 chars × 8px = 24px; avail = 100 → whole word fits.
        let off = super::char_break_offset("abc", 100.0, 16.0, 0.0, &Fixed8);
        assert_eq!(off, 3); // "abc".len() == 3
    }

    #[test]
    fn char_break_offset_splits_after_second_char() {
        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }
        // "abcde", avail = 25px; "ab" = 20px fits, "abc" = 30px > 25 → split at 2.
        let off = super::char_break_offset("abcde", 25.0, 16.0, 0.0, &Fixed10);
        assert_eq!(off, 2); // byte offset 2 = between 'b' and 'c'
    }

    #[test]
    fn char_break_offset_emits_at_least_one_char() {
        struct Wide;
        impl super::super::TextMeasurer for Wide {
            fn char_width(&self, _: char, _: f32) -> f32 { 100.0 }
        }
        // avail = 5px, char width 100px — even first char doesn't fit.
        // Must return offset past first char to avoid infinite loop.
        let off = super::char_break_offset("abc", 5.0, 16.0, 0.0, &Wide);
        assert_eq!(off, 1); // emit 'a' anyway
    }

    // ── text-wrap-mode: nowrap ────────────────────────────────────────────────

    #[test]
    fn text_wrap_mode_nowrap_no_line_break() {
        // text-wrap-mode: nowrap should prevent wrapping (like white-space: nowrap).
        // Container 50px wide, word each 8px × 5 chars = 40px ("Hello" + " " + "World").
        let html = "<p>Hello World</p>";
        let css = "p { width: 50px; text-wrap-mode: nowrap; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_inline_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_inline_run(c) { return Some(f); } }
            None
        }
        let ir = find_inline_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { lines, .. } = &ir.kind {
            assert_eq!(lines.len(), 1, "text-wrap-mode:nowrap must produce 1 line, got {}", lines.len());
        }
    }

    // ── overflow-wrap: break-word ─────────────────────────────────────────────

    #[test]
    fn overflow_wrap_break_word_splits_long_word() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens, OverflowWrap, WordBreak};
        use lumen_core::geom::Size;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // "Superlongword" = 13 chars × 10px = 130px; max_width = 80px.
        // overflow-wrap: break-word should split it across lines.
        let seg = InlineSegment {
            text: "Superlongword".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
        };

        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(
            &[seg], 80.0, 16.0, 0.0,
            Size::new(800.0, 600.0),
            &m, Hyphens::None, &hp,
            crate::style::WhiteSpace::Normal,
            WordBreak::Normal,
            OverflowWrap::BreakWord,
        );
        // 13 chars at 10px = 130px > 80px, so must wrap.
        assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());
        // No line should exceed max_width.
        for (i, line) in lines.iter().enumerate() {
            if let Some(last) = line.last() {
                let line_w = last.x + last.width;
                assert!(line_w <= 81.0, "line {} width {line_w} exceeds max_width 80", i);
            }
        }
        // All characters of "Superlongword" must appear in the output.
        let all_text: String = lines.iter().flat_map(|l| l.iter().map(|f| f.text.as_str())).collect();
        assert_eq!(all_text, "Superlongword", "all chars must be emitted: {all_text}");
    }

    // ── word-break: break-all ─────────────────────────────────────────────────

    #[test]
    fn word_break_break_all_breaks_at_current_position() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens, OverflowWrap, WordBreak};
        use lumen_core::geom::Size;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // Two words: "Hi" (20px) then "World" (50px). max_width = 60px.
        // Normal: "Hi" fits, gap(10)+50=80 > 60 → wrap → line2 = "World".
        // break-all: "Hi" fits; gap(10)+"World" → need 80 > 60 → char-break.
        //   avail at current pos = 60 - 20 - 10 = 30px → "Wor" (30px) fits.
        //   Emit "Wor" at end of line1, line2 = "ld".
        let seg = InlineSegment {
            text: "Hi World".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
        };

        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(
            &[seg], 60.0, 16.0, 0.0,
            Size::new(800.0, 600.0),
            &m, Hyphens::None, &hp,
            crate::style::WhiteSpace::Normal,
            WordBreak::BreakAll,
            OverflowWrap::Normal,
        );
        assert_eq!(lines.len(), 2, "expected 2 lines with break-all, got {}", lines.len());
        // All text must be preserved.
        let all_text: String = lines.iter()
            .flat_map(|l| l.iter().map(|f| f.text.as_str()))
            .collect::<Vec<_>>()
            .join(" "); // words may be merged by frag-merging
        assert!(all_text.contains("Hi"), "line1 must contain 'Hi': {all_text}");
        // Line 2 must have the remainder of "World".
        let line2_text: String = lines[1].iter().map(|f| f.text.as_str()).collect();
        assert!(!line2_text.is_empty(), "line2 must not be empty");
    }

    // ── display: flow-root (BFC) ──────────────────────────────────────────────

    #[test]
    fn flow_root_produces_flow_root_kind() {
        let html = r#"<div id="bfc"></div>"#;
        let css = "#bfc { display: flow-root; width: 200px; height: 50px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_flow_root(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::FlowRoot) {
                return Some(b);
            }
            for child in &b.children {
                if let Some(found) = find_flow_root(child) {
                    return Some(found);
                }
            }
            None
        }
        let bfc = find_flow_root(&root).expect("FlowRoot box not found");
        assert_eq!(bfc.rect.width, 200.0);
        assert_eq!(bfc.rect.height, 50.0);
    }

    #[test]
    fn flow_root_lays_out_children_like_block() {
        // A flow-root containing two block children should stack them vertically.
        let html = r#"<div class="bfc"><div class="a"></div><div class="b"></div></div>"#;
        let css = ".bfc { display: flow-root; width: 200px; } .a { height: 30px; } .b { height: 20px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_flow_root(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::FlowRoot) { return Some(b); }
            for c in &b.children { if let Some(f) = find_flow_root(c) { return Some(f); } }
            None
        }
        let bfc = find_flow_root(&root).expect("FlowRoot box not found");
        // Height auto → sum of children (30 + 20 = 50).
        assert_eq!(bfc.rect.height, 50.0, "flow-root auto height wrong: {}", bfc.rect.height);
        // Children stacked vertically.
        let blocks: Vec<_> = bfc.children.iter()
            .filter(|c| matches!(c.kind, super::BoxKind::Block))
            .collect();
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].rect.y > blocks[0].rect.y, "children not stacked vertically");
    }

    // ── display: contents (box elimination) ──────────────────────────────────

    #[test]
    fn contents_box_is_eliminated_from_layout_tree() {
        // The display:contents wrapper should not appear as a box; its child
        // block should be a direct child of the outer div.
        let html = r#"<div id="outer"><div id="wrap"><div id="inner"></div></div></div>"#;
        let css = "#outer { width: 400px; } #wrap { display: contents; } #inner { height: 40px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_by_id<'a>(b: &'a super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a super::LayoutBox> {
            if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
                && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
            {
                return Some(b);
            }
            for child in &b.children { if let Some(f) = find_by_id(child, doc, id) { return Some(f); } }
            None
        }
        // display:contents wrapper must not appear as a Contents box in the tree.
        fn find_contents(b: &super::LayoutBox) -> bool {
            if matches!(b.kind, super::BoxKind::Contents) { return true; }
            b.children.iter().any(find_contents)
        }
        assert!(!find_contents(&root), "Contents box must be flattened out of layout tree");
        // Inner block must exist with correct height.
        let inner = find_by_id(&root, &doc, "inner").expect("inner div not found");
        assert_eq!(inner.rect.height, 40.0, "inner height wrong: {}", inner.rect.height);
    }

    #[test]
    fn nested_contents_flattened() {
        // Two nested display:contents wrappers — both should be eliminated.
        let html = r#"<div id="root"><div id="a"><div id="b"><div id="leaf"></div></div></div></div>"#;
        let css = "#a, #b { display: contents; } #leaf { height: 25px; width: 100px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_contents(b: &super::LayoutBox) -> bool {
            if matches!(b.kind, super::BoxKind::Contents) { return true; }
            b.children.iter().any(find_contents)
        }
        assert!(!find_contents(&root), "nested Contents boxes must be fully flattened");
    }

    // ── CSS 2.1 §10.3.3 — auto horizontal-margin centering ───────────────────

    fn find_by_id_all<'a>(b: &'a super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a super::LayoutBox> {
        if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
            && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
        {
            return Some(b);
        }
        for child in &b.children {
            if let Some(f) = find_by_id_all(child, doc, id) { return Some(f); }
        }
        None
    }

    #[test]
    fn margin_auto_both_centers_block() {
        // margin: 0 auto on a 200px block inside an 800px viewport → x = 300.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { width: 200px; height: 50px; margin: 0 auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // (800 - 200) / 2 = 300
        assert_eq!(b.rect.x, 300.0, "centered x expected 300, got {}", b.rect.x);
        assert_eq!(b.rect.width, 200.0, "width must stay 200px");
    }

    #[test]
    fn margin_auto_left_only_pushes_to_right() {
        // margin-left: auto, margin-right: 0 → element flush-right.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { width: 200px; height: 50px; margin-left: auto; margin-right: 0; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // available=800, width=200, mr=0 → remaining=600 → ml_computed=600 → x=600
        assert_eq!(b.rect.x, 600.0, "flush-right x expected 600, got {}", b.rect.x);
    }

    #[test]
    fn margin_auto_right_only_no_x_shift() {
        // margin-right: auto, margin-left: 20px → element at x=20.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { width: 200px; height: 50px; margin-left: 20px; margin-right: auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // margin-left is fixed at 20px → x=20
        assert_eq!(b.rect.x, 20.0, "x with fixed left margin expected 20, got {}", b.rect.x);
    }

    #[test]
    fn margin_auto_no_explicit_width_fills_container() {
        // Without explicit width, auto margins resolve to 0 (width takes remaining).
        let html = r#"<div id="box"></div>"#;
        let css = "#box { height: 50px; margin: 0 auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // No explicit width → margin auto resolves to 0 → element fills 800px, x=0.
        assert_eq!(b.rect.x, 0.0, "x without explicit width must be 0, got {}", b.rect.x);
        assert_eq!(b.rect.width, 800.0, "width without explicit must fill 800px, got {}", b.rect.width);
    }

    #[test]
    fn margin_auto_position_sticky_centers() {
        // position:sticky element with margin: 20px auto 0 in 1022px container.
        // Static view: sticky behaves like normal flow → centering applies.
        let html = r#"<div id="wrap"><div id="sticky"></div></div>"#;
        let css = "#wrap { width: 1022px; position: relative; } \
                   #sticky { position: sticky; top: 10px; width: 600px; height: 60px; margin: 20px auto 0; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
        let s = find_by_id_all(&root, &doc, "sticky").expect("sticky not found");
        // (1022 - 600) / 2 = 211 → x = wrap.content_x + 211
        assert_eq!(s.rect.width, 600.0, "width must be 600, got {}", s.rect.width);
        let centered_x = s.rect.x;
        // Should be (1022-600)/2 = 211 relative to wrap's content_x (0).
        assert!((centered_x - 211.0).abs() < 1.0, "centered x expected ~211, got {centered_x}");
        assert_eq!(s.rect.y, 20.0, "top margin 20px must be respected, got {}", s.rect.y);
    }

    #[test]
    fn margin_auto_float_not_centered() {
        // float:left with margin: 0 auto must NOT be centered — floats ignore auto margins.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { float: left; width: 100px; height: 50px; margin: 0 auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // Float placed at left edge (auto = 0).
        assert_eq!(b.rect.x, 0.0, "float with auto margins must be at x=0, got {}", b.rect.x);
    }
}
