//! Stacking-aware hit testing (P2 п.2B).
//!
//! Принимает точку в viewport-координатах и `LayoutBox`-дерево; возвращает
//! верхний DOM-узел, который попадает под курсор. Используется P3 для
//! `event.target` (click / mousemove / pointerdown).
//!
//! Алгоритм — обратный CSS painting order (CSS 2.1 Appendix E + Painting Order
//! L3 §3). Внутри одного бокса дети группируются:
//! - positive-z stacking-context children (sorted descending by z),
//! - in-flow non-positioned + auto/0-z stacking-context children в обратном
//!   DOM-порядке (позже в DOM = выше в paint),
//! - negative-z stacking-context children (sorted descending, т.е. ближе к
//!   0 — выше).
//!
//! Hit-тест поочерёдно проверяет каждую группу; первое попадание возвращается.
//! Внутри ребёнка спускаемся рекурсивно. Если все дети промахнулись — пробуем
//! сам бокс. `pointer-events: none` пропускает бокс; дети остаются
//! hit-testable (свойство наследуется, так что без явного `auto` на ребёнке
//! они тоже пропускаются, но `auto` на потомке возвращает его в игру). `display: none` и `Skip`-боксы исключены
//! целиком вместе со своим поддеревом.
//!
//! Transform inversion: если бокс имеет CSS `transform`, forward-матрица для
//! него — `T(pivot) · M · T(-pivot)` в viewport-координатах, где `pivot =
//! box.origin + transform_origin`. Перед спуском в детей точка инвертируется
//! этой forward-матрицей: дети получают точку в системе, в которой их
//! `rect`-ы валидны.
//!
//! Phase 0 ограничения:
//! - Фазы 3/4/5 (Block / Floats / InlineContent) не разделяются — мы
//!   обходим всех in-flow детей одним проходом в reverse-DOM. Реальное
//!   разделение float / inline станет нужно при появлении CSS float layout
//!   (P1 4B).
//! - InlineRun-ы — анонимные контейнеры с `node = id родителя`; hit на текст
//!   возвращает родительский DOM-элемент. Точное определение «какой
//!   текстовый узел под курсором» отложено до Selection / Range model
//!   (P1 6+).
//! - Только 2D affine transforms. 3D transforms потребуют полного 4×4 invert
//!   в `Mat4`.

use std::collections::HashMap;

use crate::display_list::clip_path_contains;
use lumen_core::geom::{Point, Rect};
use lumen_dom::NodeId;
use lumen_layout::style::WritingMode;
use lumen_layout::{
    creates_stacking_context, forward_box_transform, owns_paint_layer, paint_child_order, BoxKind,
    Cursor, Display, FormControlKind, InlineFrag, LayoutBox, Mat4, PointerEvents, UserSelect,
};

/// Результат hit-теста.
#[derive(Debug, Clone)]
pub struct HitTestResult {
    /// DOM-узел верхнего слоя, попавшего под курсор. Для InlineRun — это
    /// DOM-предок (тот же, кому принадлежит inline-контент).
    pub node: NodeId,
    /// Конкретный DOM-узел, породивший попавший под курсор `InlineFrag`.
    /// Для `InlineRun` это текстовый DOM-узел (дочерний по отношению к `<a>`,
    /// `<span>` и т.д.), что позволяет `find_link_href` найти `<a>` при
    /// обходе вверх: `text_node → <a href="…"> → найдено`.
    /// Для не-InlineRun боксов равен `node`.
    pub source_node: NodeId,
    /// Узел-источник `InlineFrag`, в чей горизонтальный/вертикальный интервал
    /// точка попала **строго** (без fallback «последний фрагмент строки»):
    /// текстовый узел под `<a>`/`<span>`/`<b>`. Для `source_node` fallback
    /// нужен клику по ссылке, для `document.elementFromPoint` (CSSOM View §3) —
    /// нет: пустое место справа от текста принадлежит блоку, не inline-элементу.
    /// `None` — точка не над фрагментом (или бокс не `InlineRun`).
    pub inline_source: Option<NodeId>,
    /// Координаты попадания в системе hit-узла после всех transform-инверсий
    /// по цепочке предков (та же система, в которой `b.rect` валиден).
    pub local_point: Point,
    /// Ancestor chain снизу-вверх: `path[0] == node`, `path.last()` — корень
    /// layout-дерева (документа). Используется event dispatch-ом для bubble
    /// stage без повторного walk-а.
    pub path: Vec<NodeId>,
    /// CSS `cursor` hit-узла — уже разрешённое used-value: `auto` доводится
    /// до `text`/`default` по контексту (UA-хинт `:any-link` + фрагмент
    /// текста), см. [`resolve_auto_cursor`]. Для wire-up к OS cursor.
    pub cursor: Cursor,
    /// CSS `user-select` hit-узла — для запрета выделения текста (`None`)
    /// или принудительного выделения всего элемента (`All`).
    pub user_select: UserSelect,
}

/// Hit-тест точки в viewport-координатах. `root` — layout-дерево из
/// `lumen_layout::layout` или `layout_measured`.
///
/// Возвращает `None`, если точка не попала ни в один бокс (например, упала
/// за пределы viewport-а или в полностью прозрачное место без боксов).
pub fn hit_test(point: Point, root: &LayoutBox) -> Option<HitTestResult> {
    let layers = Layers::build(root);
    hit_test_box(point, root, &layers, &[])
}

/// Полный hit-тест: все боксы, чей rect содержит точку, в paint-порядке
/// (топ-слой первый). Бэкенд `document.elementsFromPoint` (CSSOM View §3,
/// BUG-464/BUG-477) — в отличие от [`hit_test`], не останавливается на первом
/// попадании, а обходит дерево тем же алгоритмом (группы positive-z / in-flow
/// reverse-DOM / negative-z, transform-инверсия, `pointer-events`/`display`
/// фильтры) и собирает КАЖДОЕ попадание, включая предков хита. Отдельная
/// функция, а не обёртка над `hit_test`, — hit-тест клика/курсора (частый,
/// interactive путь) не должен платить за полный обход дерева ради первого
/// попадания.
///
/// Один DOM-узел может отдать больше одного бокса (принципал + `InlineRun`);
/// вызывающий (JS-натив `_lumen_elements_from_point`) отвечает за дедупликацию
/// по `NodeId`, сохраняя порядок первого вхождения.
pub fn hit_test_all(point: Point, root: &LayoutBox) -> Vec<HitTestResult> {
    let layers = Layers::build(root);
    let mut out = Vec::new();
    hit_test_all_box(point, root, &layers, &[], &mut out);
    out
}

