//! Diff двух display list-ов в строки полосы скролл-композитора
//! (ADR-033, THREAD-11, срез 1).
//!
//! Чистая функция без GPU: по дайджестам команд (`hash_one_command`) находит
//! окно изменений и возвращает диапазоны документных Y, которые надо
//! перерисовать, либо причину, по которой границы неизвестны и нужна полная
//! полоса (откат на прежнее поведение — корректность не зависит от догадок).
//!
//! 1. Общий префикс и суффикс по дайджестам отрезаются — вставка команды не
//!    сдвигает хвост в «всё изменилось».
//! 2. Окно растёт до замкнутых групп `Push*/Pop*`: изменившийся `PushClipRect`
//!    меняет всё до своего `PopClip`, даже если тот лежит в суффиксе.
//! 3. Границы берутся из [`DisplayCommand::cull_rect`] команд окна с обеих
//!    сторон (старой и новой). Безопасны только группы, чей эффект ограничен
//!    объединением содержимого: клип, прозрачность, режим смешивания.
//!    Transform, фильтры, маски, sticky/fixed/scroll-слои и листья без границ
//!    дают [`FullReason`]; то же — охватывающая такая группа вокруг окна
//!    (координаты листьев внутри неё локальные, не документные).

use crate::display_list::DisplayCommand;

/// Запас вокруг границ команды, CSS px: сглаживание краёв выходит за `rect`.
const PAD_CSS: f32 = 1.0;

/// Вертикальный диапазон документных Y, CSS px (`top < bottom`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YRange {
    pub top: f32,
    pub bottom: f32,
}

