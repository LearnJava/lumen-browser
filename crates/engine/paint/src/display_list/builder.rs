//! P1/SPLIT-DL15: build_display_list*-семейство + `SplitTracker` +
//! `ordered_with_anim_internal` — `fn build_display_list` … до конца
//! `fn ordered_with_anim_internal`. Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-15).

use super::*;

pub fn build_display_list(root: &LayoutBox) -> DisplayList {
    let mut list = Vec::new();
    walk(root, &mut list, 1.0, None);
    list
}

/// Like [`build_display_list`] but applies `::selection` CSS highlight styles
/// to text fragments that fall within `sel`.
///
/// Pass `Some(&SelectionHighlight)` to enable `::selection` rendering — selected
/// text receives a `FillRect` background (from `sel.bg_color`) and optionally an
/// overridden text colour (from `sel.fg_color`). Pass `None` to get the same
/// output as `build_display_list`.
///
/// This function is a pure function per ADR-008 Invariant 3: it depends only on
/// the function parameters and carries no hidden global state.
pub fn build_display_list_with_selection(
    root: &LayoutBox,
    sel: Option<&SelectionHighlight>,
) -> DisplayList {
    let mut list = Vec::new();
    walk(root, &mut list, 1.0, sel);
    list
}

/// Like `build_display_list` but applies compositor animation overrides per node.
///
/// For each node that has an entry in `anim`, opacity and/or transform values
/// from the override replace the style's values in the emitted PushOpacity /
/// PushTransform commands. Layout geometry (rect, padding, children) is unchanged —
/// this avoids a full relayout while still producing correct frames.
///
/// Pass `None` (or an empty frame) to fall back to the same output as
/// `build_display_list`.
pub fn build_display_list_with_anim(
    root: &LayoutBox,
    anim: Option<&CompositorAnimFrame>,
) -> DisplayList {
    let mut list = Vec::new();
    walk_with_anim(root, anim, &mut list, 1.0);
    list
}

/// Билдер display list-а, **уважающий painting order** (CSS 2.1 Appendix E).
///
/// Разница с [`build_display_list`]: для документа с несколькими
/// stacking-контекстами child-SC рисуются в правильных слотах parent SC
/// (negative-z до контента, auto/0 и positive-z после).
///
/// Phase 0 упрощение: фазы `BlockBackgrounds` / `Floats` / `InlineContent`
/// лумпятся в один «контент» bucket per SC, эмитимый при фазе
/// `InlineContent`. Точное разделение по фазам 3/4/5 (block vs float vs
/// inline-level descendant) — отдельная задача после flex / float layout.
///
/// Bucket-per-SC структура:
/// - `pre`: layer-ops, открываемые при входе в SC (PushOpacity / PushBlendMode
///   / PushClipRect) — собственный SC-owner с `opacity<1` / `mix-blend-mode`
///   ≠ normal / `overflow` ≠ visible.
/// - `root_bg`: bg/border SC-owner box-а (фаза 1 «RootBackground»).
/// - `contents`: всё остальное содержимое SC (descendants, исключая собственно
///   SC-creating потомков — те идут в свои buckets).
/// - `post`: парные Pop-команды, в обратном порядке к `pre`.
///
/// **Layer-ops nesting invariant:** `pre` / `post` SC-owner-а охватывают
/// `root_bg + contents` собственного SC **и все child-SC потомков**. Это
/// реализуется через `PaintPhase::CloseLayer`: `post` эмитится в `CloseLayer`,
/// которая добавляется в `paint_sc` последней — уже ПОСЛЕ всех дочерних SC.
/// Таким образом Pop-команды родителя (PopTransform и т.д.) приходят после
/// Push-команд всех детей — nested transforms и opacity корректно компонуются
/// (BUG-139). Старый подход (post в InlineContent) был Phase-0-заглушкой.
pub fn build_display_list_ordered(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
) -> (DisplayList, ProvenanceIndex) {
    build_display_list_ordered_dpr(root, tree, order, 1.0)
}

