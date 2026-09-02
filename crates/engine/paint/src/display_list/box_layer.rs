//! P1/SPLIT-DL13: box-layer-ops/bucket-fill — `struct BoxLayerOps` … до
//! `fn clip_pop_for` (конец региона перед `inline_frag.rs`). Вынесено из
//! `display_list.rs` (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL,
//! батч DL-13).

use super::*;

/// Layer-ops одного бокса, разделённые на эффекты и overflow-клип.
///
/// Per CSS Overflow L3 §3.2 overflow-клип обрезает только **детей** до
/// padding-box; собственные background/border бокса не клиппятся (BUG-123 —
/// рамка scroll-контейнера целиком срезалась scissor-ом своего же
/// PushScrollLayer). Caller эмитит `pre` → bg/border → `overflow_pre` →
/// дети → `overflow_post` → `post` — зеркало порядка не-композиторного
/// `walk` (bg/border до PushScrollLayer).
struct BoxLayerOps {
    /// Эффекты, оборачивающие весь painted output бокса (включая его
    /// собственные background/border): clip-path, blend, opacity,
    /// transform, backdrop-filter, filter.
    pre: Vec<DisplayCommand>,
    /// Парные Pop к `pre`, уже в обратном (LIFO) порядке.
    post: Vec<DisplayCommand>,
    /// Overflow-клип / scroll-слой — оборачивает только детей.
    overflow_pre: Vec<DisplayCommand>,
    /// Парные Pop к `overflow_pre`.
    overflow_post: Vec<DisplayCommand>,
}

