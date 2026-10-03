//! PERF-16, срез 2: кэш emit display list по поддереву (`SubtreeEmitCache`).
//!
//! Инкрементальный цикл хрома (CC-12) меняет в кадре крошечную часть дерева, а `fill_buckets`
//! эмитит заново все ~400 боксов (emit = 0,52 из 1,40 мс цикла). Кэш запоминает для **куска**
//! дерева его команды и provenance-спаны вместе со снимком самого поддерева; в следующем кадре
//! кусок, у которого [`subtree_paint_eq`] признал снимок равным, воспроизводится копированием.
//!
//! **Кусок** — non-SC поддерево размером от [`CHUNK_MIN_BOXES`] до [`CHUNK_MAX_BOXES`] боксов
//! без stacking context внутри: его emit целиком уходит в `contents` одного бакета и не
//! выделяет `StackingContextId`. Куски не вложены друг в друга (предок крупнее порога), поэтому
//! снимок каждого бокса хранится не более одного раза, а промах стоит клонирования ≤ порога
//! боксов, а не всей ветки до корня.
//!
//! **Что в ключе кроме равенства дерева** — `dpr` и вьюпорт fixed-фонов (единственные входы emit,
//! которых нет в `LayoutBox`); при смене любого из них кэш сбрасывается целиком. Compositor-override
//! и static/animated split кэш не поддерживает вовсе: владелец вызывает его только на пути без
//! `anim`, где `ov == None` у каждого бокса.
//!
//! Безопасность («равно ⇒ тот же emit») доказана дифференциально в
//! `tests/subtree_emit_cache.rs`: сборка с прогретым кэшем побайтно равна сборке без него.

use std::collections::HashMap;

use lumen_dom::NodeId;
use lumen_layout::{subtree_paint_eq, BoxRole};

use super::*;

/// Больше — кусок не кэшируется (его промах стоил бы слишком дорогого клона снимка).
pub const CHUNK_MAX_BOXES: usize = 64;
/// Меньше — поиск и сравнение не окупаются: emit пары боксов дешевле хеш-запроса.
pub const CHUNK_MIN_BOXES: usize = 4;

/// Счётчики одного кэша за всё время жизни — для тестов и диагностики.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmitCacheStats {
    /// Куски, воспроизведённые из кэша.
    pub hits: u64,
    /// Куски, эмитнутые заново и сохранённые в кэше.
    pub stored: u64,
    /// Куски, которые нельзя кэшировать: внутри оказался stacking context.
    pub uncacheable: u64,
    /// Боксы, воспроизведённые из кэша (по `hits`).
    pub replayed_boxes: u64,
}

/// Кэш emit по поддереву; живёт у вызывающего между кадрами.
#[derive(Debug, Default)]
pub struct SubtreeEmitCache {
    entries: HashMap<(NodeId, BoxRole), Entry>,
    /// Номер текущей сборки; запись, не тронутая в ней, при `end_build` удаляется.
    epoch: u64,
    /// `dpr` и вьюпорт fixed-фонов, при которых записаны `entries`.
    env: Option<Env>,
    stats: EmitCacheStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Env {
    dpr_bits: u32,
    viewport: Option<[u32; 4]>,
}

#[derive(Debug)]
struct Entry {
    /// Клон поддерева, по которому записаны `commands`; стили в нём — `Arc`, так что клон дёшев.
    snapshot: LayoutBox,
    /// Всё, что поддерево добавило в `contents` своего SC.
    commands: Vec<DisplayCommand>,
    /// Спаны, смещения которых отсчитаны от начала `commands`.
    spans: Vec<EntrySpan>,
    boxes: usize,
    epoch: u64,
}

#[derive(Debug)]
struct EntrySpan {
    range: std::ops::Range<usize>,
    origin: BoxOrigin,
    fragment: u32,
}

/// Что `fill_buckets` делает с non-SC ребёнком.
pub(crate) enum Lookup {
    /// Команды и спаны уже дописаны в бакет — ребёнка обходить не надо.
    Replayed,
    /// Обойти как обычно и по завершении сохранить результат ([`SubtreeEmitCache::store`]).
    Capture,
    /// Обойти как обычно, не сохраняя.
    Skip,
}

/// Откуда `store` берёт результат обхода куска.
pub(crate) struct CaptureMark {
    pub(crate) contents_start: usize,
    pub(crate) spans_start: usize,
    /// `next_sc_id` до обхода: если вырос, внутри оказался stacking context.
    pub(crate) next_sc_id: u32,
}

impl SubtreeEmitCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> EmitCacheStats {
        self.stats
    }