/// Почему границы изменения неизвестны и нужна полная полоса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullReason {
    /// Число дайджестов не равно числу команд.
    DigestMismatch,
    /// Push/Pop не сбалансированы или перепутаны по виду.
    Unbalanced,
    /// В окне слой с эффектом, выходящим за содержимое (имя команды).
    UnsafeLayer(&'static str),
    /// В окне лист без границ (`cull_rect() == None`).
    UnboundedLeaf(&'static str),
    /// Нечисловая граница (NaN/∞).
    NonFinite,
}

/// Результат сравнения.
#[derive(Debug, Clone, PartialEq)]
pub enum BandDiff {
    /// Списки совпадают поэлементно.
    Identical,
    /// Перерисовать эти диапазоны (отсортированы, не пересекаются).
    Rows(Vec<YRange>),
    /// Границы неизвестны — перерисовывать всю полосу.
    Full(FullReason),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Clip,
    Opacity,
    Blend,
    Mask,
    MaskLayer,
    Transform,
    Filter,
    Backdrop,
    Sticky,
    FixedLayer,
    FixedBackground,
    Scroll,
}

impl Kind {
    /// Эффект группы ограничен объединением её содержимого.
    fn extent_bounded(self) -> bool {
        matches!(self, Kind::Clip | Kind::Opacity | Kind::Blend)
    }
}

enum Role {
    Leaf,
    Open(Kind),
    Close(Kind),
}

fn role(cmd: &DisplayCommand) -> Role {
    use DisplayCommand as D;
    match cmd {
        D::PushClipRect { .. } | D::PushClipRoundedRect { .. } | D::PushClipPath { .. } => {
            Role::Open(Kind::Clip)
        }
        D::PopClip => Role::Close(Kind::Clip),
        D::PushOpacity { .. } => Role::Open(Kind::Opacity),
        D::PopOpacity => Role::Close(Kind::Opacity),
        D::PushBlendMode { .. } => Role::Open(Kind::Blend),
        D::PopBlendMode => Role::Close(Kind::Blend),
        D::PushMaskImage { .. }
        | D::PushMaskLinearGradient { .. }
        | D::PushMaskRadialGradient { .. }
        | D::PushMaskConicGradient { .. } => Role::Open(Kind::Mask),
        D::PopMask => Role::Close(Kind::Mask),
        D::PushMaskLayer { .. } => Role::Open(Kind::MaskLayer),
        D::PopMaskLayer => Role::Close(Kind::MaskLayer),
        D::PushTransform { .. } => Role::Open(Kind::Transform),
        D::PopTransform => Role::Close(Kind::Transform),
        D::PushFilter { .. } => Role::Open(Kind::Filter),
        D::PopFilter => Role::Close(Kind::Filter),
        D::PushBackdropFilter { .. } => Role::Open(Kind::Backdrop),
        D::PopBackdropFilter => Role::Close(Kind::Backdrop),
        D::BeginStickyLayer { .. } => Role::Open(Kind::Sticky),
        D::EndStickyLayer => Role::Close(Kind::Sticky),
        D::BeginFixedLayer => Role::Open(Kind::FixedLayer),
        D::EndFixedLayer => Role::Close(Kind::FixedLayer),
        D::BeginFixedBackground => Role::Open(Kind::FixedBackground),
        D::EndFixedBackground => Role::Close(Kind::FixedBackground),
        D::PushScrollLayer { .. } => Role::Open(Kind::Scroll),
        D::PopScrollLayer => Role::Close(Kind::Scroll),
        _ => Role::Leaf,
    }
}

const NO_PARTNER: usize = usize::MAX;

/// Парный индекс каждой Push/Pop-команды (`NO_PARTNER` у листьев). `None` —
/// список не сбалансирован.
fn partners(list: &[DisplayCommand]) -> Option<Vec<usize>> {
    let mut out = vec![NO_PARTNER; list.len()];
    let mut stack: Vec<(usize, Kind)> = Vec::new();
    for (i, cmd) in list.iter().enumerate() {
        match role(cmd) {
            Role::Leaf => {}
            Role::Open(k) => stack.push((i, k)),
            Role::Close(k) => {
                let (open, ok) = stack.pop()?;
                if ok != k {
                    return None;
                }
                out[open] = i;
                out[i] = open;
            }
        }
    }
    stack.is_empty().then_some(out)
}

/// Сравнивает старый и новый списки по дайджестам и считает строки к
/// перерисовке. `*_digests[i]` — `hash_one_command(&*[i])`.
#[must_use]
pub fn diff_band(
    old: &[DisplayCommand],
    old_digests: &[u64],
    new: &[DisplayCommand],
    new_digests: &[u64],
) -> BandDiff {
    if old.len() != old_digests.len() || new.len() != new_digests.len() {
        return BandDiff::Full(FullReason::DigestMismatch);
    }
    let min_len = old.len().min(new.len());
    let mut p = old_digests.iter().zip(new_digests).take_while(|(a, b)| a == b).count();
    if p == old.len() && p == new.len() {
        return BandDiff::Identical;
    }
    let mut s = old_digests
        .iter()
        .rev()
        .zip(new_digests.iter().rev())
        .take(min_len - p)
        .take_while(|(a, b)| a == b)
        .count();

    let (Some(po), Some(pn)) = (partners(old), partners(new)) else {
        return BandDiff::Full(FullReason::Unbalanced);
    };

    // Рост окна до замкнутых групп: фиксированная точка по `p` (вниз) и `s`.
    loop {
        let mut changed = false;
        for (len, partner) in [(old.len(), &po), (new.len(), &pn)] {
            let mut i = p;
            while i < len - s {
                let j = partner[i];
                if j != NO_PARTNER {
                    let (lo, hi) = (i.min(j), i.max(j));
                    if lo < p {
                        p = lo;
                        changed = true;
                    }
                    if hi >= len - s {
                        s = len - 1 - hi;
                        changed = true;
                    }
                }
                i += 1;
            }
        }
        if !changed {
            break;
        }
    }

    // Охватывающая группа с преобразованием координат (transform, scroll,
    // sticky, fixed) делает `cull_rect` листьев окна локальным, а не
    // документным. Окно замкнуто по группам, поэтому охватывающая группа
    // открыта в префиксе и закрывается за окном.
    for (i, cmd) in old[..p].iter().enumerate() {
        if let Role::Open(k) = role(cmd)
            && !k.extent_bounded()
            && po[i] >= old.len() - s
        {
            return BandDiff::Full(FullReason::UnsafeLayer(cmd.variant_name()));
        }
    }

    let mut ranges: Vec<YRange> = Vec::new();
    for list in [old, new] {
        for cmd in &list[p..list.len() - s] {
            match role(cmd) {
                Role::Open(k) | Role::Close(k) => {
                    if !k.extent_bounded() {
                        return BandDiff::Full(FullReason::UnsafeLayer(cmd.variant_name()));
                    }
                }
                Role::Leaf => {
                    let Some(r) = cmd.cull_rect() else {
                        return BandDiff::Full(FullReason::UnboundedLeaf(cmd.variant_name()));
                    };
                    let (top, bottom) = (r.y - PAD_CSS, r.y + r.height + PAD_CSS);
                    if !top.is_finite() || !bottom.is_finite() {
                        return BandDiff::Full(FullReason::NonFinite);
                    }
                    ranges.push(YRange { top, bottom });
                }
            }
        }
    }
    BandDiff::Rows(merge(ranges))
}

/// Сортирует и склеивает пересекающиеся/соприкасающиеся диапазоны.
fn merge(mut v: Vec<YRange>) -> Vec<YRange> {
    v.sort_by(|a, b| a.top.total_cmp(&b.top));
    let mut out: Vec<YRange> = Vec::with_capacity(v.len());
    for r in v {
        match out.last_mut() {
            Some(last) if r.top <= last.bottom => last.bottom = last.bottom.max(r.bottom),
            _ => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display_list::hash_one_command;
    use lumen_core::geom::Rect;
    use lumen_layout::Color;

    fn fill(y: f32, h: f32) -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: Rect::new(0.0, y, 100.0, h),
            color: Color { r: 1, g: 2, b: 3, a: 255 },
        }
    }

    fn clip(y: f32, h: f32) -> DisplayCommand {
        DisplayCommand::PushClipRect { rect: Rect::new(0.0, y, 100.0, h) }
    }

    fn d(l: &[DisplayCommand]) -> Vec<u64> {
        l.iter().map(hash_one_command).collect()
    }

    fn run(old: &[DisplayCommand], new: &[DisplayCommand]) -> BandDiff {
        diff_band(old, &d(old), new, &d(new))
    }

    fn rows(r: &BandDiff) -> Vec<(f32, f32)> {
        match r {
            BandDiff::Rows(v) => v.iter().map(|y| (y.top + PAD_CSS, y.bottom - PAD_CSS)).collect(),
            other => panic!("ожидались строки, получено {other:?}"),
        }
    }

    #[test]
    fn identical_lists() {
        let l = vec![fill(0.0, 10.0), fill(10.0, 10.0)];
        assert_eq!(run(&l, &l), BandDiff::Identical);
        assert_eq!(run(&[], &[]), BandDiff::Identical);
    }

    #[test]
    fn one_changed_leaf_gives_old_and_new_rows() {
        let old = vec![fill(0.0, 10.0), fill(500.0, 10.0), fill(900.0, 10.0)];
        let new = vec![fill(0.0, 10.0), fill(520.0, 10.0), fill(900.0, 10.0)];
        // Старая позиция 500..510 и новая 520..530 — два отдельных диапазона.
        assert_eq!(rows(&run(&old, &new)), vec![(500.0, 510.0), (520.0, 530.0)]);
    }

    #[test]
    fn insertion_does_not_dirty_the_tail() {
        let old = vec![fill(0.0, 10.0), fill(900.0, 10.0), fill(2000.0, 10.0)];
        let new = vec![fill(0.0, 10.0), fill(500.0, 10.0), fill(900.0, 10.0), fill(2000.0, 10.0)];
        assert_eq!(rows(&run(&old, &new)), vec![(500.0, 510.0)]);
    }

    #[test]
    fn removal_marks_removed_rows() {
        let old = vec![fill(0.0, 10.0), fill(500.0, 10.0), fill(900.0, 10.0)];
        let new = vec![fill(0.0, 10.0), fill(900.0, 10.0)];
        assert_eq!(rows(&run(&old, &new)), vec![(500.0, 510.0)]);
    }

    #[test]
    fn overlapping_ranges_merge() {
        let old = vec![fill(100.0, 50.0), fill(120.0, 50.0)];
        let new = vec![fill(100.0, 60.0), fill(120.0, 60.0)];
        assert_eq!(rows(&run(&old, &new)), vec![(100.0, 180.0)]);
    }

    #[test]
    fn changed_clip_grows_to_its_group_in_the_suffix() {
        // Клип изменился, его закрытие и дети лежат в суффиксе — группа целиком.
        let old = vec![fill(0.0, 5.0), clip(300.0, 100.0), fill(310.0, 10.0), fill(380.0, 10.0), DisplayCommand::PopClip, fill(900.0, 5.0)];
        let new = vec![fill(0.0, 5.0), clip(300.0, 120.0), fill(310.0, 10.0), fill(380.0, 10.0), DisplayCommand::PopClip, fill(900.0, 5.0)];
        assert_eq!(rows(&run(&old, &new)), vec![(310.0, 320.0), (380.0, 390.0)]);
    }

    #[test]
    fn changed_leaf_inside_equal_clip_stays_local() {
        let old = vec![clip(0.0, 1000.0), fill(100.0, 10.0), fill(700.0, 10.0), DisplayCommand::PopClip];
        let new = vec![clip(0.0, 1000.0), fill(100.0, 12.0), fill(700.0, 10.0), DisplayCommand::PopClip];
        assert_eq!(rows(&run(&old, &new)), vec![(100.0, 112.0)]);
    }

    #[test]
    fn opacity_group_is_bounded() {
        let op = |a: f32| DisplayCommand::PushOpacity { alpha: a, bounds: None };
        let old = vec![op(0.5), fill(40.0, 10.0), DisplayCommand::PopOpacity, fill(900.0, 5.0)];
        let new = vec![op(0.6), fill(40.0, 10.0), DisplayCommand::PopOpacity, fill(900.0, 5.0)];
        assert_eq!(rows(&run(&old, &new)), vec![(40.0, 50.0)]);
    }

    #[test]
    fn transform_group_falls_back() {
        let tr = DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::IDENTITY };
        let old = vec![tr.clone(), fill(40.0, 10.0), DisplayCommand::PopTransform];
        let new = vec![tr, fill(41.0, 10.0), DisplayCommand::PopTransform];
        assert_eq!(run(&old, &new), BandDiff::Full(FullReason::UnsafeLayer("PushTransform")));
    }

    #[test]
    fn unbalanced_list_falls_back() {
        let old = vec![fill(0.0, 5.0)];
        let new = vec![fill(0.0, 5.0), DisplayCommand::PopClip];
        assert_eq!(run(&old, &new), BandDiff::Full(FullReason::Unbalanced));
        let mismatched = vec![clip(0.0, 5.0), DisplayCommand::PopOpacity];
        assert_eq!(run(&old, &mismatched), BandDiff::Full(FullReason::Unbalanced));
    }

    #[test]
    fn digest_length_mismatch_falls_back() {
        let l = vec![fill(0.0, 5.0)];
        assert_eq!(diff_band(&l, &[], &l, &d(&l)), BandDiff::Full(FullReason::DigestMismatch));
    }

    #[test]
    fn non_finite_bound_falls_back() {
        let old = vec![fill(0.0, 5.0)];
        let new = vec![fill(f32::INFINITY, 5.0)];
        assert_eq!(run(&old, &new), BandDiff::Full(FullReason::NonFinite));
    }

    #[test]
    fn nested_groups_reach_fixed_point() {
        // Изменение внутри внутреннего клипа тянет только внутренний клип;
        // изменение внешнего клипа тянет всё до его закрытия.
        let old = vec![clip(0.0, 900.0), clip(10.0, 100.0), fill(20.0, 5.0), DisplayCommand::PopClip, fill(800.0, 5.0), DisplayCommand::PopClip, fill(950.0, 5.0)];
        let new = vec![clip(0.0, 910.0), clip(10.0, 100.0), fill(20.0, 5.0), DisplayCommand::PopClip, fill(800.0, 5.0), DisplayCommand::PopClip, fill(950.0, 5.0)];
        assert_eq!(rows(&run(&old, &new)), vec![(20.0, 25.0), (800.0, 805.0)]);
    }
}
