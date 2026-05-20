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
    FlexDirection, Length, LengthOrAuto, TextAlign,
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
}

/// Позиционированный текстовый фрагмент в строке (после layout).
/// `x` — смещение от левого края inline-контейнера, `width` — ширина текста
/// фрагмента в пикселях (нужна для text-align и подрисовки text-decoration).
#[derive(Debug, Clone)]
pub struct InlineFrag {
    pub x: f32,
    pub width: f32,
    pub text: String,
    pub style: ComputedStyle,
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
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
}

pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport);
    propagate_canvas_background(doc, &mut root);
    lay_out(&mut root, 0.0, 0.0, viewport.width, None, viewport);
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
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(measurer), viewport);
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
            out.push(InlineSegment { text, style: inherited.clone() });
        }
        NodeData::Text(_) => {}
        NodeData::Element { .. } => {
            let s = compute_style(doc, id, sheet, inherited, viewport);
            if s.display == Display::None {
                return;
            }
            let children: Vec<NodeId> = doc.get(id).children.clone();
            for child_id in children {
                collect_inline_segments(doc, sheet, child_id, &s, viewport, out);
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

                loop {
                    if i >= dom_children.len() {
                        break;
                    }
                    let cid = dom_children[i];
                    if let NodeData::Text(s) = &doc.get(cid).data
                        && s.chars().all(char::is_whitespace)
                    {
                        i += 1;
                        continue;
                    }
                    if is_inline_content(doc, sheet, cid, &style, viewport) {
                        collect_inline_segments(doc, sheet, cid, &style, viewport, &mut pending);
                        i += 1;
                    } else if is_inline_block(doc, sheet, cid, &style, viewport) {
                        if !pending.is_empty() {
                            row_items.push(anon_inline_run(
                                id,
                                &style,
                                std::mem::take(&mut pending),
                            ));
                        }
                        row_items.push(build_box(doc, sheet, cid, &style, viewport));
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

fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
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
            *lines = wrap_inline_run(segments, wrap_width, s.font_size, text_indent_px, m);
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

    match &mut b.kind {
        BoxKind::Block | BoxKind::Image { .. } => {
            // Flex containers dispatch to lay_out_flex before block-flow.
            if matches!(s.display, Display::Flex | Display::InlineFlex) {
                let content_height = lay_out_flex(
                    &mut b.children, &s, content_x, content_y, content_width, measurer, viewport,
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
            for child in &mut b.children {
                lay_out(child, content_x, child_y, content_width, measurer, viewport);
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
            // Горизонтальный layout: inline-block боксы + InlineRun-ы в одном потоке.
            // InlineRun получает оставшуюся ширину (после предшествующих inline-block).
            // Inline-block дети используют полную ширину контейнера для CSS-auto.
            let mut cur_x = content_x;
            let mut max_h: f32 = 0.0;
            for child in &mut b.children {
                let child_avail = if matches!(child.kind, BoxKind::InlineRun { .. }) {
                    // Оставшаяся ширина после уже разложенных inline-block детей.
                    (content_width - (cur_x - content_x)).max(0.0)
                } else {
                    content_width
                };
                lay_out(child, cur_x, content_y, child_avail, measurer, viewport);
                if matches!(child.kind, BoxKind::Skip) {
                    continue;
                }
                // child margins resolved against parent content_width (cb for child).
                let c_em = child.style.font_size;
                let child_mr = child.style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                let child_mt = child.style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let child_mb = child.style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                cur_x = child.rect.x + child.rect.width + child_mr;
                let child_full_h = child_mt + child.rect.height + child_mb;
                max_h = max_h.max(child_full_h);
            }
            b.rect.height = max_h;
        }
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::Skip => unreachable!(),
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
) -> f32 {
    let is_column = matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
    let is_reverse = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );

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
    let item_gap = if is_column {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };
    let n_items = item_idxs.len();
    let total_gap = if n_items > 1 { item_gap * (n_items - 1) as f32 } else { 0.0 };

    // Step 1 — compute hypothetical main sizes.
    // Do a preliminary layout for each item to get intrinsic sizes.
    for &i in &item_idxs {
        let item = &mut children[i];
        lay_out(item, content_x, content_y, content_width, measurer, viewport);
    }

    let cb = content_width;

    // hyp_mains[k] = total outer main-axis span of item k (including item's margins).
    let mut hyp_mains: Vec<f32> = item_idxs
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
                    // Use result of preliminary layout.
                    if is_column {
                        item.rect.height + m_t + m_b
                    } else {
                        item.rect.width + m_l + m_r
                    }
                }
                FlexBasis::Length(l) => {
                    let base = l.resolve(iem, Some(cb), viewport).unwrap_or(0.0).max(0.0);
                    if is_column {
                        base + m_t + m_b
                    } else {
                        base + m_l + m_r
                    }
                }
            }
        })
        .collect();

    // Step 2 — distribute free space (flex-grow / flex-shrink).
    let total_hyp: f32 = hyp_mains.iter().sum();
    let free_space = if is_column { 0.0 } else { container_main - total_hyp - total_gap };

    if free_space > 0.0 {
        let total_grow: f32 = item_idxs.iter().map(|&i| children[i].style.flex_grow).sum();
        if total_grow > 0.0 {
            for (k, &i) in item_idxs.iter().enumerate() {
                let grow = children[i].style.flex_grow;
                hyp_mains[k] += free_space * (grow / total_grow);
            }
        }
    } else if free_space < 0.0 {
        // Weighted shrink: weight = shrink_factor * hypothetical_main.
        let weights: Vec<f32> = item_idxs
            .iter()
            .enumerate()
            .map(|(k, &i)| children[i].style.flex_shrink * hyp_mains[k])
            .collect();
        let total_weight: f32 = weights.iter().sum();
        if total_weight > 0.0 {
            for (k, _) in item_idxs.iter().enumerate() {
                hyp_mains[k] = (hyp_mains[k] + free_space * (weights[k] / total_weight)).max(0.0);
            }
        }
    }

    // Step 3 — justify-content: compute start offset and gap between items.
    let n = item_idxs.len();
    let resolved_main: f32 = hyp_mains.iter().sum();
    let remaining = if is_column { 0.0 } else { (content_width - resolved_main - total_gap).max(0.0) };

    let (jc_start, jc_gap) = match s.justify_content {
        AlignValue::End => (remaining, 0.0),
        AlignValue::Center => (remaining / 2.0, 0.0),
        AlignValue::SpaceBetween => {
            if n <= 1 {
                (0.0, 0.0)
            } else {
                (0.0, remaining / (n - 1) as f32)
            }
        }
        AlignValue::SpaceAround => {
            let per = remaining / n as f32;
            (per / 2.0, per)
        }
        AlignValue::SpaceEvenly => {
            let per = remaining / (n + 1) as f32;
            (per, per)
        }
        _ => (0.0, 0.0), // Start / Auto / Normal
    };

    // Step 4 — final layout with resolved sizes.
    // Order items (reverse if needed).
    let ordered_keys: Vec<usize> = if is_reverse {
        (0..n).rev().collect()
    } else {
        (0..n).collect()
    };

    let mut main_cursor = jc_start;

    for &k in &ordered_keys {
        let i = item_idxs[k];
        let outer_main = hyp_mains[k];
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
            );
            main_cursor += outer_main + item_gap + jc_gap;
        } else {
            let inner_main = (outer_main - m_l - m_r).max(0.0);
            children[i].style.width = Some(Length::Px(inner_main));
            lay_out(
                &mut children[i],
                content_x + main_cursor + m_l,
                content_y + m_t,
                inner_main,
                measurer,
                viewport,
            );
            main_cursor += outer_main + item_gap + jc_gap;
        }
    }

    // Step 5 — align-items on the cross axis.
    // Row: cross axis = height.  Column: cross axis = width (auto = stretch).
    if !is_column {
        let cross_max: f32 = item_idxs
            .iter()
            .map(|&i| children[i].rect.height)
            .fold(0.0_f32, f32::max);

        for &i in &item_idxs {
            let item = &mut children[i];
            let is = &item.style;
            let iem = is.font_size;
            let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);

            let align = if matches!(is.align_self, AlignValue::Auto) {
                s.align_items
            } else {
                is.align_self
            };

            let outer_cross = item.rect.height + m_t + m_b;
            match align {
                AlignValue::End => {
                    item.rect.y = content_y + cross_max - outer_cross + m_t;
                }
                AlignValue::Center => {
                    item.rect.y = content_y + m_t + (cross_max - outer_cross) / 2.0;
                }
                AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                    // Stretch: override item height to fill cross axis.
                    let stretch_h = (cross_max - m_t - m_b).max(item.rect.height);
                    if item.rect.height < stretch_h {
                        item.rect.height = stretch_h;
                    }
                    item.rect.y = content_y + m_t;
                }
                _ => {
                    // Start / Baseline etc.: leave at content_y + m_t.
                    item.rect.y = content_y + m_t;
                }
            }
        }

        // Container content height = max cross size.
        return cross_max;
    }

    // Column: content height = sum of item outer heights.
    main_cursor
}