/// A box that owns the paint layer the hit test enters, with the boxes without a
/// paint layer between it and the level that orders it (that level's direct child
/// first). A plain box is transparent for z-order — CSS 2.1 Appendix E stacks a
/// positioned descendant in its nearest ancestor stacking context, not inside the
/// plain box it sits in — so its layer competes at that level, and the chain is
/// what keeps `HitTestResult::path` the full ancestry.
struct Candidate<'a> {
    b: &'a LayoutBox,
    chain: Vec<&'a LayoutBox>,
}

/// The layers each box hands up to the stacking context it belongs to, in pre-order:
/// layers below it that are not stacking contexts of their own, and everything the
/// plain boxes below it hand up in turn. Built once per top-level call: the hit test
/// enters plain boxes one level at a time and would otherwise re-walk every subtree
/// for every level it passes.
struct Layers<'a> {
    of: HashMap<*const LayoutBox, Vec<Candidate<'a>>>,
}

impl<'a> Layers<'a> {
    fn build(root: &'a LayoutBox) -> Self {
        let mut of = HashMap::new();
        collect_layers(root, &mut of);
        Self { of }
    }

    fn of(&self, b: &'a LayoutBox) -> &[Candidate<'a>] {
        self.of.get(&(b as *const LayoutBox)).map_or(&[], Vec::as_slice)
    }
}

/// Whether `b` orders its descendants by `z-index` on its own — a stacking context
/// in the sense of `lumen_layout::stacking::walk`: the layers below it stop here
/// and never reach the level above.
fn is_stacking_context(b: &LayoutBox) -> bool {
    owns_paint_layer(b) && creates_stacking_context(&b.style)
}

fn children_in_paint_order(b: &LayoutBox) -> Vec<&LayoutBox> {
    if matches!(b.kind, BoxKind::Canvas { layout_subtree: true, .. }) {
        return Vec::new();
    }
    let order = paint_child_order(b);
    (0..b.children.len()).map(|k| &b.children[order.as_ref().map_or(k, |o| o[k])]).collect()
}

/// Records in `of` what every box under `b` hands up to the stacking context it
/// belongs to. A stacking context hands up nothing: its layers are ordered inside it.
fn collect_layers<'a>(b: &'a LayoutBox, of: &mut HashMap<*const LayoutBox, Vec<Candidate<'a>>>) {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return;
    }
    let children = children_in_paint_order(b);
    for &child in &children {
        collect_layers(child, of);
    }
    if is_stacking_context(b) {
        return;
    }
    let mut handed_up = Vec::new();
    for &child in &children {
        if owns_paint_layer(child) {
            handed_up.push(Candidate { b: child, chain: Vec::new() });
            if is_stacking_context(child) {
                continue;
            }
        }
        if let Some(below) = of.get(&(child as *const LayoutBox)) {
            for cand in below {
                let mut chain = Vec::with_capacity(cand.chain.len() + 1);
                chain.push(child);
                chain.extend_from_slice(&cand.chain);
                handed_up.push(Candidate { b: cand.b, chain });
            }
        }
    }
    if !handed_up.is_empty() {
        of.insert(b as *const LayoutBox, handed_up);
    }
}

/// The boxes one level of the hit test enters, in the order it enters them.
struct Level<'a> {
    positive: Vec<(Candidate<'a>, i32)>,
    /// Layers with `z-index: auto` or `0`.
    zero: Vec<Candidate<'a>>,
    negative: Vec<(Candidate<'a>, i32)>,
    /// Plain boxes, each entered afterwards for its own in-flow content.
    in_flow: Vec<&'a LayoutBox>,
    /// Every layer this level ordered on behalf of the boxes below it. Those are
    /// placed here already, so the plain box must not enter them a second time.
    handed_up: Vec<&'a LayoutBox>,
}

/// `LayoutBox` has no value equality; a layer is the box itself.
fn skipped(skip: &[&LayoutBox], b: &LayoutBox) -> bool {
    skip.iter().any(|s| std::ptr::eq(*s, b))
}

fn place<'a>(level: &mut Level<'a>, skip: &[&'a LayoutBox], cand: Candidate<'a>) {
    if skipped(skip, cand.b) {
        return;
    }
    match cand.b.style.z_index {
        Some(z) if z > 0 => level.positive.push((cand, z)),
        Some(z) if z < 0 => level.negative.push((cand, z)),
        _ => level.zero.push(cand),
    }
}

/// Splits what the level around `b` has to enter by paint order. A layer `b` is a
/// stacking context of orders against the layers the plain boxes below hand up —
/// that is what puts a panel nested in `#root > div > div` above a scrim that is a
/// direct child of `<body>`.
fn classify<'a>(b: &'a LayoutBox, layers: &Layers<'a>, skip: &[&'a LayoutBox]) -> Level<'a> {
    let mut level = Level {
        positive: Vec::new(),
        zero: Vec::new(),
        negative: Vec::new(),
        in_flow: Vec::new(),
        handed_up: Vec::new(),
    };
    for child in children_in_paint_order(b) {
        if owns_paint_layer(child) {
            place(&mut level, skip, Candidate { b: child, chain: Vec::new() });
            if is_stacking_context(child) {
                continue;
            }
        } else {
            level.in_flow.push(child);
        }
        for cand in layers.of(child) {
            if skipped(skip, cand.b) {
                continue;
            }
            let mut chain = Vec::with_capacity(cand.chain.len() + 1);
            chain.push(child);
            chain.extend_from_slice(&cand.chain);
            level.handed_up.push(cand.b);
            place(&mut level, skip, Candidate { b: cand.b, chain });
        }
    }
    level.positive.sort_by_key(|(_, z)| std::cmp::Reverse(*z));
    level.negative.sort_by_key(|(_, z)| std::cmp::Reverse(*z));
    level
}

