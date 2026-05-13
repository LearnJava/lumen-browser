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

use crate::style::{compute_style, BoxSizing, ComputedStyle, Display, TextAlign};
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
    lay_out(&mut root, 0.0, 0.0, viewport.width, None);
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
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(measurer));
    root
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
            compute_style(doc, id, sheet, inherited, viewport).display == Display::Inline
        }
        _ => false,
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
    let style = compute_style(doc, id, sheet, inherited, viewport);

    let kind = match &doc.get(id).data {
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. } => BoxKind::Skip,
        NodeData::Document | NodeData::Element { .. } => {
            if style.display == Display::None {
                BoxKind::Skip
            } else if is_image_element(doc, id) {
                let node = doc.get(id);
                BoxKind::Image {
                    src: node.get_attr("src").unwrap_or("").to_string(),
                    alt: node.get_attr("alt").unwrap_or("").to_string(),
                }
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
            if is_inline_content(doc, sheet, child_id, &style, viewport) {
                // Собираем последовательный run inline-контента в один InlineRun.
                let mut segs: Vec<InlineSegment> = Vec::new();
                while i < dom_children.len()
                    && is_inline_content(doc, sheet, dom_children[i], &style, viewport)
                {
                    collect_inline_segments(doc, sheet, dom_children[i], &style, viewport, &mut segs);
                    i += 1;
                }
                if !segs.is_empty() {
                    // Анонимный контейнер не имеет собственного box-model spacing.
                    let mut inline_style = style.clone();
                    inline_style.margin_top = 0.0;
                    inline_style.margin_right = 0.0;
                    inline_style.margin_bottom = 0.0;
                    inline_style.margin_left = 0.0;
                    inline_style.padding_top = 0.0;
                    inline_style.padding_right = 0.0;
                    inline_style.padding_bottom = 0.0;
                    inline_style.padding_left = 0.0;
                    inline_style.background_color = None;
                    inline_style.width = None;
                    inline_style.height = None;
                    inline_style.min_width = None;
                    inline_style.max_width = None;
                    inline_style.min_height = None;
                    inline_style.max_height = None;
                    inline_style.border_top_width = 0.0;
                    inline_style.border_right_width = 0.0;
                    inline_style.border_bottom_width = 0.0;
                    inline_style.border_left_width = 0.0;
                    inline_style.box_sizing = BoxSizing::ContentBox;
                    children.push(LayoutBox {
                        node: id,
                        rect: Rect::ZERO,
                        style: inline_style,
                        kind: BoxKind::InlineRun { segments: segs, lines: vec![] },
                        children: vec![],
                    });
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

fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    measurer: Option<&dyn TextMeasurer>,
) {
    if matches!(b.kind, BoxKind::Skip) {
        b.rect = Rect::new(start_x, start_y, 0.0, 0.0);
        return;
    }

    let s = b.style.clone();
    b.rect.x = start_x + s.margin_left;
    b.rect.y = start_y + s.margin_top;
    // Block: auto-ширина = весь доступный inline-размер контейнера.
    // Replaced element (Image): auto-ширина = intrinsic (0 в Phase 0, без
    // декодированных пикселей). Это CSS 2.1 §10.3.2 — replaced-боксы
    // НЕ растягиваются на весь контейнер при отсутствии width.
    let is_replaced = matches!(b.kind, BoxKind::Image { .. });
    b.rect.width = if is_replaced {
        0.0
    } else {
        (available_width - s.margin_left - s.margin_right).max(0.0)
    };
    // Явная ширина (CSS width: Npx) перекрывает auto-ширину.
    // box-sizing определяет, к какой части бокса относится `width`:
    //   - content-box: width — это размер контента, padding+border прибавляются;
    //   - border-box: width — общий размер вместе с padding+border.
    if let Some(w) = s.width {
        b.rect.width = match s.box_sizing {
            BoxSizing::ContentBox => (w + s.padding_left + s.padding_right
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
        BoxSizing::ContentBox => v + s.padding_left + s.padding_right
            + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_w) = s.max_width {
        b.rect.width = b.rect.width.min(outer_horiz(max_w).max(0.0));
    }
    if let Some(min_w) = s.min_width {
        b.rect.width = b.rect.width.max(outer_horiz(min_w).max(0.0));
    }

    let content_x = b.rect.x + s.padding_left + s.border_left_width;
    let content_y = b.rect.y + s.padding_top + s.border_top_width;
    let content_width = (b.rect.width
        - s.padding_left - s.padding_right
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
            *lines = wrap_inline_run(segments, wrap_width, s.font_size, s.text_indent, m);
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
            // Image не имеет flow-детей, поэтому child-цикл просто пуст —
            // объединяем с Block, чтобы общий код width/height/min-max/borders
            // не дублировался. content_height = 0 для Image без явной высоты
            // даёт коробку только из padding+border (что для пустой картинки
            // визуально корректно).
            let mut child_y = content_y;
            for child in &mut b.children {
                lay_out(child, content_x, child_y, content_width, measurer);
                if matches!(child.kind, BoxKind::Skip) {
                    continue;
                }
                child_y = child.rect.y + child.rect.height + child.style.margin_bottom;
            }
            let content_height = (child_y - content_y).max(0.0);
            // Явная высота (CSS height: Npx) перекрывает auto-высоту по содержимому.
            // box-sizing работает симметрично width: content-box прибавляет
            // padding+border, border-box оставляет h как итоговую высоту.
            b.rect.height = if let Some(h) = s.height {
                match s.box_sizing {
                    BoxSizing::ContentBox => h
                        + s.padding_top + s.padding_bottom
                        + s.border_top_width + s.border_bottom_width,
                    BoxSizing::BorderBox => h.max(0.0),
                }
            } else {
                content_height + s.padding_top + s.padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
            // CSS 2.1 §10.4: clamp [min-height, max-height]. Симметрия с
            // width: max сначала, потом min → «min побеждает max». Content
            // оверфлоу-ит коробку если min режет ниже — это правильное
            // поведение CSS.
            let outer_vert = |v: f32| match s.box_sizing {
                BoxSizing::ContentBox => v + s.padding_top + s.padding_bottom
                    + s.border_top_width + s.border_bottom_width,
                BoxSizing::BorderBox => v,
            };
            if let Some(max_h) = s.max_height {
                b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
            }
            if let Some(min_h) = s.min_height {
                b.rect.height = b.rect.height.max(outer_vert(min_h).max(0.0));
            }
        }
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::Skip => unreachable!(),
    }
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