/// Like [`build_display_list_ordered`] but resolves `image-set()` background
/// variants for the device pixel ratio `dpr` (CSS Images L4 §5). Shell passes
/// the window scale factor; `build_display_list_ordered` defaults to `1.0`.
///
/// The returned [`ProvenanceIndex`] (ADR-025 §3, DEVX-7 п.4) is built by
/// translating the `RawSpan`s `fill_buckets` recorded — local to one
/// `ScBucket` field — into global indices at the exact point each field is
/// flushed into `out` below. This keeps span-tracking decoupled from the
/// four-phase bucket assembly: `fill_buckets` never sees the final list.
pub fn build_display_list_ordered_dpr(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    dpr: f32,
) -> (DisplayList, ProvenanceIndex) {
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    let mut split = SplitTracker::disabled();
    let mut raw_spans: Vec<RawSpan> = Vec::new();
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, None, dpr, &[], &mut split, &mut raw_spans);

    let mut spans_by_field: HashMap<(u32, BucketField), Vec<RawSpan>> = HashMap::new();
    for rs in raw_spans {
        spans_by_field.entry((rs.sc, rs.field)).or_default().push(rs);
    }

    let mut out = Vec::new();
    let mut final_spans: Vec<ProvenanceSpan> = Vec::new();
    let mut flush = |field_vec: &mut Vec<DisplayCommand>,
                      sc: u32,
                      field: BucketField,
                      out: &mut Vec<DisplayCommand>,
                      final_spans: &mut Vec<ProvenanceSpan>| {
        let offset = out.len();
        out.append(field_vec);
        if let Some(list) = spans_by_field.remove(&(sc, field)) {
            for rs in list {
                final_spans.push(ProvenanceSpan {
                    range: (offset + rs.range.start)..(offset + rs.range.end),
                    origin: rs.origin,
                    fragment: rs.fragment,
                    // Filled below by `annotate_clip_depth` once `out` is complete.
                    clip_depth: 0,
                });
            }
        }
    };
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                flush(&mut bucket.pre, sc_id.0, BucketField::Pre, &mut out, &mut final_spans);
                flush(&mut bucket.root_bg, sc_id.0, BucketField::RootBg, &mut out, &mut final_spans);
            }
            PaintPhase::InlineContent => {
                flush(&mut bucket.contents, sc_id.0, BucketField::Contents, &mut out, &mut final_spans);
                // post (PopTransform / PopOpacity / etc.) is now in CloseLayer —
                // emitted AFTER all child SCs so nested transforms compose correctly
                // (BUG-139). Do NOT move post back here.
            }
            // CloseLayer is emitted last in paint_sc, after all child SCs, so the
            // parent's Pop-commands wrap the children's Push-commands correctly.
            PaintPhase::CloseLayer => {
                flush(&mut bucket.post, sc_id.0, BucketField::Post, &mut out, &mut final_spans);
            }
            // Phase 0: BlockBackgrounds / Floats merged into InlineContent;
            // marker-фазы (NegativeZ / PositionedAndZAuto / PositiveZ) в
            // выводе `PaintOrder::from_tree` не появляются — рекурсия
            // энкодирует их позицию через линейный порядок.
            _ => {}
        }
    }
    annotate_clip_depth(&out, &mut final_spans);
    let index = ProvenanceIndex { spans: final_spans };
    #[cfg(debug_assertions)]
    crate::invariants::check(&out, &index, root);
    (out, index)
}

/// Post-processes `spans` in place with `clip_depth` (ADR-025 §3): the number
/// of open rect/rounded-rect/path clips at each span's first command. A
/// single linear scan over the finished list is simpler and cheaper than
/// threading a running counter through `fill_buckets`'s recursion, and gives
/// the same answer since clip nesting is a property of the final painting
/// order, not of the bucket-assembly process that produced it.
fn annotate_clip_depth(out: &[DisplayCommand], spans: &mut [ProvenanceSpan]) {
    let mut depth_at: Vec<u16> = Vec::with_capacity(out.len() + 1);
    let mut depth: i32 = 0;
    for cmd in out {
        depth_at.push(depth.max(0) as u16);
        match cmd {
            DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. } => depth += 1,
            DisplayCommand::PopClip => depth -= 1,
            _ => {}
        }
    }
    depth_at.push(depth.max(0) as u16);
    for s in spans.iter_mut() {
        s.clip_depth = depth_at.get(s.range.start).copied().unwrap_or(0);
    }
}

/// Like [`build_display_list_ordered`] but applies compositor animation overrides per node.
///
/// Opacity and transform values from `anim` replace the style's values in the emitted
/// PushOpacity / PushTransform commands. Stacking context paint ordering is preserved.
/// Pass `None` to get the same output as `build_display_list_ordered`.
pub fn build_display_list_ordered_with_anim(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
) -> DisplayList {
    build_display_list_ordered_with_anim_dpr(root, tree, order, anim, 1.0)
}

/// Like [`build_display_list_ordered_with_anim`] but resolves `image-set()`
/// background variants for the device pixel ratio `dpr` (CSS Images L4 §5).
pub fn build_display_list_ordered_with_anim_dpr(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
) -> DisplayList {
    ordered_with_anim_internal(root, tree, order, anim, dpr, false).0
}

/// Static/animated split (EXPERIMENT.md §2): как
/// [`build_display_list_ordered_with_anim`], но дополнительно возвращает
/// отсортированные непересекающиеся диапазоны команд итогового списка,
/// содержимое которых зависит от anim-override-ов (поддеревья анимируемых
/// узлов целиком: layer-ops + собственные команды + потомки).
///
/// Пустой Vec означает «split в этом кадре неприменим» (нет overrides,
/// override на корне, либо SC-потомок разрывает inline-спан non-SC узла) —
/// список при этом валиден и идентичен обычной anim-сборке.
///
/// Скролл-композитор использует диапазоны, чтобы кэшировать статичную часть
/// страницы в полосе (ключ полосы считается ТОЛЬКО по статике), а анимируемые
/// сегменты рисовать поверх каждым кадром.
pub fn build_display_list_ordered_with_anim_split(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
) -> (DisplayList, Vec<std::ops::Range<usize>>) {
    ordered_with_anim_internal(root, tree, order, anim, 1.0, true)
}

