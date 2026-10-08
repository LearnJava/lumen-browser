//! CSS Scroll Snap L2 §4 — `scroll-initial-target: none | nearest`.
//!
//! После первичного layout документа каждый элемент с `scroll-initial-target:
//! nearest` прокручивается в видимость во всех своих scroll-контейнерах
//! (от ближайшего к внешнему) и, если контейнеров больше нет, во вьюпорте
//! страницы. Выравнивание — по `scroll-snap-align` цели (start/center/end),
//! при `none` — по началу области (start), как делают Blink/Gecko для этого
//! свойства.
//!
//! Раскладка делится на две фазы, потому что `set_scroll_position` берёт
//! дерево по `&mut`, а поиск целей — по `&`: фаза 1 собирает [`Job`]-ы,
//! фаза 2 применяет их.

use crate::box_tree::BoxKind;
use crate::style::{Overflow, ScrollInitialTarget, ScrollSnapAlignKeyword};
use crate::{LayoutBox, padding_box, set_scroll_position, snap_offset_x, snap_offset_y};
use lumen_core::geom::{Rect, Size};
use lumen_dom::NodeId;
use std::collections::HashSet;

/// Предок цели, который может быть прокручен (или вьюпорт страницы).
#[derive(Clone)]
struct Scroller {
    node: NodeId,
    /// Padding box в координатах документа (без учёта scroll-смещений).
    pb: Rect,
    /// `scroll-padding-{top,right,bottom,left}` контейнера.
    padding: [f32; 4],
}

/// Одна цель вместе с цепочкой своих scroll-контейнеров.
struct Job {
    /// Border box цели в координатах документа.
    area: Rect,
    /// `scroll-margin-{top,right,bottom,left}` цели.
    margin: [f32; 4],
    inline: ScrollSnapAlignKeyword,
    block: ScrollSnapAlignKeyword,
    /// Контейнеры от ближайшего к внешнему (корень-вьюпорт сюда не входит).
    containers: Vec<Scroller>,
}

fn is_scroll_container(b: &LayoutBox) -> bool {
    let scrollable =
        |o: Overflow| matches!(o, Overflow::Scroll | Overflow::Auto | Overflow::Hidden);
    scrollable(b.style.overflow_x) || scrollable(b.style.overflow_y)
}

fn scroller_of(b: &LayoutBox) -> Scroller {
    Scroller {
        node: b.node,
        pb: padding_box(b),
        padding: [
            b.style.scroll_padding_top,
            b.style.scroll_padding_right,
            b.style.scroll_padding_bottom,
            b.style.scroll_padding_left,
        ],
    }
}

/// Фаза 1: обход в порядке документа, цель — каждый бокс с `nearest`.
///
/// Явный стек (как `collect_snap_rec`), а не рекурсия: глубокие деревья не
/// должны переполнять стек вызовов. `chain[0]` — корень (вьюпорт), остальные
/// элементы — scroll-контейнеры-предки; `depth` восстанавливает длину цепочки.
fn collect_jobs(root: &LayoutBox) -> (Vec<Job>, Scroller) {
    let viewport = scroller_of(root);
    let mut jobs = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();
    // Первая цель на контейнер выигрывает (CSS Scroll Snap L2 §4: у
    // scroll-контейнера одна начальная цель).
    let mut claimed: HashSet<NodeId> = HashSet::new();
    let mut chain: Vec<Scroller> = vec![viewport.clone()];
    let mut stack: Vec<(&LayoutBox, usize)> = Vec::new();
    for child in root.children.iter().rev() {
        stack.push((child, 1));
    }
    while let Some((b, depth)) = stack.pop() {
        chain.truncate(depth);
        if matches!(b.kind, BoxKind::Skip) {
            continue;
        }
        if b.style.scroll_initial_target == ScrollInitialTarget::Nearest && seen.insert(b.node) {
            let innermost = chain.last().map_or(viewport.node, |s| s.node);
            if claimed.insert(innermost) {
                let s = &b.style;
                jobs.push(Job {
                    area: b.rect,
                    margin: [
                        s.scroll_margin_top,
                        s.scroll_margin_right,
                        s.scroll_margin_bottom,
                        s.scroll_margin_left,
                    ],
                    inline: s.scroll_snap_align.inline,
                    block: s.scroll_snap_align.block,
                    containers: chain.iter().skip(1).rev().cloned().collect(),
                });
            }
        }
        if is_scroll_container(b) {
            chain.push(scroller_of(b));
        }
        let child_depth = chain.len();
        for child in b.children.iter().rev() {
            stack.push((child, child_depth));
        }
    }
    (jobs, viewport)
}