    /// Число записей (для тестов на утечку снимков).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Начало сборки: сбрасывает кэш, если сменился `dpr` или вьюпорт fixed-фонов.
    pub(crate) fn begin_build(&mut self, dpr: f32) {
        let env = Env {
            dpr_bits: dpr.to_bits(),
            viewport: fixed_bg_viewport().map(|r| [r.x, r.y, r.width, r.height].map(f32::to_bits)),
        };
        if self.env != Some(env) {
            self.entries.clear();
            self.env = Some(env);
        }
        self.epoch += 1;
    }

    /// Конец сборки: выбрасывает записи, которых эта сборка не коснулась (узел исчез или
    /// границы кусков сдвинулись) — иначе снимки копились бы бесконечно.
    pub(crate) fn end_build(&mut self) {
        let epoch = self.epoch;
        self.entries.retain(|_, e| e.epoch == epoch);
    }

    /// Пытается воспроизвести `child` из кэша в `contents` (бакет `sc`).
    pub(crate) fn lookup(
        &mut self,
        child: &LayoutBox,
        sc: u32,
        contents: &mut Vec<DisplayCommand>,
        raw_spans: &mut Vec<RawSpan>,
    ) -> Lookup {
        let key = (child.node, child.origin.role);
        if let Some(e) = self.entries.get_mut(&key)
            && subtree_paint_eq(&e.snapshot, child)
        {
            let base = contents.len();
            contents.extend_from_slice(&e.commands);
            raw_spans.extend(e.spans.iter().map(|s| RawSpan {
                sc,
                field: BucketField::Contents,
                range: base + s.range.start..base + s.range.end,
                origin: s.origin,
                fragment: s.fragment,
            }));
            e.epoch = self.epoch;
            self.stats.hits += 1;
            self.stats.replayed_boxes += e.boxes as u64;
            return Lookup::Replayed;
        }
        match boxes_within(child, CHUNK_MAX_BOXES) {
            Some(n) if n >= CHUNK_MIN_BOXES => Lookup::Capture,
            _ => {
                // Кусок вырос или съёжился за порог — старый снимок больше не нужен.
                self.entries.remove(&key);
                Lookup::Skip
            }
        }
    }

    /// Сохраняет результат только что обойдённого куска `child`.
    pub(crate) fn store(
        &mut self,
        child: &LayoutBox,
        sc: u32,
        mark: &CaptureMark,
        next_sc_id: u32,
        contents: &[DisplayCommand],
        raw_spans: &[RawSpan],
    ) {
        let key = (child.node, child.origin.role);
        let spans = &raw_spans[mark.spans_start..];
        // Stacking context внутри уводит часть команд в чужие бакеты, а спан не из `contents`
        // нашего SC означает то же самое: такой кусок воспроизвести копированием нельзя.
        let cacheable = next_sc_id == mark.next_sc_id
            && spans
                .iter()
                .all(|s| s.sc == sc && s.field == BucketField::Contents && s.range.start >= mark.contents_start);
        if !cacheable {
            self.entries.remove(&key);
            self.stats.uncacheable += 1;
            return;
        }
        let commands = contents[mark.contents_start..].to_vec();
        let spans = spans
            .iter()
            .map(|s| EntrySpan {
                range: s.range.start - mark.contents_start..s.range.end - mark.contents_start,
                origin: s.origin,
                fragment: s.fragment,
            })
            .collect();
        let boxes = boxes_within(child, CHUNK_MAX_BOXES).unwrap_or(CHUNK_MAX_BOXES);
        self.entries.insert(
            key,
            Entry { snapshot: child.clone(), commands, spans, boxes, epoch: self.epoch },
        );
        self.stats.stored += 1;
    }
}

/// Размер поддерева, если он не больше `cap`; иначе `None`. Рекурсия ограничена `cap` посещёнными
/// боксами, так что её глубина не зависит от документа.
fn boxes_within(b: &LayoutBox, cap: usize) -> Option<usize> {
    fn go(b: &LayoutBox, left: &mut usize) -> bool {
        if *left == 0 {
            return false;
        }
        *left -= 1;
        b.children.iter().all(|c| go(c, left))
    }
    let mut left = cap;
    go(b, &mut left).then(|| cap - left)
}
