//! Bounded-parallelism helper for subresource fetching.
//!
//! Every subresource pass of the load pipeline (stylesheets, images, scripts,
//! frames) is I/O-bound on the network, and doing it sequentially on the UI
//! thread was the main cost of a heavy page — hence [`parallel_map`], which
//! keeps result order while capping the thread count.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// Максимум одновременных потоков загрузки подресурсов одного типа.
/// Подобрано под типичный браузерный лимит соединений на хост (~6) + запас.
/// Работа I/O-bound (сетевой fetch), потоки в основном спят на сокете, поэтому
/// небольшой оверсабскрипшн относительно числа ядер допустим и полезен.
const MAX_PARALLEL_FETCHES: usize = 8;

/// Применить `f` к каждому элементу `items` параллельно, СОХРАНЯЯ порядок
/// результатов (результат `i` соответствует `items[i]`). Число одновременных
/// потоков ограничено `MAX_PARALLEL_FETCHES`, чтобы не плодить по потоку на
/// каждый подресурс — тяжёлые страницы имеют десятки картинок/скриптов.
///
/// Главный тормоз тяжёлых страниц — последовательная сетевая загрузка
/// подресурсов в UI-потоке (диагноз 2026-06-16: ~5.3 с из 7.5 с на lenta.ru).
/// Параллелизация fetch'а даёт ×5–6 по замерам curl. Декодирование (CPU)
/// при желании тоже выполняется внутри `f`, разъезжаясь по потокам.
///
/// `f` получает `(индекс, &элемент)` и должна быть `Sync` (вызывается из всех
/// потоков). Паники внутри `f` не глотаются — пробрасываются при join.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn parallel_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        // Один элемент — не поднимаем потоки.
        return vec![f(0, &items[0])];
    }

    let workers = MAX_PARALLEL_FETCHES.min(n);
    let next = AtomicUsize::new(0);
    // По одной ячейке на результат; воркеры пишут строго в свой индекс, гонок нет.
    let slots: Vec<Mutex<Option<R>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let f = &f;
    let slots_ref = &slots;
    let next_ref = &next;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(move || loop {
                let i = next_ref.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let r = f(i, &items[i]);
                *slots_ref[i].lock().unwrap() = Some(r);
            });
        }
    });

    slots
        .into_iter()
        .map(|cell| cell.into_inner().unwrap().expect("worker filled every slot"))
        .collect()
}
