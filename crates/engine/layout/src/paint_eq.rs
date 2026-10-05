//! Равенство paint-входов двух поддеревьев боксов (PERF-16, срез 1).
//!
//! Кэш display list по поддереву (срез 2) может пропустить emit только если доказал, что
//! эмиссия вернёт те же команды. Display list на ordered-пути — функция одних лишь полей
//! `LayoutBox` поддерева (а также `dpr`, compositor-override и вьюпорта fixed-фонов — они в
//! ключе кэша, а не здесь). [`subtree_paint_eq`] сверяет эти поля.
//!
//! **Равенство, а не хеш.** `InlineFrag::style` — `Arc<ComputedStyle>` (с 3-го среза; `PartialEq for InlineFrag` сверяет указатель первым), а у `ComputedStyle`
//! (302 поля) есть только `PartialEq`: структурного `Hash` нет, и хеш по `Debug` стоил бы
//! десятков микросекунд на фрагмент. Сравнение с моментальным снимком использует
//! выведенный `PartialEq`, который покрывает каждое поле стиля автоматически: поле, добавленное
//! завтра, попадает в сравнение без правки этого файла.
//!
//! **Консервативность.** Любое расхождение — «не равны», промах кэша (лишний emit), а не устаревший
//! кадр. Поэтому стили сравниваются целиком, включая поля, которых emit не читает (например, used-значения,
//! записанные layout в `style`).
//!
//! **Что сознательно не сравнивается** — поля, которых emit ordered-пути не читает:
//! `dirty` (служебные биты инкрементального layout) и у `InlineRun` — `segments` (до-layout),
//! `first_line_style`, `row_continuation_width`, `first_line_inset` (входы layout; их результат
//! уже лежит в `lines`). `match` по `BoxKind` без `_` — новый вариант не скомпилируется,
//! пока его не разберут здесь.

use crate::box_tree::{BoxKind, LayoutBox};
use lumen_core::geom::Rect;
use std::sync::Arc;

/// `true`, если эмиссия display list для `a` и `b` (и всех их потомков) даст одинаковые команды.
///
/// Сравнение итеративное — глубина документа не ограничена стеком вызовов (как у `Drop for
/// LayoutBox`). Дерево масок SVG-фигур обходится тем же рабочим списком.
///
/// Предусловие вызова — одинаковые `dpr`, compositor-override и вьюпорт fixed-фонов у обоих emit-ов;
/// их сравнивает владелец кэша.
pub fn subtree_paint_eq(a: &LayoutBox, b: &LayoutBox) -> bool {
    let mut work: Vec<(&LayoutBox, &LayoutBox)> = vec![(a, b)];
    while let Some((x, y)) = work.pop() {
        if !box_self_paint_eq(x, y, &mut work) {
            return false;
        }
    }
    true
}

/// Сравнивает собственные поля пары боксов и кладёт в `work` пары, которые ещё надо сверить:
/// дети и содержимое масок.
fn box_self_paint_eq<'a>(
    a: &'a LayoutBox,
    b: &'a LayoutBox,
    work: &mut Vec<(&'a LayoutBox, &'a LayoutBox)>,
) -> bool {
    // `LayoutBox` разобран поимённо, без `..`: новое поле не скомпилируется, пока его не отнесут
    // к «читает emit» или «не читает». `dirty` и `grid_baselines` (читает только раскладка родителя) —
    // сознательно пропущены.
    let LayoutBox {
        node,
        rect,
        style,
        used_line_height,
        kind,
        children,
        col_span,
        row_span,
        svg_group_transform,
        scroll_x,
        scroll_y,
        dirty: _,
        grid_baselines: _,
        fieldset_legend,
        subgrid_tracks,
        origin,
    } = a;
    node == &b.node
        && origin == &b.origin
        && rect_bits_eq(rect, &b.rect)
        && styles_eq(style, &b.style)
        && used_line_height.to_bits() == b.used_line_height.to_bits()
        && subgrid_tracks == &b.subgrid_tracks
        && fieldset_legend == &b.fieldset_legend
        && col_span == &b.col_span
        && row_span == &b.row_span
        && svg_group_transform == &b.svg_group_transform
        && scroll_x.to_bits() == b.scroll_x.to_bits()
        && scroll_y.to_bits() == b.scroll_y.to_bits()
        && children.len() == b.children.len()
        && kind_paint_eq(kind, &b.kind, work)
        && {
            work.extend(children.iter().zip(b.children.iter()));
            true
        }
}