/// The layers a `zero` entry ordered on behalf of the boxes below it — what its
/// own recursion must skip. A stacking context has none: its layers start afresh.
fn handed_up_below<'a>(b: &'a LayoutBox, layers: &Layers<'a>) -> Vec<&'a LayoutBox> {
    layers.of(b).iter().map(|c| c.b).collect()
}

fn hit_test_box<'a>(
    point: Point,
    b: &'a LayoutBox,
    layers: &Layers<'a>,
    skip: &[&'a LayoutBox],
) -> Option<HitTestResult> {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return None;
    }

    let child_point = match invert_box_transform(b) {
        Some(inv) => {
            let (x, y) = inv.transform_point_2d(point.x, point.y);
            Point::new(x, y)
        }
        None if !b.style.transform.is_empty() => {
            return None;
        }
        None => point,
    };
    if !clip_path_contains(b, child_point) {
        return None;
    }

    let level = classify(b, layers, skip);
    for (cand, _) in &level.positive {
        if let Some(mut hit) = enter(child_point, cand, &[], layers) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }
    for cand in level.zero.iter().rev() {
        if let Some(mut hit) = enter(child_point, cand, &handed_up_below(cand.b, layers), layers) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }
    // Everything placed here stays placed when a plain box is entered for its own
    // content: what it hands up is a part of the same stacking context.
    let skip_below: Vec<&'a LayoutBox> =
        skip.iter().copied().chain(level.handed_up.iter().copied()).collect();
    for child in level.in_flow.iter().rev() {
        if let Some(mut hit) = hit_test_box(child_point, child, layers, &skip_below) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }
    for (cand, _) in &level.negative {
        if let Some(mut hit) = enter(child_point, cand, &[], layers) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }

    if !rect_contains(b.rect, child_point) {
        return None;
    }
    let (source_node, frag_pe, text_frag) = find_inline_source(b, child_point);
    if matches!(frag_pe.unwrap_or_else(|| b.style.effective_pointer_events()), PointerEvents::None) {
        return None;
    }
    let css_cursor = text_frag.map_or(b.style.cursor, |f| f.style.cursor);
    Some(HitTestResult {
        node: b.node,
        source_node,
        inline_source: text_frag.map(|f| f.source_node),
        local_point: child_point,
        path: vec![b.node],
        cursor: resolve_auto_cursor(css_cursor, b, text_frag),
        user_select: b.style.user_select,
    })
}

/// Enters a layer the level ordered. The plain boxes on the way down clip it like
/// any ancestor does and stay in its `path`.
fn enter<'a>(
    child_point: Point,
    cand: &Candidate<'a>,
    skip: &[&'a LayoutBox],
    layers: &Layers<'a>,
) -> Option<HitTestResult> {
    for via in &cand.chain {
        if matches!(via.kind, BoxKind::Skip)
            || via.style.display == Display::None
            || !clip_path_contains(via, child_point)
        {
            return None;
        }
    }
    let mut hit = hit_test_box(child_point, cand.b, layers, skip)?;
    for via in cand.chain.iter().rev() {
        hit.path.push(via.node);
    }
    Some(hit)
}

fn hit_test_all_box<'a>(
    point: Point,
    b: &'a LayoutBox,
    layers: &Layers<'a>,
    skip: &[&'a LayoutBox],
    out: &mut Vec<HitTestResult>,
) {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return;
    }

    let child_point = match invert_box_transform(b) {
        Some(inv) => {
            let (x, y) = inv.transform_point_2d(point.x, point.y);
            Point::new(x, y)
        }
        None if !b.style.transform.is_empty() => {
            return;
        }
        None => point,
    };
    if !clip_path_contains(b, child_point) {
        return;
    }

    let level = classify(b, layers, skip);
    let start = out.len();
    for (cand, _) in &level.positive {
        enter_all(child_point, cand, &[], layers, out);
    }
    for cand in level.zero.iter().rev() {
        enter_all(child_point, cand, &handed_up_below(cand.b, layers), layers, out);
    }
    let skip_below: Vec<&'a LayoutBox> =
        skip.iter().copied().chain(level.handed_up.iter().copied()).collect();
    for child in level.in_flow.iter().rev() {
        hit_test_all_box(child_point, child, layers, &skip_below, out);
    }
    for (cand, _) in &level.negative {
        enter_all(child_point, cand, &[], layers, out);
    }
    for r in &mut out[start..] {
        r.path.push(b.node);
    }

    if !rect_contains(b.rect, child_point) {
        return;
    }
    let (source_node, frag_pe, text_frag) = find_inline_source(b, child_point);
    if matches!(frag_pe.unwrap_or_else(|| b.style.effective_pointer_events()), PointerEvents::None) {
        return;
    }
    let css_cursor = text_frag.map_or(b.style.cursor, |f| f.style.cursor);
    out.push(HitTestResult {
        node: b.node,
        source_node,
        inline_source: text_frag.map(|f| f.source_node),
        local_point: child_point,
        path: vec![b.node],
        cursor: resolve_auto_cursor(css_cursor, b, text_frag),
        user_select: b.style.user_select,
    })
}

fn enter_all<'a>(
    child_point: Point,
    cand: &Candidate<'a>,
    skip: &[&'a LayoutBox],
    layers: &Layers<'a>,
    out: &mut Vec<HitTestResult>,
) {
    for via in &cand.chain {
        if matches!(via.kind, BoxKind::Skip)
            || via.style.display == Display::None
            || !clip_path_contains(via, child_point)
        {
            return;
        }
    }
    let start = out.len();
    hit_test_all_box(child_point, cand.b, layers, skip, out);
    for r in &mut out[start..] {
        for via in cand.chain.iter().rev() {
            r.path.push(via.node);
        }
    }
}