/// Собирает layer-effect триггеры одного box-а в [`BoxLayerOps`].
/// Push-команды складываются в `pre` в порядке, парные `Pop` в `post` —
/// в обратном порядке (LIFO). Возвращает пустые векторы для боксов без
/// триггеров **или для анонимных боксов** (InlineRun / Skip), у которых
/// нет своего DOM-элемента, к которому компилятор стиля привязал бы
/// triggering свойство.
///
/// Симметрия с `box_can_own_stacking_context` / `box_can_own_property_node`:
/// анонимные InlineRun-ы клонируют style родителя (включая opacity и
/// overflow), и эмиссия layer-ops для них дала бы фантомные парные
/// Push/Pop поверх настоящих от parent-Block-а. Та же защита здесь.
///
/// Триггеры:
/// - `opacity < 1.0` → `PushOpacity { alpha } / PopOpacity`.
/// - `mix-blend-mode != Normal` → `PushBlendMode { mode } / PopBlendMode`.
/// - `overflow-x / overflow-y` ∈ {hidden, clip, scroll, auto} →
///   `PushClipRect { rect: b.rect } / PopClip`.
/// - `transform != []` → `PushTransform { matrix } / PopTransform`.
///   Matrix считается через `forward_box_transform`: T(pivot)·M·T(-pivot)
///   в viewport-координатах, pivot = b.rect.origin + transform_origin.
///
/// Порядок Push-команд (для child compositor-а смысла не несёт, но
/// детерминирован для тестируемости): Blend → Opacity → Transform →
/// ClipPath → BackdropFilter → Filter. Pop — в обратном порядке. Transform
/// пушится до clip-path: клип задан в локальной системе элемента и
/// переносится его transform-ом (CSS Masking L1 §9, BUG-140), при этом
/// transform преобразует всё содержимое SC (включая собственные
/// background/border бокса, эмитимые в `root_bg`).
fn box_layer_ops(b: &LayoutBox, ov: Option<&CompositorOverride>) -> BoxLayerOps {
    let mut pre = Vec::new();
    let mut post = Vec::new();
    let mut overflow_pre = Vec::new();
    let mut overflow_post = Vec::new();
    if !box_can_own_stacking_context(b) {
        // SVG §7.4: the outermost SVG viewport establishes a clip (UA default
        // `overflow: hidden`). With object-fit: cover (or a viewBox larger than
        // the viewport) the scaled content overflows the SVG box; without this
        // clip it would paint over sibling boxes. SvgRoot is not a stacking-
        // context owner, so emit the viewport clip here. BUG-110.
        if matches!(b.kind, BoxKind::SvgRoot { .. }) {
            let s = &b.style;
            let px = b.rect.x + s.border_left_width;
            let py = b.rect.y + s.border_top_width;
            let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
            let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
            overflow_pre.push(DisplayCommand::PushClipRect { rect: Rect::new(px, py, pw, ph) });
            overflow_post.push(DisplayCommand::PopClip);
        }
        return BoxLayerOps { pre, post, overflow_pre, overflow_post };
    }
    let s = &b.style;

    // CSS Masking L1 §4 (BUG-183): mask-image wraps the fully composited element
    // (background + border + content + children), so it must be the OUTERMOST
    // layer. Pushed first into `pre` and `post` here — after `post.reverse()` its
    // PopMask becomes the last command, balancing the PushMask. `walk` emits the
    // same pair inline via `emit_push_mask`; the SC bucket path lost it before
    // (mask-image makes the box a stacking context → painted via `fill_buckets`/
    // `emit_box_self`, which never opened the mask group).
    // Слоёв может быть несколько (`mask-composite: intersect` — вложенные
    // группы, см. `rendered_mask_layers`), поэтому закрываем ровно столько
    // `PopMask`, сколько групп открылось.
    let mask_groups = emit_push_mask(&mut pre, b);
    if mask_groups > 0 {
        for _ in 0..mask_groups {
            post.push(DisplayCommand::PopMask);
        }
        // CSS Masking L1 §4.6 — `mask-clip` restricts the masked painting to the
        // padding/content box. Pushed inside the mask group (after PushMask); the
        // clip result is identical whether the scissor sits inside or outside the
        // offscreen. `post` is reversed later, so pushing PopClip after PopMask
        // yields `… PopClip PopMask` — PopClip nests inside the mask group.
        if let Some(clip) = mask_clip_paint_rect(b) {
            pre.push(DisplayCommand::PushClipRect { rect: clip });
            post.push(DisplayCommand::PopClip);
        }
    }

    // CSS Overflow L3 §3.2: overflow clip to padding-box edge; unconstrained
    // axis uses a BIG sentinel so the GPU scissor doesn't cut off content in
    // that direction. CSS Containment L3 §3.5: contain:paint clips both axes.
    // CSS: overflow — P4 wires: once overflow:scroll/auto are parsed, the
    // PushScrollLayer branch below automatically picks them up.
    let paint_contain = s.contain.0 & ContainFlags::PAINT.0 != 0;
    let clip_x = overflow_clips(s.overflow_x) || paint_contain;
    let clip_y = overflow_clips(s.overflow_y) || paint_contain;
    if clip_x || clip_y {
        const BIG: f32 = 1_000_000.0;
        let px = b.rect.x + s.border_left_width;
        let py = b.rect.y + s.border_top_width;
        let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
        let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
        let cr = Rect::new(
            if clip_x { px } else { -BIG },
            if clip_y { py } else { -BIG },
            if clip_x { pw } else { 2.0 * BIG },
            if clip_y { ph } else { 2.0 * BIG },
        );
        // scroll/auto → PushScrollLayer (applies clip + scroll translate).
        // hidden/clip/paint-contain → PushClipRect (clip only, no scroll).
        // BUG-132 fix: если есть border-radius, использовать PushClipRoundedRect
        // вместо PushClipRect (scissors) для скруглённого клипа.
        let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
        let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
        if (is_scroll_x || is_scroll_y) && !paint_contain {
            overflow_pre.push(DisplayCommand::PushScrollLayer {
                clip_rect: cr,
                scroll_x: b.scroll_x,
                scroll_y: b.scroll_y,
            });
            overflow_post.push(DisplayCommand::PopScrollLayer);
            // BUG-220: the ordered (stacking-context) path lost the scrollbar —
            // only `walk` emitted DrawScrollbar. Emit it here too, into
            // `overflow_post` after PopScrollLayer (caller flushes overflow_post
            // after children, so the bars render at a fixed position over the
            // scrolled content). Same helper as `walk` for pixel parity.
            emit_scrollbars(b, (px, py, pw, ph), is_scroll_x, is_scroll_y, &mut overflow_post);
        } else {
            // BUG-132: скруглённый клип для border-radius + overflow:hidden
            // Разрешаем border-radius значения используя padding-box width как basis
            // (аналогично CornerRadii::from_style в display_list.rs:188).
            let padding_w = b.rect.width - s.border_left_width - s.border_right_width;

            let resolve_radius = |len: &Length, basis: f32| -> f32 {
                match len {
                    Length::Px(v) => *v,
                    Length::Percent(p) => (p / 100.0) * basis,
                    _ => 0.0,
                }
            };

            let tl = resolve_radius(&s.border_top_left_radius, padding_w);
            let tr = resolve_radius(&s.border_top_right_radius, padding_w);
            let br = resolve_radius(&s.border_bottom_right_radius, padding_w);
            let bl = resolve_radius(&s.border_bottom_left_radius, padding_w);
            let has_border_radius = tl > 0.0 || tr > 0.0 || br > 0.0 || bl > 0.0;

            if has_border_radius {
                // PushClipRoundedRect: скруглённый клип с border-radius
                let radii = [tl, tr, br, bl];
                overflow_pre.push(DisplayCommand::PushClipRoundedRect { rect: cr, radii });
            } else {
                // Стандартный PushClipRect (rect-только)
                overflow_pre.push(DisplayCommand::PushClipRect { rect: cr });
            }
            overflow_post.push(DisplayCommand::PopClip);
        }
    }
    if s.mix_blend_mode != LayoutBlendMode::Normal {
        pre.push(DisplayCommand::PushBlendMode {
            mode: map_blend_mode(s.mix_blend_mode),
            bounds: b.rect,
        });
        post.push(DisplayCommand::PopBlendMode);
    }
    // Opacity: animation override wins over style value. CSS Transforms L2
    // §5.1 — `backface-visibility: hidden` culls the whole box (self +
    // descendants) once its 3D transform has rotated the face away from the
    // viewer; reusing the opacity-0 compositing layer is the SC-bucket path's
    // equivalent of `walk`'s early return, since a box with any 3D rotation
    // already owns a stacking context (`creates_stacking_context`).
    let effective_opacity = if is_backface_hidden(b) {
        0.0
    } else {
        ov.and_then(|o| o.opacity).unwrap_or(s.opacity)
    };
    if effective_opacity < 1.0 {
        pre.push(DisplayCommand::PushOpacity { alpha: effective_opacity, bounds: Some(b.rect) });
        post.push(DisplayCommand::PopOpacity);
    } else if s.isolation == Isolation::Isolate
        && box_can_own_stacking_context(b)
        && s.filter.is_empty()
        && s.backdrop_filter.is_empty()
        && s.mix_blend_mode == LayoutBlendMode::Normal
    {
        // CSS Compositing & Blending L1 §2.1 — `isolation: isolate` turns the
        // element into an isolated group: descendant `mix-blend-mode`s must
        // composite against a transparent backdrop that only contains the
        // group's own content, never the page behind it. When any of
        // opacity/filter/backdrop-filter/mix-blend-mode is present the element
        // already renders through an offscreen group layer (which is isolated),
        // so the dedicated layer is only needed when `isolate` is the sole
        // trigger. Reuse the opacity offscreen layer at full alpha: it clears a
        // transparent backdrop, redirects the subtree into it, then composites
        // the result back unchanged — exactly the isolated-group semantics.
        pre.push(DisplayCommand::PushOpacity { alpha: 1.0, bounds: Some(b.rect) });
        post.push(DisplayCommand::PopOpacity);
    }
    // Transform: animation override wins over style value.
    let transform = if let Some(fns) = ov.and_then(|o| o.transform.as_deref()) {
        let (ox, oy, _) = s.transform_origin;
        transform_fns_to_matrix(fns, b.rect.x + ox.resolve(b.rect.width), b.rect.y + oy.resolve(b.rect.height))
    } else {
        forward_box_transform(b)
    };
    if let Some(matrix) = transform {
        pre.push(DisplayCommand::PushTransform { matrix });
        post.push(DisplayCommand::PopTransform);
    }
    // CSS Masking L1 §9 + BUG-140: clip-path задан в локальной системе
    // элемента и переносится его transform-ом — эмитится ВНУТРИ
    // PushTransform, но снаружи filter/backdrop-filter (клип применяется к
    // отфильтрованному выводу, CSS Filter Effects L1 §4).
    if let Some(clip) = &s.clip_path {
        match clip_path_to_shape(clip, b.rect) {
            Some(shape) => pre.push(DisplayCommand::PushClipPath { shape }),
            None => pre.push(DisplayCommand::PushClipRect {
                rect: clip_path_to_rect(clip, b.rect),
            }),
        }
        post.push(DisplayCommand::PopClip);
    }
    // backdrop-filter: outermost SC — captures parent content, filters it, then
    // composites element on top. Must wrap PushFilter so the element's own `filter`
    // applies to the element content before it's blended over the filtered backdrop.
    if !s.backdrop_filter.is_empty() {
        pre.push(DisplayCommand::PushBackdropFilter {
            filters: s.backdrop_filter.clone(),
            bounds: b.rect,
        });
        post.push(DisplayCommand::PopBackdropFilter);
    }
    if !s.filter.is_empty() {
        pre.push(DisplayCommand::PushFilter {
            filters: s.filter.clone(),
            bounds: Some(b.rect),
        });
        post.push(DisplayCommand::PopFilter);
    }
    // post в LIFO порядке относительно pre.
    post.reverse();
    BoxLayerOps { pre, post, overflow_pre, overflow_post }
}