/// Побитовое сравнение: `-0.0` и `NaN` не прячутся за `==`. Расхождение только даёт лишний emit.
fn rect_bits_eq(a: &Rect, b: &Rect) -> bool {
    a.x.to_bits() == b.x.to_bits()
        && a.y.to_bits() == b.y.to_bits()
        && a.width.to_bits() == b.width.to_bits()
        && a.height.to_bits() == b.height.to_bits()
}

/// Указатель сначала: после инкрементального каскада большинство боксов делят один `Arc` с прошлым деревом.
fn styles_eq(a: &Arc<crate::style::ComputedStyle>, b: &Arc<crate::style::ComputedStyle>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

fn kind_paint_eq<'a>(
    a: &'a BoxKind,
    b: &'a BoxKind,
    work: &mut Vec<(&'a LayoutBox, &'a LayoutBox)>,
) -> bool {
    match a {
        BoxKind::Block => matches!(b, BoxKind::Block),
        BoxKind::InlineBlockRow => matches!(b, BoxKind::InlineBlockRow),
        BoxKind::TableRow => matches!(b, BoxKind::TableRow),
        BoxKind::InlineSpace => matches!(b, BoxKind::InlineSpace),
        BoxKind::Skip => matches!(b, BoxKind::Skip),
        BoxKind::FlowRoot => matches!(b, BoxKind::FlowRoot),
        BoxKind::Contents => matches!(b, BoxKind::Contents),
        BoxKind::Table => matches!(b, BoxKind::Table),
        BoxKind::TableRowGroup => matches!(b, BoxKind::TableRowGroup),
        BoxKind::InlineRun {
            segments: _,
            lines,
            first_line_style: _,
            row_continuation_width: _,
            first_line_inset: _,
        } => matches!(b, BoxKind::InlineRun { lines: other, .. } if lines == other),
        BoxKind::Image { src, alt, is_lazy } => matches!(
            b,
            BoxKind::Image { src: s, alt: al, is_lazy: l } if src == s && alt == al && is_lazy == l
        ),
        BoxKind::Video { src, poster } => {
            matches!(b, BoxKind::Video { src: s, poster: p } if src == s && poster == p)
        }
        BoxKind::Canvas { width, height } => {
            matches!(b, BoxKind::Canvas { width: w, height: h } if width == w && height == h)
        }
        BoxKind::Audio { src, controls } => {
            matches!(b, BoxKind::Audio { src: s, controls: c } if src == s && controls == c)
        }
        BoxKind::Iframe { src, srcdoc } => {
            matches!(b, BoxKind::Iframe { src: s, srcdoc: d } if src == s && srcdoc == d)
        }
        BoxKind::FormControl { kind } => {
            matches!(b, BoxKind::FormControl { kind: k } if kind == k)
        }
        BoxKind::Ruby { shape } => matches!(b, BoxKind::Ruby { shape: s } if shape == s),
        BoxKind::Marker { text, position, list_style_type, image } => matches!(
            b,
            BoxKind::Marker { text: t, position: p, list_style_type: l, image: i }
                if text == t && position == p && list_style_type == l && image == i
        ),
        BoxKind::SvgRoot { view_box, preserve_aspect_ratio } => matches!(
            b,
            BoxKind::SvgRoot { view_box: v, preserve_aspect_ratio: p }
                if view_box == v && preserve_aspect_ratio == p
        ),
        BoxKind::SvgShape { shape, svg_transform, svg_paint_matrix, svg_mask } => {
            let BoxKind::SvgShape {
                shape: s,
                svg_transform: t,
                svg_paint_matrix: m,
                svg_mask: mask,
            } = b
            else {
                return false;
            };
            if shape != s || svg_transform != t || svg_paint_matrix != m {
                return false;
            }
            match (svg_mask, mask) {
                (None, None) => true,
                (Some(x), Some(y)) => {
                    if x.mode != y.mode || x.content.len() != y.content.len() {
                        return false;
                    }
                    work.extend(x.content.iter().zip(y.content.iter()));
                    true
                }
                _ => false,
            }
        }
        BoxKind::SvgText {
            text,
            x,
            y,
            dx,
            dy,
            text_anchor,
            dominant_baseline,
            baseline_shift,
            svg_transform,
        } => matches!(
            b,
            BoxKind::SvgText {
                text: t,
                x: x2,
                y: y2,
                dx: dx2,
                dy: dy2,
                text_anchor: ta,
                dominant_baseline: db,
                baseline_shift: bs,
                svg_transform: st,
            } if text == t
                && x.to_bits() == x2.to_bits()
                && y.to_bits() == y2.to_bits()
                && dx.to_bits() == dx2.to_bits()
                && dy.to_bits() == dy2.to_bits()
                && text_anchor == ta
                && dominant_baseline == db
                && baseline_shift == bs
                && svg_transform == st
        ),
    }
}