fn find_inline_source(
    b: &LayoutBox,
    point: Point,
) -> (NodeId, Option<PointerEvents>, Option<&InlineFrag>) {
    let BoxKind::InlineRun { lines, .. } = &b.kind else {
        return (b.node, None, None);
    };
    let line_h = b.used_line_height;
    if line_h <= 0.0 || lines.is_empty() {
        return (b.node, None, None);
    }
    // Вертикальные writing-mode: строки идут по x (`vertical-rl` — справа налево), фрагменты
    // раскладываются по y; `frag.x` — смещение вдоль строки (по inline-оси).
    let (rel_line, rel_x) = match b.style.writing_mode {
        WritingMode::HorizontalTb => (point.y - b.rect.y, point.x - b.rect.x),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            (b.rect.x + b.rect.width - point.x, point.y - b.rect.y)
        }
        WritingMode::VerticalLr | WritingMode::SidewaysLr => (point.x - b.rect.x, point.y - b.rect.y),
    };
    let line_idx = (rel_line / line_h).floor().max(0.0) as usize;
    let exact_line = lines.get(line_idx).is_some() && rel_line >= 0.0;
    let line = lines.get(line_idx).or_else(|| lines.last());
    let Some(line) = line else { return (b.node, None, None); };
    // `InlineRun` несёт стиль блок-контейнера, а `pointer-events` инлайнового
    // элемента может отличаться (`<span style="pointer-events:auto">` внутри
    // блока с `none`), поэтому значение берётся с попавшего под точку фрагмента.
    for frag in line {
        let start = frag.x - frag.padding_left;
        let end = frag.x + frag.width + frag.padding_right;
        if rel_x >= start && rel_x < end {
            let pe = Some(frag.style.effective_pointer_events());
            // A frag `wrap_inline_run` merged across same-style text nodes
            // carries several DOM sources (`merged_sources`); `source_node`
            // alone is only the FIRST one, so resolve the span under `rel_x`
            // — clicking "abc " of `<label>abc <input></label>` must reach
            // that label's text, not the preceding "jkl " of the previous
            // label merged into the same frag (WPT checkable-active-onblur).
            let mut src = frag.source_node;
            if !frag.merged_sources.is_empty() {
                for span in lumen_layout::text_geometry::frag_source_spans(frag) {
                    let sx = frag.x + span.x;
                    let ex = sx + span.width;
                    if rel_x >= sx && rel_x < ex && span.source_node.index() != 0 {
                        src = span.source_node;
                        break;
                    }
                }
            }
            let exact = exact_line.then_some(frag).filter(|f| f.source_node.index() != 0);
            if src.index() != 0 {
                return (src, pe, exact);
            }
            return (b.node, pe, exact);
        }
    }
    // Fallback: return source_node of the last frag on the line (closest to right edge).
    if let Some(last) = line.last() {
        let src = if last.source_node.index() != 0 { last.source_node } else { b.node };
        return (src, Some(last.style.effective_pointer_events()), None);
    }
    (b.node, None, None)
}

/// CSS UI L4 §10.7 `auto`: курсор выбирает UA по контексту — I-beam над
/// выделяемым текстом и текстовыми полями ввода, стрелка везде остальное.
/// Любое другое значение — уже resolved cascade-значение (включая
/// `:any-link { cursor: pointer }` из UA-таблицы), его не трогаем.
fn resolve_auto_cursor(css: Cursor, b: &LayoutBox, text_frag: Option<&InlineFrag>) -> Cursor {
    if css != Cursor::Auto {
        return css;
    }
    let text_control = match &b.kind {
        BoxKind::FormControl { kind: FormControlKind::Textarea { .. } } => true,
        BoxKind::FormControl { kind: FormControlKind::Input { input_type, .. } } => {
            input_type.is_textual()
        }
        _ => false,
    };
    // Inline-replaced `<img>` — тоже фрагмент строки, но не текст.
    let over_text = text_frag.is_some_and(|f| f.img_src.is_none());
    if over_text || text_control { Cursor::Text } else { Cursor::Default }
}

/// `Rect::contains(Point)`. Включаем левую/верхнюю границы, исключаем
/// правую/нижнюю — стандартная half-open семантика (точка ровно на
/// `box.right` принадлежит уже соседнему боксу).
fn rect_contains(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height
}