/// Желаемое смещение прокрутки (x, y) контейнера `sc` для цели `job`,
/// `shift` — уже применённая прокрутка внутренних контейнеров.
fn desired_offset(job: &Job, sc: &Scroller, shift: (f32, f32)) -> (f32, f32) {
    let area = Rect::new(
        job.area.x - shift.0,
        job.area.y - shift.1,
        job.area.width,
        job.area.height,
    );
    let [mt, mr, mb, ml] = job.margin;
    let [pt, pr, pb_, pl] = sc.padding;
    let x = snap_offset_x(job.inline, area, sc.pb, ml, mr, pl, pr)
        .unwrap_or(area.x - sc.pb.x - ml - pl);
    let y = snap_offset_y(job.block, area, sc.pb, mt, mb, pt, pb_)
        .unwrap_or(area.y - sc.pb.y - mt - pt);
    (x, y)
}

/// Текущее смещение прокрутки бокса узла `node`.
fn scroll_of(root: &LayoutBox, node: NodeId) -> Option<(f32, f32)> {
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if b.node == node {
            return Some((b.scroll_x, b.scroll_y));
        }
        stack.extend(b.children.iter());
    }
    None
}

/// Итог [`apply_scroll_initial_targets`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InitialScroll {
    /// Желаемое смещение вьюпорта страницы `(scroll_x, scroll_y)` — клампинг по
    /// размеру документа делает shell, у которого есть `max_scroll`. `None` —
    /// ни одна цель не прокручивает сам вьюпорт.
    pub page: Option<(f32, f32)>,
}

/// Применить `scroll-initial-target: nearest` к только что построенному
/// дереву `root`.
///
/// Scroll-контейнеры внутри дерева прокручиваются на месте
/// ([`set_scroll_position`], с его клампингом), поэтому `Some(_)` означает
/// «на странице есть цели, дерево могло измениться — перерисуй display list».
/// `None` — целей нет, ничего не тронуто.
///
/// Не покрыто: повторное срабатывание, когда цель появляется после загрузки
/// (`display: none` → `block`), и RTL-инверсия начала inline-оси.
// CSS: scroll-initial-target
pub fn apply_scroll_initial_targets(root: &mut LayoutBox, viewport: Size) -> Option<InitialScroll> {
    let (jobs, vp) = collect_jobs(root);
    if jobs.is_empty() {
        return None;
    }
    // Снап-порт страницы — вьюпорт, а не весь документ.
    let vp_scroller = Scroller {
        pb: Rect::new(0.0, 0.0, viewport.width, viewport.height),
        ..vp
    };
    let mut page: Option<(f32, f32)> = None;
    for job in &jobs {
        let mut shift = (0.0_f32, 0.0_f32);
        for sc in &job.containers {
            let (x, y) = desired_offset(job, sc, shift);
            set_scroll_position(root, sc.node, x, y);
            if let Some((sx, sy)) = scroll_of(root, sc.node) {
                shift.0 += sx;
                shift.1 += sy;
            }
        }
        // Вьюпорт страницы прокручивается, только если у цели нет
        // собственных scroll-контейнеров: иначе страница «прыгала» бы за
        // каждым внутренним скроллером.
        if job.containers.is_empty() {
            page = Some(desired_offset(job, &vp_scroller, shift));
        }
    }
    Some(InitialScroll { page })
}
