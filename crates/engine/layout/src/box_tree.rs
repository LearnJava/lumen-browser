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
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};
use lumen_html_parser::{
    PictureParams, SizesViewport, pick_img_source, pick_picture_source,
};

use crate::style::{
    compute_style, AlignValue, BackgroundImage, BoxSizing, ComputedStyle, Display, FlexBasis,
    FlexDirection, FlexWrap, GridAutoFlow, GridLine, GridTrackSize, Length, LengthOrAuto, Position,
    TextAlign, VerticalAlign,
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
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved padding_left of this frag's inline box start (0 if not a box start).
    pub padding_left: f32,
    /// Resolved padding_right of this frag's inline box end (0 if not a box end).
    pub padding_right: f32,
    /// True when this frag comes from an inline element box (not anonymous text).
    /// Used by the painter to draw element background/border.
    pub is_element_box: bool,
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
    /// Схлопнутый межэлементный пробел в InlineBlockRow.
    /// Не рисуется; участвует только как горизонтальный gap между
    /// inline-block соседями (CSS white-space collapsing §4.1.2).
    InlineSpace,
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
}

pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    lay_out(&mut root, 0.0, 0.0, viewport.width, None, viewport, init_pcb);
    root
}

pub fn layout_measured(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(measurer), viewport, init_pcb);
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
            if is_image_element(doc, id) {
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
) {
    match &doc.get(id).data {
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
            });
        }
        NodeData::Text(_) => {}
        NodeData::Element { .. } => {
            let s = compute_style(doc, id, sheet, inherited, viewport);
            if s.display == Display::None {
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
            let children: Vec<NodeId> = doc.get(id).children.clone();
            for child_id in children {
                collect_inline_segments(doc, sheet, child_id, &s, viewport, out);
            }
            let added = out.len() - start;
            // Mark all segments from this element as element boxes.
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

fn build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
) -> LayoutBox {
    let mut style = compute_style(doc, id, sheet, inherited, viewport);

    let kind = match &doc.get(id).data {
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. } => BoxKind::Skip,
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
            } else {
                BoxKind::Block
            }
        }
    };

    let mut children = Vec::new();
    if matches!(kind, BoxKind::Block) {
        let dom_children: Vec<NodeId> = doc.get(id).children.clone();
        // CSS Grid L1 §6: all direct children of a grid/flex container are
        // "blockified" — they participate as individual items, not wrapped in
        // InlineRun. Skip the inline-collection logic for these containers.
        let is_item_container = matches!(
            style.display,
            Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
        );
        if is_item_container {
            for child_id in dom_children {
                let child_box = build_box(doc, sheet, child_id, &style, viewport);
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
                        collect_inline_segments(doc, sheet, cid, &style, viewport, &mut pending);
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
                        row_items.push(build_box(doc, sheet, cid, &style, viewport));
                        had_ws = false;
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
                children.push(build_box(doc, sheet, child_id, &style, viewport));
                i += 1;
            }
        }
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
fn preferred_inline_block_width(b: &LayoutBox, viewport: Size) -> Option<f32> {
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
            BoxSizing::BorderBox => w,
        };
        return Some(outer.max(0.0));
    }
    let max_child = b
        .children
        .iter()
        .filter_map(|c| preferred_inline_block_width(c, viewport))
        .fold(0.0_f32, f32::max);
    if max_child > 0.0 {
        Some(
            (max_child + pl + pr
                + s.border_left_width + s.border_right_width)
                .max(0.0),
        )
    } else {
        None
    }
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