/// Inverse forward-transform для hit-теста. Forward-матрица в viewport-
/// координатах — `T(pivot) · M · T(-pivot)`, где `pivot = box.origin +
/// transform_origin` (transform-origin задан в локальных px бокса).
/// Возвращает `None`, если transform пуст (для caller — это «использовать
/// оригинальную точку», не «промахнуться») или если матрица сингулярна
/// (caller интерпретирует как «бокс не существует визуально»).
fn invert_box_transform(b: &LayoutBox) -> Option<Mat4> {
    // Тот же путь, что и у отрисовки: `translate`/`rotate`/`scale`, `transform`
    // (в т. ч. `translate*()` с процентами от border box) и motion path.
    forward_box_transform(b)?.invert_2d_affine()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;
    use lumen_dom::{Document, NodeData};
    use lumen_layout::layout;

    fn build(html: &str, css: &str) -> (Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("p{{margin:0}}{css}"));
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        (doc, root)
    }

    /// Найти DOM-узел по значению атрибута `class`. Panic, если не найден —
    /// в тестах удобнее, чем `Option`.
    fn by_class(doc: &Document, class: &str) -> NodeId {
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            let n = doc.get(id);
            if matches!(n.data, NodeData::Element { .. })
                && n.get_attr("class") == Some(class)
            {
                return id;
            }
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
        panic!("no element with class={class}");
    }

    /// Найти первый element с указанным local tag-name.
    fn by_tag(doc: &Document, tag: &str) -> NodeId {
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            let n = doc.get(id);
            if let NodeData::Element { name, .. } = &n.data
                && name.local == tag
            {
                return id;
            }
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
        panic!("no element with tag={tag}");
    }

    /// Страница портала после входа: панель `z-index: 1600` лежит в `#root > div > div`,
    /// полупрозрачный слой под ней (`z-index: 1500`) — прямой ребёнок `<body>` позже.
    fn panel_over_scrim() -> (Document, LayoutBox) {
        build(
            r#"<div class="wrap"><div class="deep"><div class="panel"></div></div></div><div class="scrim"></div>"#,
            ".panel { position: fixed; top: 0; left: 0; width: 200px; height: 100px; z-index: 1600; } \
             .scrim { position: fixed; top: 0; left: 0; width: 800px; height: 600px; z-index: 1500; }",
        )
    }

    /// Слой, вложенный в обычные блоки, складывается в ближайшем stacking context,
    /// а не внутри этих блоков (CSS 2.1 Appendix E). Хит-тест раньше входил в
    /// обычные блоки только после всех положительных слоёв уровня — и клик по
    /// открытой левой панели портала доставался слою-затемнению за ней.
    #[test]
    fn layer_nested_in_plain_boxes_outranks_a_lower_layer_beside_them() {
        let (doc, root) = panel_over_scrim();
        let r = hit_test(Point::new(50.0, 50.0), &root).expect("hit");
        assert_eq!(r.node, by_class(&doc, "panel"), "клик должен уходить панели, не слою за ней");
    }

    /// Промежуточные обычные блоки остаются в `path`: событие всплывает по всей
    /// цепочке предков.
    #[test]
    fn nested_layer_keeps_the_plain_boxes_in_its_path() {
        let (doc, root) = panel_over_scrim();
        let r = hit_test(Point::new(50.0, 50.0), &root).expect("hit");
        let path = r.path;
        let pos = |class: &str| path.iter().position(|&n| n == by_class(&doc, class)).expect("в path");
        assert!(
            pos("panel") < pos("deep") && pos("deep") < pos("wrap"),
            "path идёт снизу вверх: {path:?}"
        );
    }

    /// Полный обход отдаёт вложенный слой один раз и раньше слоя, что ниже него.
    #[test]
    fn hit_test_all_lists_a_nested_layer_once_above_the_lower_layer() {
        let (doc, root) = panel_over_scrim();
        let nodes: Vec<NodeId> = hit_test_all(Point::new(50.0, 50.0), &root)
            .into_iter()
            .map(|h| h.node)
            .collect();
        let (panel, scrim) = (by_class(&doc, "panel"), by_class(&doc, "scrim"));
        assert_eq!(nodes.iter().filter(|&&n| n == panel).count(), 1, "{nodes:?}");
        let at = |n: NodeId| nodes.iter().position(|&m| m == n).expect("в списке");
        assert!(at(panel) < at(scrim), "{nodes:?}");
    }

    /// Позиционированный слой с `z-index: auto` под обычным блоком рисуется позже
    /// in-flow содержимого соседа (шаг 8 против шага 3 Appendix E) — и первым
    /// попадает под клик.
    #[test]
    fn positioned_auto_layer_nested_in_a_plain_box_is_above_later_in_flow_content() {
        let (doc, root) = build(
            r#"<div class="wrap"><div class="tip"></div></div><div class="later"></div>"#,
            ".wrap { height: 10px; } \
             .tip { position: absolute; top: 0; left: 0; width: 200px; height: 100px; } \
             .later { height: 100px; }",
        );
        let r = hit_test(Point::new(50.0, 50.0), &root).expect("hit");
        assert_eq!(r.node, by_class(&doc, "tip"));
    }

    /// Отрицательный `z-index` под обычным блоком остаётся ниже in-flow содержимого.
    #[test]
    fn negative_layer_nested_in_a_plain_box_stays_below_in_flow_content() {
        let (doc, root) = build(
            r#"<div class="wrap"><div class="bg"></div></div><div class="later"></div>"#,
            ".wrap { height: 10px; } \
             .bg { position: absolute; top: 0; left: 0; width: 800px; height: 600px; z-index: -1; } \
             .later { height: 100px; }",
        );
        let r = hit_test(Point::new(50.0, 50.0), &root).expect("hit");
        assert_eq!(r.node, by_class(&doc, "later"));
    }

    #[test]
    fn miss_outside_viewport_returns_none() {
        // Точка глубоко за пределами 800×600 viewport-а.
        let (_, root) = build("<p>x</p>", "");
        assert!(hit_test(Point::new(5000.0, 5000.0), &root).is_none());
    }

    #[test]
    fn hit_inside_simple_block_returns_node() {
        // `<p>` занимает первую строку viewport-а; hit в центр (10, 10)
        // должен попасть в `<p>` (или в его InlineRun, который держит
        // `node = id(<p>)`).
        let (doc, root) = build("<p>hello</p>", "p { height: 50px; }");
        let p_id = by_tag(&doc, "p");
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(r.node, p_id, "hit на <p> ожидаемо даёт его NodeId");
    }

    #[test]
    fn pointer_events_none_skips_box_but_descends_to_children() {
        // Outer div c pointer-events:none, внутри — child div c content.
        // Hit должен либо вообще промахнуться, либо попасть НЕ в outer.
        let (doc, root) = build(
            r#"<div class="outer"><div class="inner">hi</div></div>"#,
            ".outer { pointer-events: none; height: 50px; }",
        );
        let outer = by_class(&doc, "outer");
        let r = hit_test(Point::new(10.0, 10.0), &root);
        if let Some(res) = r {
            assert_ne!(res.node, outer, "pointer-events:none box не должен быть target");
            assert!(
                !res.path.contains(&outer)
                    || res.path.iter().position(|&n| n == outer) != Some(0),
                "outer может быть только ancestor в path, не target"
            );
        }
    }

    #[test]
    fn pointer_events_none_is_inherited_by_child_box() {
        // Наследуется: у .inner своей декларации нет, значит он тоже `none`
        // и под курсором не остаётся ни одной цели, кроме предков вне outer.
        let (doc, root) = build(
            r#"<div class="outer"><div class="inner"></div></div>"#,
            ".outer { pointer-events: none; } .inner { height: 50px; }",
        );
        let outer = by_class(&doc, "outer");
        let inner = by_class(&doc, "inner");
        if let Some(res) = hit_test(Point::new(10.0, 10.0), &root) {
            assert_ne!(res.node, outer);
            assert_ne!(res.node, inner, "унаследованный none: inner не цель");
        }
    }

    #[test]
    fn pointer_events_auto_child_is_target_inside_none_parent() {
        let (doc, root) = build(
            r#"<div class="outer"><div class="inner"></div></div>"#,
            ".outer { pointer-events: none; } .inner { pointer-events: auto; height: 50px; }",
        );
        let inner = by_class(&doc, "inner");
        let res = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(res.node, inner, "auto на потомке возвращает его в hit-тест");
    }

    #[test]
    fn inert_box_is_not_a_target_even_with_pointer_events_auto() {
        // `[inert]` → `interactivity: inert` (UA !important): `pointer-events: auto` не спасает.
        let (doc, root) = build(
            r#"<div class="a" inert></div>"#,
            ".a { pointer-events: auto; height: 50px; }",
        );
        let a = by_class(&doc, "a");
        if let Some(res) = hit_test(Point::new(10.0, 10.0), &root) {
            assert_ne!(res.node, a, "inert-бокс не цель указателя");
        }
    }

    #[test]
    fn interactivity_auto_descendant_is_a_target_inside_inert_subtree() {
        let (doc, root) = build(
            r#"<div class="outer" inert><div class="inner"></div></div>"#,
            ".inner { interactivity: auto; height: 50px; }",
        );
        let inner = by_class(&doc, "inner");
        let res = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(res.node, inner, "interactivity:auto на потомке возвращает его в hit-тест");
    }

    #[test]
    fn pointer_events_auto_lets_box_be_target() {
        let (doc, root) = build("<div>x</div>", "div { height: 100px; }");
        let div = by_tag(&doc, "div");
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        // Hit либо в сам <div>, либо в его InlineRun (anonymous, node = id(<div>)).
        // В обоих случаях target NodeId совпадает с <div>.
        assert_eq!(r.node, div);
    }

    #[test]
    fn z_index_upper_layer_wins_in_overlap() {
        // .below и .above перекрываются (margin-top: -50px у .above).
        // .above (z=2) должен выиграть в overlap-е.
        let html = r#"<div class="below"></div><div class="above"></div>"#;
        let css = "
            div { position: relative; width: 200px; height: 100px; }
            .below { z-index: 1; }
            .above { z-index: 2; margin-top: -50px; }
        ";
        let (doc, root) = build(html, css);
        let above = by_class(&doc, "above");
        let r = hit_test(Point::new(10.0, 60.0), &root).expect("hit in overlap");
        assert_eq!(r.node, above, "higher z-index выигрывает в overlap-е");
    }

    #[test]
    fn z_order_ignores_dom_order_when_overlapping() {
        // .first в DOM раньше, но с большим z-index — должен выиграть.
        let html = r#"<div class="first"></div><div class="second"></div>"#;
        let css = "
            div { position: relative; width: 200px; height: 100px; }
            .first { z-index: 5; }
            .second { z-index: 1; margin-top: -50px; }
        ";
        let (doc, root) = build(html, css);
        let first = by_class(&doc, "first");
        let r = hit_test(Point::new(10.0, 60.0), &root).expect("hit");
        assert_eq!(r.node, first, "z-index важнее DOM-order при overlap-е");
    }

    #[test]
    fn negative_z_below_in_flow() {
        // .neg c z=-1, .normal — обычный in-flow. .normal перекрывает .neg
        // на 50px → в overlap-е .normal (фаза 5) выигрывает над .neg
        // (фаза 2, negative-z SC).
        let html = r#"<div class="neg"></div><div class="normal"></div>"#;
        let css = "
            .neg { position: relative; z-index: -1; width: 200px; height: 100px; }
            .normal { width: 200px; height: 100px; margin-top: -50px; }
        ";
        let (doc, root) = build(html, css);
        let normal = by_class(&doc, "normal");
        let r = hit_test(Point::new(10.0, 75.0), &root).expect("hit in overlap");
        assert_eq!(r.node, normal, "in-flow рисуется поверх negative-z SC");
    }

    #[test]
    fn transform_translate_moves_hit_zone() {
        // div с translate(150, 0): рисуется на 150 правее. Hit в исходный
        // rect-position (10, 50) должен промахнуться мимо div; hit в
        // (200, 50) — попасть.
        let (doc, root) = build(
            r#"<div class="moved"></div>"#,
            ".moved { width: 100px; height: 100px; transform: translate(150px, 0); }",
        );
        let div = by_class(&doc, "moved");

        // Внутри сдвинутого положения.
        let inside = hit_test(Point::new(200.0, 50.0), &root)
            .expect("hit in translated rect");
        assert_eq!(inside.node, div);

        // На исходном положении бокса (без transform-а он был бы в 0..100).
        // С transform — пусто; hit должен либо промахнуться, либо попасть
        // в предок (root / html), но НЕ в .moved.
        let outside = hit_test(Point::new(10.0, 50.0), &root);
        if let Some(out) = outside {
            assert_ne!(out.node, div, "translated box не должен hit-тестся в исходной позиции");
        }
    }

    #[test]
    fn transform_scale_zero_makes_box_unhittable() {
        let (doc, root) = build(
            r#"<div class="zero"></div>"#,
            ".zero { width: 100px; height: 100px; transform: scale(0); }",
        );
        let div = by_class(&doc, "zero");
        // Любая точка не должна попасть в .zero (forward matrix сингулярна).
        let r = hit_test(Point::new(10.0, 10.0), &root);
        if let Some(res) = r {
            assert_ne!(res.node, div, "scale(0) box визуально не существует");
            assert!(!res.path.contains(&div), "и не появляется в path");
        }
    }

    #[test]
    fn display_none_skips_subtree() {
        let (doc, root) = build(
            r#"<div class="hidden"><span class="child">x</span></div>"#,
            ".hidden { display: none; }",
        );
        let hidden = by_class(&doc, "hidden");
        let child = by_class(&doc, "child");
        let r = hit_test(Point::new(10.0, 10.0), &root);
        if let Some(res) = r {
            assert_ne!(res.node, hidden);
            assert_ne!(res.node, child);
            assert!(!res.path.contains(&hidden));
            assert!(!res.path.contains(&child));
        }
    }

    #[test]
    fn path_starts_with_hit_node_and_ends_with_root() {
        let (_, root) = build("<div>content</div>", "div { height: 50px; }");
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(r.path[0], r.node);
        assert_eq!(r.path.last(), Some(&root.node));
    }

    #[test]
    fn local_point_equals_viewport_when_no_transform() {
        let (_, root) = build("<div>x</div>", "div { height: 50px; }");
        let p = Point::new(12.5, 17.5);
        let r = hit_test(p, &root).expect("hit");
        assert!((r.local_point.x - 12.5).abs() < 1e-5);
        assert!((r.local_point.y - 17.5).abs() < 1e-5);
    }

    #[test]
    fn local_point_after_translate_inverse() {
        // div с translate(50, 30) — hit в (100, 60) в viewport.
        // В локальной системе бокса (после инверсии) точка должна быть
        // (50, 30).
        let (doc, root) = build(
            r#"<div class="t"></div>"#,
            ".t { width: 200px; height: 200px; transform: translate(50px, 30px); }",
        );
        let div = by_class(&doc, "t");
        let r = hit_test(Point::new(100.0, 60.0), &root).expect("hit");
        assert_eq!(r.node, div);
        assert!((r.local_point.x - 50.0).abs() < 1e-3);
        assert!((r.local_point.y - 30.0).abs() < 1e-3);
    }

    #[test]
    fn local_point_after_percent_translate_and_translate_property() {
        // BUG-1292: `translateX(50%)` и свойство `translate: 50% 0` сдвигают
        // бокс 200×100 на 100 px; hit-тест видит тот же сдвиг, что и отрисовка.
        for decl in ["transform: translateX(50%)", "translate: 50% 0"] {
            let (doc, root) = build(
                r#"<div class="t"></div>"#,
                &format!(".t {{ width: 200px; height: 100px; {decl}; }}"),
            );
            let div = by_class(&doc, "t");
            let r = hit_test(Point::new(250.0, 50.0), &root).expect("hit");
            assert_eq!(r.node, div, "{decl}");
            assert!((r.local_point.x - 150.0).abs() < 1e-3, "{decl}: {:?}", r.local_point);
        }
    }

    #[test]
    fn nested_transforms_compose() {
        // Outer translate(100, 0) + inner translate(50, 0): итог — inner в
        // viewport (150, 0). Точка (175, 50) должна попасть в inner.
        let (doc, root) = build(
            r#"<div class="outer"><div class="inner"></div></div>"#,
            "
                .outer { width: 400px; height: 200px; transform: translate(100px, 0); }
                .inner { width: 100px; height: 100px; transform: translate(50px, 0); }
            ",
        );
        let inner = by_class(&doc, "inner");
        let r = hit_test(Point::new(175.0, 50.0), &root).expect("hit");
        assert_eq!(r.node, inner);
    }

    #[test]
    fn user_select_none_propagated_in_hit_result() {
        let (doc, root) = build(
            "<div>text</div>",
            "div { height: 50px; user-select: none; }",
        );
        let div = by_tag(&doc, "div");
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(r.node, div);
        assert_eq!(r.user_select, UserSelect::None, "user-select:none пропагируется в HitTestResult");
    }

    #[test]
    fn user_select_auto_default_in_hit_result() {
        let (_, root) = build("<div>text</div>", "div { height: 50px; }");
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(r.user_select, UserSelect::Auto, "user-select:auto по умолчанию");
    }

    #[test]
    fn user_select_all_propagated_in_hit_result() {
        let (_, root) = build(
            "<div>text</div>",
            "div { height: 50px; user-select: all; }",
        );
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(r.user_select, UserSelect::All);
    }

    #[test]
    fn hit_test_all_includes_ancestors_topmost_first() {
        // <div class="outer"><div class="inner">x</div></div>: точка внутри
        // .inner должна вернуть и .inner (топ), и .outer, и root — в этом
        // порядке (потомок раньше предка).
        let (doc, root) = build(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { height: 100px; } .inner { height: 50px; }",
        );
        let outer = by_class(&doc, "outer");
        let inner = by_class(&doc, "inner");
        let hits = hit_test_all(Point::new(10.0, 10.0), &root);
        let nodes: Vec<_> = hits.iter().map(|h| h.node).collect();
        let inner_pos = nodes.iter().position(|&n| n == inner).expect(".inner hit");
        let outer_pos = nodes.iter().position(|&n| n == outer).expect(".outer hit");
        assert!(inner_pos < outer_pos, ".inner (topmost) должен идти раньше .outer");
        assert_eq!(nodes.last(), Some(&root.node), "root — последний (нижний слой)");
    }

    #[test]
    fn hit_test_all_includes_both_overlapping_siblings() {
        // .below и .above перекрываются; hit_test (одиночный) вернул бы только
        // .above (выше по z-index), но elementsFromPoint обязан вернуть ОБА —
        // это как раз то, чего единичный hit_test/path не даёт.
        let html = r#"<div class="below"></div><div class="above"></div>"#;
        let css = "
            div { position: relative; width: 200px; height: 100px; }
            .below { z-index: 1; }
            .above { z-index: 2; margin-top: -50px; }
        ";
        let (doc, root) = build(html, css);
        let below = by_class(&doc, "below");
        let above = by_class(&doc, "above");
        let hits = hit_test_all(Point::new(10.0, 60.0), &root);
        let nodes: Vec<_> = hits.iter().map(|h| h.node).collect();
        let above_pos = nodes.iter().position(|&n| n == above).expect(".above hit");
        let below_pos = nodes.iter().position(|&n| n == below).expect(".below hit — sibling, not just ancestor");
        assert!(above_pos < below_pos, "выше по z-index — раньше в списке");
    }

    #[test]
    fn grid_item_with_higher_order_is_topmost_in_hit_test() {
        // BUG-1312: два grid-item в одной ячейке; `order:1` рисуется поверх `order:0`,
        // хотя стоит раньше в DOM — значит, и клик достаётся ему.
        let html = r#"<div class="g"><div class="hi"></div><div class="lo"></div></div>"#;
        let css = "
            .g { display: grid; width: 200px; height: 100px; }
            .g > div { grid-area: 1 / 1; }
            .hi { order: 1; }
        ";
        let (doc, root) = build(html, css);
        let r = hit_test(Point::new(10.0, 10.0), &root).expect("hit");
        assert_eq!(r.node, by_class(&doc, "hi"));
        let nodes: Vec<_> = hit_test_all(Point::new(10.0, 10.0), &root).iter().map(|h| h.node).collect();
        let pos = |n| nodes.iter().position(|&x| x == n).expect("hit");
        assert!(pos(by_class(&doc, "hi")) < pos(by_class(&doc, "lo")));
    }

    #[test]
    fn hit_test_all_empty_outside_viewport() {
        let (_, root) = build("<p>x</p>", "");
        assert!(hit_test_all(Point::new(5000.0, 5000.0), &root).is_empty());
    }

    #[test]
    fn hit_test_all_skips_pointer_events_none_box_itself() {
        let (doc, root) = build(
            r#"<div class="outer"><div class="inner">hi</div></div>"#,
            ".outer { pointer-events: none; height: 50px; }",
        );
        let outer = by_class(&doc, "outer");
        let hits = hit_test_all(Point::new(10.0, 10.0), &root);
        assert!(
            !hits.iter().any(|h| h.node == outer),
            "pointer-events:none box никогда не должен попасть в elementsFromPoint"
        );
    }

    /// BUG-1412: `clip-path` режет попадание — точка вне формы уходит в предка.
    #[test]
    fn clip_path_shape_limits_hit_zone() {
        let (doc, root) = build(
            r#"<div class="a"></div><div class="b"></div>"#,
            ".a { width: 200px; height: 100px; clip-path: inset(0 100px 0 0); }             .b { width: 200px; height: 200px; clip-path: circle(50px at 100px 100px); }",
        );
        let (a, b) = (by_class(&doc, "a"), by_class(&doc, "b"));
        let node_at = |x: f32, y: f32| hit_test(Point::new(x, y), &root).map(|r| r.node);
        assert_eq!(node_at(50.0, 50.0), Some(a), "внутри inset");
        assert_ne!(node_at(150.0, 50.0), Some(a), "вне inset");
        assert_eq!(node_at(100.0, 200.0), Some(b), "центр круга");
        assert_ne!(node_at(10.0, 110.0), Some(b), "угол вне круга");
        let all: Vec<_> = hit_test_all(Point::new(150.0, 50.0), &root).iter().map(|h| h.node).collect();
        assert!(!all.contains(&a), "elementsFromPoint тоже не видит обрезанное");
    }

    /// `clip-path` на предке отсекает и потомков, лежащих вне формы.
    #[test]
    fn clip_path_clips_descendant_hits() {
        let (doc, root) = build(
            r#"<div class="o"><div class="i"></div></div>"#,
            ".o { width: 200px; height: 100px; clip-path: inset(0 100px 0 0); }             .i { width: 200px; height: 100px; }",
        );
        let i = by_class(&doc, "i");
        assert!(hit_test(Point::new(150.0, 50.0), &root).is_none_or(|r| r.node != i));
        assert_eq!(hit_test(Point::new(50.0, 50.0), &root).map(|r| r.node), Some(i));
    }

    // ──────────── UX-CURSOR-AUTO: `cursor: auto` по контексту ────────────

    /// Текстовые метрики для cursor-тестов: без measurer-а фрагменты текста
    /// получают нулевую ширину, и строгое попадание «в текст» не проверить.
    struct HalfEmMeasurer;

    impl lumen_layout::TextMeasurer for HalfEmMeasurer {
        fn char_width(&self, _: char, size: f32) -> f32 {
            size * 0.5
        }
        fn line_gap_px(&self, size: f32) -> f32 {
            size * 0.2
        }
    }

    fn build_measured(html: &str, css: &str) -> (Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("body,p{{margin:0}}{css}"));
        let root = lumen_layout::layout_measured(
            &doc,
            &sheet,
            Size::new(800.0, 600.0),
            &HalfEmMeasurer,
        );
        (doc, root)
    }

    #[test]
    fn auto_cursor_over_link_text_is_pointer() {
        // UA `:any-link { cursor: pointer }`: ссылка без авторских стилей.
        let (_, root) = build_measured(r#"<p><a href="/x">link</a></p>"#, "");
        let r = hit_test(Point::new(5.0, 5.0), &root).expect("hit на текст ссылки");
        assert_eq!(r.cursor, Cursor::Pointer);
    }

    #[test]
    fn auto_cursor_over_paragraph_text_is_text() {
        let (_, root) = build_measured("<p>hello</p>", "");
        let r = hit_test(Point::new(5.0, 5.0), &root).expect("hit на текст абзаца");
        assert_eq!(r.cursor, Cursor::Text);
    }

    #[test]
    fn auto_cursor_over_empty_background_is_default() {
        let (_, root) = build_measured("<p>x</p>", "p { height: 50px; }");
        let r = hit_test(Point::new(5.0, 40.0), &root).expect("hit на пустой фон");
        assert_eq!(r.cursor, Cursor::Default);
    }

    #[test]
    fn author_cursor_default_beats_ua_link_hint() {
        let (_, root) =
            build_measured(r#"<p><a href="/x">link</a></p>"#, "a { cursor: default; }");
        let r = hit_test(Point::new(5.0, 5.0), &root).expect("hit");
        assert_eq!(r.cursor, Cursor::Default);
    }

    #[test]
    fn auto_cursor_over_text_input_is_text() {
        let (_, root) = build_measured(r#"<input type="text">"#, "");
        let r = hit_test(Point::new(5.0, 5.0), &root).expect("hit на input");
        assert_eq!(r.cursor, Cursor::Text);
    }

    #[test]
    fn auto_cursor_over_checkbox_is_default() {
        let (_, root) = build_measured(r#"<input type="checkbox">"#, "");
        let r = hit_test(Point::new(3.0, 3.0), &root).expect("hit на checkbox");
        assert_eq!(r.cursor, Cursor::Default);
    }
}