/// Трекер static/animated split — собирается в `fill_buckets`, конвертируется
/// в диапазоны итогового списка при сборке бакетов.
pub(crate) struct SplitTracker {
    /// Собираем ли split-метаданные (false в обычных сборках — нулевая цена).
    pub(crate) enabled: bool,
    /// SC-owner-ы с anim-override: диапазон = RootBackground..CloseLayer их SC.
    pub(crate) animated_scs: Vec<u32>,
    /// Спаны non-SC override-узлов в `contents` их бакета: (sc, start, end).
    pub(crate) content_spans: Vec<(u32, usize, usize)>,
    /// Split невозможен в этом кадре (override на корневом SC, SC-потомок
    /// внутри inline-спана и т.п.).
    pub(crate) invalid: bool,
    /// Счётчик входов в SC-ветку — детектор «SC-потомок сбежал из спана
    /// в собственный бакет» (его команды не были бы покрыты диапазоном).
    pub(crate) sc_entries: u32,
}

impl SplitTracker {
    fn disabled() -> Self {
        Self {
            enabled: false,
            animated_scs: Vec::new(),
            content_spans: Vec::new(),
            invalid: false,
            sc_entries: 0,
        }
    }
}

/// Общее тело ordered-сборки с anim-override-ами. При `track_split` собирает
/// диапазоны анимируемых сегментов (см. [`build_display_list_ordered_with_anim_split`]);
/// иначе возвращает пустой Vec диапазонов и ведёт себя байт-в-байт как раньше.
fn ordered_with_anim_internal(
    root: &LayoutBox,
    tree: &StackingTree,
    order: &PaintOrder,
    anim: Option<&CompositorAnimFrame>,
    dpr: f32,
    track_split: bool,
) -> (DisplayList, Vec<std::ops::Range<usize>>) {
    let n_sc = tree.contexts.len().max(1);
    let mut buckets: Vec<ScBucket> = vec![ScBucket::default(); n_sc];
    let mut next_sc_id: u32 = 1;
    let mut split = SplitTracker::disabled();
    split.enabled = track_split && anim.is_some_and(|a| !a.is_empty());
    // Compositor-animation path does not consume provenance (only the
    // introspection-facing `build_display_list_ordered*` does) — discard.
    fill_buckets(root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, anim, dpr, &[], &mut split, &mut Vec::new());

    let animated_scs: std::collections::HashSet<u32> =
        split.animated_scs.iter().copied().collect();
    let mut spans_by_sc: std::collections::HashMap<u32, Vec<(usize, usize)>> =
        std::collections::HashMap::new();
    for &(sc, s, e) in &split.content_spans {
        spans_by_sc.entry(sc).or_default().push((s, e));
    }

    let mut out = Vec::new();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    // Открытый диапазон анимируемого SC: (sc_id, старт в out). Вложенные
    // анимируемые SC/спаны внутри открытого диапазона уже покрыты им.
    let mut open: Option<(u32, usize)> = None;
    for (sc_id, phase) in &order.steps {
        let idx = sc_id.0 as usize;
        if idx >= buckets.len() {
            continue;
        }
        let bucket = &mut buckets[idx];
        match phase {
            PaintPhase::RootBackground => {
                if open.is_none() && animated_scs.contains(&sc_id.0) {
                    open = Some((sc_id.0, out.len()));
                }
                out.append(&mut bucket.pre);
                out.append(&mut bucket.root_bg);
            }
            PaintPhase::InlineContent => {
                let base = out.len();
                out.append(&mut bucket.contents);
                // post (PopTransform / PopOpacity / etc.) is now in CloseLayer —
                // emitted AFTER all child SCs so nested transforms compose correctly
                // (BUG-139). Do NOT move post back here.
                if open.is_none()
                    && let Some(spans) = spans_by_sc.get(&sc_id.0)
                {
                    for &(s, e) in spans {
                        if e > s {
                            ranges.push(base + s..base + e);
                        }
                    }
                }
            }
            // CloseLayer is emitted last in paint_sc, after all child SCs, so the
            // parent's Pop-commands wrap the children's Push-commands correctly.
            PaintPhase::CloseLayer => {
                out.append(&mut bucket.post);
                if let Some((id, start)) = open
                    && id == sc_id.0
                {
                    if out.len() > start {
                        ranges.push(start..out.len());
                    }
                    open = None;
                }
            }
            _ => {}
        }
    }
    // Незакрытый диапазон (SC без CloseLayer-шага) — split невалиден.
    if split.invalid || open.is_some() {
        return (out, Vec::new());
    }
    // Сортировка + выбрасывание вложенных спанов (спан текстового потомка
    // внутри спана его элемента и т.п.). Частичное пересечение диапазонов
    // невозможно по построению; если встретилось — split невалиден.
    ranges.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    let mut dedup: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match dedup.last() {
            Some(last) if r.start < last.end => {
                if r.end <= last.end {
                    continue; // вложенный — покрыт внешним
                }
                return (out, Vec::new()); // частичное пересечение — не бывает
            }
            _ => dedup.push(r),
        }
    }
    (out, dedup)
}