/// Разбивает потоковые сегменты на строки, объединяя слова с одинаковым стилем.
///
/// Алгоритм: жадный word-wrap (как в CSS normal flow). Слова одного стиля
/// на одной строке сливаются в один `InlineFrag` — это даёт один DrawText
/// на стилевой пробег, как ожидает рендерер.
fn wrap_inline_run(
    segments: &[InlineSegment],
    max_width: f32,
    container_font_size: f32,
    text_indent: f32,
    m: &dyn TextMeasurer,
) -> Vec<Vec<InlineFrag>> {
    let space_w = m.char_width(' ', container_font_size);

    // Токенизируем все сегменты в пары (слово, стиль).
    let tagged: Vec<(String, &ComputedStyle)> = segments
        .iter()
        .flat_map(|seg| seg.text.split_whitespace().map(move |w| (w.to_string(), &seg.style)))
        .collect();

    if tagged.is_empty() {
        return vec![];
    }

    let mut result: Vec<Vec<InlineFrag>> = Vec::new();
    let mut current_line: Vec<InlineFrag> = Vec::new();
    // CSS Text L3 §7.1: text-indent добавляется только к первой строке.
    // На последующих строках начинаем с 0.
    let mut current_x = text_indent;

    for (word, style) in &tagged {
        // letter-spacing: между каждой парой символов в слове + на word
        // boundary. word-spacing: только на word boundary (CSS Text L3
        // §11.2-3).
        let ls = style.letter_spacing;
        let ws = style.word_spacing;
        let word_w: f32 = word
            .chars()
            .map(|c| m.char_width(c, style.font_size) + ls)
            .sum::<f32>()
            - if word.is_empty() { 0.0 } else { ls }; // последний symbol не добавляет ls справа
        let gap_with_ls = space_w + ls + ws;

        // Перенос: слово не влезает (но первое слово строки добавляем всегда).
        if !current_line.is_empty() && current_x + gap_with_ls + word_w > max_width {
            result.push(std::mem::take(&mut current_line));
            current_x = 0.0;
        }

        let gap = if current_line.is_empty() { 0.0 } else { gap_with_ls };
        let frag_x = current_x + gap;

        // Если стиль визуально эквивалентен предыдущему фрагменту — сливаем.
        let merged = if let Some(last) = current_line.last_mut() {
            if last.style.text_rendering_eq(style) {
                last.text.push(' ');
                last.text.push_str(word);
                last.width += gap_with_ls + word_w;
                true
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
                text: word.clone(),
                style: (*style).clone(),
            });
        }

        current_x = frag_x + word_w;
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
            });
        }
    }
    if frags.is_empty() { vec![] } else { vec![frags] }
}
