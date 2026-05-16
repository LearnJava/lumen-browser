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
//! сам бокс. `pointer-events: none` пропускает бокс (но дети остаются
//! hit-testable, как и в Chrome). `display: none` и `Skip`-боксы исключены
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

use lumen_core::geom::{Point, Rect};
use lumen_dom::NodeId;
use lumen_layout::{
    box_can_own_stacking_context, creates_stacking_context, BoxKind, Display, LayoutBox, Mat4,
    PointerEvents, TransformFn,
};

/// Результат hit-теста.
#[derive(Debug, Clone)]
pub struct HitTestResult {
    /// DOM-узел верхнего слоя, попавшего под курсор. Для InlineRun — это
    /// DOM-предок (тот же, кому принадлежит inline-контент).
    pub node: NodeId,
    /// Координаты попадания в системе hit-узла после всех transform-инверсий
    /// по цепочке предков (та же система, в которой `b.rect` валиден).
    pub local_point: Point,
    /// Ancestor chain снизу-вверх: `path[0] == node`, `path.last()` — корень
    /// layout-дерева (документа). Используется event dispatch-ом для bubble
    /// stage без повторного walk-а.
    pub path: Vec<NodeId>,
}

/// Hit-тест точки в viewport-координатах. `root` — layout-дерево из
/// `lumen_layout::layout` или `layout_measured`.
///
/// Возвращает `None`, если точка не попала ни в один бокс (например, упала
/// за пределы viewport-а или в полностью прозрачное место без боксов).
pub fn hit_test(point: Point, root: &LayoutBox) -> Option<HitTestResult> {
    hit_test_box(point, root)
}

fn hit_test_box(point: Point, b: &LayoutBox) -> Option<HitTestResult> {
    if matches!(b.kind, BoxKind::Skip) || b.style.display == Display::None {
        return None;
    }

    // Inverse-transform точки для детей. Если у этого бокса нет transform-а
    // (или матрица сингулярная — clamp к None, hit тогда проваливается),
    // дети видят оригинальную точку.
    let child_point = match invert_box_transform(b) {
        Some(inv) => {
            let (x, y) = inv.transform_point_2d(point.x, point.y);
            Point::new(x, y)
        }
        None if !b.style.transform.is_empty() => {
            // Сингулярный transform (например, scale(0)). По CSS такой бокс
            // не отрисовывается; hit-тест не попадает ни в бокс, ни в его
            // потомков.
            return None;
        }
        None => point,
    };

    // Группируем детей: positive-z SC, negative-z SC, остальные (in-flow +
    // auto/0-z SC). Не-SC дети идут в in-flow вместе с auto/0-z SC: в paint
    // order они на одной «полке» (фаза 6 родительского SC), различение
    // нужно было бы только для опт-цели «не лезть в children без hit-rect».
    let mut positive: Vec<(&LayoutBox, i32)> = Vec::new();
    let mut negative: Vec<(&LayoutBox, i32)> = Vec::new();
    let mut in_flow: Vec<&LayoutBox> = Vec::new();
    for child in &b.children {
        let creates_sc = box_can_own_stacking_context(child)
            && creates_stacking_context(&child.style);
        match (creates_sc, child.style.z_index) {
            (true, Some(z)) if z > 0 => positive.push((child, z)),
            (true, Some(z)) if z < 0 => negative.push((child, z)),
            _ => in_flow.push(child),
        }
    }
    // descending по z: больший z = ближе к зрителю = первый кандидат.
    positive.sort_by_key(|c| std::cmp::Reverse(c.1));
    // descending: -1 ближе к зрителю чем -10; среди negative обход тоже
    // от самого «верхнего» (наименьший по модулю) к самому «нижнему».
    negative.sort_by_key(|c| std::cmp::Reverse(c.1));

    // 1. positive-z children (фаза 7) — последние в paint-order.
    for (child, _) in &positive {
        if let Some(mut hit) = hit_test_box(child_point, child) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }
    // 2. in-flow + auto/0-z children в reverse DOM (фазы 3-6).
    for child in in_flow.iter().rev() {
        if let Some(mut hit) = hit_test_box(child_point, child) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }
    // 3. negative-z children (фаза 2).
    for (child, _) in &negative {
        if let Some(mut hit) = hit_test_box(child_point, child) {
            hit.path.push(b.node);
            return Some(hit);
        }
    }

    // Сам бокс. Проверка bbox + pointer-events.
    if !rect_contains(b.rect, child_point) {
        return None;
    }
    if matches!(b.style.pointer_events, PointerEvents::None) {
        return None;
    }
    Some(HitTestResult {
        node: b.node,
        local_point: child_point,
        path: vec![b.node],
    })
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
    if b.style.transform.is_empty() {
        return None;
    }
    let (ox, oy, _) = b.style.transform_origin;
    let pivot_x = b.rect.x + ox;
    let pivot_y = b.rect.y + oy;
    let mut m = Mat4::IDENTITY;
    for f in &b.style.transform {
        let step = transform_fn_to_mat4(f);
        m = m.multiply(&step);
    }
    let forward = if pivot_x == 0.0 && pivot_y == 0.0 {
        m
    } else {
        Mat4::translation_2d(pivot_x, pivot_y)
            .multiply(&m)
            .multiply(&Mat4::translation_2d(-pivot_x, -pivot_y))
    };
    forward.invert_2d_affine()
}

fn transform_fn_to_mat4(f: &TransformFn) -> Mat4 {
    match *f {
        TransformFn::Translate(x, y) => Mat4::translation_2d(x, y),
        TransformFn::TranslateX(x) => Mat4::translation_2d(x, 0.0),
        TransformFn::TranslateY(y) => Mat4::translation_2d(0.0, y),
        TransformFn::Rotate(theta) => Mat4::rotate_2d(theta),
        TransformFn::Scale(sx, sy) => Mat4::scale_2d(sx, sy),
        TransformFn::ScaleX(sx) => Mat4::scale_2d(sx, 1.0),
        TransformFn::ScaleY(sy) => Mat4::scale_2d(1.0, sy),
        TransformFn::SkewX(a) => Mat4::skew_x(a),
        TransformFn::SkewY(a) => Mat4::skew_y(a),
        TransformFn::Matrix([a, b, c, d, e, f]) => Mat4::from_2d_affine(a, b, c, d, e, f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;
    use lumen_dom::{Document, NodeData};
    use lumen_layout::layout;

    fn build(html: &str, css: &str) -> (Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
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
}
