//! PERF-16, срез 2: `SubtreeEmitCache` в `fill_buckets`. Кэш обязан быть невидимым: сборка с
//! кэшем, прогретым на другой раскладке, даёт те же команды и те же provenance-спаны, что
//! сборка без кэша. Доказательство — дифференциальное, на корпусе `graphic_tests/*.html` с теми
//! же CSS-возмущениями, что у ключа (`subtree_paint_eq.rs`): кэш прогревается раскладкой A,
//! затем собирается возмущённая B (часть кусков — попадания, часть — промахи), затем снова A.

use super::subtree_paint_eq::{corpus, layout_page, PERTURBATIONS, SELECTORS};
use super::*;

/// Команды (в `Debug`, как у `assert_dl_eq`) и спаны одной сборки.
type Built = (Vec<DisplayCommand>, Vec<ProvenanceSpan>);

fn build(root: &LayoutBox, dpr: f32, cache: Option<&mut SubtreeEmitCache>) -> Built {
    let tree = StackingTree::build(root);
    let order = PaintOrder::from_tree(&tree);
    let (cmds, index) = match cache {
        Some(c) => build_display_list_ordered_dpr_cached(root, &tree, &order, dpr, c),
        None => build_display_list_ordered_dpr(root, &tree, &order, dpr),
    };
    (cmds, index.spans)
}

/// Различие двух сборок: индекс первой расходящейся команды либо спана.
fn first_diff(a: &Built, b: &Built) -> Option<String> {
    if a.0.len() != b.0.len() {
        return Some(format!("команд {} против {}", a.0.len(), b.0.len()));
    }
    if let Some(i) = a.0.iter().zip(&b.0).position(|(x, y)| x != y) {
        return Some(format!("команда #{i}: {:?} против {:?}", a.0[i], b.0[i]));
    }
    if a.1 != b.1 {
        let i = a.1.iter().zip(&b.1).position(|(x, y)| x != y).unwrap_or(a.1.len().min(b.1.len()));
        return Some(format!("спан #{i} (всего {} против {})", a.1.len(), b.1.len()));
    }
    None
}

#[test]
fn cached_build_equals_uncached_build_on_the_page_corpus() {
    let pages = corpus();
    assert!(pages.len() > 100, "корпус graphic_tests пуст или не найден: {}", pages.len());
    let (mut builds, mut mismatches, mut skipped) = (0_usize, Vec::new(), 0_usize);
    let mut totals = EmitCacheStats::default();
    for (page_ix, (name, html)) in pages.iter().enumerate() {
        let base = layout_page(html, "");
        let mut cache = SubtreeEmitCache::new();
        // Прогрев и повтор на той же раскладке: второй проход — сплошные попадания.
        for pass in 0..2 {
            let cached = build(&base, 1.0, Some(&mut cache));
            if let Some(d) = first_diff(&cached, &build(&base, 1.0, None)) {
                mismatches.push(format!("{name} / проход {pass}: {d}"));
            }
            builds += 1;
        }
        for (i, decl) in PERTURBATIONS.iter().enumerate() {
            let sel = SELECTORS[(i + page_ix) % SELECTORS.len()];
            let css = format!("{sel} {{ {decl} }}");
            // Раскладка в debug может упасть на инвариантах (`DEVX-8a`) — дефект раскладки, не кэша.
            let Ok(perturbed) = std::panic::catch_unwind(|| layout_page(html, &css)) else {
                skipped += 1;
                continue;
            };
            // A (кэш тёплый) → B → B → A: промахи, попадания, возврат к исходному.
            for (step, root) in [("B", &perturbed), ("B2", &perturbed), ("A", &base)] {
                let cached = build(root, 1.0, Some(&mut cache));
                if let Some(d) = first_diff(&cached, &build(root, 1.0, None)) {
                    mismatches.push(format!("{name} / {css} / {step}: {d}"));
                }
                builds += 1;
            }
        }
        let s = cache.stats();
        totals.hits += s.hits;
        totals.stored += s.stored;
        totals.uncacheable += s.uncacheable;
        totals.replayed_boxes += s.replayed_boxes;
    }
    eprintln!(
        "[subtree_emit_cache] builds={builds} hits={} stored={} uncacheable={} replayed_boxes={} \
         skipped_layouts={skipped} mismatches={}",
        totals.hits,
        totals.stored,
        totals.uncacheable,
        totals.replayed_boxes,
        mismatches.len(),
    );
    assert!(
        mismatches.is_empty(),
        "сборка с кэшем разошлась со сборкой без него ({} шт.), первые: {:#?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(10)],
    );
    assert!(
        skipped * 50 < pages.len() * PERTURBATIONS.len(),
        "слишком много возмущённых раскладок упало на инвариантах: {skipped}",
    );
    // Не пустой тест: кэш и воспроизводит, и сохраняет.
    assert!(totals.hits > 5_000, "слишком мало попаданий: {}", totals.hits);
    assert!(totals.stored > 5_000, "слишком мало сохранений: {}", totals.stored);
}

/// `dpr` — часть окружения кэша: смена сбрасывает записи, команды совпадают с несложенной сборкой.
#[test]
fn dpr_change_flushes_the_cache() {
    let pages = corpus();
    let (_, html) = pages.iter().find(|(n, _)| n.contains("flex")).unwrap_or(&pages[0]);
    let root = layout_page(html, "");
    let mut cache = SubtreeEmitCache::new();
    build(&root, 1.0, Some(&mut cache));
    let hits_before = cache.stats().hits;
    let cached = build(&root, 2.0, Some(&mut cache));
    assert_eq!(cache.stats().hits, hits_before, "смена dpr не должна попадать в записи старого dpr");
    assert_eq!(first_diff(&cached, &build(&root, 2.0, None)), None);
}

/// Записи, которых сборка не коснулась, удаляются: снимки не копятся при смене страницы.
#[test]
fn untouched_entries_are_dropped_at_end_of_build() {
    let pages = corpus();
    let big = pages.iter().max_by_key(|(_, h)| h.len().min(60_000)).expect("корпус не пуст");
    let mut cache = SubtreeEmitCache::new();
    build(&layout_page(&big.1, ""), 1.0, Some(&mut cache));
    assert!(!cache.is_empty(), "на крупной странице есть куски для кэша");
    // Пустая страница: ни один прежний кусок в сборке не участвует.
    build(&layout_page("<p>x</p>", ""), 1.0, Some(&mut cache));
    assert!(cache.len() <= 1, "остались снимки прежней страницы: {}", cache.len());
}