/// Walk-функция, идентичная по триггерам `StackingTree::build`: pre-order,
/// SC-id присваивается монотонно при обнаружении SC-creating потомка.
/// Boxes без SC-trigger остаются в `current_sc`.
///
/// Layer-ops эмиссия:
/// - Для SC-owner (`is_sc_root == true`) Push идёт в `bucket.pre`, Pop в
///   `bucket.post`.
/// - Для non-SC box-а (typically `overflow: hidden` без других триггеров —
///   opacity/blend сами триггерят SC) Push/Pop эмитятся inline в
///   `bucket.contents` вокруг собственного contents-emit-а и потомков.
///
/// `inherited_clips` (BUG-131): rect-клипы (`PushClipRect` /
/// `PushClipRoundedRect`) от non-SC предков, чьи inline push/pop остались в
/// бакете родительского SC и уже закрылись там. Дочерний stacking context
/// рисуется в более позднем слоте painting order, поэтому эти клипы к моменту
/// его отрисовки уже неактивны — их надо переустановить как внешний слой
/// данного SC (push в начало `pre`, pop после `post`/CloseLayer). Без этого
/// трансформированный ребёнок (собственный SC) сбегает из `overflow:hidden`
/// предка.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_buckets(
    b: &LayoutBox,
    current_sc: StackingContextId,
    next_sc_id: &mut u32,
    buckets: &mut [ScBucket],
    is_sc_root: bool,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
    inherited_clips: &[DisplayCommand],
    split: &mut SplitTracker,
    raw_spans: &mut Vec<RawSpan>,
) {
    let ov = anim.and_then(|a| a.get(b.node));
    let ops = box_layer_ops(b, ov);

    if is_sc_root {
        split.sc_entries += 1;
        if split.enabled && ov.is_some() {
            if current_sc == StackingContextId::ROOT {
                // Override на владельце корневого SC анимирует всю страницу —
                // статики не остаётся, split бессмыслен.
                split.invalid = true;
            } else {
                split.animated_scs.push(current_sc.0);
            }
        }
        let bucket = &mut buckets[current_sc.0 as usize];
        // BUG-131: переустановить клипы non-SC предков как внешний слой SC.
        // Note (ADR-025): re-established clip commands are physically new
        // `DisplayCommand`s, but conceptually belong to whichever ancestor
        // established them first (already spanned there) — attributing this
        // copy to `b` too is a documented over-approximation, not a lie: `b`
        // genuinely re-emits them as its own layer wrapper.
        let pre_start = bucket.pre.len();
        for clip in inherited_clips {
            bucket.pre.push(clip.clone());
        }
        bucket.pre.extend(ops.pre);
        let pre_end = bucket.pre.len();
        record_span(raw_spans, current_sc.0, BucketField::Pre, pre_start, pre_end, b.origin, 0);

        let bg_start = bucket.root_bg.len();
        emit_box_self(b, &mut bucket.root_bg, dpr, None, ov);
        // Overflow-клип — после собственных bg/border (они не клиппятся
        // своим overflow, BUG-123), но до contents с детьми.
        bucket.root_bg.extend(ops.overflow_pre);
        let bg_end = bucket.root_bg.len();
        record_span(raw_spans, current_sc.0, BucketField::RootBg, bg_start, bg_end, b.origin, 0);

        // `post` эмитится в фазе InlineContent после descendants — заполним
        // его сейчас, чтобы не повторно вычислять триггеры.
        let post_start = bucket.post.len();
        bucket.post.extend(ops.overflow_post);
        bucket.post.extend(ops.post);
        // PopClip для переустановленных клипов — в LIFO порядке, после
        // собственных Pop-команд SC (CloseLayer).
        for clip in inherited_clips.iter().rev() {
            bucket.post.push(clip_pop_for(clip));
        }
        let post_end = bucket.post.len();
        record_span(raw_spans, current_sc.0, BucketField::Post, post_start, post_end, b.origin, 0);

        // Этот SC становится новым clip-anchor: его собственный клип +
        // переустановленные inherited-клипы охватывают дочерние SC через
        // root_bg/post (PopClip в CloseLayer после всех детей). Цепочка
        // сбрасывается.
        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true, anim, dpr, &[], split, raw_spans);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false, anim, dpr, &[], split, raw_spans);
            }
        }
        // BUG-200: redraw collapsed cell borders on top of all cell backgrounds —
        // see the non-SC branch below for the full rationale.
        if collapse_border_repass_applies(b) {
            let mut cells: Vec<&LayoutBox> = Vec::new();
            collect_table_cells(b, &mut cells);
            let bucket = &mut buckets[current_sc.0 as usize];
            for cell in &cells {
                let start = bucket.post.len();
                emit_table_cell_border(cell, &mut bucket.post);
                let end = bucket.post.len();
                record_span(raw_spans, current_sc.0, BucketField::Post, start, end, cell.origin, 0);
            }
        }
    } else {
        // Non-SC box: inline Push/Pop в contents текущего SC. Это нужно для
        // `overflow:hidden` на обычном in-flow box-е (opacity/blend
        // триггерят SC сами, до сюда не дойдут с не-пустым pre).
        // Static/animated split: non-SC узел с override эмитит всё поддерево
        // (layer-ops + self + потомки) подряд в contents текущего SC —
        // запоминаем спан. SC-потомок внутри спана уводит свои команды в
        // собственный бакет (другая позиция painting order) — спан рвётся,
        // split этого кадра инвалидируется через счётчик sc_entries.
        let split_span_start = (split.enabled && ov.is_some())
            .then(|| (buckets[current_sc.0 as usize].contents.len(), split.sc_entries));
        let bucket = &mut buckets[current_sc.0 as usize];
        let lead_start = bucket.contents.len();
        bucket.contents.extend(ops.pre);
        emit_box_self(b, &mut bucket.contents, dpr, None, ov);
        // Overflow-клип после собственных bg/border (BUG-123).
        bucket.contents.extend(ops.overflow_pre.iter().cloned());
        let lead_end = bucket.contents.len();
        record_span(raw_spans, current_sc.0, BucketField::Contents, lead_start, lead_end, b.origin, 0);

        // BUG-131: собственный rect-клип этого non-SC box-а добавляется к
        // цепочке для дочерних SC (его inline push/pop их не охватывает).
        // BUG-159: scroll-слой такого non-SC box-а наследуем ТОЖЕ. Плоский
        // `overflow:auto`/`scroll` контейнер, не являющийся SC-owner, эмитит
        // `PushScrollLayer`/`PopScrollLayer` inline в `contents` текущего SC;
        // их Pop закрывается ДО того, как дочерний stacking context рисуется
        // (более поздний слот painting order), поэтому потомок сбегал бы и из
        // scroll-клипа, и из scroll-translate — вёл бы себя как `position:fixed`
        // (не скроллился при прокрутке). Переустанавливаем scroll-слой как
        // внешний слой каждого дочернего SC, зеркалом clip-наследования. Если
        // же scroll-контейнер сам owns stacking context — он оборачивает
        // потомков через root_bg/post (PushScrollLayer в RootBackground,
        // PopScrollLayer в CloseLayer после всех детей), и сюда не попадает.
        let mut child_clips: Vec<DisplayCommand> = inherited_clips.to_vec();
        for cmd in &ops.overflow_pre {
            if matches!(
                cmd,
                DisplayCommand::PushClipRect { .. }
                    | DisplayCommand::PushClipRoundedRect { .. }
                    | DisplayCommand::PushScrollLayer { .. }
            ) {
                child_clips.push(cmd.clone());
            }
        }

        for child in &b.children {
            let child_creates_sc =
                box_can_own_stacking_context(child) && creates_stacking_context(&child.style);
            if child_creates_sc {
                // BUG-159: `position:fixed` привязан к viewport, `sticky` имеет
                // собственную scroll-aware машинерию — ни тот, ни другой не
                // должны наследовать scroll-translate предка, иначе fixed-оверлей
                // уезжал бы вместе со страницей. Rect-клипы они по-прежнему
                // наследуют (поведение BUG-131 без изменений).
                let child_layers: Vec<DisplayCommand> =
                    if matches!(child.style.position, Position::Fixed | Position::Sticky) {
                        child_clips
                            .iter()
                            .filter(|c| !matches!(c, DisplayCommand::PushScrollLayer { .. }))
                            .cloned()
                            .collect()
                    } else {
                        child_clips.clone()
                    };
                let id = StackingContextId(*next_sc_id);
                *next_sc_id += 1;
                fill_buckets(child, id, next_sc_id, buckets, true, anim, dpr, &child_layers, split, raw_spans);
            } else {
                fill_buckets(child, current_sc, next_sc_id, buckets, false, anim, dpr, &child_clips, split, raw_spans);
            }
        }

        let bucket = &mut buckets[current_sc.0 as usize];
        // BUG-200: under `border-collapse: collapse` adjacent cells overlap by the
        // shared grid-line width (layout pulls them together). Cells are emitted in
        // DOM order, each filling its background then drawing its border. When a later
        // cell has a thinner border than its earlier neighbour (e.g. a 1px `thin` cell
        // after a 3px `thick` one), the later cell's background overpaints the part of
        // the neighbour's collapsed border in the overlap region, leaving only the
        // thinner cell's 1px line instead of the spec's max width (CSS 2.1 §17.6.2).
        // Redraw every cell border once more, on top of all cell backgrounds, so the
        // shared edges composite to the wider border. Borders sit inside the cells'
        // padding, away from content, so the repass is visually a no-op except on the
        // shared grid lines.
        if collapse_border_repass_applies(b) {
            let mut cells: Vec<&LayoutBox> = Vec::new();
            collect_table_cells(b, &mut cells);
            for cell in &cells {
                let start = bucket.contents.len();
                emit_table_cell_border(cell, &mut bucket.contents);
                let end = bucket.contents.len();
                record_span(raw_spans, current_sc.0, BucketField::Contents, start, end, cell.origin, 0);
            }
        }
        let trail_start = bucket.contents.len();
        bucket.contents.extend(ops.overflow_post);
        bucket.contents.extend(ops.post);
        let trail_end = bucket.contents.len();
        record_span(raw_spans, current_sc.0, BucketField::Contents, trail_start, trail_end, b.origin, 0);
        if let Some((start, sc_before)) = split_span_start {
            if split.sc_entries != sc_before {
                split.invalid = true;
            } else {
                let end = buckets[current_sc.0 as usize].contents.len();
                split.content_spans.push((current_sc.0, start, end));
            }
        }
    }
}

/// True when the box is a table using the collapsing-borders model and therefore
/// needs the BUG-200 cell-border repass (cells overlap on shared grid lines).
fn collapse_border_repass_applies(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Table)
        && matches!(b.style.border_collapse, BorderCollapse::Collapse)
}

/// Парный `Pop` для переустановленного push-клипа (BUG-131 clip inheritance).
/// `inherited_clips` содержит только `PushClipRect` / `PushClipRoundedRect`
/// (scroll-слои отфильтрованы), поэтому всегда `PopClip`; match оставлен общим
/// на случай расширения набора наследуемых клипов.
fn clip_pop_for(push: &DisplayCommand) -> DisplayCommand {
    match push {
        DisplayCommand::PushScrollLayer { .. } => DisplayCommand::PopScrollLayer,
        _ => DisplayCommand::PopClip,
    }
}