/// `pcb` — rect positioned containing block (ближайший предок с position != static),
/// используется для layout абсолютно-позиционированных потомков.
fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
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
    let is_replaced = matches!(b.kind, BoxKind::Image { .. });
    b.rect.width = if is_replaced {
        0.0
    } else {
        (available_width - margin_left - margin_right).max(0.0)
    };
    // Явная ширина (CSS width: Npx) перекрывает auto-ширину.
    // box-sizing определяет, к какой части бокса относится `width`:
    //   - content-box: width — это размер контента, padding+border прибавляются;
    //   - border-box: width — общий размер вместе с padding+border.
    if let Some(w_len) = &s.width
        && let Some(w) = w_len.resolve(em, Some(cb), viewport)
    {
        b.rect.width = match s.box_sizing {
            BoxSizing::ContentBox => (w + padding_left + padding_right
                + s.border_left_width + s.border_right_width).max(0.0),
            BoxSizing::BorderBox => w.max(0.0),
        };
    }
    // CSS 2.1 §10.4: tentative width → clamp в [min-width, max-width].
    // Порядок «max сначала, потом min» автоматически даёт правило
    // «при min > max побеждает min». min-/max- интерпретируются в той же
    // box-sizing модели, что и width: content-box добавляет padding+border,
    // border-box оставляет как есть.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_left + padding_right
            + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_width
        && let Some(max_w) = max_len.resolve(em, Some(cb), viewport)
    {
        b.rect.width = b.rect.width.min(outer_horiz(max_w).max(0.0));
    }
    if let Some(min_len) = &s.min_width
        && let Some(min_w) = min_len.resolve(em, Some(cb), viewport)
    {
        b.rect.width = b.rect.width.max(outer_horiz(min_w.max(0.0)));
    }
    // Phase 0 shrink-to-fit для inline-block без явной CSS width.
    // Полный алгоритм (CSS 2.1 §10.3.9) требует двух проходов; здесь —
    // упрощение: ищем максимальную explicit-width среди потомков.
    if s.width.is_none() && s.display == Display::InlineBlock
        && let Some(pref_w) = preferred_inline_block_width(b, viewport)
    {
        b.rect.width = pref_w.min(b.rect.width);
    }

    let content_x = b.rect.x + padding_left + s.border_left_width;
    let content_y = b.rect.y + padding_top + s.border_top_width;
    let content_width = (b.rect.width
        - padding_left - padding_right
        - s.border_left_width - s.border_right_width).max(0.0);

    // pcb для потомков: если текущий элемент positioned — он сам CB для абсолютных детей.
    // Высота ещё неизвестна, используем 0 — корректируем after layout.
    let is_positioned = !matches!(s.position, Position::Static);
    let children_pcb = if is_positioned {
        Rect::new(b.rect.x, b.rect.y, b.rect.width, 0.0)
    } else {
        pcb
    };

    // InlineRun обрабатывается до основного match.
    if let BoxKind::InlineRun { segments, lines } = &mut b.kind {
        if let Some(m) = measurer {
            // white-space: nowrap → передаём «бесконечную» max_width в wrap,
            // чтобы перенос не сработал; остальная логика (letter-spacing,
            // word-spacing, объединение фрагментов) остаётся.
            let wrap_width = if s.white_space == crate::style::WhiteSpace::Nowrap {
                f32::INFINITY
            } else {
                content_width
            };
            let text_indent_px = s.text_indent.resolve_or_zero(em, cb, viewport);
            *lines = wrap_inline_run(segments, wrap_width, s.font_size, text_indent_px, viewport, m);
            if s.text_align != TextAlign::Left {
                align_lines(lines, content_width, s.text_align);
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
        BoxKind::Block | BoxKind::Image { .. } => {
            // Flex containers dispatch to lay_out_flex before block-flow.
            if matches!(s.display, Display::Flex | Display::InlineFlex) {
                let content_height = lay_out_flex(
                    &mut b.children, &s, content_x, content_y, content_width, measurer, viewport,
                    children_pcb,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = h_len.resolve(em, Some(cb), viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(0.0),
                    }
                } else {
                    content_height + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Grid containers dispatch to lay_out_grid before block-flow.
            if matches!(s.display, Display::Grid | Display::InlineGrid) {
                let content_height = lay_out_grid(
                    &mut b.children, &s, content_x, content_y, content_width, measurer, viewport,
                    children_pcb,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = h_len.resolve(em, Some(cb), viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(0.0),
                    }
                } else {
                    content_height + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Image не имеет flow-детей, поэтому child-цикл просто пуст —
            // объединяем с Block, чтобы общий код width/height/min-max/borders
            // не дублировался. content_height = 0 для Image без явной высоты
            // даёт коробку только из padding+border (что для пустой картинки
            // визуально корректно).
            let mut child_y = content_y;
            for (i, child) in b.children.iter_mut().enumerate() {
                if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                    // Записываем статичную позицию и пропускаем в normal flow.
                    abs_deferred.push((i, content_x, child_y));
                    continue;
                }
                lay_out(child, content_x, child_y, content_width, measurer, viewport, children_pcb);
                if matches!(child.kind, BoxKind::Skip) {
                    continue;
                }
                // child margins resolved against parent content_width (= cb_width for child).
                let child_mb = child.style.margin_bottom.resolve_or_zero(
                    child.style.font_size, content_width, viewport);
                child_y = child.rect.y + child.rect.height + child_mb;
            }
            let content_height = (child_y - content_y).max(0.0);
            // Явная высота (CSS height: Npx) перекрывает auto-высоту по содержимому.
            // box-sizing работает симметрично width: content-box прибавляет
            // padding+border, border-box оставляет h как итоговую высоту.
            b.rect.height = if let Some(h_len) = &s.height {
                if let Some(h) = h_len.resolve(em, Some(cb), viewport) {
                    match s.box_sizing {
                        BoxSizing::ContentBox => h
                            + padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                        BoxSizing::BorderBox => h.max(0.0),
                    }
                } else {
                    content_height + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width
                }
            } else {
                content_height + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
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
                && let Some(max_h) = max_len.resolve(em, Some(cb), viewport)
            {
                b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
            }
            if let Some(min_len) = &s.min_height
                && let Some(min_h) = min_len.resolve(em, Some(cb), viewport)
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
            // высоте каждой строки, так как baseline пустых inline-block
            // совпадает с нижним краем margin-box, а descent опускается ниже.
            let strut_descent = measurer.map_or(0.0, |m| m.descent_px(b.style.font_size));
            let mut rows: Vec<(f32, f32, Vec<usize>)> = Vec::new();
            let mut cur_x = content_x;
            let mut cur_y = content_y;
            let mut row_max_h: f32 = 0.0;
            let mut row_y = cur_y;
            let mut cur_row: Vec<usize> = Vec::new();
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
                lay_out(&mut b.children[i], cur_x, cur_y, child_avail, measurer, viewport, children_pcb);
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
                    // Строка завершена: row_max_h — высота контента (для vertical-align),
                    // row_spacing = row_max_h + strut_descent — для cur_y (IFC strut).
                    let row_spacing = row_max_h + strut_descent;
                    rows.push((row_y, row_max_h, std::mem::take(&mut cur_row)));
                    total_h += row_spacing;
                    cur_y += row_spacing;
                    row_y = cur_y;
                    cur_x = content_x;
                    row_max_h = 0.0;
                    lay_out(&mut b.children[i], cur_x, cur_y, content_width, measurer, viewport, children_pcb);
                }
                cur_row.push(i);
                cur_x = b.children[i].rect.x + b.children[i].rect.width + child_mr;
                row_max_h = row_max_h.max(child_full_h);
            }
            if !cur_row.is_empty() {
                rows.push((row_y, row_max_h, cur_row));
            }
            b.rect.height = total_h + row_max_h + strut_descent;

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
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::InlineSpace => unreachable!(),
        BoxKind::Skip => unreachable!(),
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
        lay_out_abs_children(b, &abs_deferred, measurer, viewport, my_pcb);
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

/// Positions absolutely/fixed-positioned deferred children of `parent`.
/// Called after parent's height is finalized so `my_pcb` is complete.
fn lay_out_abs_children(
    parent: &mut LayoutBox,
    deferred: &[(usize, f32, f32)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
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

        lay_out(&mut parent.children[idx], 0.0, 0.0, avail_w, measurer, viewport, my_pcb);

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
) -> f32 {
    let is_column = matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
    let is_reverse = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let is_wrap = matches!(s.flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let is_wrap_reverse = matches!(s.flex_wrap, FlexWrap::WrapReverse);

    // Indices of non-Skip children (actual flex items).
    let item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

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
        lay_out(&mut children[i], content_x, content_y, content_width, measurer, viewport, pcb);
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
                    measurer,
                    viewport,
                    pcb,
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
                    measurer,
                    viewport,
                    pcb,
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
        let cs = resolve_grid_line(&is.grid_column_start, n_explicit_cols as u32);
        let ce = resolve_grid_line_end(&is.grid_column_end, cs, n_explicit_cols as u32);
        let rs = resolve_grid_line(&is.grid_row_start, 0);
        let re = resolve_grid_line_end(&is.grid_row_end, rs, 0);

        if cs != 0 && rs != 0 {
            // Fully explicit: both axes known.
            placements[k] = (cs, ce, rs, re);
        } else if cs != 0 {
            // Column fixed, row auto.
            placements[k] = (cs, ce, 0, 0);
        } else if rs != 0 {
            // Row fixed, column auto.
            placements[k] = (0, 0, rs, re);
        }
        // both auto: handled in pass 2
    }

    // Pass 2: auto-place remaining items.
    // Simple auto-placement: scan in row order, fill left-to-right then wrap.
    let mut cursor_row: u32 = 1;
    let mut cursor_col: u32 = 1;

    for (k, _) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs != 0 && rs != 0 {
            continue; // already placed
        }

        // Determine span
        let col_span = if ce > cs { ce - cs } else { 1 };
        let row_span = if re > rs { re - rs } else { 1 };

        if row_flow {
            // If column is fixed, row needs auto-placement.
            let fixed_cs = if cs != 0 { cs } else { 0 };
            let fixed_ce = if cs != 0 { ce } else { 0 };

            // Find next empty cell starting at cursor.
            loop {
                let try_col = if fixed_cs != 0 { fixed_cs } else { cursor_col };
                let try_ce = if fixed_cs != 0 { fixed_ce } else { try_col + col_span };
                // Check if this cell overlaps any already-placed item in the same row.
                let overlaps = (0..k).any(|j| {
                    let (ocs, oce, ors, ore) = placements[j];
                    ocs != 0 && ors != 0
                        && cursor_row < ore && cursor_row + row_span > ors
                        && try_col < oce && try_ce > ocs
                });

                if !overlaps && (try_ce - 1) <= n_explicit_cols as u32 || n_explicit_cols == 1 {
                    placements[k] = (try_col, try_ce, cursor_row, cursor_row + row_span);
                    // Advance cursor.
                    cursor_col = try_ce;
                    if cursor_col > n_explicit_cols as u32 {
                        cursor_col = 1;
                        cursor_row += 1;
                    }
                    break;
                }
                // Try next column.
                if fixed_cs != 0 {
                    // Column is fixed, just advance row.
                    cursor_row += 1;
                    cursor_col = 1;
                } else {
                    cursor_col += 1;
                    if cursor_col > n_explicit_cols as u32 {
                        cursor_col = 1;
                        cursor_row += 1;
                    }
                }
            }
        } else {
            // Column flow: fill top-to-bottom, wrap into next column.
            let n_explicit_rows = s.grid_template_rows.len().max(1) as u32;
            let fixed_rs = if rs != 0 { rs } else { 0 };
            let fixed_re = if rs != 0 { re } else { 0 };

            loop {
                let try_row = if fixed_rs != 0 { fixed_rs } else { cursor_row };
                let try_re = if fixed_rs != 0 { fixed_re } else { try_row + row_span };

                let overlaps = (0..k).any(|j| {
                    let (ocs, oce, ors, ore) = placements[j];
                    ocs != 0 && ors != 0
                        && cursor_col < oce && cursor_col + col_span > ocs
                        && try_row < ore && try_re > ors
                });

                if !overlaps && (try_re - 1) <= n_explicit_rows || n_explicit_rows == 1 {
                    placements[k] = (cursor_col, cursor_col + col_span, try_row, try_re);
                    cursor_row = try_re;
                    if cursor_row > n_explicit_rows {
                        cursor_row = 1;
                        cursor_col += 1;
                    }
                    break;
                }
                if fixed_rs != 0 {
                    cursor_col += 1;
                    cursor_row = 1;
                } else {
                    cursor_row += 1;
                    if cursor_row > n_explicit_rows {
                        cursor_row = 1;
                        cursor_col += 1;
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
        lay_out(&mut children[i], content_x + col_offsets[c0], 0.0, cell_w, measurer, viewport, pcb);
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
            lay_out(&mut children[i], content_x, content_y + y_off, content_width, measurer, viewport, pcb);
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
        lay_out(&mut children[i], cell_x, cell_y, cell_w, measurer, viewport, pcb);

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
        GridLine::Auto => 0,
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
        GridLine::Auto => {
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
            if start > 0 { start + n } else { 0 }
        }
    }
}

/// Разбивает потоковые сегменты на строки.
///
/// Алгоритм: жадный word-wrap. Слова одного стиля на одной строке сливаются
/// в один `InlineFrag`. Сегменты обрабатываются по одному, чтобы учитывать
/// `pre_space` / `post_space` (inline box model: margin + border + padding).
fn wrap_inline_run(
    segments: &[InlineSegment],
    max_width: f32,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
) -> Vec<Vec<InlineFrag>> {
    let space_w = m.char_width(' ', container_font_size);

    let mut result: Vec<Vec<InlineFrag>> = Vec::new();
    let mut current_line: Vec<InlineFrag> = Vec::new();
    // CSS Text L3 §7.1: text-indent только на первой строке.
    let mut current_x = text_indent;

    for seg in segments {
        let words: Vec<&str> = seg.text.split_whitespace().collect();
        if words.is_empty() {
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

        let n = words.len();
        for (wi, word) in words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;

            // Space that the inline box model contributes at the word boundaries.
            let pre = if is_seg_first { seg.pre_space } else { 0.0 };
            let post = if is_seg_last { seg.post_space } else { 0.0 };

            let word_w: f32 = word
                .chars()
                .map(|c| m.char_width(c, style.font_size) + ls)
                .sum::<f32>()
                - if word.is_empty() { 0.0 } else { ls };

            let gap = if current_line.is_empty() { 0.0 } else { inter_word };

            // Wrap: слово не влезает (но первое слово строки добавляем всегда).
            if !current_line.is_empty() && current_x + gap + pre + word_w > max_width {
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
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
                        last.text.push_str(word);
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
                    width: word_w,
                    text: word.to_string(),
                    style: style.clone(),
                    padding_left: if is_seg_first { pad_l } else { 0.0 },
                    padding_right: if is_seg_last { pad_r } else { 0.0 },
                    is_element_box: seg.is_element_box,
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

/// Сдвигает фрагменты каждой строки вправо для center/right выравнивания.
/// Для Left — no-op.
fn align_lines(
    lines: &mut [Vec<InlineFrag>],
    content_width: f32,
    text_align: TextAlign,
) {
    for line in lines.iter_mut() {
        let Some(last) = line.last() else { continue };
        let line_width = last.x + last.width;
        let offset = match text_align {
            TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
            TextAlign::Right => (content_width - line_width).max(0.0),
            TextAlign::Left => 0.0,
        };
        if offset > 0.0 {
            for frag in line.iter_mut() {
                frag.x += offset;
            }
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
        let text: String = seg.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let merged = if let Some(last) = frags.last_mut() {
            if last.style.text_rendering_eq(&seg.style) {
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
                width: 0.0,
                text,
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
            });
        }
    }
    if frags.is_empty() { vec![] } else { vec![frags] }
}
